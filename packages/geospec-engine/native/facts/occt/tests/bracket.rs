//! The box with a bracket (P5) on OCCT-built solids with closed-form boxes:
//! each side's closed form lies between the bracket's inner and outer values,
//! the bracket is narrower than its target, the bits are identical at 1, 2
//! and 4 threads, and the bridge's per-face `AddOptimal` box is reported
//! beside it.

// The expected values use the platform's sine and cosine; the bracket's
// own tolerance covers their last bits.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    clippy::disallowed_methods
)]

use geospec_facts::{bodies_boxes, BodyBox, BoxOptions};
use geospec_facts_occt::Subject;

fn boxes(kind: i32, threads: usize) -> Vec<BodyBox> {
    let mut s = Subject::synthetic(kind).unwrap_or_else(|e| panic!("kind {kind}: {e}"));
    let definitions: Vec<_> = (0..s.body_count())
        .map(|i| {
            s.export_body(i)
                .expect("solid record")
                .definition
                .unwrap_or_else(|r| panic!("kind {kind}: refused {r}"))
        })
        .collect();
    bodies_boxes(
        &definitions,
        BoxOptions {
            accuracy: 1e-7,
            threads,
        },
    )
    .into_iter()
    .map(|r| r.unwrap_or_else(|r| panic!("kind {kind}: box refused {r}")))
    .collect()
}

fn check(kind: i32, lo: [f64; 3], hi: [f64; 3]) {
    let b = boxes(kind, 1);
    assert_eq!(b.len(), 1, "kind {kind}: one solid");
    let b = &b[0];
    let truth = [-lo[0], -lo[1], -lo[2], hi[0], hi[1], hi[2]];
    let slack = 1e-9 * (1.0 + truth.iter().fold(0.0_f64, |m, x| m.max(x.abs())));
    for k in 0..6 {
        assert!(
            b.sides.inner[k] <= truth[k] + slack && truth[k] <= b.sides.outer[k] + slack,
            "kind {kind} side {k}: {} not in [{}, {}]",
            truth[k],
            b.sides.inner[k],
            b.sides.outer[k]
        );
    }
    assert!(
        b.bound <= b.epsilon,
        "kind {kind}: bound {:e} above {:e}",
        b.bound,
        b.epsilon
    );
    assert!(
        b.fallback_faces.is_empty() && b.capped_faces.is_empty(),
        "kind {kind}: {b:?}"
    );
    for threads in [2, 4] {
        let other = boxes(kind, threads);
        assert_eq!(
            format!("{b:?}"),
            format!("{:?}", other[0]),
            "kind {kind}: threads {threads}"
        );
    }
}

/// The box of a cylinder (or frustum) of radii `r0` at `p` and `r1` at `p +
/// h d` about the unit axis `d`.
fn tube(p: [f64; 3], d: [f64; 3], h: f64, r0: f64, r1: f64) -> ([f64; 3], [f64; 3]) {
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for k in 0..3 {
        let perp = (1.0 - d[k] * d[k]).max(0.0).sqrt();
        let q = p[k] + h * d[k];
        lo[k] = (p[k] - r0 * perp).min(q - r1 * perp);
        hi[k] = (p[k] + r0 * perp).max(q + r1 * perp);
    }
    (lo, hi)
}

#[test]
fn box_and_box_with_a_hole() {
    check(0, [1.0, 2.0, 3.0], [11.0, 22.0, 33.0]);
    check(6, [0.0, 0.0, 0.0], [10.0, 20.0, 30.0]);
}

#[test]
fn tilted_cylinder_as_cylinder_and_as_b_splines() {
    let d = [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0];
    let (lo, hi) = tube([1.0, -2.0, 0.5], d, 7.0, 3.0, 3.0);
    check(1, lo, hi);
    check(7, lo, hi);
}

#[test]
fn frustum() {
    let (lo, hi) = tube([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 5.0, 4.0, 1.0);
    check(2, lo, hi);
}

#[test]
fn spheres() {
    check(3, [0.5, -1.5, -4.5], [5.5, 3.5, 0.5]);
    check(9, [-2.0, -2.0, -2.0], [2.0, 2.0, 2.0]);
    // Kind 3's sphere rotated by 0.7 about (1, 1, 0) and moved by (4, -3, 2).
    let a = [
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
    ];
    let p = [3.0, 1.0, -2.0];
    let (c, s) = (0.7_f64.cos(), 0.7_f64.sin());
    let dot = a[0] * p[0] + a[1] * p[1] + a[2] * p[2];
    let cross = [
        a[1] * p[2] - a[2] * p[1],
        a[2] * p[0] - a[0] * p[2],
        a[0] * p[1] - a[1] * p[0],
    ];
    let t = [4.0, -3.0, 2.0];
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for k in 0..3 {
        let q = p[k] * c + cross[k] * s + a[k] * dot * (1.0 - c) + t[k];
        lo[k] = q - 2.5;
        hi[k] = q + 2.5;
    }
    check(8, lo, hi);
}

#[test]
fn tori() {
    check(4, [-6.5, -6.5, -0.5], [6.5, 6.5, 2.5]);
    check(10, [-5.0, -5.0, -1.0], [5.0, 5.0, 1.0]);
}

#[test]
fn seamed_cylinder_and_periodic_net() {
    check(11, [-2.0, -2.0, 0.0], [2.0, 2.0, 5.0]);
    check(12, [-2.0, -2.0, 0.0], [2.0, 2.0, 5.0]);
}

#[test]
fn addoptimal_reference_is_reported() {
    let mut s = Subject::synthetic(3).expect("sphere");
    let a = s.addoptimal_box(0).expect("AddOptimal");
    let [lo, hi] = a.bounds.expect("a box");
    // AddOptimal's box is the sphere's box (r 2.5 at (3, 1, -2)) to within
    // its sampling and the shape's tolerance.
    let (want_lo, want_hi) = ([0.5, -1.5, -4.5], [5.5, 3.5, 0.5]);
    for k in 0..3 {
        assert!((lo[k] - want_lo[k]).abs() <= 1e-3, "lo {k}: {}", lo[k]);
        assert!((hi[k] - want_hi[k]).abs() <= 1e-3, "hi {k}: {}", hi[k]);
    }
    assert_eq!(a.void_faces, 0);
    let free = s.free_counts().expect("counts");
    assert_eq!((free.faces, free.edges, free.vertices), (0, 0, 0));
    // A solid with 1 free face, 2 free edges and 3 free vertices beside it.
    let mut s = Subject::synthetic(18).expect("box with free topology");
    assert_eq!(s.body_count(), 1);
    let free = s.free_counts().expect("counts");
    assert_eq!((free.faces, free.edges, free.vertices), (1, 2, 3));
}
