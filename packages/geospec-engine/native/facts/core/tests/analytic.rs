//! Bodies with closed-form area, volume and centre of mass: every fact within
//! its bound of the closed form, identical bits at 1, 2 and 4 threads, and
//! typed refusals where the definition or a body invariant fails.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use geospec_facts::{
    bodies_facts, body_facts, face_mass, BodyDefinition, BodyFacts, Curve2, FaceDefinition,
    FaceDefinitions, Frame, Material, Method, NurbsCurve2, NurbsSurface, Options, Orientation,
    Refusal, RefusalCode, Surface, TrimPiece, Vec3,
};

const STD: Frame = Frame {
    origin: Vec3::ZERO,
    x: Vec3::new(1.0, 0.0, 0.0),
    y: Vec3::new(0.0, 1.0, 0.0),
    z: Vec3::new(0.0, 0.0, 1.0),
};

fn at(origin: Vec3) -> Frame {
    Frame { origin, ..STD }
}

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

/// The counter-clockwise rectangle `[u0, u1] x [v0, v1]`.
fn rectangle(u0: f64, u1: f64, v0: f64, v1: f64) -> Vec<TrimPiece> {
    vec![
        line([u0, v0], [u1, v0]),
        line([u1, v0], [u1, v1]),
        line([u1, v1], [u0, v1]),
        line([u0, v1], [u0, v0]),
    ]
}

/// The rational quadratic full circle (nine poles), counter-clockwise.
fn circle_poles(c: [f64; 2], r: f64) -> (Vec<[f64; 2]>, Vec<f64>, Vec<f64>) {
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
    let poles = unit
        .iter()
        .map(|p| [c[0] + r * p[0], c[1] + r * p[1]])
        .collect();
    let w = FRAC_1_SQRT_2;
    let weights = vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0];
    let knots = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    (poles, weights, knots)
}

fn circle(c: [f64; 2], r: f64) -> TrimPiece {
    let (poles, weights, knots) = circle_poles(c, r);
    TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree: 2,
            knots,
            poles,
            weights: Some(weights),
        }),
        range: [0.0, 1.0],
        reversed: false,
    }
}

fn face(surface: Surface, trims: Vec<TrimPiece>) -> FaceDefinition {
    FaceDefinition {
        surface,
        loops: vec![trims.len()],
        trims,
        orientation: Orientation::Forward,
        material: Material::OneSide,
    }
}

fn plane(origin: Vec3, u: Vec3, v: Vec3, trims: Vec<TrimPiece>) -> FaceDefinition {
    face(
        Surface::Plane {
            origin,
            u_axis: u,
            v_axis: v,
        },
        trims,
    )
}

fn body(lo: Vec3, hi: Vec3, faces: Vec<FaceDefinition>) -> BodyDefinition {
    let e = hi - lo;
    BodyDefinition {
        centre: 0.5 * (lo + hi),
        size: e.max_abs(),
        bounds: Some([lo, hi]),
        faces,
        file_tolerance: None,
        closed: Some(true),
    }
}

/// The box `[o, o + (a, b, c)]`, every face's natural normal outward.
fn cuboid(o: Vec3, a: f64, b: f64, c: f64) -> BodyDefinition {
    let (x, y, z) = (
        Vec3::new(a, 0.0, 0.0),
        Vec3::new(0.0, b, 0.0),
        Vec3::new(0.0, 0.0, c),
    );
    let unit = || rectangle(0.0, 1.0, 0.0, 1.0);
    body(
        o,
        o + x + y + z,
        vec![
            plane(o + z, x, y, unit()),
            plane(o, y, x, unit()),
            plane(o + x, y, z, unit()),
            plane(o, z, y, unit()),
            plane(o + y, z, x, unit()),
            plane(o, x, z, unit()),
        ],
    )
}

/// Planar disks closing a solid of revolution about z: the top one (normal
/// +z) at `top`, the bottom one (normal -z) at `bottom`.
fn disks(bottom: (f64, f64), top: (f64, f64)) -> [FaceDefinition; 2] {
    let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    [
        plane(
            Vec3::new(0.0, 0.0, top.0),
            x,
            y,
            vec![circle([0.0, 0.0], top.1)],
        ),
        plane(
            Vec3::new(0.0, 0.0, bottom.0),
            y,
            x,
            vec![circle([0.0, 0.0], bottom.1)],
        ),
    ]
}

struct Expect {
    volume: f64,
    area: f64,
    centroid: Vec3,
}

/// Every fact within its bound of the closed form, and the bounds not loose
/// beyond `slack` relative.
fn check(name: &str, facts: &BodyFacts, e: &Expect, slack: f64) {
    let vb = facts.volume.accuracy.bound.unwrap();
    let ab = facts.area.accuracy.bound.unwrap();
    let ve = (facts.volume.value - e.volume).abs();
    let ae = (facts.area.value - e.area).abs();
    assert!(
        ve <= vb,
        "{name}: volume {} off {} by {ve} > bound {vb}",
        facts.volume.value,
        e.volume
    );
    assert!(
        ae <= ab,
        "{name}: area {} off {} by {ae} > bound {ab}",
        facts.area.value,
        e.area
    );
    assert!(vb <= slack * e.volume, "{name}: volume bound {vb} loose");
    assert!(ab <= slack * e.area, "{name}: area bound {ab} loose");
    let c = facts.centroid.unwrap();
    let d = c.value - e.centroid;
    for (err, b) in [
        (d.x, c.axis_bounds.x),
        (d.y, c.axis_bounds.y),
        (d.z, c.axis_bounds.z),
    ] {
        assert!(
            err.abs() <= b,
            "{name}: centroid {:?} off {:?} beyond {:?}",
            c.value,
            e.centroid,
            c.axis_bounds
        );
    }
    assert!(
        facts.closure[0].abs() <= facts.closure[1].max(1e-12 * e.area),
        "{name}: closure {:?}",
        facts.closure
    );
}

fn facts(b: &BodyDefinition) -> BodyFacts {
    let f = body_facts(b, Options::default()).unwrap_or_else(|r| panic!("{r}"));
    assert!(f.closed_within_bounds, "closure {:?}", f.closure);
    f
}

#[test]
fn cuboid_is_exact_by_degree() {
    for (o, a, b, c) in [
        (Vec3::ZERO, 1.0, 1.0, 1.0),
        (Vec3::new(1000.0, -2000.0, 3000.0), 3.0, 0.5, 7.25),
        (Vec3::new(-1e-3, 2e-3, 0.0), 1e-3, 2e-3, 3e-3),
    ] {
        let f = facts(&cuboid(o, a, b, c));
        check(
            "cuboid",
            &f,
            &Expect {
                volume: a * b * c,
                area: 2.0 * (a * b + b * c + c * a),
                centroid: o + Vec3::new(a / 2.0, b / 2.0, c / 2.0),
            },
            1e-10,
        );
        assert_eq!(f.volume.accuracy.method, Method::ExactByDegree);
        assert_eq!(f.area.accuracy.method, Method::ExactByDegree);
    }
}

#[test]
fn reversed_faces_count_with_the_opposite_sign() {
    let mut b = cuboid(Vec3::ZERO, 2.0, 3.0, 4.0);
    // The top face with its parameters swapped: its natural normal points in.
    b.faces[0] = FaceDefinition {
        orientation: Orientation::Reversed,
        ..plane(
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            rectangle(0.0, 1.0, 0.0, 1.0),
        )
    };
    let f = facts(&b);
    assert!((f.volume.value - 24.0).abs() <= f.volume.accuracy.bound.unwrap());
}

#[test]
fn cylinder_with_disks() {
    let (r, h) = (2.5, 7.0);
    let lateral = face(
        Surface::Cylinder {
            frame: STD,
            radius: r,
        },
        rectangle(0.0, TAU, 0.0, h),
    );
    let [top, bottom] = disks((0.0, r), (h, r));
    let b = body(
        Vec3::new(-r, -r, 0.0),
        Vec3::new(r, r, h),
        vec![lateral, top, bottom],
    );
    let f = facts(&b);
    check(
        "cylinder",
        &f,
        &Expect {
            volume: PI * r * r * h,
            area: TAU * r * h + TAU * r * r,
            centroid: Vec3::new(0.0, 0.0, h / 2.0),
        },
        1e-6,
    );
    assert_eq!(f.volume.accuracy.method, Method::Adaptive);
}

#[test]
fn cone_frustum_with_disks() {
    let (r1, a, l) = (3.0, 0.4_f64, 5.0);
    let (s, c) = (libm::sin(a), libm::cos(a));
    let (h, r2) = (l * c, r1 + l * s);
    let lateral = face(
        Surface::Cone {
            frame: STD,
            radius: r1,
            semi_angle: a,
        },
        rectangle(0.0, TAU, 0.0, l),
    );
    let [top, bottom] = disks((0.0, r1), (h, r2));
    let b = body(
        Vec3::new(-r2, -r2, 0.0),
        Vec3::new(r2, r2, h),
        vec![lateral, top, bottom],
    );
    let volume = PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
    let zc = h * (r1 * r1 + 2.0 * r1 * r2 + 3.0 * r2 * r2) / (4.0 * (r1 * r1 + r1 * r2 + r2 * r2));
    check(
        "cone",
        &facts(&b),
        &Expect {
            volume,
            area: PI * (r1 + r2) * l + PI * (r1 * r1 + r2 * r2),
            centroid: Vec3::new(0.0, 0.0, zc),
        },
        1e-6,
    );
}

#[test]
fn sphere_and_torus() {
    let r = 4.0;
    let o = Vec3::new(10.0, -3.0, 2.0);
    let sphere = body(
        o - Vec3::new(r, r, r),
        o + Vec3::new(r, r, r),
        vec![face(
            Surface::Sphere {
                frame: at(o),
                radius: r,
            },
            rectangle(0.0, TAU, -FRAC_PI_2, FRAC_PI_2),
        )],
    );
    check(
        "sphere",
        &facts(&sphere),
        &Expect {
            volume: 4.0 / 3.0 * PI * r * r * r,
            area: 4.0 * PI * r * r,
            centroid: o,
        },
        1e-6,
    );
    let (big, small) = (5.0, 1.5);
    let e = big + small;
    let torus = body(
        Vec3::new(-e, -e, -small),
        Vec3::new(e, e, small),
        vec![face(
            Surface::Torus {
                frame: STD,
                major_radius: big,
                minor_radius: small,
            },
            rectangle(0.0, TAU, 0.0, TAU),
        )],
    );
    check(
        "torus",
        &facts(&torus),
        &Expect {
            volume: 2.0 * PI * PI * big * small * small,
            area: 4.0 * PI * PI * big * small,
            centroid: Vec3::ZERO,
        },
        1e-6,
    );
}

/// A cylinder's lateral face as a rational NURBS net: a quadratic circle in u
/// swept linearly along z.
fn nurbs_cylinder(r: f64, h: f64) -> FaceDefinition {
    let (poles2, weights, knots) = circle_poles([0.0, 0.0], r);
    let mut poles = Vec::new();
    let mut w = Vec::new();
    for (p, wi) in poles2.iter().zip(&weights) {
        for z in [0.0, h] {
            poles.push(Vec3::new(p[0], p[1], z));
            w.push(*wi);
        }
    }
    face(
        Surface::Nurbs(NurbsSurface {
            degree_u: 2,
            degree_v: 1,
            knots_u: knots,
            knots_v: vec![0.0, 0.0, 1.0, 1.0],
            poles,
            weights: Some(w),
            count_u: 9,
            count_v: 2,
        }),
        rectangle(0.0, 1.0, 0.0, 1.0),
    )
}

fn nurbs_cylinder_body(r: f64, h: f64) -> BodyDefinition {
    let [top, bottom] = disks((0.0, r), (h, r));
    body(
        Vec3::new(-r, -r, 0.0),
        Vec3::new(r, r, h),
        vec![nurbs_cylinder(r, h), top, bottom],
    )
}

#[test]
fn nurbs_cylinder_with_disks() {
    let (r, h) = (12.0, 30.0);
    check(
        "nurbs cylinder",
        &facts(&nurbs_cylinder_body(r, h)),
        &Expect {
            volume: PI * r * r * h,
            area: TAU * r * h + TAU * r * r,
            centroid: Vec3::new(0.0, 0.0, h / 2.0),
        },
        1e-6,
    );
}

#[test]
fn a_bicubic_net_with_interior_knots_closes_a_cuboid() {
    // The top face of a unit cube as a non-rational bicubic net with interior
    // knots, its poles on z = 1 at their Greville points (an exact plane), and
    // trims crossing its knot lines diagonally.
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0];
    let greville: Vec<f64> = (0..6)
        .map(|i| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0)
        .collect();
    let mut poles = Vec::new();
    for &gu in &greville {
        for &gv in &greville {
            poles.push(Vec3::new(gu, gv, 1.0));
        }
    }
    let net = Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles,
        weights: None,
        count_u: 6,
        count_v: 6,
    });
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0] = face(
        net,
        vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    );
    check(
        "bicubic top",
        &facts(&b),
        &Expect {
            volume: 1.0,
            area: 6.0,
            centroid: Vec3::new(0.5, 0.5, 0.5),
        },
        1e-6,
    );
}

/// The derivative of the uniform cubic B-spline basis on `[0, 4]`.
fn bump_slope(s: f64) -> f64 {
    if s < 1.0 {
        0.5 * s * s
    } else if s < 2.0 {
        ((-9.0 * s + 24.0) * s - 12.0) / 6.0
    } else if s < 3.0 {
        ((9.0 * s - 48.0) * s + 60.0) / 6.0
    } else {
        -0.5 * (4.0 - s) * (4.0 - s)
    }
}

/// Review A4-R1 P3: a flat bicubic net on `[0, 1]^2` with uniform single
/// knots (`nu` by `nv` spans, poles at their Greville points, so `x = u` and
/// `y = v`), pole column `j` (`along_u`) or pole row `j` raised by `amp`: a
/// rib four spans wide, `z = amp N_j`. Its area is
/// `1 - 4/n + (1/n) ∫_0^4 sqrt(1 + (amp n N'(s))^2) ds`, here by 5-point
/// Gauss-Legendre on 64 cells per span.
fn ribbed_net(nu: usize, nv: usize, j: usize, along_u: bool, amp: f64) -> (FaceDefinition, f64) {
    let clamped = |n: usize| {
        let mut k = vec![0.0; 4];
        k.extend((1..n).map(|i| i as f64 / n as f64));
        k.extend([1.0; 4]);
        k
    };
    let (ku, kv) = (clamped(nu), clamped(nv));
    let greville = |k: &[f64], i: usize| k.iter().skip(i + 1).take(3).sum::<f64>() / 3.0;
    let mut poles = Vec::new();
    for i in 0..nu + 3 {
        for jj in 0..nv + 3 {
            let raised = if along_u { i == j } else { jj == j };
            poles.push(Vec3::new(
                greville(&ku, i),
                greville(&kv, jj),
                if raised { amp } else { 0.0 },
            ));
        }
    }
    let net = Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: ku,
        knots_v: kv,
        poles,
        weights: None,
        count_u: nu + 3,
        count_v: nv + 3,
    });
    let n = if along_u { nu } else { nv };
    assert!((3..n).contains(&j), "pole {j} outside the uniform knots");
    let scale = amp * n as f64;
    let (x, w) = (
        [
            0.0,
            0.538_469_310_105_683_1,
            -0.538_469_310_105_683_1,
            0.906_179_845_938_664,
            -0.906_179_845_938_664,
        ],
        [
            0.568_888_888_888_888_9,
            0.478_628_670_499_366_5,
            0.478_628_670_499_366_5,
            0.236_926_885_056_189_1,
            0.236_926_885_056_189_1,
        ],
    );
    let cells = 4 * 64;
    let h = 4.0 / f64::from(cells);
    let mut rib = 0.0;
    for c in 0..cells {
        let mid = (f64::from(c) + 0.5) * h;
        for (xk, wk) in x.iter().zip(&w) {
            let d = scale * bump_slope(mid + 0.5 * h * xk);
            rib += 0.5 * h * wk * (1.0 + d * d).sqrt();
        }
    }
    let area = 1.0 - 4.0 / n as f64 + rib / n as f64;
    (face(net, rectangle(0.0, 1.0, 0.0, 1.0)), area)
}

/// Review A4-R1 P3: a rib on a dense uniform net, along v (the inner
/// integral's soft v breaks) and along u (the outer soft u breaks), at the
/// default accuracy: the area's bound covers its error at every position
/// (grouped panels missed the rib by 4.7% within a bound of 3e-12).
#[test]
fn a_rib_on_a_dense_net_stays_within_the_area_bound() {
    for (nu, nv, along_u) in [(2, 128, false), (128, 2, true)] {
        // The grouped build missed the rib at poles 35 and 99.
        for j in [3, 35, 99] {
            let (f, area) = ribbed_net(nu, nv, j, along_u, 0.05);
            let m = face_mass(&f, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7).unwrap();
            let err = (m.raw[0] - area).abs();
            assert!(
                err <= m.bound[0] + 1e-13,
                "{nu}x{nv} rib at {j}: area {} off {area} by {err:e} > bound {:e}",
                m.raw[0],
                m.bound[0]
            );
        }
    }
}

fn bits(f: &BodyFacts) -> Vec<u64> {
    let mut out = vec![
        f.area.value.to_bits(),
        f.volume.value.to_bits(),
        f.area.accuracy.bound.unwrap().to_bits(),
        f.volume.accuracy.bound.unwrap().to_bits(),
    ];
    if let Some(c) = f.centroid {
        out.extend(
            [
                c.value.x,
                c.value.y,
                c.value.z,
                c.axis_bounds.x,
                c.axis_bounds.y,
                c.axis_bounds.z,
            ]
            .map(f64::to_bits),
        );
    }
    for m in &f.faces {
        out.extend(m.raw.iter().chain(m.bound.iter()).map(|x| x.to_bits()));
    }
    out
}

#[test]
fn facts_are_bit_identical_at_1_2_and_4_threads() {
    let mut bodies = vec![
        nurbs_cylinder_body(12.0, 30.0),
        cuboid(Vec3::new(5.0, 6.0, 7.0), 1.0, 2.0, 3.0),
    ];
    // One body with many faces: a stack of NURBS cylinders' lateral faces
    // between two disks is not closed, so take three closed cylinders' faces
    // in one body (three disjoint shells).
    let mut many = nurbs_cylinder_body(1.0, 2.0);
    for k in 1..4 {
        let mut other = nurbs_cylinder_body(1.0 + f64::from(k), 2.0);
        many.faces.append(&mut other.faces);
    }
    many.bounds = None;
    many.size = 8.0;
    bodies.push(many);
    let reference: Vec<Vec<u64>> = bodies_facts(&bodies, Options::default())
        .iter()
        .map(|r| bits(r.as_ref().unwrap()))
        .collect();
    for threads in [2, 4] {
        let options = Options {
            threads,
            ..Options::default()
        };
        let got: Vec<Vec<u64>> = bodies_facts(&bodies, options)
            .iter()
            .map(|r| bits(r.as_ref().unwrap()))
            .collect();
        assert_eq!(got, reference, "{threads} threads");
        for b in &bodies {
            assert_eq!(bits(&body_facts(b, options).unwrap()), bits(&facts(b)));
        }
    }
}

fn refusal(b: &BodyDefinition) -> Refusal {
    body_facts(b, Options::default()).unwrap_err()
}

#[test]
fn a_trim_outside_its_nets_domain_refuses() {
    let mut b = nurbs_cylinder_body(1.0, 1.0);
    b.faces[0].trims = rectangle(0.0, 1.0, 0.0, 1.5);
    let r = refusal(&b);
    assert_eq!((r.code, r.face), (RefusalCode::TrimOutsideDomain, Some(0)));
}

/// A body whose faces 1 and 3 refuse (a trim leaving the net's domain only
/// after a trim inside it, and a clockwise outer loop): the body refuses
/// with face 1's refusal at 1, 2 and 4 threads, whichever faces after it
/// are integrated or skipped.
#[test]
fn a_body_refuses_with_its_first_refusing_face_at_every_thread_count() {
    let mut b = nurbs_cylinder_body(1.0, 1.0);
    let cylinder = b.faces[0].clone();
    let mut outside = cylinder.clone();
    outside.trims = rectangle(0.0, 1.0, 0.0, 1.5);
    let mut clockwise = cylinder.clone();
    clockwise.trims = rectangle(0.0, 1.0, 0.0, 1.0)
        .into_iter()
        .rev()
        .map(|mut t| {
            t.reversed = !t.reversed;
            t
        })
        .collect();
    let centre = b.centre;
    for (f, code) in [
        (&outside, RefusalCode::TrimOutsideDomain),
        (&clockwise, RefusalCode::TrimOrientation),
    ] {
        let r = face_mass(f, centre, 1.0, 1e-7).unwrap_err();
        assert_eq!(r.code, code, "{r}");
    }
    b.faces = vec![
        b.faces[1].clone(),
        outside,
        cylinder.clone(),
        clockwise,
        cylinder,
        b.faces[2].clone(),
    ];
    let want = refusal(&b);
    assert_eq!(
        (want.code, want.face),
        (RefusalCode::TrimOutsideDomain, Some(1)),
        "{want}"
    );
    for threads in [1, 2, 4] {
        let got = bodies_facts(
            &[b.clone(), b.clone()],
            Options {
                threads,
                ..Options::default()
            },
        );
        for r in got {
            assert_eq!(r.unwrap_err(), want, "{threads} threads");
        }
    }
}

#[test]
fn malformed_definitions_refuse() {
    let mut b = nurbs_cylinder_body(1.0, 1.0);
    if let Surface::Nurbs(n) = &mut b.faces[0].surface {
        n.knots_u.pop();
    }
    let r = refusal(&b);
    assert_eq!((r.code, r.face), (RefusalCode::InvalidDefinition, Some(0)));
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[2].trims[1].range = [1.0, 1.0];
    assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition);
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.size = f64::NAN;
    assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition);
}

#[test]
fn an_open_shell_refuses_its_volume() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces.remove(0);
    assert_eq!(refusal(&b).code, RefusalCode::OpenBody);
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.closed = Some(false);
    assert_eq!(refusal(&b).code, RefusalCode::OpenBody);
}

/// Review finding 2: a cube missing its x = 1 face has a closed n_z integral
/// and the right z n_z volume; the other volume forms and n_x expose it.
#[test]
fn a_cube_missing_a_side_face_refuses() {
    for missing in 0..6 {
        let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
        b.faces.remove(missing);
        let r = refusal(&b);
        assert_eq!(r.code, RefusalCode::OpenBody, "face {missing}: {r}");
    }
}

/// Review finding 1: the top face with tiny axes and huge trims, and a cube of
/// side 1e-7, keep their volume and centre of mass.
#[test]
fn tiny_parameterisations_and_tiny_bodies_keep_their_volume() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0] = plane(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1e-7, 0.0, 0.0),
        Vec3::new(0.0, 1e-7, 0.0),
        rectangle(0.0, 1e7, 0.0, 1e7),
    );
    check(
        "reparameterised top",
        &facts(&b),
        &Expect {
            volume: 1.0,
            area: 6.0,
            centroid: Vec3::new(0.5, 0.5, 0.5),
        },
        1e-10,
    );
    let s = 1e-7;
    check(
        "tiny cube",
        &facts(&cuboid(Vec3::ZERO, s, s, s)),
        &Expect {
            volume: s * s * s,
            area: 6.0 * s * s,
            centroid: Vec3::new(s / 2.0, s / 2.0, s / 2.0),
        },
        1e-10,
    );
}

fn holed_top(hole: Vec<TrimPiece>) -> BodyDefinition {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    let top = b.faces.first_mut().unwrap();
    top.loops.push(hole.len());
    top.trims.extend(hole);
    b
}

/// Review finding 3: a hole must run clockwise and loops must close.
#[test]
fn trim_loops_must_close_and_holes_run_clockwise() {
    // A clockwise square hole on the cube's top face: the area drops by it
    // (the face no longer bounds the volume, so check the face itself).
    let mut cw = rectangle(0.25, 0.75, 0.25, 0.75);
    cw.reverse();
    for p in &mut cw {
        p.reversed = true;
    }
    let b = holed_top(cw);
    let m = geospec_facts::face_mass(&b.faces[0], b.centre, b.size, 1e-7).unwrap();
    assert!(
        (m.raw[0] - 0.75).abs() <= m.bound[0].max(1e-15),
        "{:?}",
        m.raw
    );
    // The same hole counter-clockwise refuses.
    let b = holed_top(rectangle(0.25, 0.75, 0.25, 0.75));
    let r = refusal(&b);
    assert_eq!(
        (r.code, r.face),
        (RefusalCode::TrimOrientation, Some(0)),
        "{r}"
    );
    // A loop with a gap refuses.
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0].trims[1] = line([1.0, 0.0], [1.0, 0.9]);
    let r = refusal(&b);
    assert_eq!(
        (r.code, r.face),
        (RefusalCode::OpenTrimLoop, Some(0)),
        "{r}"
    );
    // Loops that do not partition the trims refuse.
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[1].loops = vec![3];
    assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition);
}

/// A cylinder's lateral face bounded by two loops winding once around it in
/// opposite directions (no seam).
#[test]
fn winding_loops_bound_a_cylinder() {
    let (r, h) = (2.0, 3.0);
    let mut lateral = face(
        Surface::Cylinder {
            frame: STD,
            radius: r,
        },
        vec![line([0.0, 0.0], [TAU, 0.0]), line([TAU, h], [0.0, h])],
    );
    lateral.loops = vec![1, 1];
    let [top, bottom] = disks((0.0, r), (h, r));
    let b = body(
        Vec3::new(-r, -r, 0.0),
        Vec3::new(r, r, h),
        vec![lateral.clone(), top, bottom],
    );
    check(
        "winding cylinder",
        &facts(&b),
        &Expect {
            volume: PI * r * r * h,
            area: TAU * r * h + TAU * r * r,
            centroid: Vec3::new(0.0, 0.0, h / 2.0),
        },
        1e-6,
    );
    // CodeRabbit's review of b80e491: overlapping holes on a face bounded by
    // winding loops refuse.
    let mut holed = lateral.clone();
    holed.trims.extend(clockwise(0.5, 2.0, 0.5, 2.0));
    holed.trims.extend(clockwise(1.0, 2.5, 1.0, 2.5));
    holed.loops = vec![1, 1, 4, 4];
    assert_eq!(
        holed.check_loops().unwrap_err().code,
        RefusalCode::TrimOrientation
    );
    // Holes on either side of the seam overlap there (one reaches past
    // u = 2 pi into the other) and refuse; apart, they pass.
    let seam = |right: f64| {
        let mut f = lateral.clone();
        f.trims.extend(clockwise(0.05, 0.5, 1.0, 2.0));
        f.trims.extend(clockwise(TAU - 0.2, right, 1.2, 1.8));
        f.loops = vec![1, 1, 4, 4];
        f.check_loops().map_err(|r| r.code)
    };
    assert_eq!(seam(TAU + 0.1), Err(RefusalCode::TrimOrientation));
    assert!(seam(TAU - 0.01).is_ok());
    // CodeRabbit 4490503306: a hole inside another hole, and a hole outside
    // the band between the winding loops (above v = h on the unbounded
    // cylinder), refuse; a hole inside the band passes.
    let holes = |hs: &[[f64; 4]]| {
        let mut f = lateral.clone();
        f.loops = vec![1, 1];
        for c in hs {
            f.trims.extend(clockwise(c[0], c[1], c[2], c[3]));
            f.loops.push(4);
        }
        f.check_loops().map_err(|r| r.code)
    };
    assert_eq!(
        holes(&[[0.5, 2.5, 0.5, 2.5], [1.0, 2.0, 1.0, 2.0]]),
        Err(RefusalCode::TrimOrientation)
    );
    assert_eq!(
        holes(&[[1.0, 2.0, 3.5, 4.0]]),
        Err(RefusalCode::TrimOrientation)
    );
    assert!(holes(&[[1.0, 2.0, 1.0, 2.0]]).is_ok());
    // CodeRabbit 4490528429: a hole inside another hole's periodic image
    // across the seam (u in [0.1, 0.4] inside [2 pi - 0.3, 2 pi + 0.6] one
    // period back) refuses; beside it, it passes.
    assert_eq!(
        holes(&[[TAU - 0.3, TAU + 0.6, 0.5, 2.5], [0.1, 0.4, 1.0, 2.0]]),
        Err(RefusalCode::TrimOrientation)
    );
    assert!(holes(&[[TAU - 0.3, TAU - 0.1, 0.5, 2.5], [0.1, 0.4, 1.0, 2.0]]).is_ok());
    // One winding loop alone does not bound the face.
    lateral.trims.pop();
    lateral.loops = vec![1];
    assert_eq!(
        lateral.check_loops().unwrap_err().code,
        RefusalCode::TrimOrientation
    );
}

/// CodeRabbit 4490475959: winding loops are compared with each other's
/// periodic images. One loop runs (0, 0) -> (2 pi, 0); the other, written one
/// period over, runs (4 pi, 1) -> (3 pi, -1) -> (2 pi, 1). On the cylinder it
/// crosses the first twice (at u = pi / 2 and 3 pi / 2 one period back),
/// though the two parameter paths do not meet; it refuses. Kept above the
/// first loop, the same pair passes. A loop spanning too many periods for
/// its images to be checked refuses.
#[test]
fn winding_loops_crossing_through_their_periodic_images_refuse() {
    let pair = |lo: f64| {
        let mut f = face(
            Surface::Cylinder {
                frame: STD,
                radius: 2.0,
            },
            vec![
                line([0.0, 0.0], [TAU, 0.0]),
                line([2.0 * TAU, lo + 2.0], [1.5 * TAU, lo]),
                line([1.5 * TAU, lo], [TAU, lo + 2.0]),
            ],
        );
        f.loops = vec![1, 2];
        f.check_loops().map_err(|r| r.code)
    };
    assert_eq!(pair(-1.0), Err(RefusalCode::TrimOrientation));
    assert!(pair(1.0).is_ok());
    // A winding loop wandering 40 periods along u needs more images than
    // the face allows: undecidable, it refuses.
    let mut far = face(
        Surface::Cylinder {
            frame: STD,
            radius: 2.0,
        },
        vec![
            line([0.0, 0.0], [40.0 * TAU, 0.5]),
            line([40.0 * TAU, 0.5], [TAU, 0.0]),
            line([TAU, 3.0], [0.0, 3.0]),
        ],
    );
    far.loops = vec![2, 1];
    assert_eq!(
        far.check_loops().map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// Review findings 6 and 9: a non-finite or negative file tolerance, a centre
/// outside the box, no faces and a face without trims are invalid.
#[test]
fn invalid_body_numbers_and_empty_definitions_refuse() {
    for t in [f64::NAN, -1.0, f64::INFINITY] {
        let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
        b.file_tolerance = Some(t);
        assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition, "{t}");
    }
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.file_tolerance = Some(1e-6);
    let f = facts(&b);
    assert_eq!(f.volume.accuracy.gap_bound, Some(6e-6));
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.centre = Vec3::new(2.0, 0.5, 0.5);
    assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition);
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces.clear();
    assert_eq!(refusal(&b).code, RefusalCode::InvalidDefinition);
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[4].trims.clear();
    b.faces[4].loops.clear();
    let r = refusal(&b);
    assert_eq!((r.code, r.face), (RefusalCode::InvalidDefinition, Some(4)));
}

/// Review finding 12: extra multiplicity at a knot vector's end (an unused
/// last pole) leaves an empty span at the domain's end, which must not be
/// evaluated.
#[test]
fn extra_end_knot_multiplicity_evaluates() {
    let (r, h) = (3.0, 4.0);
    let mut b = nurbs_cylinder_body(r, h);
    if let Surface::Nurbs(n) = &mut b.faces[0].surface {
        n.knots_u.push(1.0);
        for z in [0.0, h] {
            n.poles.push(Vec3::new(r, 0.0, z));
        }
        if let Some(w) = &mut n.weights {
            w.extend([1.0, 1.0]);
        }
        n.count_u += 1;
    }
    check(
        "extra end multiplicity",
        &facts(&b),
        &Expect {
            volume: PI * r * r * h,
            area: TAU * r * h + TAU * r * r,
            centroid: Vec3::new(0.0, 0.0, h / 2.0),
        },
        1e-6,
    );
}

#[test]
fn clockwise_trims_refuse() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    let t = &mut b.faces[3].trims;
    t.reverse();
    for p in t.iter_mut() {
        p.reversed = true;
    }
    let r = refusal(&b);
    assert_eq!((r.code, r.face), (RefusalCode::TrimOrientation, Some(3)));
}

#[test]
fn a_face_far_outside_the_body_refuses() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0] = plane(
        Vec3::new(0.0, 0.0, 1e6),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        rectangle(0.0, 1.0, 0.0, 1.0),
    );
    let r = refusal(&b);
    assert_eq!(
        (r.code, r.face),
        (RefusalCode::FaceTermExceedsArea, Some(0))
    );
}

#[test]
fn a_box_smaller_than_the_body_refuses() {
    let mut b = cuboid(Vec3::ZERO, 2.0, 2.0, 2.0);
    b.bounds = Some([Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)]);
    assert_eq!(refusal(&b).code, RefusalCode::VolumeExceedsBox);
    let mut b = cuboid(Vec3::ZERO, 2.0, 2.0, 2.0);
    b.bounds = Some([Vec3::new(1.5, 0.0, 0.0), Vec3::new(9.5, 2.0, 2.0)]);
    b.centre = Vec3::new(2.0, 1.0, 1.0);
    b.size = 8.0;
    assert_eq!(refusal(&b).code, RefusalCode::CentroidOutsideBox);
}

/// An adapter over bodies already in memory.
struct InMemory(Vec<BodyDefinition>);

impl FaceDefinitions for InMemory {
    fn body_count(&self) -> usize {
        self.0.len()
    }

    fn body(&mut self, index: usize) -> Result<BodyDefinition, Refusal> {
        self.0
            .get(index)
            .cloned()
            .ok_or_else(|| Refusal::new(RefusalCode::InvalidDefinition, "no such body"))
    }
}

#[test]
fn an_adapter_feeds_the_integrator() {
    let mut source = InMemory(vec![cuboid(Vec3::ZERO, 1.0, 2.0, 3.0)]);
    let bodies: Vec<BodyDefinition> = (0..source.body_count())
        .map(|i| source.body(i).unwrap())
        .collect();
    let f = bodies_facts(&bodies, Options::default());
    assert!((f[0].as_ref().unwrap().volume.value - 6.0).abs() < 1e-13);
    assert!(source.body(1).is_err());
}

#[test]
fn sheet_faces_have_area_and_no_volume() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    for f in &mut b.faces {
        f.material = Material::Neither;
    }
    let f = facts(&b);
    assert_eq!(f.volume.value, 0.0);
    assert!(f.centroid.is_none());
    assert!(f.closed_within_bounds);
    assert!((f.area.value - 6.0).abs() < 1e-14);
}

fn clockwise(u0: f64, u1: f64, v0: f64, v1: f64) -> Vec<TrimPiece> {
    let mut t = rectangle(u0, u1, v0, v1);
    t.reverse();
    for p in &mut t {
        p.reversed = true;
    }
    t
}

/// Re-review A: matching holes in all six faces cancel every closure term, so
/// a material body with unknown closure refuses.
#[test]
fn unknown_closure_refuses() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    for f in &mut b.faces {
        f.loops.push(4);
        f.trims.extend(clockwise(0.25, 0.75, 0.25, 0.75));
    }
    b.closed = None;
    assert_eq!(refusal(&b).code, RefusalCode::ClosureUnknown);
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.closed = None;
    assert_eq!(refusal(&b).code, RefusalCode::ClosureUnknown);
    // Sheet bodies need no closure.
    for f in &mut b.faces {
        f.material = Material::Neither;
    }
    assert!(body_facts(&b, Options::default()).is_ok());
}

/// Re-review B and C: a top-face hole of area 2.5e-5 widens the volume bound
/// by about 1.3e-5; with a file tolerance of 1e-7 that is beyond four gap
/// volumes and refuses; without one it is within 1e-4 of the volume, and the
/// method is an estimate.
#[test]
fn a_small_hole_refuses_against_the_file_tolerance() {
    let h = 0.0025;
    let mut b = holed_top(clockwise(0.5 - h, 0.5 + h, 0.5 - h, 0.5 + h));
    let f = body_facts(&b, Options::default()).unwrap();
    assert!(!f.closed_within_bounds);
    assert_eq!(f.volume.accuracy.method, Method::Adaptive);
    assert!((f.volume.value - 1.0).abs() <= f.volume.accuracy.bound.unwrap());
    b.file_tolerance = Some(1e-7);
    assert_eq!(refusal(&b).code, RefusalCode::OpenBody);
}

/// Re-review D: a hole outside the outer loop and a loop whose area overflows
/// refuse.
#[test]
fn holes_lie_inside_and_loop_areas_are_finite() {
    let b = holed_top(clockwise(2.0, 2.5, 0.25, 0.75));
    let r = refusal(&b);
    assert_eq!(
        (r.code, r.face),
        (RefusalCode::TrimOrientation, Some(0)),
        "{r}"
    );
    let big = 1e300;
    let mut f = face(
        Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        vec![
            line([0.0, 0.0], [big, 0.0]),
            line([big, 0.0], [big, big]),
            line([big, big], [0.0, 0.0]),
        ],
    );
    f.loops = vec![3];
    assert_eq!(f.check_loops().unwrap_err().code, RefusalCode::NonFinite);
}

/// Review F4: a face whose loop is a collinear (zero-area) triangle on an
/// oblique plane integrates to round-off about zero within its bound, and a
/// closed body with it as an extra face neither refuses nor loosens.
#[test]
fn a_degenerate_face_stays_within_its_bound() {
    let sliver = plane(
        Vec3::new(0.3, 0.1, 0.5),
        Vec3::new(0.7, 0.1, 0.0),
        Vec3::new(0.2, 0.9, 0.0),
        vec![
            line([0.1, 0.2], [0.4, 0.5]),
            line([0.4, 0.5], [0.7, 0.8]),
            line([0.7, 0.8], [0.1, 0.2]),
        ],
    );
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    let m = geospec_facts::face_mass(&sliver, b.centre, b.size, 1e-7).unwrap();
    for (r, e) in m.raw.iter().zip(&m.bound) {
        assert!(r.abs() <= *e, "{:?} beyond {:?}", m.raw, m.bound);
    }
    b.faces.push(sliver);
    check(
        "cube with a sliver",
        &facts(&b),
        &Expect {
            volume: 1.0,
            area: 6.0,
            centroid: Vec3::new(0.5, 0.5, 0.5),
        },
        1e-10,
    );
}

/// Review F7: a trim 1e-9 beyond its net's domain refuses.
#[test]
fn a_trim_just_outside_its_domain_refuses() {
    let mut b = nurbs_cylinder_body(1.0, 1.0);
    b.faces[0].trims = rectangle(0.0, 1.0, 0.0, 1.0 + 1e-9);
    let r = refusal(&b);
    assert_eq!((r.code, r.face), (RefusalCode::TrimOutsideDomain, Some(0)));
}

/// Review F10: a unit cube 1e6 mm from the origin keeps its centre of mass
/// within a bound that covers the coordinates' round-off.
#[test]
fn a_far_body_bounds_its_centroid_round_off() {
    let o = Vec3::new(1e6, -1e6, 1e6);
    let f = facts(&cuboid(o, 1.0, 1.0, 1.0));
    check(
        "far cube",
        &f,
        &Expect {
            volume: 1.0,
            area: 6.0,
            centroid: o + Vec3::new(0.5, 0.5, 0.5),
        },
        1e-6,
    );
    let c = f.centroid.unwrap();
    for b in [c.axis_bounds.x, c.axis_bounds.y, c.axis_bounds.z] {
        assert!(b >= f64::EPSILON * 1e6, "{b}");
    }
}

fn unit_plane(trims: Vec<TrimPiece>, loops: Vec<usize>) -> FaceDefinition {
    let mut f = plane(
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        trims,
    );
    f.loops = loops;
    f
}

fn polyline(points: &[[f64; 2]]) -> Vec<TrimPiece> {
    (0..points.len())
        .map(|i| {
            let n = points.len();
            line(
                points.get(i).copied().unwrap(),
                points.get((i + 1) % n).copied().unwrap(),
            )
        })
        .collect()
}

/// CodeRabbit 4489696791, 4489696799 and 4489696804: a loop crossing itself,
/// overlapping holes, a hole crossing the outer loop and a hole inside a hole
/// refuse; separate holes pass.
#[test]
fn crossing_and_nested_loops_refuse() {
    let code = |f: &FaceDefinition| f.check_loops().map_err(|r| r.code);
    let bow = polyline(&[
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 1.0],
        [1.0, 1.0],
        [1.0, -1.0],
        [0.0, -1.0],
    ]);
    assert_eq!(
        code(&unit_plane(bow, vec![6])),
        Err(RefusalCode::TrimOrientation)
    );
    let with_holes = |outer: Vec<TrimPiece>, holes: &[[f64; 4]]| {
        let mut trims = outer;
        let mut loops = vec![trims.len()];
        for h in holes {
            trims.extend(clockwise(h[0], h[1], h[2], h[3]));
            loops.push(4);
        }
        unit_plane(trims, loops)
    };
    let big = || rectangle(0.0, 3.0, 0.0, 3.0);
    let overlapping = with_holes(big(), &[[0.5, 2.0, 0.5, 2.0], [1.0, 2.5, 1.0, 2.5]]);
    assert_eq!(code(&overlapping), Err(RefusalCode::TrimOrientation));
    let nested = with_holes(big(), &[[0.5, 2.5, 0.5, 2.5], [1.0, 2.0, 1.0, 2.0]]);
    assert_eq!(code(&nested), Err(RefusalCode::TrimOrientation));
    let separate = with_holes(big(), &[[0.5, 1.0, 0.5, 1.0], [2.0, 2.5, 2.0, 2.5]]);
    assert!(code(&separate).is_ok());
    let unit = || rectangle(0.0, 1.0, 0.0, 1.0);
    let crossing = with_holes(unit(), &[[0.75, 1.25, 0.25, 0.75]]);
    assert_eq!(code(&crossing), Err(RefusalCode::TrimOrientation));
}

/// CodeRabbit's review of 77b51ae: contacts that are not proper crossings
/// leave the arrangement undecidable on the sampled polygon and refuse.
#[test]
fn touching_holes_refuse() {
    let code = |f: &FaceDefinition| f.check_loops().map_err(|r| r.code);
    let holes = |outer: Vec<TrimPiece>, holes: &[[f64; 4]]| {
        let mut trims = outer;
        let mut loops = vec![trims.len()];
        for h in holes {
            trims.extend(clockwise(h[0], h[1], h[2], h[3]));
            loops.push(4);
        }
        unit_plane(trims, loops)
    };
    // Holes sharing collinear edge segments, neither inside the other.
    let big = || rectangle(0.0, 3.0, 0.0, 3.0);
    let sharing = holes(big(), &[[0.5, 2.0, 0.5, 2.0], [1.0, 2.5, 0.5, 2.0]]);
    assert_eq!(code(&sharing), Err(RefusalCode::TrimOrientation));
    // Separated holes on the same horizontal lines still pass.
    let separated = holes(big(), &[[0.5, 1.4, 0.5, 2.0], [1.6, 2.5, 0.5, 2.0]]);
    assert!(code(&separated).is_ok());
}

/// A clockwise hole that meets the unit square's edge `u = 1` exactly at two
/// of its vertices refuses.
#[test]
fn hole_touching_the_outer_loop_at_vertices_refuses() {
    let mut trims = rectangle(0.0, 1.0, 0.0, 1.0);
    trims.extend(polyline(&[
        [1.0, 0.75],
        [1.25, 0.5],
        [1.0, 0.25],
        [0.75, 0.5],
    ]));
    assert_eq!(
        unit_plane(trims, vec![4, 4])
            .check_loops()
            .map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// A counter-clockwise loop that crosses itself exactly at one of its own
/// polygon vertices (the joint at (2, 1) lies on the edge from (0, 1.5) to
/// (6, 0)) refuses.
#[test]
fn self_crossing_at_a_vertex_refuses() {
    let looped = polyline(&[[0.0, 0.0], [0.0, 1.5], [6.0, 0.0], [6.0, 3.0], [2.0, 1.0]]);
    assert_eq!(
        unit_plane(looped, vec![5])
            .check_loops()
            .map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// The quadratic Bézier trim with poles `a`, `b`, `c` over `[0, 1]`.
fn bezier(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> TrimPiece {
    TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            poles: vec![a, b, c],
            weights: None,
        }),
        range: [0.0, 1.0],
        reversed: false,
    }
}

/// CodeRabbit's review of 1b978ed: curves that cross between their samples
/// while the sampled chords do not. Hole A is the region under the parabola
/// `v = u (1 - u)` (peak 0.25 at u = 0.5, sampled chord there at 0.24902);
/// hole B is a rectangle above it. With B's bottom at 0.2495 the curves cross
/// though no chords meet, and the face refuses; at 0.2505 they are separate
/// and it passes (after halving the chords near the peak).
#[test]
fn curved_holes_crossing_between_samples_refuse() {
    let code = |bottom: f64| {
        let mut trims = rectangle(-1.0, 2.0, -1.0, 2.0);
        // Clockwise: over the top to the right, back along the bottom.
        trims.push(bezier([0.0, 0.0], [0.5, 0.5], [1.0, 0.0]));
        trims.push(line([1.0, 0.0], [0.0, 0.0]));
        trims.extend(clockwise(0.45, 0.55, bottom, bottom + 0.1));
        unit_plane(trims, vec![4, 2, 4])
            .check_loops()
            .map_err(|r| r.code)
    };
    assert_eq!(code(0.2495), Err(RefusalCode::TrimOrientation));
    assert!(code(0.2505).is_ok());
}

/// A clockwise hole whose curved right edge leaves the unit square between
/// samples (u = 1.00002 at t = 1/16) while every sampled point stays inside
/// refuses.
#[test]
fn hole_bulging_out_of_the_outer_loop_between_samples_refuses() {
    let (a, b, c) = (
        [0.999_629_375, 0.25],
        [1.005_879_375, 0.50],
        [0.912_129_375, 0.75],
    );
    let mut trims = rectangle(0.0, 1.0, 0.0, 1.0);
    // Clockwise: left along the bottom, up the left side, right along the
    // top, down the curve.
    trims.push(line(a, [0.75, 0.25]));
    trims.push(line([0.75, 0.25], [0.75, 0.75]));
    trims.push(line([0.75, 0.75], c));
    let mut edge = bezier(a, b, c);
    edge.reversed = true;
    trims.push(edge);
    assert_eq!(
        unit_plane(trims, vec![4, 4])
            .check_loops()
            .map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// Hole B's curved left edge reaches u = 0.9999 at its midpoint, inside hole
/// A = [0.5, 1] x [0.5, 2], while B's sampled polygon stays at u >= 1.00088:
/// the overlap is between samples and refuses.
#[test]
fn holes_overlapping_between_samples_refuse() {
    let mut trims = rectangle(0.0, 3.0, 0.0, 3.0);
    trims.extend(clockwise(0.5, 1.0, 0.5, 2.0));
    // Clockwise: up the curve, right along the top, down at u = 2.5, left
    // along the bottom.
    trims.push(bezier([1.2499, 0.5], [0.7499, 1.25], [1.2499, 2.0]));
    trims.push(line([1.2499, 2.0], [2.5, 2.0]));
    trims.push(line([2.5, 2.0], [2.5, 0.5]));
    trims.push(line([2.5, 0.5], [1.2499, 0.5]));
    assert_eq!(
        unit_plane(trims, vec![4, 4, 4])
            .check_loops()
            .map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// CodeRabbit's review of b80e491: consecutive chords are not tested
/// against each other, so a curve that crosses its neighbour within one
/// sample interval of their joint passed. The cubic from (0, 0) with poles
/// (-0.002, 0.003), (-0.002, -0.003), (-0.5, -1) and weights `4^i` dips
/// across the line from (0, 0) to (-1, 0) at u = -0.00074 within its first
/// sample interval (parameter 0.022 < 1/32); it refuses.
#[test]
fn trim_crossing_its_neighbour_within_a_sample_interval_refuses() {
    let l = 4.0;
    let dip = TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            poles: vec![[0.0, 0.0], [-0.002, 0.003], [-0.002, -0.003], [-0.5, -1.0]],
            weights: Some(vec![1.0, l, l * l, l * l * l]),
        }),
        range: [0.0, 1.0],
        reversed: true,
    };
    // Counter-clockwise: down to (-0.5, -1), up the dip to (0, 0), back
    // left along v = 0.
    let trims = vec![
        line([-1.0, 0.0], [-0.5, -1.0]),
        dip,
        line([0.0, 0.0], [-1.0, 0.0]),
    ];
    assert_eq!(
        unit_plane(trims, vec![3]).check_loops().map_err(|r| r.code),
        Err(RefusalCode::TrimOrientation)
    );
}

/// A planar biquadratic rational patch `h x h` at `c` (so its area is `h^2`
/// whatever its weights), poles at `0`, `a1` and `1` of the side in each
/// direction, weights `wu[i] wv[j]`.
fn rational_patch(c: Vec3, h: f64, a1: f64, wu: [f64; 3], wv: [f64; 3]) -> FaceDefinition {
    let a = [0.0, a1, 1.0];
    let (mut poles, mut weights) = (Vec::new(), Vec::new());
    for (ai, wi) in a.iter().zip(wu) {
        for (aj, wj) in a.iter().zip(wv) {
            poles.push(Vec3::new(c.x + h * ai, c.y + h * aj, c.z));
            weights.push(wi * wj);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    face(
        Surface::Nurbs(NurbsSurface {
            degree_u: 2,
            degree_v: 2,
            knots_u: knots.clone(),
            knots_v: knots,
            poles,
            weights: Some(weights),
            count_u: 3,
            count_v: 3,
        }),
        rectangle(0.0, 1.0, 0.0, 1.0),
    )
}

/// CodeRabbit's review of 77b51ae (mass.rs round-off): a tiny rational face
/// far from the origin whose weights vary by 1e20 within its span changes
/// its parametrization over less than the parameter's resolution; 77b51ae
/// returned an area of 0 with a bound near 1e-34 against the true 2^-20. It
/// refuses as ill-conditioned; at the largest accepted weight ratio the
/// error (carrying the evaluator's round-off bound) stays within the bound.
#[test]
fn ill_conditioned_rational_faces_refuse_or_stay_within_bound() {
    let h = 1.0 / 1024.0;
    let c = Vec3::new(3e7, 3e7, 3e7);
    let spike = rational_patch(c, h, 1e-6, [1.0, 1e10, 1e-10], [1.0; 3]);
    assert_eq!(
        geospec_facts::face_mass(&spike, c, 2.0 * h, 1e-7)
            .unwrap_err()
            .code,
        RefusalCode::IllConditionedSurface
    );
    for (wu, wv) in [
        ([1.0, 1e3, 1e-3], [1.0; 3]),
        ([1.0, 1e-3, 1e3], [1.0; 3]),
        ([1.0, 1e3, 1.0], [1.0, 1e3, 1.0]),
    ] {
        let m = geospec_facts::face_mass(&rational_patch(c, h, 1e-3, wu, wv), c, 2.0 * h, 1e-7)
            .unwrap();
        let err = (m.raw[0] - h * h).abs();
        assert!(
            err <= m.bound[0],
            "{wu:?} {wv:?}: {err:e} > {:e}",
            m.bound[0]
        );
        assert!(m.bound[0] > 0.0 && m.raw[0] > 0.0);
    }
}

/// CodeRabbit 4489696772: an outer loop of reversed pieces keeps its corners,
/// so a hole near a corner lies inside it.
#[test]
fn reversed_outer_pieces_keep_their_corners() {
    // The unit square counter-clockwise as reversed clockwise edges.
    let mut outer = polyline(&[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
    outer.reverse();
    for p in &mut outer {
        p.reversed = true;
    }
    let mut trims = outer;
    trims.extend(clockwise(0.97, 0.995, 0.97, 0.995));
    assert!(unit_plane(trims, vec![4, 4]).check_loops().is_ok());
}

/// CodeRabbit 4489696818: loops winding around a torus in v refuse.
#[test]
fn v_winding_loops_refuse() {
    let mut band = face(
        Surface::Torus {
            frame: STD,
            major_radius: 5.0,
            minor_radius: 1.0,
        },
        vec![line([1.0, TAU], [1.0, 0.0]), line([0.0, 0.0], [0.0, TAU])],
    );
    band.loops = vec![1, 1];
    assert_eq!(
        band.check_loops().unwrap_err().code,
        RefusalCode::UnsupportedTrim
    );
}

/// CodeRabbit 4489696850: a piece that crosses the whole face over a
/// parameter interval too short to integrate refuses instead of vanishing.
#[test]
fn a_trim_over_an_unsteppable_interval_refuses() {
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0].trims[0] = TrimPiece {
        curve: Curve2::Line {
            origin: [0.0, 0.0],
            direction: [1e13, 0.0],
        },
        range: [0.0, 1e-13],
        reversed: false,
    };
    let r = refusal(&b);
    assert_eq!(
        (r.code, r.face),
        (RefusalCode::InvalidDefinition, Some(0)),
        "{r}"
    );
}

/// CodeRabbit 4489696754, 4489696761, 4489696785 and 4489703188: tiny plane
/// axes are independent, overflowing loop counts and discontinuous B-splines
/// are invalid, and a non-finite line does not evaluate.
#[test]
fn structural_edge_cases_validate() {
    let tiny = Surface::Plane {
        origin: Vec3::ZERO,
        u_axis: Vec3::new(1e-100, 0.0, 0.0),
        v_axis: Vec3::new(0.0, 1e-100, 0.0),
    };
    assert!(tiny.validate().is_ok());
    let mut f = unit_plane(rectangle(0.0, 1.0, 0.0, 1.0), vec![usize::MAX, 5]);
    assert_eq!(
        f.validate().unwrap_err().code,
        RefusalCode::InvalidDefinition
    );
    f.loops = vec![4];
    assert!(f.validate().is_ok());
    let jump = NurbsCurve2 {
        degree: 1,
        knots: vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
        poles: vec![[0.0, 0.0], [1.0, 0.0], [5.0, 5.0], [1.0, 1.0]],
        weights: None,
    };
    assert_eq!(
        jump.validate().unwrap_err().code,
        RefusalCode::InvalidDefinition
    );
    let bad = Curve2::Line {
        origin: [f64::NAN, 0.0],
        direction: [1.0, 0.0],
    };
    assert_eq!(
        bad.evaluate(0.5).unwrap_err().code,
        RefusalCode::InvalidDefinition
    );
}

/// CodeRabbit 4489696810: a small rational NURBS cylinder far from the
/// origin keeps its area and volume (dyadic sizes and offset, so that the
/// shifted poles are exact and the closed form is the net's).
#[test]
fn a_small_nurbs_body_far_from_the_origin_keeps_its_facts() {
    let (r, h) = (1.0 / 1024.0, 1.0 / 512.0);
    let o = Vec3::new(1_048_576.0, -1_048_576.0, 1_048_576.0);
    let mut b = nurbs_cylinder_body(r, h);
    for f in &mut b.faces {
        match &mut f.surface {
            Surface::Nurbs(n) => {
                for p in &mut n.poles {
                    *p += o;
                }
            }
            Surface::Plane { origin, .. } => *origin += o,
            _ => {}
        }
    }
    let [lo, hi] = b.bounds.unwrap();
    b.bounds = Some([lo + o, hi + o]);
    b.centre += o;
    check(
        "far small nurbs cylinder",
        &facts(&b),
        &Expect {
            volume: PI * r * r * h,
            area: TAU * r * h + TAU * r * r,
            centroid: o + Vec3::new(0.0, 0.0, h / 2.0),
        },
        1e-4,
    );
}

/// CodeRabbit nitpick: a non-affine bicubic top face split by its diagonal
/// into two triangular faces whose trims cross its interior knot lines.
#[test]
fn diagonal_trims_cross_a_non_affine_nets_knot_lines() {
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0];
    let greville: Vec<f64> = (0..6)
        .map(|i| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0)
        .collect();
    // x = g(u) with poles at the squared Greville points: on z = 1, not
    // affine in u or v, and onto the unit square.
    let mut poles = Vec::new();
    for &gu in &greville {
        for &gv in &greville {
            poles.push(Vec3::new(gu * gu, gv * gv, 1.0));
        }
    }
    let net = Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles,
        weights: None,
        count_u: 6,
        count_v: 6,
    });
    let mut b = cuboid(Vec3::ZERO, 1.0, 1.0, 1.0);
    b.faces[0] = face(net.clone(), polyline(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]));
    b.faces
        .push(face(net, polyline(&[[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]])));
    check(
        "diagonal bicubic top",
        &facts(&b),
        &Expect {
            volume: 1.0,
            area: 6.0,
            centroid: Vec3::new(0.5, 0.5, 0.5),
        },
        1e-6,
    );
}
