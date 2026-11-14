//! P6: the whole shape's mass facts ([`Subject::shape_mass`]) are each
//! solid's facts, bit for bit for one solid and at every thread count, the
//! combination of several solids holds their closed forms within its bound,
//! and a shape the facts cannot measure refuses with its typed reason.

#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::f64::consts::PI;

use geospec_facts::{bodies_facts, Options};
use geospec_facts_occt::{combine, ShapeMass, Subject};

fn options(threads: usize) -> Options {
    Options {
        accuracy: 1e-7,
        threads,
    }
}

fn shape_mass(kind: i32, threads: usize) -> ShapeMass {
    let mut s = Subject::synthetic(kind).unwrap_or_else(|e| panic!("kind {kind}: {e}"));
    s.shape_mass(options(threads))
        .unwrap_or_else(|r| panic!("kind {kind}: refused {r:?}"))
}

fn refusal(kind: i32) -> (&'static str, Option<usize>, Option<usize>) {
    let mut s = Subject::synthetic(kind).unwrap_or_else(|e| panic!("kind {kind}: {e}"));
    let r = s
        .shape_mass(options(1))
        .expect_err("the shape has no mass facts");
    (r.code, r.body, r.face)
}

#[test]
fn one_solid_reports_its_own_facts_at_every_thread_count() {
    for kind in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 15] {
        let m = shape_mass(kind, 1);
        let mut s = Subject::synthetic(kind).expect("synthetic");
        let mut body = s.mass_body(0).expect("mass body");
        let own = bodies_facts(std::slice::from_ref(&body), options(1))
            .remove(0)
            .expect("facts");
        assert_eq!(m.bodies, 1);
        assert_eq!(m.area, own.area, "kind {kind}");
        assert_eq!(m.volume, own.volume, "kind {kind}");
        assert_eq!(m.centroid, own.centroid, "kind {kind}");
        // The file tolerance is the solid's largest, carried into the volume.
        let t = m.volume.accuracy.file_tolerance.expect("file tolerance");
        assert!(t > 0.0 && t < 1e-3, "kind {kind}: {t}");
        assert_eq!(m.volume.accuracy.gap_bound, Some(t * m.area.value));
        assert_eq!(m.area.accuracy.gap_bound, None);
        assert_eq!(m.volume.accuracy.closed, Some(true));
        // Without a file tolerance the values are the same.
        body.file_tolerance = None;
        let plain = bodies_facts(&[body], options(1)).remove(0).expect("facts");
        assert_eq!(plain.volume.value.to_bits(), m.volume.value.to_bits());
        for threads in [2, 4] {
            assert_eq!(
                shape_mass(kind, threads),
                m,
                "kind {kind} at {threads} threads"
            );
        }
    }
}

#[test]
fn the_box_holds_its_closed_forms_within_the_bound() {
    let m = shape_mass(0, 1);
    let within = |what: &str, value: f64, truth: f64, bound: f64| {
        assert!(
            (value - truth).abs() <= bound,
            "{what}: {value} vs {truth} beyond {bound:e}"
        );
    };
    within(
        "volume",
        m.volume.value,
        6000.0,
        m.volume.accuracy.bound.expect("bound"),
    );
    within(
        "area",
        m.area.value,
        2200.0,
        m.area.accuracy.bound.expect("bound"),
    );
    let c = m.centroid.expect("centre");
    within("x", c.value.x, 6.0, c.axis_bounds.x);
    within("y", c.value.y, 12.0, c.axis_bounds.y);
    within("z", c.value.z, 18.0, c.axis_bounds.z);
}

#[test]
fn several_solids_combine_within_their_bound() {
    // The box of kind 0 (6000 at (6, 12, 18)) and the sphere of kind 3
    // (r 2.5 at (3, 1, -2)), combined in that order.
    let facts: Vec<_> = [0, 3]
        .iter()
        .map(|&kind| {
            let mut s = Subject::synthetic(kind).expect("synthetic");
            let body = s.mass_body(0).expect("mass body");
            bodies_facts(&[body], options(1)).remove(0).expect("facts")
        })
        .collect();
    let m = combine(&facts).expect("combined");
    assert_eq!(m.bodies, 2);
    let sphere = 4.0 / 3.0 * PI * (2.5 * 2.5 * 2.5);
    let volume = 6000.0 + sphere;
    let area = 2200.0 + 4.0 * PI * 2.5 * 2.5;
    let centre = [
        (6000.0 * 6.0 + sphere * 3.0) / volume,
        (6000.0 * 12.0 + sphere * 1.0) / volume,
        (6000.0 * 18.0 + sphere * -2.0) / volume,
    ];
    let vb = m.volume.accuracy.bound.expect("bound");
    assert!((m.volume.value - volume).abs() <= vb, "volume");
    assert!(vb >= facts[0].volume.accuracy.bound.expect("bound"));
    let ab = m.area.accuracy.bound.expect("bound");
    assert!((m.area.value - area).abs() <= ab, "area");
    let c = m.centroid.expect("centre");
    let got = [c.value.x, c.value.y, c.value.z];
    let b = [c.axis_bounds.x, c.axis_bounds.y, c.axis_bounds.z];
    for k in 0..3 {
        assert!(
            (got[k] - centre[k]).abs() <= b[k],
            "centre {k}: {} vs {} beyond {:e}",
            got[k],
            centre[k],
            b[k]
        );
        assert!(b[k] < 1e-6, "centre {k}: bound {:e} is loose", b[k]);
    }
    // The gap volumes add; the file tolerance is the larger.
    let gaps: f64 = facts
        .iter()
        .map(|f| f.volume.accuracy.gap_bound.expect("gap"))
        .sum();
    assert_eq!(m.volume.accuracy.gap_bound, Some(gaps));
}

#[test]
fn a_shape_the_facts_cannot_measure_refuses_with_its_reason() {
    assert_eq!(refusal(13), ("unsupported-surface", Some(0), Some(0)));
    assert_eq!(refusal(14), ("invalid-definition", Some(0), None));
    assert_eq!(refusal(16), ("tolerance-gap", Some(0), Some(0)));
    assert_eq!(refusal(18).0, "free-geometry");
}
