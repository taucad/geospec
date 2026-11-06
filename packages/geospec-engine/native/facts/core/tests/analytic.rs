//! Bodies with closed-form area, volume and centre of mass: every fact within
//! its bound of the closed form, identical bits at 1, 2 and 4 threads, and
//! typed refusals where the definition or a body invariant fails.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use geospec_facts::{
    bodies_facts, body_facts, BodyDefinition, BodyFacts, Curve2, FaceDefinition, FaceDefinitions,
    Frame, Material, Method, NurbsCurve2, NurbsSurface, Options, Orientation, Refusal, RefusalCode,
    Surface, TrimPiece, Vec3,
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

fn bits(f: &BodyFacts) -> Vec<u64> {
    let mut out = vec![
        f.area.value.to_bits(),
        f.volume.value.to_bits(),
        f.area.accuracy.bound.unwrap().to_bits(),
        f.volume.accuracy.bound.unwrap().to_bits(),
    ];
    if let Some(c) = f.centroid {
        out.extend([c.value.x, c.value.y, c.value.z, c.axis_bounds.max_abs()].map(f64::to_bits));
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
    // One winding loop alone does not bound the face.
    lateral.trims.pop();
    lateral.loops = vec![1];
    assert_eq!(
        lateral.check_loops().unwrap_err().code,
        RefusalCode::TrimOrientation
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
