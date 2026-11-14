//! Joints apart by round-off only, and trim intervals too short to step
//! that move by round-off only (review A7 F1).
//!
//! A joint whose two ends differ by at most 64 unit round-offs of their
//! coordinates is not a closing segment, but the closing integral `-F du`
//! across it grows with `F` (up to the body's size squared), not with the
//! face's area. On a thin face far from `base_v` it exceeds the round-off
//! allowance many times over, so it goes into the bound. The pieces below
//! close up to one such joint; every number is a binary fraction, so the
//! area of the loop closed by its joint is known exactly.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use geospec_facts::{
    body_facts, face_mass, BodyDefinition, Curve2, FaceDefinition, Material, Method, NurbsCurve2,
    NurbsSurface, Options, Orientation, Surface, TrimPiece, Vec3,
};

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

/// The joint's width: 2^-42, below 64 unit round-offs of 33.
const D: f64 = 1.0 / 4_398_046_511_104.0;

/// A 0.125 by 32 strip at `u = 20`, whose right side leans out by `D` at
/// the top, where the top starts back at `u = 20.125`.
fn strip() -> FaceDefinition {
    FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            line([20.0, 0.0], [20.125, 0.0]),
            line([20.125, 0.0], [20.125 + D, 32.0]),
            line([20.125, 32.0], [20.0, 32.0]),
            line([20.0, 32.0], [20.0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

/// The loop closed by its joint: the strip and the lean, `4 + 16 D`.
const CLOSED: f64 = 4.0 + 16.0 * D;

#[test]
fn a_round_off_joint_is_in_the_area_bound() {
    let m = face_mass(&strip(), Vec3::ZERO, 33.0, 1e-7).unwrap();
    let (area, bound) = (m.raw[0], m.bound[0]);
    // The value leaves the joint out: 4 - 16 D, 32 D (7.3e-12) from the
    // closed loop.
    assert_eq!(area, 4.0 - 16.0 * D);
    assert!(
        (area - CLOSED).abs() <= bound,
        "area {area:e} is {:e} from the closed loop, bound {bound:e}",
        (area - CLOSED).abs()
    );
    // Round-off only: the face stays exact.
    assert!(m.exact);
}

#[test]
fn the_body_fact_carries_the_joint() {
    let body = BodyDefinition {
        centre: Vec3::new(20.0, 16.0, 0.0),
        size: 32.0,
        bounds: None,
        faces: vec![strip()],
        file_tolerance: None,
        closed: None,
    };
    let f = body_facts(&body, Options::default()).unwrap();
    let bound = f.area.accuracy.bound.unwrap();
    assert!((f.area.value - CLOSED).abs() <= bound);
    assert_eq!(f.area.accuracy.method, Method::ExactByDegree);
}

#[test]
fn a_joint_apart_in_v_alone_adds_nothing() {
    // The same strip with the top starting at the right side's end, and the
    // left side ending `D` below the bottom's start: `-F du` across a joint
    // in v alone is zero, so the bound is the round-off allowance alone.
    let mut f = strip();
    f.trims[2] = line([20.125 + D, 32.0], [20.0, 32.0]);
    f.trims[3] = line([20.0, 32.0], [20.0, -D]);
    let m = face_mass(&f, Vec3::ZERO, 33.0, 1e-7).unwrap();
    assert!(m.bound[0] < 1e-12);
    assert!(m.exact);
}

#[test]
fn a_round_off_chord_is_in_the_area_bound() {
    // The same strip whose top starts at the right side's end and steps
    // back by `D` in u over a first knot span of 1e-13, too short to step.
    let mut f = strip();
    f.trims[2] = TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree: 1,
            knots: vec![0.0, 0.0, 1e-13, 1.0, 1.0],
            poles: vec![[20.125 + D, 32.0], [20.125, 32.0], [20.0, 32.0]],
            weights: None,
        }),
        range: [0.0, 1.0],
        reversed: false,
    };
    let m = face_mass(&f, Vec3::ZERO, 33.0, 1e-7).unwrap();
    let (area, bound) = (m.raw[0], m.bound[0]);
    assert!(
        (area - CLOSED).abs() <= bound,
        "area {area:e} is {:e} from the closed loop, bound {bound:e}",
        (area - CLOSED).abs()
    );
}

#[test]
fn a_joint_that_closes_only_in_floating_point_is_in_the_bound() {
    // A strip at u = 2^20 whose top runs from c(0.1) = 2^20 + 0.1 back to
    // 2^20, while its right side stands at the rounded 2^20 + 0.1: the
    // computed ends meet, but the definition's are apart by that rounding
    // (review A7 F3). The loop the definition closes spans the rounded
    // width.
    let o = 1_048_576.0;
    let r = 0.1;
    let x1 = o + r;
    let f = FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            line([o, 0.0], [x1, 0.0]),
            line([x1, 0.0], [x1, 1.0]),
            TrimPiece {
                curve: Curve2::Line {
                    origin: [o, 1.0],
                    direction: [1.0, 0.0],
                },
                range: [0.0, r],
                reversed: true,
            },
            line([o, 1.0], [o, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let closed = x1 - o;
    assert_ne!(closed, r);
    let m = face_mass(&f, Vec3::new(o, 0.5, 0.0), 1.0, 1e-7).unwrap();
    let (area, bound) = (m.raw[0], m.bound[0]);
    assert!(
        (area - closed).abs() <= bound,
        "area {area:e} is {:e} from the closed loop, bound {bound:e}",
        (area - closed).abs()
    );
}

#[test]
fn a_joint_on_the_domain_end_is_bounded_inside_the_domain() {
    // The strip on a flat degree-1 net whose u domain ends at the right
    // side's top, `20.125 + D`: the joint's segment runs back into the
    // domain instead of past its end, so the face neither refuses
    // `trim-outside-domain` nor leaves the joint out of its bound.
    let x = 20.125 + D;
    let mut f = strip();
    f.surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![20.0, 20.0, x, x],
        knots_v: vec![0.0, 0.0, 32.0, 32.0],
        poles: vec![
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(20.0, 32.0, 0.0),
            Vec3::new(x, 0.0, 0.0),
            Vec3::new(x, 32.0, 0.0),
        ],
        weights: None,
        count_u: 2,
        count_v: 2,
    });
    let m = face_mass(&f, Vec3::ZERO, 33.0, 1e-7).unwrap();
    let (area, bound) = (m.raw[0], m.bound[0]);
    assert!(
        (area - CLOSED).abs() <= bound,
        "area {area:e} is {:e} from the closed loop, bound {bound:e}",
        (area - CLOSED).abs()
    );
}
