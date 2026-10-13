//! Deterministic single-byte corruption checks through the public validator.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{Options, validate};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-malformed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Mutate a bounded set of byte positions while checking public parser stability.
fn check_mutations(
    scratch: &Scratch,
    name: &str,
    original: &[u8],
    options: &Options,
    replacements: impl Fn(u8) -> [u8; 3],
) {
    let path = scratch.0.join(name);
    std::fs::write(&path, original).unwrap();
    assert!(validate(&path, options).is_ok(), "valid {name} corpus");
    let stride = (original.len() / 512).max(1);
    let mut rejected = 0;
    for index in (0..original.len()).step_by(stride) {
        for replacement in replacements(original[index]) {
            if replacement == original[index] {
                continue;
            }
            let mut changed = original.to_vec();
            changed[index] = replacement;
            std::fs::write(&path, changed).unwrap();
            rejected += usize::from(validate(&path, options).is_err());
        }
    }
    assert!(rejected > 0, "{name} mutations unexpectedly all validated");
}

/// Preserve or perturb ASCII grammar without inserting invalid UTF-8 by default.
fn text_replacements(old: u8) -> [u8; 3] {
    let structural = if old.is_ascii_digit() {
        if old == b'9' { b'0' } else { b'9' }
    } else if old.is_ascii_alphabetic() {
        if old == b'X' { b'Y' } else { b'X' }
    } else if old.is_ascii() {
        b' '
    } else {
        old.wrapping_add(0x55)
    };
    let boundary = if old.is_ascii_digit() {
        b'0'
    } else {
        old ^ 0x01
    };
    let alternate = if old.is_ascii_digit() {
        if old == b'1' { b'2' } else { b'1' }
    } else {
        old ^ 0x20
    };
    [structural, boundary, alternate]
}

/// Corrupt one byte at sampled positions in each supported container family.
#[test]
fn malformed_corpus_returns_results_without_panicking() {
    let scratch = Scratch::new();
    let mut corpora: Vec<(&str, &[u8])> = vec![
        ("sample.bdf", include_bytes!("../examples/plate.bdf")),
        ("sample.unv", include_bytes!("fixtures/gmsh-six-kind.unv")),
        ("sample.msh", include_bytes!("fixtures/named-boundary.msh")),
        ("sample.vtk", include_bytes!("fixtures/vtk-5.1-mixed.vtk")),
        ("sample.frd", include_bytes!("fixtures/linear-results.frd")),
        ("sample.exo", include_bytes!("fixtures/exodus-two-blocks.exo")),
        ("sample.inp", b"*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n"),
        ("sample.su2", b"NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 0\n"),
        ("sample.stl", b"solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid s\n"),
    ];
    let vtu_path = scratch.0.join("source.vtu");
    caexfer::convert(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.bdf"),
        &vtu_path,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let vtu = std::fs::read(&vtu_path).unwrap();
    corpora.push(("sample.vtu", &vtu));

    for (name, original) in corpora {
        check_mutations(
            &scratch,
            name,
            original,
            &Options::default(),
            text_replacements,
        );
    }

    let op2_path = scratch.0.join("source.op2");
    caexfer::convert(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.bdf"),
        &op2_path,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let op2 = std::fs::read(&op2_path).unwrap();
    for (name, original, options) in [
        (
            "damaged.op2",
            op2.as_slice(),
            Options {
                mesh: Some(PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/examples/plate.bdf"
                ))),
                ..Options::default()
            },
        ),
        (
            "damaged.pch",
            include_bytes!("fixtures/pch-multiple.pch").as_slice(),
            Options {
                mesh: Some(PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/pch-companion.bdf"
                ))),
                subcase: Some(1),
                ..Options::default()
            },
        ),
    ] {
        check_mutations(&scratch, name, original, &options, |old| {
            [old ^ 0x40, old ^ 0x01, old ^ 0x80]
        });
    }
}
