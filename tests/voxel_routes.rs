//! Public file API checks for voxel inputs, outputs, and rejected metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{InfillShape, Options, convert, preview, validate};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "caexfer-voxel-routes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn grid_input(scratch: &Scratch) -> PathBuf {
    scratch.write(
        "source.vti",
        r#"<?xml version="1.0"?>
<VTKFile type="ImageData" version="0.1" byte_order="LittleEndian">
<ImageData WholeExtent="0 2 0 1 0 1" Origin="2 3 4" Spacing="0.5 0.5 0.5">
<Piece Extent="0 2 0 1 0 1"><PointData/><CellData Scalars="occupancy">
<DataArray type="UInt8" Name="occupancy" format="ascii">1 1</DataArray>
</CellData></Piece></ImageData></VTKFile>"#,
    )
}

/// Measure binary STL independently of caexfer's readers and writers.
fn stl_surface_metrics(path: &Path) -> (f64, [f64; 3], [f64; 3], i64, bool) {
    let bytes = std::fs::read(path).unwrap();
    let triangles = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 84 + triangles * 50);
    let mut vertices = BTreeSet::new();
    let mut edges = BTreeMap::<([u32; 3], [u32; 3]), usize>::new();
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    let mut signed_volume = 0.0;
    for facet in bytes[84..].chunks_exact(50) {
        let raw_points: [[f32; 3]; 3] = std::array::from_fn(|corner| {
            std::array::from_fn(|axis| {
                let offset = 12 + corner * 12 + axis * 4;
                f32::from_le_bytes(facet[offset..offset + 4].try_into().unwrap())
            })
        });
        let points = raw_points.map(|point| point.map(f64::from));
        let keys = raw_points
            .map(|point| point.map(|value| if value == 0.0 { 0 } else { value.to_bits() }));
        for (point, key) in points.iter().zip(keys) {
            vertices.insert(key);
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(point[axis]);
                maximum[axis] = maximum[axis].max(point[axis]);
            }
        }
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let edge = if keys[a] < keys[b] {
                (keys[a], keys[b])
            } else {
                (keys[b], keys[a])
            };
            *edges.entry(edge).or_default() += 1;
        }
        let [a, b, c] = points;
        signed_volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    (
        signed_volume,
        minimum,
        maximum,
        i64::try_from(vertices.len()).unwrap() - i64::try_from(edges.len()).unwrap()
            + i64::try_from(triangles).unwrap(),
        edges.values().all(|&count| count == 2),
    )
}

#[test]
fn donut_voxel_round_trip_preserves_volume_and_torus_topology() {
    let scratch = Scratch::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/donut.stl");
    let occupancy = scratch.path("donut.vti");
    let shell = scratch.path("donut-roundtrip.stl");
    let accepted = Options {
        accept_all: true,
        ..Options::default()
    };
    convert(
        &source,
        &occupancy,
        &Options {
            infill_shape: Some(InfillShape::Hex8),
            infill_unit_size: Some(0.25),
            ..accepted.clone()
        },
    )
    .unwrap();
    convert(&occupancy, &shell, &accepted).unwrap();

    let (original_volume, original_min, original_max, original_euler, original_closed) =
        stl_surface_metrics(&source);
    let (sampled_volume, sampled_min, sampled_max, sampled_euler, sampled_closed) =
        stl_surface_metrics(&shell);
    assert!(
        original_closed && sampled_closed,
        "surface has open or nonmanifold edges"
    );
    assert_eq!(
        (original_euler, sampled_euler),
        (0, 0),
        "torus hole changed"
    );
    assert!(
        original_volume > 0.0 && sampled_volume > 0.0,
        "surface winding changed"
    );
    assert!(
        ((sampled_volume / original_volume) - 1.0).abs() < 0.03,
        "enclosed volume changed by more than 3%"
    );
    for axis in 0..3 {
        assert!((sampled_min[axis] - original_min[axis]).abs() <= 0.25);
        assert!((sampled_max[axis] - original_max[axis]).abs() <= 0.25);
    }
}

#[test]
fn voxel_grid_routes_preserve_geometry_or_report_loss() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let source_report = validate(&source, &Options::default()).unwrap();
    assert_eq!((source_report.points, source_report.cells), (12, 2));
    let options = Options {
        accept_all: true,
        ..Options::default()
    };
    for extension in [
        "vtu", "vtk", "msh", "inp", "bdf", "frd", "stl", "su2", "unv", "exo", "vox",
    ] {
        let output = scratch.path(&format!("result.{extension}"));
        let report = convert(&source, &output, &options)
            .unwrap_or_else(|error| panic!("VTI to {extension}: {error}"));
        assert!(output.is_file(), "{extension}");
        assert_eq!(
            report.cells,
            if extension == "stl" { 20 } else { 2 },
            "{extension}"
        );
        if extension == "vox" {
            assert!(
                report
                    .omissions
                    .iter()
                    .any(|item| item.detail.contains("origin and spacing"))
            );
        }
        if extension != "vox" {
            let reread = validate(&output, &Options::default()).unwrap();
            assert!(reread.cells > 0, "{extension}");
        }
    }
}

#[test]
fn matched_grid_preserves_reference_coordinates_and_occupancy() {
    let scratch = Scratch::new();
    let reference = grid_input(&scratch);
    let shell = scratch.path("shell.stl");
    let accepted = Options {
        accept_all: true,
        ..Options::default()
    };
    convert(&reference, &shell, &accepted).unwrap();
    let output = scratch.path("matched.vti");
    let options = Options {
        match_grid: Some(reference.clone()),
        ..accepted
    };
    let plan = preview(&shell, &output, &options).unwrap();
    let estimate = plan.voxel_estimate.unwrap();
    assert_eq!(
        (estimate.origin, estimate.spacing, estimate.dims),
        ([2.0, 3.0, 4.0], 0.5, [2, 1, 1])
    );
    assert!(!output.exists());
    assert_eq!(
        convert(&shell, &output, &options).unwrap().voxel_grid,
        Some(([2, 1, 1], 2))
    );
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("Origin=\"2 3 4\" Spacing=\"0.5 0.5 0.5\""));
    let values = text
        .split("<DataArray")
        .nth(1)
        .unwrap()
        .split('>')
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap();
    assert_eq!(values.split_whitespace().collect::<Vec<_>>(), ["1", "1"]);

    let invalid = scratch.write("wrong.vox", b"not a VTI reference");
    let rejected = scratch.path("rejected.vti");
    assert_eq!(
        preview(
            &shell,
            &rejected,
            &Options {
                match_grid: Some(invalid),
                ..Options::default()
            }
        )
        .unwrap_err()
        .code,
        "E_USAGE"
    );
    assert!(!rejected.exists());
}

#[test]
fn vti_rejects_unsupported_metadata_through_public_api() {
    let scratch = Scratch::new();
    let canonical = std::fs::read_to_string(grid_input(&scratch)).unwrap();
    let variants = [
        canonical.replace("Spacing=\"0.5 0.5 0.5\"", "Spacing=\"0.5 1 0.5\""),
        canonical.replace("WholeExtent=\"0 2", "WholeExtent=\"1 2"),
        canonical.replace("format=\"ascii\"", "format=\"binary\""),
        canonical.replace("Name=\"occupancy\"", "Name=\"density\""),
        canonical.replace("1 1</DataArray>", "1 1 1</DataArray>"),
    ];
    for (index, text) in variants.iter().enumerate() {
        let path = scratch.write(&format!("bad-{index}.vti"), text);
        assert!(
            validate(&path, &Options::default()).is_err(),
            "variant {index}"
        );
    }
}

#[test]
fn vox_rejects_corrupt_geometry_through_public_api() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let vox = scratch.path("good.vox");
    convert(
        &source,
        &vox,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let original = std::fs::read(&vox).unwrap();
    let mut variants = Vec::new();
    let mut bad = original.clone();
    bad[0] = b'X';
    variants.push(bad);
    let mut bad = original.clone();
    bad[32..36].copy_from_slice(&0_u32.to_le_bytes());
    variants.push(bad);
    let mut bad = original.clone();
    bad[60] = 2;
    variants.push(bad);
    let mut bad = original.clone();
    bad[64..68].copy_from_slice(&original[60..64]);
    variants.push(bad);
    for (index, bytes) in variants.iter().enumerate() {
        let path = scratch.write(&format!("bad-{index}.vox"), bytes);
        assert!(
            validate(&path, &Options::default()).is_err(),
            "variant {index}"
        );
    }
}

#[test]
fn infill_requires_a_mesh_source_and_explicit_size() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let vti_output = scratch.path("copy.vti");
    let error = convert(
        &source,
        &vti_output,
        &Options {
            infill_shape: Some(InfillShape::Hex8),
            infill_unit_size: Some(1.0),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!vti_output.exists());
    let plate = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
    let no_size = convert(
        &plate,
        scratch.path("plate.vti"),
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(no_size.code, "E_USAGE");
}

#[test]
fn stl_to_volume_and_bdf_to_occupancy_round_trip() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let stl = scratch.path("surface.stl");
    convert(
        &source,
        &stl,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let filled = scratch.path("filled.vtu");
    let volume = convert(
        &stl,
        &filled,
        &Options {
            infill_shape: Some(InfillShape::Hex8),
            infill_unit_size: Some(0.5),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(volume.cells, 2);
    assert!(
        volume
            .omissions
            .iter()
            .any(|item| item.detail.contains("Hex8"))
    );

    let bdf = scratch.path("volume.bdf");
    convert(
        &filled,
        &bdf,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let raster = scratch.path("back.vti");
    let report = convert(
        &bdf,
        &raster,
        &Options {
            infill_shape: Some(InfillShape::Hex8),
            infill_unit_size: Some(0.5),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(report.voxel_grid, Some(([2, 1, 1], 2)));
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property IDs"))
    );
}

#[test]
fn bounded_format_mutations_never_panic() {
    let scratch = Scratch::new();
    let vti = grid_input(&scratch);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Binary Exodus/OP2 framing is covered by focused adapter tests. Mutating
    // arbitrary encoded lengths here can request enormous allocations upstream.
    let mut inputs = vec![
        vti.clone(),
        root.join("examples/plate.bdf"),
        root.join("tests/fixtures/gmsh-2.2-mixed.msh"),
        root.join("tests/fixtures/gmsh-six-kind.unv"),
        root.join("tests/fixtures/linear-results.frd"),
        root.join("tests/fixtures/vtk-5.1-mixed.vtk"),
        root.join("tests/fixtures/pch-multiple.pch"),
    ];
    for extension in ["vtu", "stl", "vox", "inp", "su2"] {
        let output = scratch.path(&format!("generated.{extension}"));
        convert(
            &vti,
            &output,
            &Options {
                accept_all: true,
                ..Options::default()
            },
        )
        .unwrap();
        inputs.push(output);
    }
    for (format_index, input) in inputs.iter().enumerate() {
        let bytes = std::fs::read(input).unwrap();
        let extension = input.extension().unwrap().to_str().unwrap();
        let candidate = scratch.path(&format!("mutation-{format_index}.{extension}"));
        let mut options = Options {
            max_bytes: bytes.len() + 16,
            ..Options::default()
        };
        if extension == "pch" {
            options.mesh = Some(root.join("tests/fixtures/pch-companion.bdf"));
            options.subcase = Some(2);
            options.step = Some(1);
        }
        assert!(validate(input, &options).is_ok(), "{extension} baseline");
        let mut mutations = Vec::new();
        for length in [0, 1, bytes.len() / 4, bytes.len() / 2, bytes.len() - 1] {
            mutations.push(bytes[..length].to_vec());
        }
        for position in 0..bytes.len() {
            for replacement in [0, b' ', b'\n', b'0', b'9', 0xff] {
                let mut changed = bytes.clone();
                changed[position] = replacement;
                mutations.push(changed);
            }
        }
        for (mutation_index, changed) in mutations.into_iter().enumerate() {
            std::fs::write(&candidate, changed).unwrap();
            let outcome = std::panic::catch_unwind(|| validate(&candidate, &options));
            let parsed = outcome.unwrap_or_else(|_| {
                panic!("parser panicked on {extension} mutation {mutation_index}")
            });
            if let Err(error) = parsed {
                assert!(error.code.starts_with("E_"), "{extension}: {error}");
            }
        }
    }
}
