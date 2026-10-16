//! Exercise the two public file operations from an external crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{
    AssumptionKind, InfillShape, MshVersion, Options, Stage, convert, preview, validate,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-api-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn voxel_preview_rejects_options_that_conversion_rejects() {
    let scratch = Scratch::new();
    let source = fixture("examples/donut.stl");
    let output = scratch.path("donut.vti");
    let infill = Options {
        infill_shape: Some(InfillShape::Hex8),
        infill_unit_size: Some(0.5),
        ..Options::default()
    };
    for options in [
        Options {
            surface: true,
            ..infill.clone()
        },
        Options {
            gmsh: Some(PathBuf::from("gmsh")),
            ..infill.clone()
        },
        Options {
            smooth_iterations: 1,
            ..infill.clone()
        },
        Options {
            surface: true,
            ..Options::default()
        },
    ] {
        assert_eq!(
            convert(&source, &output, &options).unwrap_err().code,
            "E_USAGE"
        );
        assert_eq!(
            preview(&source, &output, &options).unwrap_err().code,
            "E_USAGE"
        );
        assert!(!output.exists());
    }
}

#[test]
fn validation_matches_cli_strictness_and_counts() {
    let source = fixture("examples/plate.bdf");
    let ordinary = validate(&source, &Options::default()).unwrap();
    assert!(ordinary.passed);
    assert_eq!(
        (ordinary.points, ordinary.cells, ordinary.fields),
        (4, 1, 0)
    );
    assert_ne!(ordinary.omissions.len(), 0);
    let strict = validate(
        &source,
        &Options {
            strict: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(!strict.passed);
    assert_eq!(strict.omissions, ordinary.omissions);
}

#[test]
fn conversion_requires_acceptance_and_never_overwrites() {
    let scratch = Scratch::new();
    let output = scratch.path("model.vtu");
    let source = fixture("examples/plate.bdf");
    let error = convert(&source, &output, &Options::default()).unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("accept_omissions"));
    assert!(!output.exists());
    let report = convert(
        &source,
        &output,
        &Options {
            accept_omissions: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((report.points, report.cells), (4, 1));
    assert_ne!(report.omissions.len(), 0);
    assert!(
        std::fs::read_to_string(&output)
            .unwrap()
            .contains("<VTKFile")
    );
    assert_eq!(
        convert(
            &source,
            &output,
            &Options {
                accept_omissions: true,
                ..Options::default()
            }
        )
        .unwrap_err()
        .code,
        "E_EXISTS"
    );
}

#[test]
fn op2_companion_is_staged_and_reported() {
    let scratch = Scratch::new();
    let output = scratch.path("results.op2");
    let mesh = scratch.path("mesh.bdf");
    let options = Options {
        mesh_output: Some(mesh.clone()),
        accept_all: true,
        ..Options::default()
    };
    let report = convert(fixture("examples/plate.bdf"), &output, &options).unwrap();
    let companion = report.mesh_output.unwrap();
    assert_eq!(companion.path, mesh);
    assert!(output.exists());
    assert!(companion.path.exists());
    let reread = validate(
        &output,
        &Options {
            mesh: Some(companion.path),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(reread.passed);
    assert_eq!(reread.points, 4);
}

#[test]
fn validation_reports_unverified_frame_without_acceptance() {
    let scratch = Scratch::new();
    let source = fixture("examples/plate.bdf");
    let op2 = scratch.path("results.op2");
    let mesh = scratch.path("mesh.vtu");
    let accepted = Options {
        accept_all: true,
        ..Options::default()
    };
    convert(&source, &op2, &accepted).unwrap();
    convert(&source, &mesh, &accepted).unwrap();

    let options = Options {
        mesh: Some(mesh),
        ..Options::default()
    };
    let report = validate(&op2, &options).unwrap();
    assert!(report.passed);
    assert!(report.omissions.iter().any(|item| {
        item.stage == Stage::Assumption && item.assumption == Some(AssumptionKind::BasicFrame)
    }));
    let strict = validate(
        &op2,
        &Options {
            strict: true,
            ..options
        },
    )
    .unwrap();
    assert!(!strict.passed);
}

#[test]
fn companion_output_requires_op2_destination() {
    let scratch = Scratch::new();
    let output = scratch.path("result.vtu");
    let companion = scratch.path("mesh.bdf");
    let error = convert(
        fixture("examples/plate.bdf"),
        &output,
        &Options {
            mesh_output: Some(companion.clone()),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!output.exists() && !companion.exists());
}

#[test]
fn op2_companion_requires_a_mesh_bearing_format() {
    let scratch = Scratch::new();
    let output = scratch.path("results.op2");
    let companion = scratch.path("companion.op2");
    let error = convert(
        fixture("examples/plate.bdf"),
        &output,
        &Options {
            mesh_output: Some(companion.clone()),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!output.exists() && !companion.exists());
}

#[test]
fn documented_frd_msh_and_pch_routes_use_only_the_root_api() {
    let scratch = Scratch::new();
    let vtu = scratch.path("results.vtu");
    let msh = scratch.path("results.msh");
    let pch_vtu = scratch.path("displacements.vtu");
    let accepted = Options {
        accept_omissions: true,
        ..Options::default()
    };
    convert(
        fixture("tests/fixtures/linear-results.frd"),
        &vtu,
        &accepted,
    )
    .unwrap();
    convert(
        &vtu,
        &msh,
        &Options {
            msh_version: Some(MshVersion::V2_2),
            ..accepted.clone()
        },
    )
    .unwrap();
    let msh_text = std::fs::read_to_string(&msh).unwrap();
    assert!(msh_text.contains("2.2 0 8"));
    let report = convert(
        fixture("tests/fixtures/pch-multiple.pch"),
        &pch_vtu,
        &Options {
            mesh: Some(fixture("tests/fixtures/pch-companion.bdf")),
            subcase: Some(1),
            ..accepted
        },
    )
    .unwrap();
    assert_eq!(report.fields, 1);
}

/// Public validation must preserve each reader's stable error code on bad input.
#[test]
fn malformed_inputs_fail_through_the_public_api() {
    let scratch = Scratch::new();
    let cases: &[(&str, &[u8], &str)] = &[
        ("bad.su2", b"NDIME= 2\nNELEM= 1\n5 0 1 9\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n", "E_SU2"),
        ("bad.unv", b"-1\n2411\n1 1 1 11\n0 0\n-1\n-1\n2412\n-1\n", "E_UNV"),
        ("bad.stl", b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nendsolid sample\n", "E_STL"),
        ("bad.msh", b"$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n1 1 1 1\n2 1 1 1\n$EndNodes\n", "E_MSH"),
        ("bad.vtk", b"# vtk DataFile Version 2.0\nbad\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 1 float\nNaN 0 0\nCELLS 0 0\nCELL_TYPES 0\n", "E_VTK"),
        ("bad.vtu", b"<VTKFile type=\"UnstructuredGrid\"><UnstructuredGrid><Piece NumberOfPoints=\"1\" NumberOfCells=\"0\"/></UnstructuredGrid></VTKFile>", "E_VTU"),
        ("bad.frd", b" 2C\n -1 1 0 0 0\n -3\n 3C\n -1 10 99 0 0\n -3\n", "E_FRD"),
        ("bad.exo", b"not NetCDF", "E_EXODUS"),
    ];
    for (name, bytes, code) in cases {
        let path = scratch.path(name);
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            validate(&path, &Options::default()).unwrap_err().code,
            *code,
            "{name}"
        );
    }
    let oversized = scratch.path("oversized.vtu");
    std::fs::write(&oversized, b"12345").unwrap();
    assert_eq!(
        validate(
            &oversized,
            &Options {
                max_bytes: 4,
                ..Options::default()
            }
        )
        .unwrap_err()
        .code,
        "E_LIMIT"
    );
}

/// Every public destination route should produce a file the reader accepts.
#[test]
fn public_conversion_routes_write_valid_destinations() {
    let scratch = Scratch::new();
    let source = fixture("examples/plate.bdf");
    let accepted = Options {
        accept_all: true,
        ..Options::default()
    };
    for extension in [
        "bdf", "inp", "unv", "su2", "msh", "vtu", "vtk", "frd", "exo", "op2",
    ] {
        let output = scratch.path(&format!("plate.{extension}"));
        let report = convert(&source, &output, &accepted).unwrap();
        assert_eq!((report.points, report.cells), (4, 1), "{extension}");
        assert!(output.metadata().unwrap().len() > 0, "{extension}");
        let validated = validate(
            &output,
            &Options {
                mesh: (extension == "op2").then(|| source.clone()),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!((validated.points, validated.cells), (4, 1), "{extension}");
    }
    let triangle = scratch.path("triangle.su2");
    std::fs::write(
        &triangle,
        "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n",
    )
    .unwrap();
    let stl = scratch.path("triangle.stl");
    convert(&triangle, &stl, &accepted).unwrap();
    assert_eq!(validate(&stl, &Options::default()).unwrap().cells, 1);
}

/// A complete two-step Exodus scalar series survives the public file route.
#[test]
fn exodus_result_series_roundtrips_through_public_api() {
    let scratch = Scratch::new();
    let output = scratch.path("series.exo");
    let report = convert(
        fixture("tests/fixtures/exodus-two-blocks.exo"),
        &output,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((report.points, report.cells, report.fields), (4, 2, 4));
    let reread = validate(&output, &Options::default()).unwrap();
    assert_eq!((reread.points, reread.cells, reread.fields), (4, 2, 4));
    let selected = validate(
        &output,
        &Options {
            step: Some(1),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(selected.fields, 2);
}

/// Selecting a later PCH step yields one OP2 table with an explicit loss report.
#[test]
fn selected_pch_step_to_op2_reports_step_loss() {
    let scratch = Scratch::new();
    let mesh = fixture("tests/fixtures/pch-companion.bdf");
    let output = scratch.path("selected.op2");
    let report = convert(
        fixture("tests/fixtures/pch-multiple.pch"),
        &output,
        &Options {
            mesh: Some(mesh.clone()),
            subcase: Some(2),
            step: Some(1),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("step number is not encoded"))
    );
    let reread = validate(
        &output,
        &Options {
            mesh: Some(mesh),
            subcase: Some(2),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((reread.points, reread.fields), (2, 1));
}

/// Source metadata outside the projection must appear in validation receipts.
#[test]
fn source_metadata_losses_are_reported_by_public_validation() {
    let scratch = Scratch::new();
    let frd = scratch.path("metadata.frd");
    let frd_text = format!(
        " 1U local parameter\n{}",
        include_str!("fixtures/linear-results.frd")
    );
    std::fs::write(&frd, frd_text).unwrap();
    assert!(
        validate(&frd, &Options::default())
            .unwrap()
            .omissions
            .iter()
            .any(|item| item.detail.contains("user/model parameter"))
    );

    let msh = scratch.path("metadata.msh");
    let msh_text = format!(
        "{}$Comments\nexternal metadata\n$EndComments\n",
        include_str!("fixtures/named-boundary.msh")
    );
    std::fs::write(&msh, msh_text).unwrap();
    assert!(
        validate(&msh, &Options::default())
            .unwrap()
            .omissions
            .iter()
            .any(|item| item.detail.contains("$Comments"))
    );

    let triangle = scratch.path("triangle.su2");
    std::fs::write(
        &triangle,
        "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n",
    )
    .unwrap();
    let stl = scratch.path("attributed.stl");
    convert(
        &triangle,
        &stl,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let mut bytes = std::fs::read(&stl).unwrap();
    bytes[84 + 48] = 1;
    std::fs::write(&stl, bytes).unwrap();
    assert!(
        validate(&stl, &Options::default())
            .unwrap()
            .omissions
            .iter()
            .any(|item| item.detail.contains("attribute bytes"))
    );
}
