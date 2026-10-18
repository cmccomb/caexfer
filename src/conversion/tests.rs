//! Conversion-engine tests for supported routes, omissions, and assertions.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::conversion::{self, Format, InfillShape, Options, ReadResult, Stage};
use crate::core::{CellSet, Field, FieldLocation, NodeSet};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

/// Temporary fixture directory unique to each test invocation.
struct Scratch(PathBuf);

impl Scratch {
    /// Create an isolated directory without changing the process working dir.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-conversion-{}-{}",
            std::process::id(),
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    /// Persist source bytes under the selected fixture filename.
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    /// Remove generated input and output files after each check.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Fail on the first byte to verify that every destination propagates I/O errors.
struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("injected output failure"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Allow a prefix to reach the adapter, then emulate a full destination.
struct LimitedWriter {
    remaining: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::other("injected partial output failure"));
        }
        let count = bytes.len().min(self.remaining);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Count successful calls and optionally fail at one exact output boundary.
struct CallBoundaryWriter {
    calls: usize,
    fail_on: Option<usize>,
}

impl Write for CallBoundaryWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.fail_on == Some(self.calls) {
            return Err(std::io::Error::other("injected write-call failure"));
        }
        self.calls += 1;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// No format adapter may report success after its destination stream fails.
#[test]
fn every_writable_destination_propagates_stream_failures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plate =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    for target in [
        Format::Bdf,
        Format::Inp,
        Format::Unv,
        Format::Su2,
        Format::Msh,
        Format::Vtu,
        Format::Vtk,
        Format::Frd,
        Format::Exodus,
    ] {
        let error = conversion::convert(
            plate.clone(),
            target,
            &Options::default(),
            &mut FailingWriter,
        )
        .unwrap_err();
        assert_eq!(error.code, "E_IO", "{target:?}: {error}");
    }
    let triangle = crate::formats::su2::read_projection(
        "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n",
    )
    .unwrap()
    .dataset;
    let source = ReadResult {
        format: Format::Su2,
        dataset: triangle,
        voxel_grid: None,
        omissions: Vec::new(),
        generated_point_ids: true,
    };
    let error = conversion::convert(source, Format::Stl, &Options::default(), &mut FailingWriter)
        .unwrap_err();
    assert_eq!(error.code, "E_IO", "STL: {error}");
    let mut displacement = plate;
    displacement.dataset.fields.push(Field {
        name: "DISP".into(),
        location: FieldLocation::Point,
        components: (1..=6).map(|index| format!("C{index}")).collect(),
        values: vec![0.0; displacement.dataset.mesh.points.len() * 6],
        step: Some(2),
        time: Some(0.0),
    });
    let error = conversion::convert(
        displacement,
        Format::Op2,
        &Options::default(),
        &mut FailingWriter,
    )
    .unwrap_err();
    assert_eq!(error.code, "E_IO", "OP2: {error}");
}

/// A writer that accepts a prefix must still fail when the remaining bytes cannot land.
#[test]
fn interrupted_writes_fail_after_partial_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plate =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    for target in [
        Format::Bdf,
        Format::Inp,
        Format::Unv,
        Format::Su2,
        Format::Msh,
        Format::Vtu,
        Format::Vtk,
        Format::Frd,
        Format::Exodus,
    ] {
        let mut complete = Vec::new();
        conversion::convert(plate.clone(), target, &Options::default(), &mut complete).unwrap();
        assert!(complete.len() > 1, "{target:?}");
        for remaining in [1, complete.len() / 2, complete.len() - 1] {
            let error = conversion::convert(
                plate.clone(),
                target,
                &Options::default(),
                &mut LimitedWriter { remaining },
            )
            .unwrap_err();
            assert_eq!(
                error.code, "E_IO",
                "{target:?} after {remaining} bytes: {error}"
            );
        }
    }
}

/// Each write call is an independent possible failure for streaming adapters.
#[test]
fn every_streaming_write_boundary_propagates_failure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plate =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    for target in [
        Format::Bdf,
        Format::Inp,
        Format::Unv,
        Format::Su2,
        Format::Msh,
        Format::Vtu,
        Format::Vtk,
        Format::Frd,
        Format::Exodus,
    ] {
        let mut counter = CallBoundaryWriter {
            calls: 0,
            fail_on: None,
        };
        conversion::convert(plate.clone(), target, &Options::default(), &mut counter).unwrap();
        assert!(counter.calls > 0, "{target:?}");
        for fail_on in 0..counter.calls {
            let mut writer = CallBoundaryWriter {
                calls: 0,
                fail_on: Some(fail_on),
            };
            let error =
                conversion::convert(plate.clone(), target, &Options::default(), &mut writer)
                    .unwrap_err();
            assert_eq!(
                error.code, "E_IO",
                "{target:?} write call {fail_on}: {error}"
            );
        }
    }
}

/// Result fields exercise additional writer calls beyond geometry headers.
#[test]
fn result_series_writers_propagate_each_stream_failure() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exodus-two-blocks.exo");
    let all = conversion::read_path(&source, &Options::default()).unwrap();
    let first = conversion::read_path(
        &source,
        &Options {
            step: Some(0),
            ..Options::default()
        },
    )
    .unwrap();
    for (read, target) in [
        (all, Format::Exodus),
        (first.clone(), Format::Msh),
        (first.clone(), Format::Vtu),
        (first.clone(), Format::Vtk),
        (first, Format::Frd),
    ] {
        let mut counter = CallBoundaryWriter {
            calls: 0,
            fail_on: None,
        };
        conversion::convert(read.clone(), target, &Options::default(), &mut counter).unwrap();
        for fail_on in 0..counter.calls {
            let mut writer = CallBoundaryWriter {
                calls: 0,
                fail_on: Some(fail_on),
            };
            let error = conversion::convert(read.clone(), target, &Options::default(), &mut writer)
                .unwrap_err();
            assert_eq!(
                error.code, "E_IO",
                "{target:?} result write call {fail_on}: {error}"
            );
        }
    }
}

/// Refuse INP mesh generators that would add unparsed geometry.
#[test]
fn inp_generators_fail_instead_of_projecting_incomplete_geometry() {
    for generator in ["*NGEN\n1,3,1\n", "*ELGEN\n10,3,1\n"] {
        let input = format!("*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n{generator}");
        let error = crate::formats::inp::read(&input).unwrap_err();
        assert_eq!(error.code, "E_INP");
        assert!(error.message.contains("expanded geometry"));
    }
}

/// Reject huge MSH counts before unsafe allocation or overflow.
#[test]
fn msh_declared_counts_cannot_allocate_beyond_input() {
    let huge_nodes = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n1 1 1 1\n3 1 0 18446744073709551615\n$EndNodes\n$Elements\n0 0 0 0\n$EndElements\n";
    assert_eq!(
        crate::formats::msh::read(huge_nodes).unwrap_err().code,
        "E_MSH"
    );

    let huge_elements = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n0 0 0 0\n$EndNodes\n$Elements\n1 1 1 1\n1 1 1 18446744073709551615\n$EndElements\n";
    assert_eq!(
        crate::formats::msh::read(huge_elements).unwrap_err().code,
        "E_MSH"
    );
}

/// Return source and destination omissions alongside converted geometry.
#[test]
fn conversion_reports_source_and_destination_losses() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
    let mut output = Vec::new();
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    let report = conversion::convert(read, Format::Msh, &Options::default(), &mut output)
        .expect("BDF mesh projects to MSH");
    assert_eq!((report.points, report.cells, report.fields), (4, 1, 0));
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.stage == Stage::Source)
    );
    assert!(
        report.omissions.iter().any(|item| {
            item.stage == Stage::Destination && item.detail.contains("property IDs")
        })
    );
    let dataset = crate::formats::msh::read(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(
        (dataset.mesh.points.len(), dataset.mesh.cells.len()),
        (4, 1)
    );
}

/// Require a basic-frame assertion when the companion lacks GRID CD.
#[test]
fn op2_non_bdf_companion_requires_explicit_frame_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = Options {
        mesh: Some(root.join("tests/fixtures/linear-results.frd")),
        ..Options::default()
    };
    let error = conversion::read_path(&root.join("tests/fixtures/solid_bending.op2"), &options)
        .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("Options.assume_basic_frame"));
}

/// Require companion identity and result selection for PCH input.
#[test]
fn pch_read_requires_matching_mesh_and_explicit_selection() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/pch-multiple.pch");
    assert_eq!(Format::from_input_path(&source).unwrap(), Format::Pch);
    assert_eq!(
        conversion::read_path(&source, &Options::default())
            .unwrap_err()
            .code,
        "E_USAGE"
    );
    let mut options = Options {
        mesh: Some(root.join("tests/fixtures/pch-companion.bdf")),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_PCH"
    );
    options.subcase = Some(2);
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_PCH"
    );
    options.step = Some(1);
    let read = conversion::read_path(&source, &options).unwrap();
    assert_eq!(
        (read.dataset.mesh.points.len(), read.dataset.fields.len()),
        (2, 1)
    );
    assert_eq!(read.dataset.fields[0].time, Some(0.5));
    assert!((read.dataset.fields[0].values[0] - 3.0).abs() < f64::EPSILON);
    assert_eq!(
        conversion::convert(read, Format::Pch, &Options::default(), &mut Vec::new())
            .unwrap_err()
            .code,
        "E_FORMAT"
    );
}

/// Apply the same non-BDF frame rule to PCH displacement input.
#[test]
fn pch_non_bdf_companion_requires_basic_frame_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/pch-multiple.pch");
    let companion = root.join("tests/fixtures/pch-companion.bdf");
    let mesh = conversion::read_path(&companion, &Options::default())
        .unwrap()
        .dataset
        .mesh;
    let folder = std::env::temp_dir().join(format!("caexfer-pch-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("mesh.vtu");
    let file = std::fs::File::create(&path).unwrap();
    crate::formats::vtu::write(&mesh, file).unwrap();
    let mut options = Options {
        mesh: Some(path.clone()),
        subcase: Some(1),
        ..Options::default()
    };
    let error = conversion::read_path(&source, &options).unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("Options.assume_basic_frame"));
    options.assume_basic_frame = true;
    let read = conversion::read_path(&source, &options).unwrap();
    assert_eq!(read.dataset.fields.len(), 1);
    assert!(
        read.omissions
            .iter()
            .any(|item| item.stage == Stage::Assumption)
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(folder).unwrap();
}

/// Expose readable and writable format capabilities without guessing.
#[test]
fn format_names_and_output_capabilities_are_explicit() {
    assert_eq!(Format::parse("PCH").unwrap(), Format::Pch);
    assert_eq!(Format::parse("not-a-format").unwrap_err().code, "E_FORMAT");
    for format in Format::ALL {
        assert_eq!(Format::parse(format.name()).unwrap(), format);
        let filename = format!("mesh.{}", format.name());
        assert_eq!(
            Format::from_input_path(Path::new(&filename)).unwrap(),
            format
        );
        assert_eq!(
            Format::from_output_path(Path::new(&filename)).is_ok(),
            format != Format::Pch
        );
    }
    assert_eq!(
        Format::from_input_path(Path::new("unknown.mesh"))
            .unwrap_err()
            .code,
        "E_FORMAT"
    );
    for extension in ["pch", "dat", "unknown"] {
        assert_eq!(
            Format::from_output_path(Path::new(&format!("result.{extension}")))
                .unwrap_err()
                .code,
            "E_FORMAT"
        );
    }
}

/// Report STL-generated IDs and reject volume cells on export.
#[test]
fn stl_route_reports_missing_identity_and_refuses_volume_export() {
    let scratch = Scratch::new();
    let source = scratch.write(
        "surface.stl",
        b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid sample\n",
    );
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    assert_eq!(read.format, Format::Stl);
    assert_eq!(read.dataset.mesh.cells.len(), 1);
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("facet-local IDs"))
    );
    let mut output = Vec::new();
    let report = conversion::convert(read, Format::Stl, &Options::default(), &mut output).unwrap();
    assert_eq!(output.len(), 134);
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.stage == Stage::Destination)
    );

    let volume = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-linear.bdf");
    let report = conversion::convert(
        conversion::read_path(&volume, &Options::default()).unwrap(),
        Format::Stl,
        &Options::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(report.cells, 16);
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("lower-dimensional"))
    );
}

/// Exercise the full format dispatcher through both voxel formats and STL.
#[test]
fn voxel_formats_bridge_surface_and_volume_meshes() {
    use crate::core::{Dataset, VoxelGrid};
    let scratch = Scratch::new();
    let grid = VoxelGrid {
        origin: [0.0; 3],
        spacing: 1.0,
        dims: [2, 1, 1],
        occupied: vec![1, 1],
    };
    let surface = Dataset {
        mesh: grid.surface_mesh().unwrap(),
        fields: Vec::new(),
    };
    let mut stl = Vec::new();
    crate::formats::stl::write_data(&surface, &mut stl).unwrap();
    let stl_path = scratch.write("blocks.stl", stl);
    let mut vti = Vec::new();
    let options = Options {
        infill_shape: Some(InfillShape::Hex8),
        infill_unit_size: Some(1.0),
        ..Options::default()
    };
    let report = conversion::convert(
        conversion::read_path(&stl_path, &options).unwrap(),
        Format::Vti,
        &options,
        &mut vti,
    )
    .unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("voxel centers"))
    );
    assert_eq!(report.voxel_grid, Some(([2, 1, 1], 2)));
    let vti_path = scratch.write("blocks.vti", vti);
    let vti_read = conversion::read_path(&vti_path, &Options::default()).unwrap();
    assert_eq!(vti_read.voxel_grid.as_ref(), Some(&grid));
    assert_eq!(vti_read.dataset.mesh.cells.len(), 2);

    let mut vox = Vec::new();
    conversion::convert(vti_read.clone(), Format::Vox, &Options::default(), &mut vox).unwrap();
    let vox_path = scratch.write("blocks.vox", vox);
    let vox_read = conversion::read_path(&vox_path, &Options::default()).unwrap();
    assert_eq!(vox_read.voxel_grid.as_ref(), Some(&grid));
    assert!(
        vox_read
            .omissions
            .iter()
            .any(|item| item.detail.contains("color indices"))
    );
    let mut reconstructed_stl = Vec::new();
    conversion::convert(
        vox_read,
        Format::Stl,
        &Options::default(),
        &mut reconstructed_stl,
    )
    .unwrap();
    assert_eq!(
        crate::formats::stl::read_projection(&reconstructed_stl)
            .unwrap()
            .dataset
            .mesh
            .cells
            .len(),
        20
    );

    let mut vtu_bytes = Vec::new();
    conversion::convert(vti_read, Format::Vtu, &Options::default(), &mut vtu_bytes).unwrap();
    assert_eq!(
        crate::formats::vtu::read_projection(std::str::from_utf8(&vtu_bytes).unwrap())
            .unwrap()
            .dataset
            .mesh
            .cells
            .len(),
        2
    );
}

/// Smoothing moves voxel surface vertices while retaining triangle topology.
#[test]
fn voxel_stl_smoothing_is_opt_in_and_route_limited() {
    use crate::core::VoxelGrid;
    let scratch = Scratch::new();
    let grid = VoxelGrid {
        origin: [0.0; 3],
        spacing: 1.0,
        dims: [2, 1, 1],
        occupied: vec![1, 1],
    };
    let mut vti = Vec::new();
    crate::formats::vti::write(&grid, &mut vti).unwrap();
    let source =
        conversion::read_path(&scratch.write("blocks.vti", vti), &Options::default()).unwrap();
    let mut original = Vec::new();
    conversion::convert(
        source.clone(),
        Format::Stl,
        &Options::default(),
        &mut original,
    )
    .unwrap();
    let options = Options {
        smooth_iterations: 10,
        ..Options::default()
    };
    let mut smoothed = Vec::new();
    let report = conversion::convert(source.clone(), Format::Stl, &options, &mut smoothed).unwrap();
    assert_ne!(smoothed, original);
    assert_eq!(report.cells, 20);
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("smoothed for 10"))
    );
    assert_eq!(
        crate::formats::stl::read_projection(&smoothed)
            .unwrap()
            .dataset
            .mesh
            .cells
            .len(),
        20
    );
    assert_eq!(
        conversion::convert(source, Format::Vtu, &options, &mut Vec::new())
            .unwrap_err()
            .code,
        "E_USAGE"
    );
}

/// Exercise voxel routes from a physical volume and back from its STL shell.
#[test]
fn volume_and_stl_voxel_routes_report_geometry_and_field_losses() {
    use crate::core::{Dataset, VoxelGrid};
    let grid = VoxelGrid {
        origin: [1.0, 2.0, 3.0],
        spacing: 0.5,
        dims: [1, 1, 1],
        occupied: vec![1],
    };
    let mut volume = grid.volume_mesh().unwrap();
    volume.cells[0].property_id = Some(9);
    let source = ReadResult {
        format: Format::Vtu,
        dataset: Dataset {
            mesh: volume,
            fields: vec![Field {
                name: "stress".into(),
                location: FieldLocation::Cell,
                components: vec!["C1".into()],
                values: vec![3.0],
                step: None,
                time: None,
            }],
        },
        voxel_grid: None,
        omissions: Vec::new(),
        generated_point_ids: false,
    };
    let options = Options {
        infill_shape: Some(InfillShape::Hex8),
        infill_unit_size: Some(0.5),
        ..Options::default()
    };
    let mut vti = Vec::new();
    let report = conversion::convert(source.clone(), Format::Vti, &options, &mut vti).unwrap();
    assert_eq!(report.voxel_grid, Some(([1, 1, 1], 1)));
    assert_eq!(
        crate::formats::vti::read(std::str::from_utf8(&vti).unwrap()).unwrap(),
        grid
    );
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property IDs"))
    );
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("numeric field"))
    );

    let mut vox = Vec::new();
    let vox_report = conversion::convert(source.clone(), Format::Vox, &options, &mut vox).unwrap();
    assert_eq!(
        crate::formats::vox::read(&vox).unwrap().grid.occupied,
        vec![1]
    );
    assert!(
        vox_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("physical origin"))
    );

    let mut stl = Vec::new();
    let stl_report =
        conversion::convert(source.clone(), Format::Stl, &Options::default(), &mut stl).unwrap();
    assert_eq!(stl_report.cells, 12);
    assert!(
        stl_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("external volume faces"))
    );
    assert_eq!(
        conversion::convert(source, Format::Vti, &Options::default(), &mut Vec::new())
            .unwrap_err()
            .code,
        "E_USAGE"
    );

    let scratch = Scratch::new();
    let stl_path = scratch.write("cube.stl", stl);
    let stl_source = conversion::read_path(&stl_path, &Options::default()).unwrap();
    let mut volume_bytes = Vec::new();
    let volume_report =
        conversion::convert(stl_source, Format::Vtu, &options, &mut volume_bytes).unwrap();
    assert_eq!(volume_report.cells, 1);
    assert!(
        volume_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("Hex8 volume"))
    );
    let restored =
        crate::formats::vtu::read_projection(std::str::from_utf8(&volume_bytes).unwrap()).unwrap();
    assert_eq!(
        restored.dataset.mesh.cells[0].kind,
        crate::core::CellKind::Hex8
    );
}

/// General mesh formats can exchange an extracted shell and a voxelized volume.
#[test]
#[allow(clippy::too_many_lines)] // One round trip checks geometry and field behavior together.
fn mesh_surface_and_volume_routes_preserve_only_mappable_fields() {
    use crate::core::{Dataset, VoxelGrid};
    let grid = VoxelGrid {
        origin: [0.0; 3],
        spacing: 1.0,
        dims: [2, 2, 2],
        occupied: vec![1; 8],
    };
    let mut mesh = grid.volume_mesh().unwrap();
    mesh.cells[0].property_id = Some(7);
    mesh.node_sets.push(NodeSet {
        name: "edge".into(),
        point_ids: vec![mesh.points[0].id],
    });
    let source = ReadResult {
        format: Format::Vtu,
        dataset: Dataset {
            fields: vec![
                Field {
                    name: "temperature".into(),
                    location: FieldLocation::Point,
                    components: vec!["C1".into()],
                    values: mesh
                        .points
                        .iter()
                        .map(|point| f64::from(u32::try_from(point.id).unwrap()))
                        .collect(),
                    step: None,
                    time: None,
                },
                Field {
                    name: "stress".into(),
                    location: FieldLocation::Cell,
                    components: vec!["C1".into()],
                    values: vec![3.0; mesh.cells.len()],
                    step: None,
                    time: None,
                },
            ],
            mesh,
        },
        voxel_grid: None,
        omissions: Vec::new(),
        generated_point_ids: false,
    };
    let mut shell_bytes = Vec::new();
    let shell_report = conversion::convert(
        source,
        Format::Vtu,
        &Options {
            surface: true,
            ..Options::default()
        },
        &mut shell_bytes,
    )
    .unwrap();
    assert_eq!(shell_report.fields, 2);
    assert_eq!(shell_report.cells, 48);
    assert_eq!(shell_report.points, 26);
    assert!(
        shell_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("cell field"))
    );
    assert!(
        shell_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property ID"))
    );
    let shell = crate::formats::vtu::read_projection(std::str::from_utf8(&shell_bytes).unwrap())
        .unwrap()
        .dataset;
    assert_eq!(shell.fields.len(), 1);
    assert_eq!(shell.fields[0].name, "temperature");
    assert_eq!(shell.fields[0].values.len(), 26);
    for (point, value) in shell.mesh.points.iter().zip(&shell.fields[0].values) {
        assert!((f64::from(u32::try_from(point.id).unwrap()) - value).abs() < 1e-12);
    }
    assert!(
        shell
            .mesh
            .cells
            .iter()
            .all(|cell| cell.kind == crate::core::CellKind::Triangle3)
    );

    let mut volume_bytes = Vec::new();
    let volume_report = conversion::convert(
        ReadResult {
            format: Format::Vtu,
            dataset: shell,
            voxel_grid: None,
            omissions: Vec::new(),
            generated_point_ids: false,
        },
        Format::Vtu,
        &Options {
            infill_shape: Some(InfillShape::Hex8),
            infill_unit_size: Some(1.0),
            ..Options::default()
        },
        &mut volume_bytes,
    )
    .unwrap();
    assert_eq!(volume_report.cells, 8);
    assert_eq!(volume_report.fields, 1);
    assert!(
        volume_report
            .omissions
            .iter()
            .any(|item| item.detail.contains("numeric field"))
    );
    let volume = crate::formats::vtu::read_projection(std::str::from_utf8(&volume_bytes).unwrap())
        .unwrap()
        .dataset;
    assert_eq!(volume.fields, Vec::new());
    assert!(
        volume
            .mesh
            .cells
            .iter()
            .all(|cell| cell.kind == crate::core::CellKind::Hex8)
    );
}

/// Each supported linear volume cell can project to a triangle-only shell.
#[test]
fn surface_extraction_handles_all_volume_cell_kinds() {
    use crate::core::CellKind;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-linear.bdf");
    let source = conversion::read_path(&path, &Options::default()).unwrap();
    for (kind, expected_faces) in [
        (CellKind::Tet4, 4),
        (CellKind::Hex8, 12),
        (CellKind::Wedge6, 8),
        (CellKind::Pyramid5, 6),
    ] {
        let mut selected = source.clone();
        selected.dataset.mesh.cells.retain(|cell| cell.kind == kind);
        let mut bytes = Vec::new();
        let report = conversion::convert(
            selected,
            Format::Vtu,
            &Options {
                surface: true,
                ..Options::default()
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(report.cells, expected_faces, "{kind:?}");
        assert_eq!(report.points, kind.node_count(), "{kind:?}");
        let projected = crate::formats::vtu::read_projection(std::str::from_utf8(&bytes).unwrap())
            .unwrap()
            .dataset;
        assert!(
            projected
                .mesh
                .cells
                .iter()
                .all(|cell| cell.kind == CellKind::Triangle3)
        );
    }
}

/// Carry a named Gmsh boundary through SU2 marker output.
#[test]
fn named_msh_boundary_survives_su2_conversion() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/named-boundary.msh");
    let mut output = Vec::new();
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    let report = conversion::convert(read, Format::Su2, &Options::default(), &mut output).unwrap();
    assert_eq!((report.points, report.cells), (3, 2));
    let decoded =
        crate::formats::su2::read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(decoded.dataset.mesh.cell_sets.len(), 2);
    assert_eq!(decoded.dataset.mesh.cell_sets[0].name, "inlet");
    assert_eq!(decoded.dataset.mesh.cell_sets[1].name, "outer wall");

    let scratch = Scratch::new();
    let path = scratch.write("converted.su2", &output);
    let read = conversion::read_path(&path, &Options::default()).unwrap();
    let mut vtu = Vec::new();
    let report = conversion::convert(read, Format::Vtu, &Options::default(), &mut vtu).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("named cell set"))
    );
}

/// Report UNV datasets omitted from geometry projection.
#[test]
fn unv_geometry_route_reports_native_omissions() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmsh-six-kind.unv");
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    assert_eq!(read.format, Format::Unv);
    assert_eq!(
        (
            read.dataset.mesh.points.len(),
            read.dataset.mesh.cells.len()
        ),
        (9, 6)
    );
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("2477"))
    );
    let mut output = Vec::new();
    let report = conversion::convert(read, Format::Unv, &Options::default(), &mut output).unwrap();
    assert_eq!((report.points, report.cells), (9, 6));
    let decoded =
        crate::formats::unv::read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(decoded.dataset.mesh.cells[5].id, 6);

    let pyramid = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmsh-2.2-mixed.msh");
    assert_eq!(
        conversion::convert(
            conversion::read_path(&pyramid, &Options::default()).unwrap(),
            Format::Unv,
            &Options::default(),
            &mut Vec::new(),
        )
        .unwrap_err()
        .code,
        "E_UNV"
    );
}

/// Carry complete Exodus scalar fields and report block semantics.
#[test]
fn classic_exodus_route_preserves_scalar_results_and_reports_blocks() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exodus-two-blocks.exo");
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    assert_eq!(read.format, Format::Exodus);
    assert_eq!(
        (
            read.dataset.mesh.points.len(),
            read.dataset.mesh.cells.len(),
            read.dataset.fields.len()
        ),
        (4, 2, 4)
    );
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("block ID"))
    );
    let mut output = Vec::new();
    let report =
        conversion::convert(read, Format::Exodus, &Options::default(), &mut output).unwrap();
    assert_eq!(report.fields, 4);
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("generated from cell topology"))
    );
    let reread = crate::formats::exodus::read_projection(&output, Some(1)).unwrap();
    assert_eq!(reread.dataset.fields.len(), 2);
    assert_eq!(reread.dataset.fields[1].values, [20.0, 21.0]);
}

/// Receipt fields, property IDs, and named sets lost by geometry outputs.
#[test]
fn geometry_outputs_receipt_fields_properties_and_group_losses() {
    // Enrich one real boundary fixture with every value that geometry-only
    // targets cannot all retain, then inspect each destination receipt.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/named-boundary.msh");
    let mut read = conversion::read_path(&source, &Options::default()).unwrap();
    let first_id = read.dataset.mesh.points[0].id;
    let surface = read
        .dataset
        .mesh
        .cells
        .iter()
        .find(|cell| cell.kind.dimension() == 2)
        .unwrap()
        .id;
    read.dataset.mesh.node_sets.push(NodeSet {
        name: "pin".into(),
        point_ids: vec![first_id],
    });
    read.dataset.mesh.cell_sets.push(CellSet {
        name: "interior".into(),
        dimension: 2,
        cell_ids: vec![surface],
    });
    read.dataset
        .mesh
        .cells
        .iter_mut()
        .find(|cell| cell.id == surface)
        .unwrap()
        .property_id = Some(17);
    read.dataset.fields.push(Field {
        name: "TEMPERATURE".into(),
        location: FieldLocation::Point,
        components: vec!["C1".into()],
        values: vec![1.0; read.dataset.mesh.points.len()],
        step: Some(1),
        time: Some(0.25),
    });
    let mut su2 = Vec::new();
    let report =
        conversion::convert(read.clone(), Format::Su2, &Options::default(), &mut su2).unwrap();
    for phrase in [
        "numeric field",
        "node set",
        "not SU2 boundary",
        "property IDs",
        "positional connectivity",
    ] {
        assert!(
            report
                .omissions
                .iter()
                .any(|item| item.detail.contains(phrase)),
            "missing {phrase}"
        );
    }
    assert_eq!(
        crate::formats::su2::read_projection(std::str::from_utf8(&su2).unwrap())
            .unwrap()
            .dataset
            .mesh
            .cell_sets
            .len(),
        2
    );

    let mut unv = Vec::new();
    let report = conversion::convert(read, Format::Unv, &Options::default(), &mut unv).unwrap();
    for phrase in [
        "named node set",
        "named cell set",
        "numeric field",
        "property ID",
    ] {
        assert!(
            report
                .omissions
                .iter()
                .any(|item| item.detail.contains(phrase)),
            "missing {phrase}"
        );
    }
    assert_eq!(
        crate::formats::unv::read_projection(std::str::from_utf8(&unv).unwrap())
            .unwrap()
            .dataset
            .mesh
            .cells
            .len(),
        2
    );
}

/// Report destination limits when exporting Exodus or STL.
#[test]
fn classic_exodus_and_stl_report_destination_projection() {
    let stl = b"solid surface\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid surface\n";
    let mut read = ReadResult {
        format: Format::Stl,
        dataset: crate::formats::stl::read_projection(stl).unwrap().dataset,
        voxel_grid: None,
        omissions: Vec::new(),
        generated_point_ids: true,
    };
    read.dataset.mesh.cells[0].property_id = Some(7);
    read.dataset.fields.push(Field {
        name: "TEMP".into(),
        location: FieldLocation::Point,
        components: vec!["C1".into()],
        values: vec![1.0, 2.0, 3.0],
        step: Some(1),
        time: Some(0.0),
    });
    let mut binary = Vec::new();
    let report =
        conversion::convert(read.clone(), Format::Stl, &Options::default(), &mut binary).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("numeric field"))
    );
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property IDs"))
    );
    assert_eq!(binary.len(), 134);

    let mut classic = Vec::new();
    let report =
        conversion::convert(read, Format::Exodus, &Options::default(), &mut classic).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property ID"))
    );
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("block IDs are generated"))
    );
    let projected = crate::formats::exodus::read_projection(&classic, None)
        .unwrap()
        .dataset;
    assert_eq!(projected.mesh.cells[0].id, 1);
    assert_eq!(projected.fields[0].values, [1.0, 2.0, 3.0]);
}

/// Include ignored STL attributes and result metadata in receipts.
#[test]
fn source_receipts_include_binary_stl_attributes_and_result_titles() {
    let scratch = Scratch::new();
    let ascii = b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid sample\n";
    let dataset = crate::formats::stl::read_projection(ascii).unwrap().dataset;
    let mut binary = Vec::new();
    crate::formats::stl::write_data(&dataset, &mut binary).unwrap();
    binary[132] = 1;
    let stl = scratch.write("attributes.stl", binary);
    let read = conversion::read_path(&stl, &Options::default()).unwrap();
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("attribute bytes"))
    );

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let titled = format!(
        "$TITLE = trial\n{}",
        std::fs::read_to_string(root.join("tests/fixtures/pch-multiple.pch")).unwrap()
    );
    let pch = scratch.write("titled.pch", titled);
    let options = Options {
        mesh: Some(root.join("tests/fixtures/pch-companion.bdf")),
        subcase: Some(1),
        ..Options::default()
    };
    let read = conversion::read_path(&pch, &options).unwrap();
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("title, subtitle"))
    );
}

/// Reject fields in direct geometry writers while conversion reports drops.
#[test]
fn geometry_only_writers_reject_fields_while_conversion_reports_them() {
    // Direct writers reject an unrepresentable dataset; the conversion layer
    // may project it only while explicitly reporting the dropped field.
    let inputs = [
        (
            Format::Stl,
            "solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid s\n",
        ),
        (
            Format::Su2,
            "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n",
        ),
    ];
    for (format, input) in inputs {
        let mut dataset = match format {
            Format::Stl => {
                crate::formats::stl::read_projection(input.as_bytes())
                    .unwrap()
                    .dataset
            }
            Format::Su2 => crate::formats::su2::read_projection(input).unwrap().dataset,
            _ => unreachable!(),
        };
        dataset.fields.push(Field {
            name: "TEMPERATURE".into(),
            location: FieldLocation::Point,
            components: vec!["C1".into()],
            values: vec![1.0; dataset.mesh.points.len()],
            step: None,
            time: None,
        });
        assert_eq!(
            match format {
                Format::Stl => crate::formats::stl::write_data(&dataset, Vec::new()),
                Format::Su2 => crate::formats::su2::write_data(&dataset, Vec::new()),
                _ => unreachable!(),
            }
            .unwrap_err()
            .code,
            match format {
                Format::Stl => "E_STL",
                Format::Su2 => "E_SU2",
                _ => unreachable!(),
            }
        );
        let mut output = Vec::new();
        let report = conversion::convert(
            ReadResult {
                format,
                dataset,
                voxel_grid: None,
                omissions: Vec::new(),
                generated_point_ids: matches!(format, Format::Stl | Format::Su2),
            },
            format,
            &Options::default(),
            &mut output,
        )
        .unwrap();
        assert_eq!(report.fields, 1);
        assert!(report.omissions.iter().any(|item| {
            item.stage == Stage::Destination && item.detail.contains("numeric field")
        }));
        assert_ne!(output.len(), 0);
    }
}

/// Reject unmapped properties directly and report them in conversion.
#[test]
fn writers_reject_unmapped_properties_while_conversion_reports_them() {
    // The direct writer and the converter have intentionally different
    // contracts for the same unsupported property ID.
    let stl = crate::formats::stl::read_projection(b"solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid s\n")
        .unwrap()
        .dataset;
    let su2 = crate::formats::su2::read_projection(
        "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n",
    )
    .unwrap()
    .dataset;
    let frd = crate::formats::frd::read(include_bytes!("../../tests/fixtures/linear-results.frd"))
        .unwrap();
    for (format, mut dataset) in [(Format::Stl, stl), (Format::Su2, su2), (Format::Frd, frd)] {
        dataset.mesh.cells[0].property_id = Some(17);
        let error = match format {
            Format::Stl => crate::formats::stl::write_data(&dataset, Vec::new()),
            Format::Su2 => crate::formats::su2::write_data(&dataset, Vec::new()),
            Format::Frd => crate::formats::frd::write(&dataset, Vec::new()),
            _ => unreachable!(),
        }
        .unwrap_err();
        assert!(error.message.contains("property IDs"));
        let mut output = Vec::new();
        let report = conversion::convert(
            ReadResult {
                format,
                dataset,
                voxel_grid: None,
                omissions: Vec::new(),
                generated_point_ids: matches!(format, Format::Stl | Format::Su2),
            },
            format,
            &Options::default(),
            &mut output,
        )
        .unwrap();
        assert!(report.omissions.iter().any(|item| {
            item.stage == Stage::Destination && item.detail.contains("property IDs")
        }));
        assert_ne!(output.len(), 0);
    }
}

/// Refuse incompatible result frames and invalid FRD step selections.
#[test]
fn result_companion_frames_and_frd_steps_fail_explicitly() {
    let scratch = Scratch::new();
    let companion = scratch.write(
        "nonbasic.bdf",
        "GRID,10,,0,0,0,42\nGRID,20,,1,0,0\nCROD,30,1,10,20\n",
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pch = root.join("tests/fixtures/pch-multiple.pch");
    let options = Options {
        mesh: Some(companion),
        subcase: Some(1),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&pch, &options).unwrap_err().code,
        "E_PCH"
    );
    let options = Options {
        mesh: Some(root.join("tests/fixtures/solid_bending.op2")),
        subcase: Some(1),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&pch, &options).unwrap_err().code,
        "E_USAGE"
    );

    let frd_source = root.join("tests/fixtures/linear-results.frd");
    let mut dataset = crate::formats::frd::read(&std::fs::read(&frd_source).unwrap()).unwrap();
    for field in &mut dataset.fields {
        field.step = Some(1);
    }
    let mut encoded = Vec::new();
    crate::formats::frd::write(&dataset, &mut encoded).unwrap();
    let frd = scratch.write("step.frd", encoded);
    let options = Options {
        step: Some(1),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&frd, &options)
            .unwrap()
            .dataset
            .fields
            .len(),
        2
    );
    let options = Options {
        step: Some(99),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&frd, &options).unwrap_err().code,
        "E_FRD"
    );
}

/// Reject result-only options on ordinary mesh inputs.
#[test]
fn result_options_cannot_change_unrelated_mesh_readers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/plate.bdf");
    for options in [
        Options {
            subcase: Some(1),
            ..Options::default()
        },
        Options {
            assume_basic_frame: true,
            ..Options::default()
        },
        Options {
            step: Some(1),
            ..Options::default()
        },
    ] {
        assert_eq!(
            conversion::read_path(&source, &options).unwrap_err().code,
            "E_USAGE"
        );
    }
}

/// Enforce file limits and text decoding before conversion starts.
#[test]
fn bounded_reads_and_bad_encodings_fail_before_projection() {
    let s = Scratch::new();
    let source = s.write("large.vtu", vec![b'x'; 80]);
    let options = Options {
        max_bytes: 10,
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_LIMIT"
    );
    for (extension, code) in [
        ("vtu", "E_VTU"),
        ("vtk", "E_VTK"),
        ("msh", "E_MSH"),
        ("inp", "E_INP"),
    ] {
        let path = s.write(&format!("invalid.{extension}"), [0xff, 0xfe]);
        assert_eq!(
            conversion::read_path(&path, &Options::default())
                .unwrap_err()
                .code,
            code
        );
    }
}

/// Report assigned identity and ignored native sections from external files.
#[test]
fn external_identity_and_ignored_sections_are_visible_in_receipts() {
    let s = Scratch::new();
    let vtk = s.write(
        "external.vtk",
        b"# vtk DataFile Version 2.0\nexternal\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 float\n0 0 0 1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\n",
    );
    let read = conversion::read_path(&vtk, &Options::default()).unwrap();
    assert_eq!(read.omissions.len(), 2);
    assert!(
        read.omissions
            .iter()
            .all(|item| item.stage == Stage::Source)
    );
    let msh = s.write(
        "tagged.msh",
        b"$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$PhysicalNames\n0\n$EndPhysicalNames\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 2 7 8 10 20\n$EndElements\n",
    );
    let read = conversion::read_path(&msh, &Options::default()).unwrap();
    assert_eq!(read.dataset.mesh.cell_sets[0].name, "physical_1_7");
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("tag list"))
    );

    let pch = s.write(
        "with-other-result.pch",
        format!(
            "$STRESSES\n10 G 1 2 3\n{}",
            include_str!("../../tests/fixtures/pch-multiple.pch")
        ),
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = Options {
        mesh: Some(root.join("tests/fixtures/pch-companion.bdf")),
        subcase: Some(1),
        ..Options::default()
    };
    let read = conversion::read_path(&pch, &options).unwrap();
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("other PCH"))
    );
}

/// Explain fields that cannot be mapped unambiguously to a destination.
#[test]
fn destination_reports_ambiguous_or_dropped_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = conversion::read_path(
        &root.join("tests/fixtures/linear-results.frd"),
        &Options::default(),
    )
    .unwrap();
    let mut duplicated = source.clone();
    duplicated
        .dataset
        .fields
        .push(duplicated.dataset.fields[0].clone());
    assert_eq!(
        conversion::convert(
            duplicated,
            Format::Vtu,
            &Options::default(),
            &mut Vec::new()
        )
        .unwrap_err()
        .code,
        "E_VTU"
    );
    let mut cell_field = source.clone();
    cell_field.dataset.fields[0].location = crate::core::FieldLocation::Cell;
    cell_field.dataset.fields[0].values.truncate(3);
    let mut encoded = Vec::new();
    let report =
        conversion::convert(cell_field, Format::Frd, &Options::default(), &mut encoded).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("cell field"))
    );
    assert_ne!(encoded, []);
}

/// OP2 cannot silently choose among multiple displacement fields.
#[test]
fn op2_requires_exactly_one_displacement_field() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut source =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    assert_eq!(
        conversion::convert(
            source.clone(),
            Format::Op2,
            &Options::default(),
            &mut Vec::new()
        )
        .unwrap_err()
        .code,
        "E_OP2"
    );
    let displacement = Field {
        name: "DISP".into(),
        location: FieldLocation::Point,
        components: ["T1", "T2", "T3", "R1", "R2", "R3"]
            .map(str::to_owned)
            .to_vec(),
        values: vec![0.0; source.dataset.mesh.points.len() * 6],
        step: None,
        time: None,
    };
    source.dataset.fields.push(displacement.clone());
    let mut output = Vec::new();
    conversion::convert(
        source.clone(),
        Format::Op2,
        &Options::default(),
        &mut output,
    )
    .unwrap();
    assert_ne!(output, []);
    source.dataset.fields.push(displacement);
    assert_eq!(
        conversion::convert(source, Format::Op2, &Options::default(), &mut Vec::new())
            .unwrap_err()
            .code,
        "E_OP2"
    );
}

/// Assign and report stable IDs for VTU without identity arrays.
#[test]
fn external_vtu_without_identity_arrays_gets_explicit_stable_ids() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    let mut encoded = Vec::new();
    crate::formats::vtu::write(&read.dataset.mesh, &mut encoded).unwrap();
    let mut source = String::from_utf8(encoded).unwrap();
    for (start, end) in [
        ("<PointData>", "</PointData>"),
        ("<CellData>", "</CellData>"),
    ] {
        let first = source.find(start).unwrap();
        let last = source.find(end).unwrap() + end.len();
        source.replace_range(first..last, "");
    }
    let projection = crate::formats::vtu::read_projection(&source).unwrap();
    assert!(projection.generated_point_ids);
    assert!(projection.generated_cell_ids);
    let dataset = projection.dataset;
    assert_eq!(
        dataset
            .mesh
            .points
            .iter()
            .map(|point| point.id)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(dataset.mesh.cells[0].id, 1);
    assert_eq!(dataset.mesh.cells[0].property_id, None);

    let scratch = Scratch::new();
    let path = scratch.write("generated.vtu", source);
    let read = conversion::read_path(&path, &Options::default()).unwrap();
    assert!(read.generated_point_ids);
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("original point IDs"))
    );
    let options = Options {
        mesh: Some(path),
        assume_basic_frame: true,
        subcase: Some(1),
        ..Options::default()
    };
    let pch = root.join("tests/fixtures/pch-multiple.pch");
    let error = conversion::read_path(&pch, &options).unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("original node IDs"));
}

/// Reject generated node IDs as proof of Nastran result identity.
#[test]
fn generated_ids_cannot_validate_nastran_result_companions() {
    let scratch = Scratch::new();
    let vtk = "# vtk DataFile Version 2.0\nmesh\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 double\n0 0 0\n1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\n";
    let stl = "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid mesh\n";
    let su2 = "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n";
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (name, path) in [
        ("vtk", scratch.write("generated.vtk", vtk)),
        ("stl", scratch.write("generated.stl", stl)),
        ("su2", scratch.write("generated.su2", su2)),
        ("exodus", root.join("tests/fixtures/exodus-legacy.exo")),
    ] {
        let read = conversion::read_path(&path, &Options::default()).unwrap();
        assert!(read.generated_point_ids, "{name}");
        let options = Options {
            mesh: Some(path),
            assume_basic_frame: true,
            subcase: Some(1),
            ..Options::default()
        };
        let error = conversion::read_path(&root.join("tests/fixtures/pch-multiple.pch"), &options)
            .unwrap_err();
        assert_eq!(error.code, "E_USAGE", "{name}");
        assert!(error.message.contains("original node IDs"), "{name}");
    }
}
