//! Review A7 attempt 2: adversarial round-off joints and hard-line crossings.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use geospec_facts::{
    face_mass, Curve2, FaceDefinition, Material, NurbsSurface, Orientation, Surface, TrimPiece,
    Vec3,
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
fn ray(origin: [f64; 2], direction: [f64; 2], range: [f64; 2], reversed: bool) -> TrimPiece {
    TrimPiece {
        curve: Curve2::Line { origin, direction },
        range,
        reversed,
    }
}

const D: f64 = 1.0 / 4_398_046_511_104.0;

/// R1: a round-off joint sitting on a hard u knot line, whose gap runs to
/// -u, into the span where |n| = jl; the span past the line has |n| = jr.
fn knot_joint(jl: f64, jr: f64) -> (f64, f64, f64) {
    let x = 20.125 + D;
    let xl = jl * (x - 20.0);
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![20.0, 20.0, x, 30.0, 30.0],
        knots_v: vec![0.0, 0.0, 32.0, 32.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 32.0, 0.0),
            Vec3::new(xl, 0.0, 0.0),
            Vec3::new(xl, 32.0, 0.0),
            Vec3::new(xl + jr * (30.0 - x), 0.0, 0.0),
            Vec3::new(xl + jr * (30.0 - x), 32.0, 0.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    // Right side leans out to the knot line at the top; the top starts back
    // at 20.125: the joint (x, 32) -> (20.125, 32) runs -u from the line.
    let f = FaceDefinition {
        surface,
        trims: vec![
            line([20.0, 0.0], [20.125, 0.0]),
            line([20.125, 0.0], [x, 32.0]),
            line([20.125, 32.0], [20.0, 32.0]),
            line([20.0, 32.0], [20.0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&f, Vec3::new(0.0, 16.0, 0.0), 33.0, 1e-7).unwrap();
    let closed = jl * (4.0 + 16.0 * D);
    (m.raw[0], m.bound[0], closed)
}

#[test]
fn r1_joint_on_hard_line_gap_into_steeper_span() {
    let (area, bound, closed) = knot_joint(10.0, 1.0);
    let err = (area - closed).abs();
    eprintln!(
        "R1 steep-left: area {area:e} closed {closed:e} err {err:e} bound {bound:e} ratio {:.2}",
        err / bound
    );
    assert!(err <= bound, "err {err:e} > bound {bound:e}");
}

#[test]
fn r1_control_joint_on_hard_line_gap_into_flatter_span() {
    let (area, bound, closed) = knot_joint(1.0, 10.0);
    let err = (area - closed).abs();
    eprintln!(
        "R1 control: area {area:e} closed {closed:e} err {err:e} bound {bound:e} ratio {:.2}",
        err / bound
    );
    assert!(err <= bound, "err {err:e} > bound {bound:e}");
}

/// R2: a crossing of a hard u line far from u = 0, where the rounding of
/// u(t), not the bisection, decides where the break lands.
#[test]
fn r2_hard_line_crossing_at_large_u() {
    let o = 524_288.0; // 2^19
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, o + 1.0, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 10.0),
            Vec3::new(2.0, 1.0, 10.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    let d = o + 0.125;
    let (x0, x1) = (d + 0.25, d + 1.75);
    let face = FaceDefinition {
        surface,
        trims: vec![
            ray([d, 0.0], [1.0, 0.0], [0.25, 1.75], false),
            ray([x1, 0.0], [0.0, 1.0], [0.0, 1.0], false),
            ray([d, 1.0], [1.0, 0.0], [0.25, 1.75], true),
            ray([x0, 0.0], [0.0, 1.0], [0.0, 1.0], true),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(1.0, 0.5, 5.0), 3.0, 1e-7).unwrap();
    let exact = 0.625 + 0.875 * 101.0_f64.sqrt();
    let (area, bound) = (m.raw[0], m.bound[0]);
    let err = (area - exact).abs();
    eprintln!(
        "R2: area {area:e} exact {exact:e} err {err:e} bound {bound:e} ratio {:.2}",
        err / bound
    );
    assert!(
        err <= bound + 4.0 * f64::EPSILON * exact,
        "err {err:e} > bound {bound:e}"
    );
}

/// R2 control: the same at u near 0.
#[test]
fn r2_control_hard_line_crossing_near_zero() {
    let o = 0.0;
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, o + 1.0, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 10.0),
            Vec3::new(2.0, 1.0, 10.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    let d = o + 0.125;
    let (x0, x1) = (d + 0.25, d + 1.75);
    let face = FaceDefinition {
        surface,
        trims: vec![
            ray([d, 0.0], [1.0, 0.0], [0.25, 1.75], false),
            ray([x1, 0.0], [0.0, 1.0], [0.0, 1.0], false),
            ray([d, 1.0], [1.0, 0.0], [0.25, 1.75], true),
            ray([x0, 0.0], [0.0, 1.0], [0.0, 1.0], true),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(1.0, 0.5, 5.0), 3.0, 1e-7).unwrap();
    let exact = 0.625 + 0.875 * 101.0_f64.sqrt();
    let (area, bound) = (m.raw[0], m.bound[0]);
    let err = (area - exact).abs();
    eprintln!("R2 control: area {area:e} exact {exact:e} err {err:e} bound {bound:e}");
    assert!(err <= bound + 4.0 * f64::EPSILON * exact);
}

/// R3: a diagonal round-off joint at base_v on a small plane face far from
/// the origin: the noise segment runs at v = base_v where F = 0, but the
/// closing integral along the joint is delta^2 / 2.
#[test]
fn r3_diagonal_joint_at_base_v() {
    let o = 1_048_576.0;
    let h = 1.0 / 1024.0;
    let dl = 1.0 / 134_217_728.0; // 2^-27
    let f = FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            line([o + dl, o + dl], [o + h, o]),
            line([o + h, o], [o + h, o + h]),
            line([o + h, o + h], [o, o + h]),
            line([o, o + h], [o, o]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&f, Vec3::new(o, o, 0.0), 1.0, 1e-7).unwrap();
    let closed = h * h - h * dl / 2.0;
    let (area, bound) = (m.raw[0], m.bound[0]);
    let err = (area - closed).abs();
    eprintln!("R3: area {area:e} closed {closed:e} err {err:e} (dl^2/2 = {:e}) bound {bound:e} ratio {:.1}", dl * dl / 2.0, err / bound);
    assert!(err <= bound, "err {err:e} > bound {bound:e}");
}

/// R4: large F: the noise joint of the strip lifted far above base_v by a
/// long thin tail, so F at the joint is about 1e4.
#[test]
fn r4_large_f_noise_joint() {
    let x = 20.125 + D;
    let top = 8192.0;
    let f = FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            line([20.0, 0.0], [20.125, 0.0]),
            line([20.125, 0.0], [x, top]),
            line([20.125, top], [20.0, top]),
            line([20.0, top], [20.0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&f, Vec3::new(20.0, 4096.0, 0.0), 8200.0, 1e-7).unwrap();
    let closed = 0.125 * top + 0.5 * top * D;
    let (area, bound) = (m.raw[0], m.bound[0]);
    let err = (area - closed).abs();
    eprintln!("R4: err {err:e} bound {bound:e}");
    assert!(err <= bound);
}

/// R2b: as R2 with every joint exact (lines over [0, 1] between exactly
/// representable corners), so no joint widens the bound by an ulp of u.
fn r2b(o: f64) -> (f64, f64, f64) {
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, o + 1.0, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 10.0),
            Vec3::new(2.0, 1.0, 10.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    let (x0, x1) = (o + 0.375, o + 1.875);
    let face = FaceDefinition {
        surface,
        trims: vec![
            line([x0, 0.0], [x1, 0.0]),
            line([x1, 0.0], [x1, 1.0]),
            line([x1, 1.0], [x0, 1.0]),
            line([x0, 1.0], [x0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(1.0, 0.5, 5.0), 3.0, 1e-7).unwrap();
    let exact = 0.625 + 0.875 * 101.0_f64.sqrt();
    (m.raw[0], m.bound[0], exact)
}

#[test]
fn r2b_exact_joints_hard_line_crossing_at_large_u() {
    for o in [0.0, 524_288.0, 524_288.0 + 0.3, 1_000_003.0] {
        let (area, bound, exact) = r2b(o);
        let err = (area - exact).abs();
        eprintln!(
            "R2b o={o}: err {err:e} bound {bound:e} ratio {:.2}",
            err / bound
        );
    }
    for o in [0.0, 524_288.0, 524_288.0 + 0.3, 1_000_003.0] {
        let (area, bound, exact) = r2b(o);
        assert!(
            (area - exact).abs() <= bound + 4.0 * f64::EPSILON * exact,
            "o={o}"
        );
    }
}
