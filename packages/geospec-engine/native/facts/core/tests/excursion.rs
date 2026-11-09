//! The domain excursion of a body given with its kernel's tolerance `T`: a
//! trim may leave a net's domain by up to a relative 1e-3, the widening is
//! capped at `T`, and without a tolerance (or on a face the kernel evaluates
//! as periodic) the limit stays 1e-6.
//!
//! The nets are A7's (`tests/extrapolation.rs`) and A7b's
//! (`tests/absolute_rounding.rs`) at a larger excursion. Each `OCCT_*` box
//! is the per-axis minimum and maximum of `Geom_BSplineSurface::Value` over
//! dense grids of the face's rectangle and of the strip beyond the domain
//! (401 by 401 points each), from the static OCCT prefix with
//! `-ffp-contract=off`, printed with 17 significant digits.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    clippy::excessive_precision
)]

use geospec_facts::{
    bodies_boxes, boxes_of, BodyBox, BodyDefinition, BoxBody, BoxJoints, BoxOptions, Curve2,
    FaceDefinition, Material, NurbsSurface, Orientation, Refusal, RefusalCode, Surface, TrimPiece,
    Vec3,
};

/// The excursion: 1e-4 of the domain's width 1, a hundred times A7's.
const E: f64 = 1e-4;
const X: [f64; 5] = [0.0, 1.0, 1.0, 1.0, 1.0];

fn line(a: [f64; 2], b: [f64; 2]) -> TrimPiece {
    TrimPiece {
        curve: Curve2::Line {
            origin: a,
            direction: [b[0] - a[0], b[1] - a[1]],
        },
        range: [0.0, 1.0],
        reversed: false,
    }
}

fn face(net: NurbsSurface, [u0, u1]: [f64; 2], [v0, v1]: [f64; 2]) -> FaceDefinition {
    FaceDefinition {
        surface: Surface::Nurbs(net),
        trims: vec![
            line([u0, v0], [u1, v0]),
            line([u1, v0], [u1, v1]),
            line([u1, v1], [u0, v1]),
            line([u0, v1], [u0, v0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

/// Degree 3 by 1: pole `(i, j)` at `(X[i], j, 0)`, the first column's
/// weight `w0`, the u knots `0^4, a, 1^4`.
fn strip_net(a: f64, w0: Option<f64>) -> NurbsSurface {
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for (i, x) in X.iter().enumerate() {
        for j in 0..2 {
            poles.push(Vec3::new(*x, j as f64, 0.0));
            weights.push(if i == 0 { w0.unwrap_or(1.0) } else { 1.0 });
        }
    }
    NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: w0.map(|_| weights),
        count_u: 5,
        count_v: 2,
    }
}

/// Degree 3 by 3, knots `0^4, a, 1^4` both ways, pole `(i, j)` at
/// `(X[i], X[j], 0)` with z 1 at the corner pole only.
fn corner_net(a: f64) -> NurbsSurface {
    let mut poles = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let corner = i == 0 && j == 0;
            poles.push(Vec3::new(X[i], X[j], f64::from(u8::from(corner))));
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0];
    NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles,
        weights: None,
        count_u: 5,
        count_v: 5,
    }
}

/// [`strip_net`] mirrored to the high end: x-poles `1, 1, 1, 1, 0`.
fn high_strip_net(a: f64) -> NurbsSurface {
    let mut n = strip_net(a, None);
    for (i, x) in X.iter().rev().enumerate() {
        for j in 0..2 {
            n.poles[i * 2 + j] = Vec3::new(*x, j as f64, 0.0);
        }
    }
    n
}

/// [`corner_net`] mirrored to the high-high corner, knots `0^4, 0.99, 1^4`.
fn high_corner_net() -> NurbsSurface {
    let mut n = corner_net(0.99);
    for i in 0..5 {
        for j in 0..5 {
            let corner = i == 4 && j == 4;
            n.poles[i * 5 + j] = Vec3::new(X[4 - i], X[4 - j], f64::from(u8::from(corner)));
        }
    }
    n
}

/// A7b's net `x0` from the origin whose end-span poles coincide, end span
/// `a`: OCCT's rounding there grows with `x0` and the span's narrowness.
fn far_net(x0: f64, a: f64) -> NurbsSurface {
    let xs = [x0, x0, x0, x0, x0 + 1.0, x0 + 1.0, x0 + 1.0];
    let zs = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let mut poles = Vec::new();
    for (x, z) in xs.iter().zip(zs) {
        for j in 0..2 {
            poles.push(Vec3::new(*x, x0 + j as f64, z));
        }
    }
    NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, a, 2.0 * a, 3.0 * a, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 7,
        count_v: 2,
    }
}

fn body(f: FaceDefinition) -> BodyDefinition {
    BodyDefinition {
        centre: Vec3::ZERO,
        size: 2.0,
        bounds: None,
        faces: vec![f],
        file_tolerance: None,
        closed: None,
    }
}

/// The body of one face given with the kernel's tolerance `t`, the face's
/// surface evaluated as periodic by the kernel when `periodic`.
fn boxed(f: FaceDefinition, t: Option<f64>, periodic: bool) -> Result<BodyBox, Refusal> {
    let joints = vec![BoxJoints {
        bridges: Vec::new(),
        ends: vec![None; f.trims.len()],
        periodic,
    }];
    let b = BoxBody::new(body(f), joints, t)?;
    boxes_of(&[b], BoxOptions::default()).pop().unwrap()
}

/// OCCT's sampled box inside the outer box, and the box's sides ordered.
fn holds(what: &str, b: &BodyBox, occt: [[f64; 3]; 2]) {
    let v = [
        -occt[0][0],
        -occt[0][1],
        -occt[0][2],
        occt[1][0],
        occt[1][1],
        occt[1][2],
    ];
    for k in 0..6 {
        assert!(
            b.sides.inner[k] <= b.sides.outer[k] && v[k] <= b.sides.outer[k],
            "{what} side {k}: OCCT {} against [{}, {}]",
            v[k],
            b.sides.inner[k],
            b.sides.outer[k]
        );
    }
    assert_eq!(b.extrapolated_faces, vec![0], "{what}");
    assert!(b.extrapolation > 0.0, "{what}");
}

const OCCT_A: [[f64; 3]; 2] = [[-0.3310000000000003, 0.0, 0.0], [1.0, 1.0, 0.0]];
const OCCT_B: [[f64; 3]; 2] = [[-1.428345836839267, 0.0, 0.0], [1.0, 1.0, 0.0]];
const OCCT_C: [[f64; 3]; 2] = [
    [-0.030301000000000036, -0.030301000000000026, 0.0],
    [1.0, 1.0, 1.061520150601],
];
const OCCT_HIGH_A: [[f64; 3]; 2] = [[-0.33099999999995977, 0.0, 0.0], [1.0, 1.0, 0.0]];
const OCCT_HIGH_C: [[f64; 3]; 2] = [
    [-0.030300999999996612, -0.0303009999999966, 0.0],
    [1.0, 1.0, 1.061520150600993],
];
const OCCT_FAR_5: [[f64; 3]; 2] = [
    [99999.999995091, 99999.999995091, 0.0],
    [100000.87499249968, 100001.00000493508, 0.8749924996874887],
];
const OCCT_FAR_6: [[f64; 3]; 2] = [
    [99999.99999957532, 99999.99366664886, 0.0],
    [100000.87499925, 100001.00616525114, 0.874999249996875],
];

#[test]
fn a_wider_excursion_holds_occt_on_a7s_nets() {
    // A narrow end span (1e-3) a tenth of a span past, a first weight of
    // 0.05, a corner past both ends, and the first and the corner
    // mirrored to the high end: each boxes under T = 100 and holds OCCT's
    // samples.
    let cases = [
        (
            "A",
            face(strip_net(1e-3, None), [-E, 1.0], [0.0, 1.0]),
            OCCT_A,
        ),
        (
            "B",
            face(strip_net(0.01, Some(0.05)), [-E, 1.0], [0.0, 1.0]),
            OCCT_B,
        ),
        (
            "corner",
            face(corner_net(0.01), [-E, 1.0], [-E, 1.0]),
            OCCT_C,
        ),
        (
            "high A",
            face(high_strip_net(0.999), [0.0, 1.0 + E], [0.0, 1.0]),
            OCCT_HIGH_A,
        ),
        (
            "high corner",
            face(high_corner_net(), [0.0, 1.0 + E], [0.0, 1.0 + E]),
            OCCT_HIGH_C,
        ),
    ];
    for (what, f, occt) in cases {
        let b = boxed(f, Some(100.0), false).unwrap_or_else(|r| panic!("{what}: {r}"));
        holds(what, &b, occt);
    }
}

#[test]
fn a_wider_excursion_holds_occt_rounding_far_from_the_origin() {
    // Review 2's F1 nets, 1e5 from the origin, trimmed to u = -0.9e-3: the
    // widening carries OCCT's absolute rounding (A7b), so its samples lie
    // in the outer box itself, 6e-3 past the exact box on y at a = 1e-6.
    for (a, occt) in [(1e-5, OCCT_FAR_5), (1e-6, OCCT_FAR_6)] {
        let f = face(far_net(1e5, a), [-0.9e-3, 0.5], [0.0, 1.0]);
        let b = boxed(f, Some(100.0), false).unwrap_or_else(|r| panic!("a {a:e}: {r}"));
        holds(&format!("far, a {a:e}"), &b, occt);
    }
}

#[test]
fn a_widening_past_the_tolerance_refuses() {
    let f = || face(strip_net(1e-3, None), [-E, 1.0], [0.0, 1.0]);
    let w = boxed(f(), Some(1.0), false).unwrap().extrapolation;
    // Just under the widening, the face refuses; at it, it boxes.
    let r = boxed(f(), Some(w * (1.0 - 1e-9)), false).unwrap_err();
    assert_eq!(r.code, RefusalCode::TrimOutsideDomain, "{r}");
    assert!(
        r.detail.contains("extrapolation widening") && r.detail.contains("body tolerance"),
        "{r}"
    );
    assert!(boxed(f(), Some(w), false).is_ok());
}

#[test]
fn an_excursion_past_the_sanity_cap_refuses() {
    let f = face(strip_net(0.5, None), [-1.1e-3, 1.0], [0.0, 1.0]);
    let r = boxed(f, Some(1e3), false).unwrap_err();
    assert_eq!(r.code, RefusalCode::TrimOutsideDomain, "{r}");
    assert!(r.detail.contains("outside the surface's domain"), "{r}");
    // Within it the same net boxes.
    let f = face(strip_net(0.5, None), [-0.9e-3, 1.0], [0.0, 1.0]);
    assert!(boxed(f, Some(1e3), false).is_ok());
}

#[test]
fn without_a_tolerance_or_on_a_periodic_face_the_excursion_stays_small() {
    let f = || face(strip_net(1e-3, None), [-E, 1.0], [0.0, 1.0]);
    for r in [
        bodies_boxes(&[body(f())], BoxOptions::default())
            .pop()
            .unwrap(),
        boxes_of(&[BoxBody::of(body(f()))], BoxOptions::default())
            .pop()
            .unwrap(),
        boxed(f(), None, false),
        boxed(f(), Some(1.0), true),
    ] {
        let r = r.unwrap_err();
        assert_eq!(r.code, RefusalCode::TrimOutsideDomain, "{r}");
    }
    // At A7's 1e-6 the periodic face boxes as the definition does.
    let near = || face(strip_net(1e-3, None), [-0.9e-6, 1.0], [0.0, 1.0]);
    let a = bodies_boxes(&[body(near())], BoxOptions::default())
        .pop()
        .unwrap()
        .unwrap();
    let b = boxed(near(), Some(1.0), true).unwrap();
    assert_eq!(a.sides, b.sides);
}

/// Degree 3 by 3 on a 7 by 7 net with a wavy interior: x-poles `0` then
/// `i / 6`, y `j / 6`, z `0.3 ((7 i + 3 j) mod 5)`, the u knots
/// `0^4, a, 0.4, 0.7, 1^4` and the v knots `0^4, 0.3, 0.6, 0.8, 1^4`.
fn wavy_net(a: f64) -> NurbsSurface {
    let mut poles = Vec::new();
    for i in 0..7_usize {
        for j in 0..7_usize {
            let x = if i == 0 { 0.0 } else { i as f64 / 6.0 };
            let z = ((i * 7 + j * 3) % 5) as f64 * 0.3;
            poles.push(Vec3::new(x, j as f64 / 6.0, z));
        }
    }
    NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, a, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 7,
        count_v: 7,
    }
}

const OCCT_WAVY: [[f64; 3]; 2] = [
    [-221.6612499553572, 0.0, -797.9804993035715],
    [1.0, 1.0, 1197.9195001607138],
];

/// The work of [`an_extrapolated_face_refines_only_to_its_widening`]'s box
/// when every side refines to the target alone, measured before the
/// change.
const WORK_TO_THE_TARGET: usize = 157;

#[test]
fn an_extrapolated_face_refines_only_to_its_widening() {
    // An end span of 1e-5 ten spans past: the widening (about 1.2e3) is far
    // above the target (2e-7), so the sides stop refining near it. The box
    // still holds OCCT's samples, with far less work.
    let f = face(wavy_net(1e-5), [-E, 1.0], [0.0, 1.0]);
    let b = boxed(f, Some(1e4), false).unwrap();
    assert!(b.extrapolation > 1e9 * b.epsilon, "{b:?}");
    holds("wavy", &b, OCCT_WAVY);
    assert!(
        4 * b.work <= WORK_TO_THE_TARGET,
        "work {} against {WORK_TO_THE_TARGET}",
        b.work
    );
}
