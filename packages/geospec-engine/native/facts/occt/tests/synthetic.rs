//! G3 and G4 on OCCT-built solids with closed-form volume, area and centre:
//! the OCCT adapter's facts lie within their bounds of the closed forms, the
//! bounds are tight, and the facts are bit-identical at 1, 2 and 4 threads.
//! The shapes cover planes, the kinds of revolution, seams, degenerate edges,
//! circle trims, an indirect (mirrored) frame, a located solid, trims past
//! 2 pi in u and v, B-spline faces and a periodic net whose seam lies inside
//! its period; two more check that an unsupported surface and a mirrored
//! placement refuse.

#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::f64::consts::PI;

use geospec_facts::{bodies_facts, BodyFacts, Options, RefusalCode};
use geospec_facts_occt::{BodyExport, Subject};

struct Truth {
    volume: f64,
    area: f64,
    centre: Option<[f64; 3]>,
}

fn export(kind: i32) -> (Vec<BodyExport>, Subject) {
    let mut s = Subject::synthetic(kind).unwrap_or_else(|e| panic!("kind {kind}: {e}"));
    let bodies = (0..s.body_count())
        .map(|i| s.export_body(i).expect("solid record"))
        .collect();
    (bodies, s)
}

fn facts(kind: i32, threads: usize) -> (BodyFacts, BodyExport) {
    let (mut bodies, _subject) = export(kind);
    assert_eq!(bodies.len(), 1, "kind {kind}: one solid");
    let body = bodies.remove(0);
    let definition = match &body.definition {
        Ok(d) => d.clone(),
        Err(r) => panic!("kind {kind}: refused {r}"),
    };
    let options = Options {
        accuracy: 1e-7,
        threads,
    };
    let f = bodies_facts(&[definition], options)
        .remove(0)
        .unwrap_or_else(|r| panic!("kind {kind}: refused {r}"));
    (f, body)
}

fn within(kind: i32, what: &str, value: f64, truth: f64, bound: f64) {
    let error = (value - truth).abs();
    assert!(
        error <= bound,
        "kind {kind} {what}: {value} vs {truth}, error {error:e} > bound {bound:e}"
    );
    // The bound is tight, not vacuous: within 1e-5 of the magnitude.
    assert!(
        bound <= 1e-5 * (1.0 + truth.abs()),
        "kind {kind} {what}: bound {bound:e} is loose"
    );
}

fn check(kind: i32, truth: &Truth) -> BodyExport {
    let (f, body) = facts(kind, 1);
    let vb = f.volume.accuracy.bound.expect("volume bound");
    let ab = f.area.accuracy.bound.expect("area bound");
    within(kind, "volume", f.volume.value, truth.volume, vb);
    within(kind, "area", f.area.value, truth.area, ab);
    if let Some(c) = truth.centre {
        let got = f.centroid.expect("centre of mass");
        let v = [got.value.x, got.value.y, got.value.z];
        let b = [got.axis_bounds.x, got.axis_bounds.y, got.axis_bounds.z];
        for k in 0..3 {
            within(kind, &format!("centre {k}"), v[k], c[k], b[k]);
        }
    }
    // G4: the same bits at 1, 2 and 4 threads.
    for threads in [2, 4] {
        let (g, _) = facts(kind, threads);
        assert_eq!(
            format!("{f:?}"),
            format!("{g:?}"),
            "kind {kind}: facts differ at {threads} threads"
        );
    }
    body
}

fn cylinder_truth(centre: [f64; 3]) -> Truth {
    Truth {
        volume: 63.0 * PI,
        area: 60.0 * PI,
        centre: Some(centre),
    }
}

fn cylinder_centre() -> [f64; 3] {
    [1.0 + 3.5 / 3.0, -2.0 + 7.0 / 3.0, 0.5 + 7.0 / 3.0]
}

#[test]
fn box_of_planes() {
    let b = check(
        0,
        &Truth {
            volume: 6000.0,
            area: 2200.0,
            centre: Some([6.0, 12.0, 18.0]),
        },
    );
    assert_eq!(b.stats.surfaces.get("plane"), Some(&6));
}

#[test]
fn cylinder_with_a_seam_and_circle_trims() {
    let b = check(1, &cylinder_truth(cylinder_centre()));
    assert_eq!(b.stats.seams, 2);
    assert!(b.stats.curves.get("circle").copied().unwrap_or(0) >= 2);
}

#[test]
fn frustum() {
    let s = 34.0_f64.sqrt();
    check(
        2,
        &Truth {
            volume: 35.0 * PI,
            area: 5.0 * PI * s + 17.0 * PI,
            centre: Some([0.0, 5.0 * 27.0 / 84.0, 0.0]),
        },
    );
}

#[test]
fn sphere_with_degenerate_edges() {
    let r: f64 = 2.5;
    let b = check(
        3,
        &Truth {
            volume: 4.0 / 3.0 * PI * r * r * r,
            area: 4.0 * PI * r * r,
            centre: Some([3.0, 1.0, -2.0]),
        },
    );
    assert_eq!(b.stats.degenerate, 2);
}

#[test]
fn torus() {
    check(
        4,
        &Truth {
            volume: 2.0 * PI * PI * 5.0 * 2.25,
            area: 4.0 * PI * PI * 7.5,
            centre: Some([0.0, 0.0, 1.0]),
        },
    );
}

#[test]
fn mirrored_cylinder_has_indirect_frames() {
    let c = cylinder_centre();
    let b = check(5, &cylinder_truth([-c[0], c[1], c[2]]));
    assert!(b.stats.reflected_faces >= 1, "{:?}", b.stats);
}

#[test]
fn box_with_a_through_hole() {
    check(
        6,
        &Truth {
            volume: 6000.0 - 120.0 * PI,
            area: 2200.0 + 112.0 * PI,
            centre: Some([5.0, 10.0, 15.0]),
        },
    );
}

#[test]
fn cylinder_as_b_spline_faces() {
    let b = check(7, &cylinder_truth(cylinder_centre()));

    assert!(
        b.stats.surfaces.contains_key("bspline")
            || b.stats.surfaces.contains_key("periodic-bspline")
    );
}

#[test]
fn located_sphere() {
    let r: f64 = 2.5;
    check(
        8,
        &Truth {
            volume: 4.0 / 3.0 * PI * r * r * r,
            area: 4.0 * PI * r * r,
            centre: None,
        },
    );
}

#[test]
fn sphere_with_its_seam_past_two_pi() {
    let b = check(
        9,
        &Truth {
            volume: 32.0 / 3.0 * PI,
            area: 16.0 * PI,
            centre: Some([0.0; 3]),
        },
    );
    assert!(b.stats.past_two_pi >= 1, "{:?}", b.stats);
}

#[test]
fn torus_with_seams_past_two_pi_in_u_and_v() {
    let b = check(
        10,
        &Truth {
            volume: 8.0 * PI * PI,
            area: 16.0 * PI * PI,
            centre: Some([0.0; 3]),
        },
    );
    assert!(b.stats.past_two_pi >= 1, "{:?}", b.stats);
}

#[test]
fn capped_cylinder_with_its_seam_past_two_pi() {
    let b = check(
        11,
        &Truth {
            volume: 20.0 * PI,
            area: 28.0 * PI,
            centre: Some([0.0, 0.0, 2.5]),
        },
    );
    assert!(b.stats.past_two_pi >= 1, "{:?}", b.stats);
}

#[test]
fn periodic_net_with_its_seam_inside_the_period() {
    let b = check(
        12,
        &Truth {
            volume: 20.0 * PI,
            area: 28.0 * PI,
            centre: Some([0.0, 0.0, 2.5]),
        },
    );
    assert_eq!(
        b.stats.surfaces.get("periodic-bspline"),
        Some(&1),
        "{:?}",
        b.stats
    );
    assert_eq!(b.stats.moved_origin_faces, 1, "{:?}", b.stats);
}

#[test]
fn offset_surface_refuses() {
    let (bodies, _s) = export(13);
    let r = bodies[0]
        .definition
        .as_ref()
        .expect_err("an offset surface has no export");
    assert_eq!(r.code, RefusalCode::UnsupportedSurface, "{r}");
    assert_eq!(bodies[0].stats.surfaces.get("offset"), Some(&1));
}

#[test]
fn mirrored_placement_refuses() {
    match Subject::synthetic(14) {
        // OCCT may forbid the location itself.
        Err(e) => assert!(!e.is_empty()),
        Ok(mut s) => {
            let b = s.export_body(0).expect("solid record");
            let r = b.definition.expect_err("a mirrored placement refuses");
            assert_eq!(r.code, RefusalCode::InvalidDefinition, "{r}");
        }
    }
}
