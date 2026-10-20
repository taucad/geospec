//! W5f straight-segment void witness facts: per-waypoint classifier state
//! and clearance, per-segment clearance and face crossing, every query
//! charged first (fixture provenance in `fixtures/void-witness/provenance.json`).
use geospec_engine_native_core::backend::brep::{BrepSubject, VoidWitnessState};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

fn open(name: &str) -> Document {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/void-witness")
            .join(name),
    )
    .expect("retained fixture must be readable");
    Document::from_step(&bytes).expect("fixture must be admitted")
}

#[test]
fn blocked_bore_reports_the_web() {
    let document = open("w2-blocked-bore.step");
    let points = [[0.0, 0.0, 1.0], [0.0, 0.0, 15.0], [0.0, 0.0, 29.0]];
    let mut charged = 0;
    let mut charge = |units| {
        charged += units;
        true
    };
    let witness = document
        .void_witness(&[0], &points, &mut charge)
        .expect("witness")
        .expect("charged");
    assert!(charged > 0);
    assert_eq!(witness.points[0].state, VoidWitnessState::Outside);
    assert_eq!(
        witness.points[1].state,
        VoidWitnessState::Inside { occurrence: 0 }
    );
    assert!((witness.points[1].clearance - 1.0).abs() < 1e-6);
    assert_eq!(witness.points[2].state, VoidWitnessState::Outside);
    assert!(witness.segments.iter().all(|row| row.crosses_material));
    assert!(witness.segments.iter().all(|row| row.clearance == 0.0));
}

#[test]
fn elbow_bend_chords_stay_clear() {
    let document = open("w0-elbow.step");
    let points = [
        [0.0, 0.0, 20.0],
        [4.019237886466843, 0.0, 35.0],
        [15.0, 0.0, 45.98076211353316],
    ];
    let witness = document
        .void_witness(&[0], &points, &mut |_| true)
        .expect("witness")
        .expect("charged");
    assert!(witness
        .points
        .iter()
        .all(|row| row.state == VoidWitnessState::Outside && (row.clearance - 6.0).abs() < 1e-6));
    // A 30 degree chord of the radius 30 centreline sags 30 (1 - cos 15°) toward the wall.
    let sag = 30.0 * (1.0 - (15.0f64).to_radians().cos());
    for row in &witness.segments {
        assert!(!row.crosses_material);
        assert!((row.clearance - (6.0 - sag)).abs() < 1e-4, "{row:?}");
    }
}

#[test]
fn a_refused_charge_returns_no_facts() {
    let document = open("w1-split-collar.step");
    let result = document
        .void_witness(&[0], &[[0.0, 0.0, 1.0], [0.0, 0.0, 19.0]], &mut |_| false)
        .expect("no error");
    assert!(result.is_none());
    assert!(document
        .void_witness(&[0], &[[0.0, 0.0, 1.0]], &mut |_| true)
        .is_err());
}
