//! Compare a real MSC Nastran result with independently recorded values.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{Options, convert};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn numbers(source: &str, before: &str) -> Vec<f64> {
    let (_, tail) = source.split_once(before).unwrap();
    let (_, tail) = tail.split_once('>').unwrap();
    let (values, _) = tail.split_once("</DataArray>").unwrap();
    values
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect()
}

fn near(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-10),
        "actual {actual:?}, expected {expected:?}"
    );
}

#[test]
fn msc_plate_mesh_and_displacements_match_external_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/msc-plate.op2");
    let companion = root.join("tests/fixtures/msc-plate.bdf");
    let output = std::env::temp_dir().join(format!(
        "caexfer-msc-plate-{}-{}.vtu",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let report = convert(
        source,
        &output,
        &Options {
            mesh: Some(companion),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((report.points, report.cells, report.fields), (36, 25, 1));
    let xml = std::fs::read_to_string(&output).unwrap();
    std::fs::remove_file(output).unwrap();

    let ids = numbers(&xml, "Name=\"nastran_node_id\"");
    let elements = numbers(&xml, "Name=\"nastran_element_id\"");
    assert_eq!(ids, (1..=36).map(f64::from).collect::<Vec<_>>());
    assert_eq!(elements, (1..=25).map(f64::from).collect::<Vec<_>>());
    let points = numbers(&xml, "<Points>");
    near(&points[0..3], &[0.0, 0.0, 0.0]);
    near(&points[3..6], &[2.0, 0.0, 0.0]);
    near(&points[17 * 3..18 * 3], &[10.0, 4.0, 0.0]);
    near(&points[35 * 3..36 * 3], &[10.0, 10.0, 0.0]);
    let connectivity = numbers(&xml, "Name=\"connectivity\"");
    assert_eq!(&connectivity[..4], &[0.0, 1.0, 7.0, 6.0]);
    assert_eq!(&connectivity[96..100], &[28.0, 29.0, 35.0, 34.0]);
    let displacement = numbers(&xml, "Name=\"DISPLACEMENT_SUBCASE_1\"");
    near(&displacement[..6], &[0.0; 6]);
    near(
        &displacement[6..12],
        &[
            6.224_123_808_351_578e-6,
            2.957_988_499_474_595_3e-6,
            0.0,
            0.0,
            0.0,
            6.779_605_996_598_548e-7,
        ],
    );
    near(
        &displacement[35 * 6..36 * 6],
        &[
            3.683_496_106_532_402e-5,
            -9.305_488_674_726_803e-6,
            0.0,
            0.0,
            0.0,
            -3.944_540_821_976_261e-6,
        ],
    );
}
