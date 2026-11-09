//! The box of a body given for the box alone ([`BoxBody`]) with tolerance
//! gaps closed by bridges: the box brackets the bridged definition, the gap
//! bound ties every bridge, chord and joint to its vertex, and each
//! condition that cannot be shown refuses. On a cylinder or a cone a bridge
//! across the period stands for its one lattice image within the vertex's
//! cube; two such images, an image neither within nor shown outside, or a
//! wrap on another kind refuse. A joint table that does not fit the body
//! refuses.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::excessive_precision,
    clippy::needless_range_loop
)]

use geospec_facts::{
    bodies_boxes, boxes_of, BodyBox, BodyDefinition, BoxBody, BoxJoints, BoxOptions, Curve2,
    FaceDefinition, Frame, Joint, Material, NurbsSurface, Orientation, Refusal, RefusalCode,
    Surface, TrimPiece, Vec3,
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

fn plane() -> Surface {
    Surface::Plane {
        origin: Vec3::ZERO,
        u_axis: Vec3::new(1.0, 0.0, 0.0),
        v_axis: Vec3::new(0.0, 1.0, 0.0),
    }
}

fn face(surface: Surface, trims: Vec<TrimPiece>) -> FaceDefinition {
    let loops = vec![trims.len()];
    FaceDefinition {
        surface,
        trims,
        loops,
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

fn body(faces: Vec<FaceDefinition>, size: f64) -> BodyDefinition {
    BodyDefinition {
        centre: Vec3::ZERO,
        size,
        bounds: None,
        faces,
        file_tolerance: None,
        closed: None,
    }
}

fn square() -> FaceDefinition {
    face(
        plane(),
        vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    )
}

fn joint(x: f64, y: f64, tolerance: f64) -> Option<Joint> {
    Some(Joint {
        vertex: Vec3::new(x, y, 0.0),
        tolerance,
    })
}

#[test]
fn a_table_that_does_not_fit_refuses() {
    let d = body(vec![square()], 1.0);
    let ends = vec![joint(1.0, 0.0, 1e-3); 4];
    let table = |bridges: Vec<usize>, ends: Vec<Option<Joint>>| vec![BoxJoints { bridges, ends }];
    let cases = [
        (Vec::new(), Some(1e-3)),
        (table(vec![4], ends.clone()), Some(1e-3)),
        (table(vec![2, 1], ends.clone()), Some(1e-3)),
        (table(vec![1, 1], ends.clone()), Some(1e-3)),
        (table(Vec::new(), ends[..3].to_vec()), Some(1e-3)),
        (
            table(Vec::new(), vec![joint(f64::NAN, 0.0, 1e-3); 4]),
            Some(1e-3),
        ),
        (
            table(Vec::new(), vec![joint(0.0, 0.0, -1.0); 4]),
            Some(1e-3),
        ),
        (table(Vec::new(), ends.clone()), Some(f64::INFINITY)),
        (table(Vec::new(), ends.clone()), Some(-1e-3)),
    ];
    for (k, (joints, tolerance)) in cases.into_iter().enumerate() {
        let r = BoxBody::new(d.clone(), joints, tolerance);
        assert_eq!(
            r.map(|_| ()).unwrap_err().code,
            RefusalCode::InvalidDefinition,
            "case {k}"
        );
    }
    // A bridge must be a line.
    let mut curved = square();
    curved.trims[1].curve = Curve2::Nurbs(geospec_facts::NurbsCurve2 {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![[1.0, 0.0], [1.0, 1.0]],
        weights: None,
    });
    let r = BoxBody::new(body(vec![curved], 1.0), table(vec![1], ends.clone()), None);
    assert_eq!(
        r.map(|_| ()).unwrap_err().code,
        RefusalCode::InvalidDefinition
    );
    let ok = BoxBody::new(d, table(vec![1], ends), Some(1e-3)).unwrap();
    assert!(ok.bridged());
}

/// Boxes one body given with its faces' joint tables and the kernel
/// tolerance 1e-3.
fn boxed(faces: Vec<(FaceDefinition, BoxJoints)>, size: f64) -> Result<BodyBox, Refusal> {
    let (faces, joints): (Vec<_>, Vec<_>) = faces.into_iter().unzip();
    let b = BoxBody::new(body(faces, size), joints, Some(1e-3))?;
    boxes_of(&[b], BoxOptions::default()).pop().unwrap()
}

fn table(bridges: Vec<usize>, ends: Vec<Option<Joint>>) -> BoxJoints {
    BoxJoints { bridges, ends }
}

/// A unit square with its corner at (1, 1) cut by a bridge `c` long, with
/// every trim's end vertex (the cut corner's is (1, 1)) at tolerance `t`.
fn cut_square(c: f64, t: f64) -> (FaceDefinition, BoxJoints) {
    let f = face(
        plane(),
        vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0 - c]),
            line([1.0, 1.0 - c], [1.0 - c, 1.0]),
            line([1.0 - c, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    );
    let ends = vec![
        joint(1.0, 0.0, t),
        joint(1.0, 1.0, t),
        joint(1.0, 1.0, t),
        joint(0.0, 1.0, t),
        joint(0.0, 0.0, t),
    ];
    (f, table(vec![2], ends))
}

fn refusal(r: Result<BodyBox, Refusal>) -> Refusal {
    match r {
        Ok(b) => panic!("boxed: {b:?}"),
        Err(r) => r,
    }
}

#[test]
fn a_plane_gap_is_bounded_by_its_end_points() {
    let b = boxed(vec![cut_square(1e-4, 1e-3)], 1.0).unwrap();
    let [lo, hi] = b.outer();
    let [ilo, ihi] = b.inner();
    for (o, i, t) in [
        (-lo.x, -ilo.x, 0.0),
        (-lo.y, -ilo.y, 0.0),
        (hi.x, ihi.x, 1.0),
        (hi.y, ihi.y, 1.0),
    ] {
        assert!(i <= t && t <= o && o - i <= 1e-7, "{i} {t} {o}");
    }
    assert_eq!(b.gap_faces, vec![0]);
    // The bridge's ends are 1e-4 from (1, 1) in one axis each.
    assert!(
        b.gap_bound >= 1e-4 && b.gap_bound <= 1e-4 + 1e-12,
        "{:e}",
        b.gap_bound
    );
    assert_eq!(b.accuracy.gap_bound, Some(b.gap_bound));
    let [klo, khi] = b.kernel_outer().unwrap();
    let grow = 1e-3 + b.gap_bound;
    assert!(
        klo.x <= lo.x - grow && khi.y >= hi.y + grow,
        "{klo:?} {khi:?}"
    );
    // Without bridges the same table adds nothing to the box.
    let (f, mut j) = cut_square(0.0, 1e-3);
    j.bridges.clear();
    let mut f = f;
    f.trims.remove(2);
    f.loops = vec![4];
    j.ends.remove(2);
    let plain = boxed(vec![(f, j)], 1.0).unwrap();
    assert_eq!(plain.gap_bound, 0.0);
    assert!(plain.gap_faces.is_empty());
    assert_eq!(plain.accuracy.gap_bound, None);
}

#[test]
fn end_points_at_the_vertex_tolerance() {
    // On a plane the bound is the end points' (no curvature): the bridge
    // ends `c` from the corner, which is within the tolerance just above
    // `c` and outside it just below.
    let c = 1e-4;
    assert!(boxed(vec![cut_square(c, c * (1.0 + 1e-9))], 1.0).is_ok());
    let r = refusal(boxed(vec![cut_square(c, c * (1.0 - 1e-9))], 1.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert!(
        r.detail.contains("bridge at trim 2 leaves its vertex cube"),
        "{r}"
    );
    assert_eq!(r.face, Some(0), "{r}");
}

/// Degree 2 by 1: `z = 2 h u (1 - u)`, `x = u`, `y = v` over the unit square.
fn ridge(h: f64) -> Surface {
    let mut poles = Vec::new();
    for (i, z) in [0.0, h, 0.0].into_iter().enumerate() {
        for j in 0..2 {
            poles.push(Vec3::new(0.5 * i as f64, j as f64, z));
        }
    }
    Surface::Nurbs(NurbsSurface {
        degree_u: 2,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 3,
        count_v: 2,
    })
}

#[test]
fn a_bridge_over_a_ridge_leaves_its_cube() {
    // The bridge's ends lie at z = 4.2, 0.2 from the vertex in x; its middle
    // rises to z = 5 over the ridge, 0.8 from the vertex in z.
    let f = face(
        ridge(10.0),
        vec![
            line([0.3, 0.2], [0.7, 0.2]),
            line([0.7, 0.2], [0.7, 0.5]),
            line([0.7, 0.5], [0.3, 0.5]),
            line([0.3, 0.5], [0.3, 0.2]),
        ],
    );
    let v = |t: f64| {
        Some(Joint {
            vertex: Vec3::new(0.5, 0.5, 4.2),
            tolerance: t,
        })
    };
    // The exact joints' vertices are the corners' points.
    let corner = |x: f64, y: f64| {
        Some(Joint {
            vertex: Vec3::new(x, y, 4.2),
            tolerance: 1e-3,
        })
    };
    let ends = |t: f64| vec![corner(0.7, 0.2), corner(0.7, 0.5), v(t), corner(0.3, 0.2)];
    let r = refusal(boxed(vec![(f.clone(), table(vec![2], ends(0.5)))], 1.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert!(r.detail.contains("leaves its vertex cube"), "{r}");
    let b = boxed(vec![(f, table(vec![2], ends(1.5)))], 1.0).unwrap();
    assert!(b.gap_bound >= 0.8 && b.gap_bound <= 1.5, "{}", b.gap_bound);
}

/// A right-handed frame at `origin` with `z` up.
fn upright(origin: Vec3) -> Frame {
    Frame {
        origin,
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 1.0, 0.0),
        z: Vec3::new(0.0, 0.0, 1.0),
    }
}

#[test]
fn a_bridge_is_never_attained() {
    // A unit sphere's patch u in [-0.5, 0.5], v in [0, 0.5]: x's largest
    // value, 1 at (0, 0), lies on the bottom trim. As a bridge it moves
    // only the outer value.
    let f = face(
        Surface::Sphere {
            frame: upright(Vec3::ZERO),
            radius: 1.0,
        },
        vec![
            line([-0.5, 0.0], [0.5, 0.0]),
            line([0.5, 0.0], [0.5, 0.5]),
            line([0.5, 0.5], [-0.5, 0.5]),
            line([-0.5, 0.5], [-0.5, 0.0]),
        ],
    );
    // The exact joints' vertices are the corners' points.
    let corner = |u: f64, v: f64| {
        Some(Joint {
            vertex: Vec3::new(
                libm::cos(v) * libm::cos(u),
                libm::cos(v) * libm::sin(u),
                libm::sin(v),
            ),
            tolerance: 1e-6,
        })
    };
    let ends = vec![
        joint(1.0, 0.0, 1.0),
        corner(0.5, 0.5),
        corner(-0.5, 0.5),
        corner(-0.5, 0.0),
    ];
    let b = boxed(vec![(f.clone(), table(vec![0], ends))], 2.0).unwrap();
    assert!(b.sides.outer[3] >= 1.0, "{:?}", b.sides);
    // The largest attained x is the right trim's corner, cos 0.5.
    assert!(b.sides.inner[3] < 0.878, "{:?}", b.sides);
    let plain = bodies_boxes(&[body(vec![f], 2.0)], BoxOptions::default())
        .pop()
        .unwrap()
        .unwrap();
    assert!(plain.sides.inner[3] > 1.0 - 1e-6, "{:?}", plain.sides);
}

#[test]
fn a_bridge_without_a_vertex_refuses() {
    let (f, mut j) = cut_square(1e-4, 1e-3);
    j.ends[2] = None;
    let r = refusal(boxed(vec![(f, j)], 1.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert!(
        r.detail.contains("bridge at trim 2 without a vertex"),
        "{r}"
    );
}

#[test]
fn an_exact_joint_adds_its_end_point_term() {
    // The bottom trim ends exactly where the right one starts, 5e-4 from its
    // vertex: the bound covers that vertex's ball too.
    let (f, mut j) = cut_square(1e-4, 1e-3);
    j.ends[0] = joint(1.0, 5e-4, 1e-3);
    let b = boxed(vec![(f, j)], 1.0).unwrap();
    assert!(
        b.gap_bound >= 5e-4 && b.gap_bound <= 5e-4 + 1e-12,
        "{:e}",
        b.gap_bound
    );
}

#[test]
fn an_exact_joint_outside_its_vertex_cube_refuses() {
    // The bottom trim ends exactly where the right one starts, 5e-3 from its
    // vertex at tolerance 1e-3: a joint leaves its cube as a bridge would.
    let (f, mut j) = cut_square(1e-4, 1e-3);
    j.ends[0] = joint(1.0, 5e-3, 1e-3);
    let r = refusal(boxed(vec![(f, j)], 1.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert!(
        r.detail
            .contains("joint after trim 0 leaves its vertex cube"),
        "{r}"
    );
}

#[test]
fn an_exact_joint_without_a_vertex_refuses() {
    // The bottom trim ends exactly where the right one starts, with no
    // vertex for that joint: on a bridged face it cannot be bounded.
    let (f, mut j) = cut_square(1e-4, 1e-3);
    j.ends[0] = None;
    let r = refusal(boxed(vec![(f, j)], 1.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert!(
        r.detail.contains("joint after trim 0 without a vertex"),
        "{r}"
    );
}

/// A7's net A: degree 3 by 1 with x-poles `0, 1, 1, 1, 1`, y the v pole
/// row, knots `0^4, 1e-7, 1^4` in u: `|S_u|` near u = 0 is 3e7.
fn net_a() -> Surface {
    let x = [0.0, 1.0, 1.0, 1.0, 1.0];
    let mut poles = Vec::new();
    for xi in x {
        for j in 0..2 {
            poles.push(Vec3::new(xi, j as f64, 0.0));
        }
    }
    Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, 1e-7, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 5,
        count_v: 2,
    })
}

/// `Geom_BSplineSurface::Value` of net A at u = -9.9e-7 (A7's pinned
/// sample): OCCT extrapolates the end span there.
const OCCT_A_X: f64 = -1294.029000000001;

#[test]
fn a_bridge_past_the_domain_holds_the_extrapolated_net() {
    // Net A's left trim is a bridge at u = -9.9e-7, within the domain
    // tolerance. Its clamped image is the line x = 0, but OCCT's lies at
    // x = -1294: the bound covers it (the in-domain Lipschitz term alone
    // is 29.7).
    let u = -0.99e-6;
    let f = face(
        net_a(),
        vec![
            line([u, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [u, 1.0]),
            line([u, 1.0], [u, 0.0]),
        ],
    );
    let v = |t: f64| {
        Some(Joint {
            vertex: Vec3::new(0.0, 0.5, 0.0),
            tolerance: t,
        })
    };
    let b = boxed(vec![(f.clone(), table(vec![3], vec![v(1e4); 4]))], 2.0).unwrap();
    assert!(b.gap_bound >= -OCCT_A_X, "{}", b.gap_bound);
    assert!(b.sides.outer[0] >= -OCCT_A_X, "{:?}", b.sides);
    let mut ends = vec![v(1e4); 4];
    ends[3] = v(100.0);
    let r = refusal(boxed(vec![(f, table(vec![3], ends))], 2.0));
    assert!(
        r.detail.contains("bridge at trim 3 leaves its vertex cube"),
        "{r}"
    );
}

/// Net A's unit square with the bottom trim starting `c` along u from the
/// left trim's end: a closing chord across the steep end span.
fn net_a_chord(c: f64) -> FaceDefinition {
    face(
        net_a(),
        vec![
            line([c, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    )
}

#[test]
fn chords_on_a_bridged_body_are_guarded() {
    // A chord 5e-7 long (under the chaining tolerance) at net A's steep end
    // moves x by 15: on a bridged body (the square's bridge) it leaves its
    // vertex's cube and refuses; on an unbridged body it is boxed as today.
    let ends = vec![None, None, None, joint(0.0, 0.0, 1.0)];
    let chord = (net_a_chord(5e-7), table(Vec::new(), ends.clone()));
    let r = refusal(boxed(vec![cut_square(1e-4, 1e-3), chord.clone()], 2.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    assert_eq!(r.face, Some(1), "{r}");
    assert!(
        r.detail
            .contains("chord after trim 3 leaves its vertex cube"),
        "{r}"
    );
    let plain = boxed(vec![chord], 2.0).unwrap();
    let today = bodies_boxes(&[body(vec![net_a_chord(5e-7)], 2.0)], BoxOptions::default())
        .pop()
        .unwrap()
        .unwrap();
    assert_eq!(
        format!("{today:?}"),
        format!(
            "{:?}",
            BodyBox {
                kernel_tolerance: None,
                ..plain
            }
        )
    );
    // Within its cube, a chord's bound joins the gap bound.
    let wide = (
        net_a_chord(5e-7),
        table(
            Vec::new(),
            vec![
                joint(1.0, 0.0, 1e-3),
                joint(1.0, 1.0, 1e-3),
                joint(0.0, 1.0, 1e-3),
                joint(0.0, 0.0, 100.0),
            ],
        ),
    );
    let b = boxed(vec![cut_square(1e-4, 1e-3), wide], 2.0).unwrap();
    assert!(b.gap_bound >= 15.0 * 0.99, "{}", b.gap_bound);
    // A chord's joint without a vertex refuses.
    let bare = (net_a_chord(5e-7), table(Vec::new(), vec![None; 4]));
    let r = refusal(boxed(vec![cut_square(1e-4, 1e-3), bare], 2.0));
    assert!(
        r.detail.contains("chord after trim 3 without a vertex"),
        "{r}"
    );
}

#[test]
fn a_join_left_open_on_a_bridged_body_refuses() {
    // A 1e-3 gap the export did not bridge is longer than the chaining
    // tolerance: the loop is open.
    let ends = vec![joint(0.0, 0.0, 1.0); 4];
    let open = (net_a_chord(1e-3), table(Vec::new(), ends));
    let r = refusal(boxed(vec![cut_square(1e-4, 1e-3), open], 2.0));
    assert_eq!(r.code, RefusalCode::OpenTrimLoop, "{r}");
    assert_eq!(r.face, Some(1), "{r}");
}

#[test]
fn an_interior_knot_past_the_degree_refuses() {
    // A knot of multiplicity degree + 1 inside the domain is a jump, across
    // which no Lipschitz bound holds: the definition refuses.
    let Surface::Nurbs(mut n) = ridge(1.0) else {
        unreachable!()
    };
    n.knots_u = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0];
    n.count_u = 6;
    n.poles = (0..12)
        .map(|i| Vec3::new(f64::from(i / 2) / 5.0, f64::from(i % 2), 0.0))
        .collect();
    let f = face(
        Surface::Nurbs(n),
        vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    );
    let r = f.validate().unwrap_err();
    assert_eq!(r.code, RefusalCode::InvalidDefinition, "{r}");
    assert!(r.detail.contains("more than 2 copies"), "{r}");
}

const TAU: f64 = std::f64::consts::TAU;

fn cylinder(r: f64) -> Surface {
    Surface::Cylinder {
        frame: upright(Vec3::new(1.0, -2.0, 0.5)),
        radius: r,
    }
}

/// A band of `surface` (a closed kind) between v = 0 and v = 2: each rim a
/// trim from u = 0 to `reach` and the export's bridge straight back, the
/// bottom rim's bridge first in its loop when `bridge_first`. The bridges'
/// vertices are the rims' points at u = 0, at tolerance `t`.
fn band(surface: Surface, reach: f64, t: f64, bridge_first: bool) -> (FaceDefinition, BoxJoints) {
    let point = |u: f64, v: f64| match &surface {
        Surface::Cylinder { frame, radius } | Surface::Sphere { frame, radius } => {
            let (c, s) = (libm::cos(u), libm::sin(u));
            let p = frame.origin + *radius * (c * frame.x + s * frame.y) + v * frame.z;
            Some(Joint {
                vertex: p,
                tolerance: t,
            })
        }
        Surface::Cone {
            frame,
            radius,
            semi_angle,
        } => {
            let (c, s) = (libm::cos(u), libm::sin(u));
            let r = radius + v * libm::sin(*semi_angle);
            let p = frame.origin
                + r * (c * frame.x + s * frame.y)
                + v * libm::cos(*semi_angle) * frame.z;
            Some(Joint {
                vertex: p,
                tolerance: t,
            })
        }
        _ => unreachable!(),
    };
    let (rim, bridge) = (
        line([0.0, 0.0], [reach, 0.0]),
        line([reach, 0.0], [0.0, 0.0]),
    );
    let (bottom, bottom_ends, b0) = if bridge_first {
        (
            vec![bridge, rim],
            vec![point(0.0, 0.0), point(reach, 0.0)],
            0,
        )
    } else {
        (
            vec![rim, bridge],
            vec![point(reach, 0.0), point(0.0, 0.0)],
            1,
        )
    };
    let mut trims = bottom;
    trims.push(line([reach, 2.0], [0.0, 2.0]));
    trims.push(line([0.0, 2.0], [reach, 2.0]));
    let mut ends = bottom_ends;
    ends.push(point(0.0, 2.0));
    ends.push(point(reach, 2.0));
    let mut f = face(surface, trims);
    f.loops = vec![2, 2];
    (f, table(vec![b0, 3], ends))
}

#[test]
fn a_long_join_on_a_cylinder_wraps_by_a_period() {
    // A band of radius 7 whose rims end 2 pi - 1e-4 from their start: the
    // bridges stand for the short joins across the seam, and the box is the
    // closed form, wherever in its loop the bridge lies.
    let truth = [-1.0 + 7.0, 2.0 + 7.0, -0.5, 1.0 + 7.0, -2.0 + 7.0, 2.5];
    for first in [false, true] {
        let b = boxed(vec![band(cylinder(7.0), TAU - 1e-4, 1e-3, first)], 16.0).unwrap();
        assert_eq!(b.wrap_joins, 2, "{b:?}");
        for k in 0..6 {
            assert!(
                b.sides.inner[k] <= truth[k] + 1e-12 && truth[k] <= b.sides.outer[k] + 1e-12,
                "side {k}: {} not in {:?}",
                truth[k],
                b.sides
            );
            assert!(
                b.sides.outer[k] - b.sides.inner[k] <= b.epsilon,
                "{:?}",
                b.sides
            );
        }
        // The short join is 7e-4 long.
        assert!(
            b.gap_bound >= 7e-4 && b.gap_bound <= 1e-3,
            "{}",
            b.gap_bound
        );
    }
    // A join twice around wraps by two periods.
    let b = boxed(
        vec![band(cylinder(7.0), 2.0 * TAU - 1e-4, 1e-3, false)],
        16.0,
    )
    .unwrap();
    assert_eq!(b.wrap_joins, 2, "{b:?}");
}

#[test]
fn a_long_join_on_a_cone_wraps_by_a_period() {
    // A cone band of radius 7 at v = 0 and semi-angle 0.3 whose rims end
    // 2 pi - 1e-4 from their start: the box is the closed form, the full
    // circle of the wider rim over the band's height.
    let a = 0.3;
    let wide = 7.0 + 2.0 * libm::sin(a);
    let top = 0.5 + 2.0 * libm::cos(a);
    let truth = [-1.0 + wide, 2.0 + wide, -0.5, 1.0 + wide, -2.0 + wide, top];
    let cone = Surface::Cone {
        frame: upright(Vec3::new(1.0, -2.0, 0.5)),
        radius: 7.0,
        semi_angle: a,
    };
    for first in [false, true] {
        let b = boxed(vec![band(cone.clone(), TAU - 1e-4, 1e-3, first)], 16.0).unwrap();
        assert_eq!(b.wrap_joins, 2, "{b:?}");
        for k in 0..6 {
            assert!(
                b.sides.inner[k] <= truth[k] + 1e-12 && truth[k] <= b.sides.outer[k] + 1e-12,
                "side {k}: {} not in {:?}",
                truth[k],
                b.sides
            );
            assert!(
                b.sides.outer[k] - b.sides.inner[k] <= b.epsilon,
                "{:?}",
                b.sides
            );
        }
        // The short joins are 7e-4 and (7 + 2 sin 0.3) 1e-4 long.
        assert!(
            b.gap_bound >= wide * 1e-4 && b.gap_bound <= 1e-3,
            "{}",
            b.gap_bound
        );
    }
}

fn wrap_refusal(surface: Surface, reach: f64, t: f64) -> Refusal {
    let r = refusal(boxed(vec![band(surface, reach, t, false)], 16.0));
    assert_eq!(r.code, RefusalCode::ToleranceGap, "{r}");
    r
}

#[test]
fn a_short_join_beyond_the_tolerance_refuses() {
    let r = wrap_refusal(cylinder(7.0), TAU - 1e-2, 1e-3);
    assert!(
        r.detail.contains("bridge at trim 1 leaves its vertex cube"),
        "{r}"
    );
}

#[test]
fn a_cylinder_within_its_vertex_cube_is_ambiguous() {
    // Radius 1e-4 under tolerance 1e-3: the straight join and the wrap both
    // stay in the cube.
    let r = wrap_refusal(cylinder(1e-4), TAU - 1e-4, 1e-3);
    assert!(
        r.detail.contains("ambiguous wrap at trim 1: 2 lattice"),
        "{r}"
    );
    // Radius 4e-4: the straight join's bound passes the tolerance, but no
    // point of it is shown outside the cube.
    let r = wrap_refusal(cylinder(4e-4), TAU - 1e-4, 1e-3);
    assert!(
        r.detail.contains("ambiguous wrap at trim 1: 1 lattice"),
        "{r}"
    );
}

#[test]
fn a_wrap_on_a_sphere_refuses() {
    // On a sphere the face's interior counts, and a shifted bridge changes
    // the region its loops bound.
    let s = Surface::Sphere {
        frame: upright(Vec3::ZERO),
        radius: 7.0,
    };
    let (f, j) = band(s, TAU - 1e-4, 1e-3, false);
    // Keep to the latitudes: the top rim at v = 1.
    let mut f = f;
    for t in &mut f.trims[2..] {
        if let Curve2::Line { origin, .. } = &mut t.curve {
            origin[1] = 1.0;
        }
    }
    let mut j = j;
    for e in j.ends[2..].iter_mut().flatten() {
        let p = 7.0 * libm::cos(1.0);
        e.vertex = Vec3::new(
            p * e.vertex.x / 7.0,
            p * e.vertex.y / 7.0,
            7.0 * libm::sin(1.0),
        );
    }
    let r = refusal(boxed(vec![(f, j)], 16.0));
    assert!(
        r.detail
            .contains("wrap join at trim 1 off a cylinder or cone"),
        "{r}"
    );
}

#[test]
fn review_ones_torus_is_ambiguous() {
    // R 10, r 1e-3, tolerance 1e-2, the join (-(2 pi - 1e-4), 3.3): every
    // shift in v stays within the tolerance of the minor circle.
    let p = [TAU - 1e-4, 0.0];
    let q = [0.0, 3.3];
    let f = face(
        Surface::Torus {
            frame: upright(Vec3::ZERO),
            major_radius: 10.0,
            minor_radius: 1e-3,
        },
        vec![line(q, p), line(p, q)],
    );
    let v = Some(Joint {
        vertex: Vec3::new(10.0 + 1e-3 * libm::cos(3.3), 0.0, 1e-3 * libm::sin(3.3)),
        tolerance: 1e-2,
    });
    let r = refusal(boxed(vec![(f, table(vec![1], vec![v, v]))], 30.0));
    assert!(
        r.detail.contains("ambiguous wrap at trim 1: 2 lattice"),
        "{r}"
    );
}
