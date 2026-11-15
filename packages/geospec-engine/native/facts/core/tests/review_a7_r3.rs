//! Reviewer's third-round adversarial cases (not for commit).
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

/// R9: hard u lines one ulp apart. Degree-1 net in u with knots
/// o, L, L+1u, L+2u, o+2 (u = ulp(L)); |n| = 1 outside, `jm` on the two
/// one-ulp spans. The top is split at a joint: top_a ends at p = q, and
/// top_b starts at fl(x0 + s) = q whose exact value is q - e. `k` picks q:
/// L + k ulps.
fn r9(jm: f64, k: f64, ef: f64) -> (f64, f64, f64) {
    let o: f64 = 524_288.0;
    let l = o + 1.0;
    let u1 = l.next_up() - l;
    let (x0, x1) = (o + 0.5, o + 1.5);
    let xs = [0.0, 1.0, 1.0 + jm * u1, 1.0 + 2.0 * jm * u1];
    let xe = xs[3] + (o + 2.0 - (l + 2.0 * u1));
    let mut poles = Vec::new();
    for x in [xs[0], xs[1], xs[2], xs[3], xe] {
        poles.push(Vec3::new(x, 0.0, 0.0));
        poles.push(Vec3::new(x, 1.0, 0.0));
    }
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, l, l + u1, l + 2.0 * u1, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 5,
        count_v: 2,
    });
    let q = l + k * u1;
    let s = (q - x0) - ef * u1;
    assert_eq!(x0 + s, q);
    let face = FaceDefinition {
        surface,
        trims: vec![
            line([x0, 0.0], [x1, 0.0]),
            line([x1, 0.0], [x1, 1.0]),
            line([x1, 1.0], [q, 1.0]),
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
    let m = face_mass(&face, Vec3::new(1.0, 0.5, 0.0), 3.0, 1e-7).unwrap();
    // The rectangle: 1 per unit u outside, jm across the two ulp spans.
    let closed = 1.0 - 2.0 * u1 + 2.0 * jm * u1;
    (m.raw[0], m.bound[0], closed)
}

#[test]
fn r9_hard_lines_one_ulp_apart() {
    let mut worst: f64 = 0.0;
    for jm in [1000.0, 1.0e5] {
        for k in [1.0, 2.0, 3.0] {
            for ef in [0.3, 0.45] {
                match std::panic::catch_unwind(|| r9(jm, k, ef)) {
                    Ok((area, bound, closed)) => {
                        let err = (area - closed).abs();
                        eprintln!(
                            "R9 jm={jm} k={k} e={ef}: err {err:e} bound {bound:e} ratio {:.3}",
                            err / bound
                        );
                        worst = worst.max(err / bound);
                    }
                    Err(_) => eprintln!("R9 jm={jm} k={k} e={ef}: panicked (refusal?)"),
                }
            }
        }
    }
    assert!(worst <= 1.0, "worst ratio {worst}");
}

/// R9b: the face [o+0.5, L] x [0, 1] stops at the cluster of one-ulp spans
/// L, L+u, L+2u; the top starts at fl(x0 + s) = L with exact L + e, inside
/// the steep span. Only the joint's box at q reaches the cluster.
fn r9b(jm: f64, ef: f64) -> (f64, f64, f64) {
    let o: f64 = 524_288.0;
    let l = o + 1.0;
    let u1 = l.next_up() - l;
    let x0 = o + 0.5;
    let xs = [0.0, 1.0, 1.0 + jm * u1, 1.0 + 2.0 * jm * u1];
    let xe = xs[3] + (o + 2.0 - (l + 2.0 * u1));
    let mut poles = Vec::new();
    for x in [xs[0], xs[1], xs[2], xs[3], xe] {
        poles.push(Vec3::new(x, 0.0, 0.0));
        poles.push(Vec3::new(x, 1.0, 0.0));
    }
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![o, o, l, l + u1, l + 2.0 * u1, o + 2.0, o + 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 5,
        count_v: 2,
    });
    let s = 0.5 + ef * u1;
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
    // The definition's top reaches e into the steep span: closing it there
    // costs e (jm - 1) beyond the value's own extension.
    (m.raw[0], m.bound[0], 0.5 + ef * u1 * jm)
}

#[test]
fn r9b_box_reaches_into_one_ulp_spans() {
    let mut worst: f64 = 0.0;
    for jm in [1.0, 1000.0, 1.0e5] {
        for ef in [0.3, 0.45] {
            let (area, bound, closed) = r9b(jm, ef);
            let err = (area - closed).abs();
            eprintln!(
                "R9b jm={jm} e={ef}: area {area:e} err {err:e} bound {bound:e} ratio {:.3}",
                err / bound
            );
            worst = worst.max(err / bound);
        }
    }
    assert!(worst <= 1.0, "worst ratio {worst}");
}

/// R10: R8 with the top's exact start past the domain end (L + e, computed
/// L): the box clamps the outside width away.
#[test]
fn r10_exact_end_past_the_domain_end() {
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
    let s = 0.5 + 0.45 * ulp;
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
    for closed in [0.5, 0.5 + 0.45 * ulp] {
        let err = (m.raw[0] - closed).abs();
        eprintln!(
            "R10 closed={closed:e}: area {:e} err {err:e} bound {:e} ratio {:.2}",
            m.raw[0],
            m.bound[0],
            err / m.bound[0]
        );
    }
}
