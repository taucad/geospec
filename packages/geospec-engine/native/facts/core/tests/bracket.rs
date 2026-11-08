//! The box with a bracket (P5) on faces and bodies with closed-form boxes or
//! densely sampled extremes: every side's true value lies between its inner
//! and outer values, the bracket is narrower than its target, the bits are
//! identical at 1, 2 and 4 threads, and a trim far outside its net's domain
//! refuses.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::needless_range_loop
)]

use std::f64::consts::{FRAC_PI_2, TAU};

use geospec_facts::{
    bodies_boxes, body_box, face_box, BodyDefinition, BoxOptions, Curve2, FaceDefinition, Frame,
    Material, NurbsCurve2, NurbsSurface, Orientation, RefusalCode, Sides, Surface, TrimPiece, Vec3,
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

fn rectangle(u0: f64, u1: f64, v0: f64, v1: f64) -> Vec<TrimPiece> {
    vec![
        line([u0, v0], [u1, v0]),
        line([u1, v0], [u1, v1]),
        line([u1, v1], [u0, v1]),
        line([u0, v1], [u0, v0]),
    ]
}

/// The rational quadratic full circle, counter-clockwise (or clockwise).
fn circle(c: [f64; 2], r: f64, clockwise: bool) -> TrimPiece {
    let unit = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let w = std::f64::consts::FRAC_1_SQRT_2;
    TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            poles: unit
                .iter()
                .map(|p| [c[0] + r * p[0], c[1] + r * p[1]])
                .collect(),
            weights: Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        }),
        range: [0.0, 4.0],
        reversed: clockwise,
    }
}

fn face(surface: Surface, trims: Vec<TrimPiece>, loops: Vec<usize>) -> FaceDefinition {
    FaceDefinition {
        surface,
        trims,
        loops,
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

fn unit(v: Vec3) -> Vec3 {
    let l = v.length();
    Vec3::new(v.x / l, v.y / l, v.z / l)
}

/// A right-handed frame with `z` along `axis`.
fn frame(origin: Vec3, axis: Vec3) -> Frame {
    let z = unit(axis);
    let helper = if z.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let x = unit(helper - helper.dot(z) * z);
    let y = z.cross(x);
    Frame { origin, x, y, z }
}

/// `side` values of a box `[lo, hi]`: `-lo` then `hi`.
fn sides_of(lo: [f64; 3], hi: [f64; 3]) -> [f64; 6] {
    [-lo[0], -lo[1], -lo[2], hi[0], hi[1], hi[2]]
}

/// Every side's truth within `[inner, outer]` (to `slack`) and the width
/// under `eps`.
fn brackets(what: &str, s: &Sides, truth: [f64; 6], eps: f64, slack: f64) {
    for k in 0..6 {
        assert!(
            s.inner[k] <= truth[k] + slack && truth[k] <= s.outer[k] + slack,
            "{what} side {k}: truth {} not in [{}, {}]",
            truth[k],
            s.inner[k],
            s.outer[k]
        );
        assert!(
            s.outer[k] - s.inner[k] <= eps,
            "{what} side {k}: width {:e} above {eps:e}",
            s.outer[k] - s.inner[k]
        );
    }
}

#[test]
fn planar_square_and_disc() {
    let o = Vec3::new(1.0, 2.0, 3.0);
    let su = Vec3::new(1.0, 1.0, 0.0);
    let sv = Vec3::new(0.0, 1.0, 1.0);
    let plane = Surface::Plane {
        origin: o,
        u_axis: su,
        v_axis: sv,
    };
    let f = face(plane.clone(), rectangle(0.0, 2.0, 0.0, 3.0), vec![4]);
    let b = face_box(&f, 1e-9).unwrap();
    // Corners (0,0), (2,0), (2,3), (0,3).
    let lo = [1.0, 2.0, 3.0];
    let hi = [3.0, 7.0, 6.0];
    brackets("square", &b.sides, sides_of(lo, hi), 1e-9, 0.0);
    assert!(!b.fallback);

    // A disc of radius 1 about (0, 0) in the plane z = 5.
    let flat = Surface::Plane {
        origin: Vec3::new(0.0, 0.0, 5.0),
        u_axis: Vec3::new(1.0, 0.0, 0.0),
        v_axis: Vec3::new(0.0, 1.0, 0.0),
    };
    let d = face(flat, vec![circle([0.0, 0.0], 1.0, false)], vec![1]);
    let b = face_box(&d, 1e-9).unwrap();
    brackets(
        "disc",
        &b.sides,
        sides_of([-1.0, -1.0, 5.0], [1.0, 1.0, 5.0]),
        1e-9,
        0.0,
    );
}

#[test]
fn sphere_cylinder_and_torus_on_tilted_frames() {
    let axis = Vec3::new(1.0, 2.0, 2.0);
    let f = frame(Vec3::new(1.0, -2.0, 0.5), axis);
    let d = unit(axis);
    let dz = [d.x, d.y, d.z];
    let c = [1.0, -2.0, 0.5];
    let perp = |k: usize| (1.0 - dz[k] * dz[k]).max(0.0).sqrt();

    // Sphere r 2.5: the box is the centre +- r.
    let s = face(
        Surface::Sphere {
            frame: f,
            radius: 2.5,
        },
        rectangle(0.0, TAU, -FRAC_PI_2, FRAC_PI_2),
        vec![4],
    );
    let b = face_box(&s, 1e-8).unwrap();
    assert!(!b.fallback, "sphere fell back");
    let lo = [c[0] - 2.5, c[1] - 2.5, c[2] - 2.5];
    let hi = [c[0] + 2.5, c[1] + 2.5, c[2] + 2.5];
    brackets("sphere", &b.sides, sides_of(lo, hi), 1e-8, 1e-12);

    // Cylinder r 3, h 7 along the axis.
    let cyl = face(
        Surface::Cylinder {
            frame: f,
            radius: 3.0,
        },
        rectangle(0.0, TAU, 0.0, 7.0),
        vec![4],
    );
    let b = face_box(&cyl, 1e-8).unwrap();
    let top = [c[0] + 7.0 * dz[0], c[1] + 7.0 * dz[1], c[2] + 7.0 * dz[2]];
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for k in 0..3 {
        lo[k] = c[k].min(top[k]) - 3.0 * perp(k);
        hi[k] = c[k].max(top[k]) + 3.0 * perp(k);
    }
    brackets("cylinder", &b.sides, sides_of(lo, hi), 1e-8, 1e-12);

    // Torus R 5, r 1.5: extent R |d_perp| + r per axis.
    let tor = face(
        Surface::Torus {
            frame: f,
            major_radius: 5.0,
            minor_radius: 1.5,
        },
        rectangle(0.3, 0.3 + TAU, 1.0, 1.0 + TAU),
        vec![4],
    );
    let b = face_box(&tor, 1e-8).unwrap();
    assert!(!b.fallback, "torus fell back");
    for k in 0..3 {
        lo[k] = c[k] - 5.0 * perp(k) - 1.5;
        hi[k] = c[k] + 5.0 * perp(k) + 1.5;
    }
    brackets("torus", &b.sides, sides_of(lo, hi), 1e-8, 1e-12);
}

/// A bicubic net over `[0, 1]^2` (two spans each way) whose z rises to an
/// interior bump.
fn bump() -> NurbsSurface {
    let n = 5;
    let mut poles = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f64 / 4.0, j as f64 / 4.0);
            let z = if (1..4).contains(&i) && (1..4).contains(&j) {
                1.0 + 0.3 * (i as f64) - 0.2 * (j as f64)
            } else {
                0.1 * x
            };
            poles.push(Vec3::new(10.0 * x, 8.0 * y + x, z));
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
    NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles,
        weights: Some((0..25).map(|k| 1.0 + 0.1 * ((k % 3) as f64)).collect()),
        count_u: n,
        count_v: n,
    }
}

/// The largest and smallest coordinates of `face`'s surface at a dense grid
/// of points inside `inside`.
fn sampled(s: &Surface, inside: impl Fn(f64, f64) -> bool) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let n = 400;
    for i in 0..=n {
        for j in 0..=n {
            let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
            if !inside(u, v) {
                continue;
            }
            let (p, _, _) = s.evaluate(u, v).unwrap();
            for (k, x) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[k] = lo[k].min(x);
                hi[k] = hi[k].max(x);
            }
        }
    }
    (lo, hi)
}

#[test]
fn nurbs_bump_with_a_hole() {
    let s = Surface::Nurbs(bump());
    let mut trims = rectangle(0.05, 0.95, 0.1, 0.9);
    trims.push(circle([0.3, 0.6], 0.12, true));
    let f = face(s.clone(), trims, vec![4, 1]);
    let eps = 1e-7;
    let b = face_box(&f, eps).unwrap();
    assert!(!b.fallback && !b.capped, "{b:?}");
    let inside = |u: f64, v: f64| {
        (0.05..=0.95).contains(&u)
            && (0.1..=0.9).contains(&v)
            && ((u - 0.3) * (u - 0.3) + (v - 0.6) * (v - 0.6)) >= 0.12 * 0.12
    };
    let (lo, hi) = sampled(&s, inside);
    let t = sides_of(lo, hi);
    for k in 0..6 {
        // Every sample inside the outer value; the inner value no further
        // below the sampled extreme than the grid's resolution allows.
        assert!(
            t[k] <= b.sides.outer[k],
            "side {k}: sample {} > outer {}",
            t[k],
            b.sides.outer[k]
        );
        assert!(
            b.sides.inner[k] >= t[k] - 1e-3,
            "side {k}: inner {} far below {}",
            b.sides.inner[k],
            t[k]
        );
        assert!(b.sides.outer[k] - b.sides.inner[k] <= eps, "side {k} width");
    }
}

#[test]
fn unclassified_loops_fall_back_to_the_rectangle() {
    // The same bump with a hole outside its outer loop: the loop check
    // refuses, so the face is bounded over its trims' box.
    let s = Surface::Nurbs(bump());
    let mut trims = rectangle(0.05, 0.5, 0.1, 0.5);
    trims.push(circle([0.75, 0.75], 0.1, true));
    let f = face(s.clone(), trims, vec![4, 1]);
    let b = face_box(&f, 1e-7).unwrap();
    assert!(b.fallback);
    let (lo, hi) = sampled(&s, |u, v| {
        (0.05..=0.85).contains(&u) && (0.1..=0.85).contains(&v)
    });
    let t = sides_of(lo, hi);
    for k in 0..6 {
        assert!(t[k] <= b.sides.outer[k] + 1e-12, "side {k}");
    }
}

#[test]
fn bodies_are_identical_at_one_two_and_four_threads() {
    let axis = Vec3::new(0.3, -1.0, 2.0);
    let f = frame(Vec3::new(-4.0, 1.0, 2.0), axis);
    let mut faces = vec![
        face(
            Surface::Sphere {
                frame: f,
                radius: 1.5,
            },
            rectangle(0.0, TAU, -FRAC_PI_2, FRAC_PI_2),
            vec![4],
        ),
        face(
            Surface::Torus {
                frame: f,
                major_radius: 3.0,
                minor_radius: 0.5,
            },
            rectangle(0.0, TAU, 0.0, TAU),
            vec![4],
        ),
        face(
            Surface::Nurbs(bump()),
            rectangle(0.05, 0.95, 0.1, 0.9),
            vec![4],
        ),
    ];
    faces.push(face(
        Surface::Cylinder {
            frame: f,
            radius: 2.0,
        },
        rectangle(0.0, TAU, -1.0, 4.0),
        vec![4],
    ));
    let body = BodyDefinition {
        centre: Vec3::ZERO,
        size: 20.0,
        bounds: None,
        faces,
        file_tolerance: None,
        closed: None,
    };
    let bodies = vec![body.clone(), body];
    let one = bodies_boxes(
        &bodies,
        BoxOptions {
            accuracy: 1e-7,
            threads: 1,
        },
    );
    for threads in [2, 4] {
        let other = bodies_boxes(
            &bodies,
            BoxOptions {
                accuracy: 1e-7,
                threads,
            },
        );
        assert_eq!(
            format!("{one:?}"),
            format!("{other:?}"),
            "threads {threads}"
        );
    }
    let b = one[0].as_ref().unwrap();
    assert!(b.bound <= b.epsilon, "{} > {}", b.bound, b.epsilon);
    assert!(b.fallback_faces.is_empty() && b.capped_faces.is_empty());
}

#[test]
fn a_trim_far_outside_the_domain_refuses() {
    let f = face(
        Surface::Nurbs(bump()),
        rectangle(0.05, 1.2, 0.1, 0.9),
        vec![4],
    );
    let body = BodyDefinition {
        centre: Vec3::ZERO,
        size: 20.0,
        bounds: None,
        faces: vec![f],
        file_tolerance: None,
        closed: None,
    };
    let r = body_box(&body, BoxOptions::default()).unwrap_err();
    assert_eq!(r.code, RefusalCode::TrimOutsideDomain);
    assert_eq!(r.face, Some(0));
}

#[test]
fn a_trim_just_outside_the_domain_widens_the_bracket() {
    let f = face(
        Surface::Nurbs(bump()),
        rectangle(0.05, 1.0 + 1e-9, 0.1, 0.9),
        vec![4],
    );
    let b = face_box(&f, 1e-7).unwrap();
    assert!(
        b.extrapolated && b.excursion > 0.0 && b.excursion < 1e-6,
        "{b:?}"
    );
}

/// A disc trimmed by a rational circle on a tilted cylinder and torus: the
/// extremes lie on the curved trim, where the second-order bound along the
/// piece converges quickly; every sample of the trim lies inside the outer
/// value and the inner value never passes the trim's sampled extreme.
#[test]
fn curved_trims_on_a_cylinder_and_a_torus() {
    let f = frame(Vec3::new(2.0, -1.0, 0.5), Vec3::new(0.4, -1.0, 1.3));
    let surfaces = [
        Surface::Cylinder {
            frame: f,
            radius: 3.0,
        },
        Surface::Torus {
            frame: f,
            major_radius: 5.0,
            minor_radius: 1.5,
        },
    ];
    for s in surfaces {
        let (c, r) = ([1.0, 0.4], 0.7);
        let disc = face(s.clone(), vec![circle(c, r, false)], vec![1]);
        let eps = 1e-9;
        let b = face_box(&disc, eps).unwrap();
        assert!(!b.fallback && !b.capped, "{b:?}");
        assert!(b.work < 20_000, "work {}", b.work);
        let mut t = [f64::NEG_INFINITY; 6];
        let n = 200_000;
        for i in 0..n {
            let a = TAU * i as f64 / n as f64;
            let (p, _, _) = s
                .evaluate(c[0] + r * libm::cos(a), c[1] + r * libm::sin(a))
                .unwrap();
            let x = sides_of([p.x, p.y, p.z], [p.x, p.y, p.z]);
            for k in 0..6 {
                t[k] = t[k].max(x[k]);
            }
        }
        for k in 0..6 {
            assert!(
                t[k] <= b.sides.outer[k] + 1e-12,
                "side {k}: sample {} > outer {}",
                t[k],
                b.sides.outer[k]
            );
            assert!(
                b.sides.inner[k] <= t[k] + 1e-9,
                "side {k}: inner {} > {}",
                b.sides.inner[k],
                t[k]
            );
            assert!(b.sides.outer[k] - b.sides.inner[k] <= eps, "side {k} width");
        }
    }
}
