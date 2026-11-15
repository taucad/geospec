//! Review A7 round 2: reach boxes split at hard u lines, and joints whose next piece starts across one.
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

/// R5: a noise joint p -> q whose next piece's exact start e_n lies across a
/// hard u line from p, beyond the reach box centred on p. |n| = jl left of
/// the line L, 1 right of it. The face is the rectangle [o+0.5, o+1.5] x
/// [0, 1]; its top is cut at the line: top_a ends at p = L + g (right), and
/// top_b starts at fl(x0 + s) = L whose exact value is L - e (left).
fn r5(o: f64, jl: f64, gulps: f64, efrac: f64) -> (f64, f64, f64) {
    let l = o + 1.0;
    let ulp = l.next_up() - l;
    let (x0, x1) = (o + 0.5, o + 1.5);
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, l, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(jl, 0.0, 0.0),
            Vec3::new(jl, 1.0, 0.0),
            Vec3::new(jl + 1.0, 0.0, 0.0),
            Vec3::new(jl + 1.0, 1.0, 0.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    let p = l + gulps * ulp;
    let s = 0.5 - efrac * ulp; // x0 + s = L - efrac ulp exactly, rounds to L
    assert_eq!(x0 + s, l);
    let face = FaceDefinition {
        surface,
        trims: vec![
            line([x0, 0.0], [x1, 0.0]),
            line([x1, 0.0], [x1, 1.0]),
            line([x1, 1.0], [p, 1.0]),
            TrimPiece {
                curve: Curve2::Line {
                    origin: [x0, 1.0],
                    direction: [1.0, 0.0],
                },
                range: [0.0, s],
                reversed: true,
            },
            line([x0, 1.0], [x0, 0.0]),
        ],
        loops: vec![5],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(jl, 0.5, 0.0), jl + 2.0, 1e-7).unwrap();
    (m.raw[0], m.bound[0], 0.5 * jl + 0.5)
}

#[test]
fn r5_joint_exact_end_across_a_hard_line_outside_the_box() {
    let mut worst: f64 = 0.0;
    for o in [524_288.0, 1024.0] {
        for jl in [10.0, 100.0] {
            for g in [2.0, 4.0] {
                for ef in [0.25, 0.45] {
                    let (area, bound, closed) = r5(o, jl, g, ef);
                    let err = (area - closed).abs();
                    eprintln!("R5 o={o} jl={jl} g={g}ulp e={ef}ulp: err {err:e} bound {bound:e} ratio {:.2}", err / bound);
                    worst = worst.max(err / bound);
                }
            }
        }
    }
    assert!(worst <= 1.0, "worst ratio {worst}");
}

/// R6: as R5 with p on the line (g = 0): the box straddles the line, but
/// each side of its split is `[lo, L.next_down()]` / `[L.next_up(), hi]`,
/// empty when the reach rounds to one ulp.
#[test]
fn r6_joint_on_the_line_box_split_leaves_nothing() {
    let mut worst: f64 = 0.0;
    for (jl, ef) in [(1.0, 0.45), (10.0, 0.45), (100.0, 0.25), (100.0, 0.45)] {
        let (area, bound, closed) = r5(524_288.0, jl, 0.0, ef);
        let err = (area - closed).abs();
        eprintln!(
            "R6 jl={jl} e={ef}ulp: err {err:e} bound {bound:e} ratio {:.2}",
            err / bound
        );
        worst = worst.max(err / bound);
    }
    assert!(worst <= 1.0, "worst ratio {worst}");
}

/// R7: the committed F3 case (a joint that closes only in floating point at
/// u = 2^20 + 0.1) on a plane, and on a flat degree-1 net whose u domain
/// ends at the joint (a clamped domain end is a hard knot line).
fn r7(net: bool) -> (f64, f64, f64) {
    let o = 1_048_576.0;
    let r = 0.1;
    let x1 = o + r;
    let surface = if net {
        Surface::Nurbs(NurbsSurface {
            degree_u: 1,
            degree_v: 1,
            knots_u: vec![o, o, x1, x1],
            knots_v: vec![0.0, 0.0, 1.0, 1.0],
            poles: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(x1 - o, 0.0, 0.0),
                Vec3::new(x1 - o, 1.0, 0.0),
            ],
            weights: None,
            count_u: 2,
            count_v: 2,
        })
    } else {
        Surface::Plane {
            origin: Vec3::new(-o, 0.0, 0.0),
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        }
    };
    let f = FaceDefinition {
        surface,
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
    let m = face_mass(&f, Vec3::new(0.05, 0.5, 0.0), 1.0, 1e-7).unwrap();
    (m.raw[0], m.bound[0], x1 - o)
}

#[test]
fn r7_float_closing_joint_on_a_domain_end() {
    let mut worst: f64 = 0.0;
    for net in [false, true] {
        let (area, bound, closed) = r7(net);
        let err = (area - closed).abs();
        eprintln!(
            "R7 net={net}: err {err:e} bound {bound:e} ratio {:.2}",
            err / bound
        );
        worst = worst.max(err / bound);
    }
    assert!(worst <= 1.0, "worst ratio {worst}");
}

/// R8: R6 at a domain end: the net's u domain ends at L, the face is
/// [o + 0.5, L] x [0, 1], and the top starts at fl(x0 + s) = L with exact
/// L - e (inside the domain).
#[test]
fn r8_joint_on_the_domain_end() {
    let o: f64 = 524_288.0;
    let l = o + 1.0;
    let ulp = l.next_up() - l;
    let x0 = o + 0.5;
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, l, l],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        count_u: 2,
        count_v: 2,
    });
    let s = 0.5 - 0.45 * ulp;
    assert_eq!(x0 + s, l);
    let face = FaceDefinition {
        surface,
        trims: vec![
            line([x0, 0.0], [l, 0.0]),
            line([l, 0.0], [l, 1.0]),
            TrimPiece {
                curve: Curve2::Line {
                    origin: [x0, 1.0],
                    direction: [1.0, 0.0],
                },
                range: [0.0, s],
                reversed: true,
            },
            line([x0, 1.0], [x0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(0.75, 0.5, 0.0), 2.0, 1e-7).unwrap();
    let err = (m.raw[0] - 0.5).abs();
    eprintln!(
        "R8: err {err:e} bound {:e} ratio {:.2}",
        m.bound[0],
        err / m.bound[0]
    );
    assert!(err <= m.bound[0]);
}
