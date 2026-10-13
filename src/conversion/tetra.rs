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
pub(super) fn tetrahedralize(surface: &Mesh, options: &Options) -> Result<Dataset> {
    require_closed_surface(surface)?;
    let work = WorkDir::new()?;
    stl::write_data(
        &Dataset {
            mesh: surface.clone(),
            fields: Vec::new(),
        },
        File::create(work.0.join("surface.stl"))?,
    )?;
    fs::write(work.0.join("mesh.geo"), script(options.tet_size))?;
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
    use super::require_closed_surface;
    use crate::core::VoxelGrid;

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
}
