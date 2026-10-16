//! Surface-conforming tetrahedralization through an explicitly invoked Gmsh.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::{CellKind, Dataset, Error, Mesh, Result};
use crate::formats::{msh, stl};

use super::Options;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self> {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "caexfer-tet-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let work = Self(path);
                    #[cfg(unix)]
                    fs::set_permissions(&work.0, fs::Permissions::from_mode(0o700))?;
                    return Ok(work);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(Error::new(
            "E_MESHER",
            "could not create a meshing directory",
        ))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// STL can duplicate vertex records, so count undirected edges by coordinates.
fn require_closed_surface(mesh: &Mesh) -> Result<()> {
    mesh.validate()?;
    if mesh.cells.is_empty()
        || mesh
            .cells
            .iter()
            .any(|cell| cell.kind != CellKind::Triangle3)
    {
        return Err(Error::new(
            "E_MESH",
            "tetrahedralization requires a nonempty triangle surface",
        ));
    }
    let mut edges = BTreeMap::<([u64; 3], [u64; 3]), usize>::new();
    for cell in &mesh.cells {
        let corners: [[u64; 3]; 3] = std::array::from_fn(|i| {
            let index = cell.connectivity[i];
            mesh.points[index]
                .position
                .map(|value| if value == 0.0 { 0 } else { value.to_bits() })
        });
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let edge = if corners[a] < corners[b] {
                (corners[a], corners[b])
            } else {
                (corners[b], corners[a])
            };
            *edges.entry(edge).or_default() += 1;
        }
    }
    if edges.values().any(|&count| count != 2) {
        return Err(Error::new(
            "E_MESH",
            "tetrahedralization requires a closed manifold surface with two triangles per edge",
        ));
    }
    Ok(())
}

fn script(tet_size: Option<f64>) -> String {
    let mut geo = String::from(
        "Merge \"surface.stl\";\nClassifySurfaces{40*Pi/180, 1, 1, 180*Pi/180};\nCreateGeometry;\nSurface Loop(1) = Surface{:};\nVolume(1) = {1};\nPhysical Volume(1) = {1};\nMesh.MshFileVersion = 2.2;\nMesh.Binary = 0;\nMesh.ElementOrder = 1;\n",
    );
    if let Some(size) = tet_size {
        write!(
            geo,
            "Mesh.MeshSizeMin = {size:.17};\nMesh.MeshSizeMax = {size:.17};\n"
        )
        .expect("writing to a String cannot fail");
    }
    geo
}

fn read_mesh(path: &Path, max_bytes: usize) -> Result<Dataset> {
    let bytes = fs::metadata(path)?.len();
    if bytes > u64::try_from(max_bytes).unwrap_or(u64::MAX) {
        return Err(Error::new("E_LIMIT", "Gmsh mesh exceeds max_bytes"));
    }
    let mut dataset = msh::read_projection(&fs::read_to_string(path)?)?.dataset;
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.kind.dimension() == 3 && cell.kind != CellKind::Tet4)
    {
        return Err(Error::new("E_MESHER", "Gmsh emitted non-Tet4 volume cells"));
    }
    dataset
        .mesh
        .cells
        .retain(|cell| cell.kind == CellKind::Tet4);
    if dataset.mesh.cells.is_empty() {
        return Err(Error::new("E_MESHER", "Gmsh emitted no Tet4 cells"));
    }
    dataset.mesh.node_sets.clear();
    dataset.mesh.cell_sets.clear();
    dataset.validate()?;
    Ok(dataset)
}

/// Generate a Tet4 volume whose boundary follows the imported surface geometry.
pub(super) fn tetrahedralize(
    surface: &Mesh,
    options: &Options,
    size: Option<f64>,
) -> Result<Dataset> {
    require_closed_surface(surface)?;
    let work = WorkDir::new()?;
    stl::write_data(
        &Dataset {
            mesh: surface.clone(),
            fields: Vec::new(),
        },
        File::create(work.0.join("surface.stl"))?,
    )?;
    fs::write(work.0.join("mesh.geo"), script(size))?;
    let gmsh = options.gmsh.as_deref().unwrap_or_else(|| Path::new("gmsh"));
    let executable =
        if gmsh.is_relative() && gmsh.parent().is_some_and(|parent| parent != Path::new("")) {
            std::env::current_dir()?.join(gmsh)
        } else {
            gmsh.to_path_buf()
        };
    let output = Command::new(executable)
        .current_dir(&work.0)
        .args([
            "mesh.geo", "-3", "-format", "msh2", "-o", "mesh.msh", "-v", "1",
        ])
        .output()
        .map_err(|error| Error::new("E_MESHER", format!("cannot run Gmsh: {error}")))?;
    if !output.status.success() {
        let detail = if output.stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout)
        } else {
            String::from_utf8_lossy(&output.stderr)
        };
        return Err(Error::new(
            "E_MESHER",
            format!(
                "Gmsh tetrahedralization failed: {}",
                detail.chars().take(1000).collect::<String>()
            ),
        ));
    }
    let mesh = work.0.join("mesh.msh");
    if !mesh.is_file() {
        return Err(Error::new("E_MESHER", "Gmsh did not write a mesh"));
    }
    read_mesh(&mesh, options.max_bytes)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::tetrahedralize;
    use super::{WorkDir, read_mesh, require_closed_surface};
    #[cfg(unix)]
    use crate::conversion::{Format, InfillShape, Options, ReadResult};
    #[cfg(unix)]
    use crate::core::Dataset;
    use crate::core::VoxelGrid;
    #[cfg(unix)]
    use crate::formats::vtu;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    const CUBE_TETS: &str = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n8\n1 0 0 0\n2 1 0 0\n3 1 1 0\n4 0 1 0\n5 0 0 1\n6 1 0 1\n7 1 1 1\n8 0 1 1\n$EndNodes\n$Elements\n6\n1 4 2 1 1 1 2 3 7\n2 4 2 1 1 1 3 4 7\n3 4 2 1 1 1 4 8 7\n4 4 2 1 1 1 8 5 7\n5 4 2 1 1 1 5 6 7\n6 4 2 1 1 1 6 2 7\n$EndElements\n";

    #[test]
    fn rejects_open_surface_before_running_mesher() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let mut surface = grid.surface_mesh().unwrap();
        require_closed_surface(&surface).unwrap();
        surface.cells.pop();
        assert_eq!(require_closed_surface(&surface).unwrap_err().code, "E_MESH");
    }

    #[test]
    fn mesher_output_requires_bounded_tetrahedral_volume() {
        let work = WorkDir::new().unwrap();
        let path = work.0.join("mesh.msh");
        std::fs::write(&path, CUBE_TETS).unwrap();
        assert_eq!(read_mesh(&path, 10).unwrap_err().code, "E_LIMIT");
        let mesh = read_mesh(&path, 10_000).unwrap().mesh;
        assert_eq!(mesh.cells.len(), 6);
        assert_eq!(mesh.points.len(), 8);

        let mixed = include_str!("../../tests/fixtures/gmsh-2.2-mixed.msh");
        std::fs::write(&path, mixed).unwrap();
        assert_eq!(read_mesh(&path, 10_000).unwrap_err().code, "E_MESHER");
        let no_volume = format!(
            "{}$Elements\n0\n$EndElements\n",
            CUBE_TETS.split("$Elements").next().unwrap()
        );
        std::fs::write(&path, no_volume).unwrap();
        assert_eq!(read_mesh(&path, 10_000).unwrap_err().code, "E_MESHER");
    }

    #[cfg(unix)]
    #[test]
    fn mesher_process_contract_keeps_known_cube_volume_and_reports_failures() {
        let work = WorkDir::new().unwrap();
        let executable = work.0.join("gmsh-test");
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let surface = grid.surface_mesh().unwrap();
        let options = Options {
            gmsh: Some(executable.clone()),
            ..Options::default()
        };
        std::fs::write(
            &executable,
            format!("#!/bin/sh\ncat > mesh.msh <<'MSH'\n{CUBE_TETS}MSH\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = tetrahedralize(&surface, &options, Some(0.5)).unwrap();
        assert_eq!(output.mesh.cells.len(), 6);
        let volume: f64 = output
            .mesh
            .cells
            .iter()
            .map(|cell| {
                let [a, b, c, d] =
                    std::array::from_fn(|i| output.mesh.points[cell.connectivity[i]].position);
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let ad = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
                (ab[0] * (ac[1] * ad[2] - ac[2] * ad[1]) - ab[1] * (ac[0] * ad[2] - ac[2] * ad[0])
                    + ab[2] * (ac[0] * ad[1] - ac[1] * ad[0]))
                    .abs()
                    / 6.0
            })
            .sum();
        assert!((volume - 1.0).abs() < 1e-12);

        let source = ReadResult {
            format: Format::Stl,
            dataset: Dataset {
                mesh: surface.clone(),
                fields: Vec::new(),
            },
            voxel_grid: None,
            omissions: Vec::new(),
            generated_point_ids: true,
        };
        let mut bytes = Vec::new();
        let report = crate::conversion::convert(
            source,
            Format::Vtu,
            &Options {
                infill_shape: Some(InfillShape::Tet4),
                infill_unit_size: Some(0.5),
                ..options.clone()
            },
            &mut bytes,
        )
        .unwrap();
        assert!(
            report
                .omissions
                .iter()
                .any(|item| item.detail.contains("conforming Tet4"))
        );
        let reread = vtu::read_projection(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(reread.dataset.mesh.cells.len(), 6);

        std::fs::write(&executable, "#!/bin/sh\necho mesher-failed >&2\nexit 7\n").unwrap();
        assert!(
            tetrahedralize(&surface, &options, None)
                .unwrap_err()
                .message
                .contains("mesher-failed")
        );
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(
            tetrahedralize(&surface, &options, None)
                .unwrap_err()
                .message
                .contains("did not write a mesh")
        );
    }
}
