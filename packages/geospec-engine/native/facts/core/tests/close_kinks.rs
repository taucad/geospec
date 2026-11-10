//! Review A6-1: kinks of the area element `|n|` that the GK estimate can
//! miss on steep nets, on one Bezier span with `x = u`, `y = v` and `z`
//! a cubic in u (or, transposed, in v), at accuracy 1e-7:
//!
//! - F1: one kink just beside a dyadic bisection point (`z' = a (t - t0)`),
//!   in the gap between a deeper panel's end node and that point;
//! - F2: two close sign changes of `z'` (`z' = a (t - r1)(t - r2)`), whose
//!   directions on both sides agree, so no comparison of directions at
//!   nodes sees them;
//! - F3: random rational nets.
//!
//! The truths are closed form (F1) or composite five-point Gauss-Legendre
//! with the roots as breaks. The probes are ignored; the regression test
//! holds the worst case of each family.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    clippy::inconsistent_digit_grouping,
    missing_docs
)]

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

fn poly_face(surface: Surface, top: [[f64; 2]; 2]) -> FaceDefinition {
    FaceDefinition {
        surface,
        loops: vec![4],
        trims: vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], top[0]),
            line(top[0], top[1]),
            line(top[1], [0.0, 0.0]),
        ],
        orientation: Orientation::Forward,
        material: Material::OneSide,
    }
}

fn square(surface: Surface) -> FaceDefinition {
    poly_face(surface, [[1.0, 1.0], [0.0, 1.0]])
}

const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

fn composite(g: impl Fn(f64) -> f64, breaks: &[f64], cells: usize) -> f64 {
    // Only the face's own range [0, 1]: a root outside it is not a break.
    let mut b: Vec<f64> = breaks.iter().map(|x| x.clamp(0.0, 1.0)).collect();
    b.sort_by(f64::total_cmp);
    b.dedup();
    let mut total = 0.0;
    for w in b.windows(2) {
        let (a, bb) = (w[0], w[1]);
        let h = (bb - a) / cells as f64;
        for c in 0..cells {
            let m = a + (c as f64 + 0.5) * h;
            for (x, wt) in GL5 {
                total += 0.5 * h * wt * g(m + 0.5 * h * x);
            }
        }
    }
    total
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// Cubic Bezier poles of the power polynomial `c[0] + c[1] t + ...` on [0, 1].
fn bezier(c: &[f64; 4]) -> [f64; 4] {
    let mut p = [0.0; 4];
    for i in 0..4 {
        for k in 0..=i {
            p[i] += binomial(i, k) / binomial(3, k) * c[k];
        }
    }
    p
}

/// `x = u`, `y = v`, `z` the cubic `c` in u (or in v with `in_v`), one span.
fn cubic_face(c: &[f64; 4], in_v: bool, weights: Option<[f64; 4]>) -> FaceDefinition {
    let z = bezier(c);
    let mut poles = Vec::new();
    let mut ws = Vec::new();
    if in_v {
        for i in 0..2 {
            for j in 0..4 {
                poles.push(Vec3::new(f64::from(i), j as f64 / 3.0, z[j]));
                ws.push(weights.map_or(1.0, |w| w[j]));
            }
        }
    } else {
        for i in 0..4 {
            for j in 0..2 {
                poles.push(Vec3::new(i as f64 / 3.0, f64::from(j), z[i]));
                ws.push(weights.map_or(1.0, |w| w[i]));
            }
        }
    }
    let k3 = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let k1 = vec![0.0, 0.0, 1.0, 1.0];
    square(Surface::Nurbs(NurbsSurface {
        degree_u: if in_v { 1 } else { 3 },
        degree_v: if in_v { 3 } else { 1 },
        knots_u: if in_v { k1.clone() } else { k3.clone() },
        knots_v: if in_v { k3 } else { k1 },
        poles,
        weights: weights.map(|_| ws),
        count_u: if in_v { 2 } else { 4 },
        count_v: if in_v { 4 } else { 2 },
    }))
}

/// Counts a face outside its bound (`err > bound + 1e-12 truth`) or refused.
fn check(face: &FaceDefinition, truth: f64, label: &str, stats: &mut Stats) {
    match face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7) {
        Ok(m) => {
            let err = (m.raw[0] - truth).abs();
            let r = err / m.bound[0];
            stats.total += 1;
            if r > stats.worst {
                stats.worst = r;
                stats.worst_label = label.to_string();
            }
            if err > m.bound[0] + 1e-12 * truth {
                stats.outside += 1;
                println!(
                    "OUTSIDE {label}: raw {:.15e} truth {truth:.15e} error {err:e} bound {:e} ratio {r:.3} panels {}",
                    m.raw[0], m.bound[0], m.panels
                );
            }
        }
        Err(r) => {
            stats.refused += 1;
            println!("REFUSED {label}: {r:?}");
        }
    }
}

#[derive(Default)]
struct Stats {
    total: usize,
    outside: usize,
    refused: usize,
    worst: f64,
    worst_label: String,
}
impl Stats {
    fn print(&self, name: &str) {
        println!(
            "{name}: {}/{} outside, {} refused, worst ratio {:.4} ({})",
            self.outside, self.total, self.refused, self.worst, self.worst_label
        );
    }
}

/// `z'(t) = a (t - u0)` (one kink at u0): z = a (t - u0)^2 / 2.
fn para(a: f64, u0: f64) -> [f64; 4] {
    [0.5 * a * u0 * u0, -a * u0, 0.5 * a, 0.0]
}
fn para_truth(a: f64, u0: f64) -> f64 {
    let f = |t: f64| 0.5 * (t * (1.0 + a * a * t * t).sqrt() + libm::asinh(a * t) / a);
    f(1.0 - u0) - f(-u0)
}

/// A kink just beside a dyadic midpoint of the single span [0, 1]: inside the
/// gap between a grandchild's end node and the midpoint, which no sibling
/// cross check sees after the first level.
#[test]
#[ignore = "a review A6-1 probe"]
fn probe_kink_beside_a_midpoint() {
    for in_v in [false, true] {
        let mut st = Stats::default();
        for &a in &[1e3, 1e4, 1e5, 1e6, 1e8] {
            for &m in &[0.5, 0.25, 0.75, 0.125, 0.375] {
                for k in 0..30 {
                    let d = 3e-3 * libm::pow(10.0, -0.15 * f64::from(k));
                    for side in [-1.0, 1.0] {
                        let u0 = m + side * d;
                        let face = cubic_face(&para(a, u0), in_v, None);
                        let label = format!("in_v {in_v} a {a} m {m} d {side:+}{d:e}");
                        check(&face, para_truth(a, u0), &label, &mut st);
                    }
                }
            }
        }
        st.print(&format!("beside a midpoint, in_v {in_v}"));
    }
}

/// `z = a (t^3/3 - (r1 + r2) t^2/2 + r1 r2 t)`, so `z' = a (t - r1)(t - r2)`,
/// and its truth.
fn two_kinks(a: f64, delta: f64, c: f64, in_v: bool) -> (FaceDefinition, f64) {
    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
    let co = [0.0, a * r1 * r2, -0.5 * a * (r1 + r2), a / 3.0];
    let fp = |t: f64| a * (t - r1) * (t - r2);
    let t = composite(
        |t| (1.0 + fp(t) * fp(t)).sqrt(),
        &[0.0, r1, r2, 1.0],
        20_000,
    );
    (cubic_face(&co, in_v, None), t)
}

/// The rational net with a kink 4.4e-4 from the level-2 point 0.75.
fn rational_worst() -> FaceDefinition {
    let z = [
        1688.399_726_426_286_2,
        -23_107.094_776_366_353,
        1833.859_171_123_717_3,
        -17_243.702_287_599_328,
    ];
    let w = [
        0.816_844_193_620_280_6,
        2.747_967_098_896_512,
        1.949_853_240_926_919_8,
        1.565_488_348_110_618_8,
    ];
    let mut poles = Vec::new();
    let mut ws = Vec::new();
    for i in 0..4 {
        for j in 0..2 {
            poles.push(Vec3::new(i as f64 / 3.0, f64::from(j), z[i]));
            ws.push(w[i]);
        }
    }
    square(Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: Some(ws),
        count_u: 4,
        count_v: 2,
    }))
}

/// The worst case of each family, in u and in v: on main, 833 (F1), 24,823
/// (F2) and 19.0 (F3) times the bound.
#[test]
fn kinks_beside_midpoints_and_close_pairs_keep_the_area_error_within_the_bound() {
    let mut cases: Vec<(String, FaceDefinition, f64)> = Vec::new();
    let u0 = 0.5 - 7.535_659_294_528_74e-4;
    for in_v in [false, true] {
        cases.push((
            format!("kink beside the midpoint, in_v {in_v}"),
            cubic_face(&para(1e5, u0), in_v, None),
            para_truth(1e5, u0),
        ));
        let (face, t) = two_kinks(1e6, 0.03, 0.3253, in_v);
        cases.push((format!("two close kinks, in_v {in_v}"), face, t));
    }
    let rat = rational_worst();
    let g = |u: f64| {
        let (_, su, sv) = rat.surface.evaluate(u, 0.5).unwrap();
        su.cross(sv).length()
    };
    let t = composite(g, &[0.0, 0.749_56, 1.0], 20_000);
    cases.push(("rational net".into(), rat, t));
    for (label, face, truth) in &cases {
        let m = face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7).unwrap();
        let err = (m.raw[0] - truth).abs();
        assert!(
            err <= m.bound[0] + 1e-12 * truth,
            "{label}: error {err:e} > bound {:e}",
            m.bound[0]
        );
    }
}

/// `z' = a (t - r1)(t - r2)` with r1, r2 close: two kinks whose directions
/// on both sides agree.
#[test]
#[ignore = "a review A6-1 probe"]
fn probe_two_close_kinks() {
    for in_v in [false, true] {
        let mut st = Stats::default();
        for &a in &[1e3, 1e4, 1e5, 1e6, 1e7] {
            for &delta in &[1e-3, 3e-3, 1e-2, 3e-2, 6e-2, 0.1] {
                for k in 0..23 {
                    let c = 0.03 + 0.042 * f64::from(k) + 0.0013;
                    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
                    // z = a (t^3/3 - (r1+r2) t^2/2 + r1 r2 t)
                    let co = [0.0, a * r1 * r2, -0.5 * a * (r1 + r2), a / 3.0];
                    let face = cubic_face(&co, in_v, None);
                    let fp = |t: f64| a * (t - r1) * (t - r2);
                    let t = composite(
                        |t| (1.0 + fp(t) * fp(t)).sqrt(),
                        &[0.0, r1, r2, 1.0],
                        20_000,
                    );
                    let label = format!("in_v {in_v} a {a} delta {delta} c {c}");
                    check(&face, t, &label, &mut st);
                }
            }
        }
        st.print(&format!("two close kinks, in_v {in_v}"));
    }
}

/// Open question 2: `z = c phi(u) (v - v0)`: the outer direction (the v
/// integral of n) cancels near phi's root, and the inner kink at v0 sits
/// near the inner span's midpoint.
#[test]
#[ignore = "a review A6-1 probe"]
fn probe_cancelling_outer_direction() {
    let mut st = Stats::default();
    for &c in &[1e2, 1e3, 1e4, 1e5] {
        for &u1 in &[0.5003, 0.4997, 0.3, 0.7501] {
            for &v0 in &[0.5, 0.4996, 0.5004, 0.5 - 2e-4, 0.3, 0.75 - 1e-4] {
                // phi(u) = u - u1 (cubic form).
                let ph = bezier(&[-u1, 1.0, 0.0, 0.0]);
                let mut poles = Vec::new();
                for i in 0..4 {
                    for j in 0..2 {
                        poles.push(Vec3::new(
                            i as f64 / 3.0,
                            f64::from(j),
                            c * ph[i] * (f64::from(j) - v0),
                        ));
                    }
                }
                let face = square(Surface::Nurbs(NurbsSurface {
                    degree_u: 3,
                    degree_v: 1,
                    knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    knots_v: vec![0.0, 0.0, 1.0, 1.0],
                    poles,
                    weights: None,
                    count_u: 4,
                    count_v: 2,
                }));
                // |n|^2 = 1 + c^2 (v - v0)^2 + c^2 (u - u1)^2.
                let inner = |u: f64| {
                    let al = 1.0 + c * c * (u - u1) * (u - u1);
                    let be = c * c;
                    let f = |t: f64| {
                        0.5 * t * (al + be * t * t).sqrt()
                            + al / (2.0 * be.sqrt()) * libm::asinh(t * (be / al).sqrt())
                    };
                    f(1.0 - v0) - f(-v0)
                };
                let t = composite(inner, &[0.0, u1, 1.0], 40_000);
                let label = format!("c {c} u1 {u1} v0 {v0}");
                check(&face, t, &label, &mut st);
            }
        }
    }
    st.print("cancelling outer direction");
}

/// A kink in v crossed by a slanted top trim near the outer midpoint.
#[test]
#[ignore = "a review A6-1 probe"]
fn probe_trim_across_a_kink() {
    let mut st = Stats::default();
    for &a in &[1e3, 1e4, 1e5, 1e6] {
        for &e in &[0.01, 0.05, 0.2] {
            for &uc in &[0.5, 0.4995, 0.5005, 0.3, 0.25 - 1e-4] {
                let v0 = 0.6;
                // top edge v(u) = v0 + 2e (u - uc), from u = 1 back to u = 0.
                let vt = |u: f64| v0 + 2.0 * e * (u - uc);
                let face = poly_face(
                    {
                        let mut f = cubic_face(&para(a, v0), true, None);
                        std::mem::replace(
                            &mut f.surface,
                            Surface::Plane {
                                origin: Vec3::new(0.0, 0.0, 0.0),
                                u_axis: Vec3::new(1.0, 0.0, 0.0),
                                v_axis: Vec3::new(0.0, 1.0, 0.0),
                            },
                        )
                    },
                    [[1.0, vt(1.0)], [0.0, vt(0.0)]],
                );
                let f = |t: f64| 0.5 * (t * (1.0 + a * a * t * t).sqrt() + libm::asinh(a * t) / a);
                let t = composite(|u| f(vt(u) - v0) - f(-v0), &[0.0, uc, 1.0], 40_000);
                let label = format!("a {a} e {e} uc {uc}");
                check(&face, t, &label, &mut st);
            }
        }
    }
    st.print("trim across a kink");
}

/// Rational review-style nets: one span, random z poles and random weights
/// (per u pole), truth by composite GL5 of |n| at v = 0.5.
#[test]
#[ignore = "a review A6-1 probe"]
fn probe_rational_nets() {
    let mut st = Stats::default();
    let mut s = 12345u64;
    let mut next = || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    for &amp in &[300.0, 3000.0, 30000.0] {
        for _ in 0..150 {
            let z = [0, 1, 2, 3].map(|_| amp * (2.0 * next() - 1.0));
            let w = [0, 1, 2, 3].map(|_| 0.3 + 2.7 * next());
            let mut poles = Vec::new();
            let mut ws = Vec::new();
            for i in 0..4 {
                for j in 0..2 {
                    poles.push(Vec3::new(i as f64 / 3.0, f64::from(j), z[i]));
                    ws.push(w[i]);
                }
            }
            let face = square(Surface::Nurbs(NurbsSurface {
                degree_u: 3,
                degree_v: 1,
                knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                knots_v: vec![0.0, 0.0, 1.0, 1.0],
                poles,
                weights: Some(ws),
                count_u: 4,
                count_v: 2,
            }));
            let g = |u: f64| {
                let (_, su, sv) = face.surface.evaluate(u, 0.5).unwrap();
                su.cross(sv).length()
            };
            let t = composite(g, &[0.0, 1.0], 100_000);
            check(&face, t, &format!("amp {amp} z {z:?} w {w:?}"), &mut st);
        }
    }
    st.print("rational nets");
}

/// Independent truths (composite GL5 of the facts evaluator's |S_u x S_v|)
/// at two resolutions for the worst cases.
#[test]
#[ignore = "a review A6-1 probe"]
fn verify_worst_cases() {
    let eval_truth = |face: &FaceDefinition, in_v: bool, br: &[f64], cells: usize| {
        composite(
            |t| {
                let (u, v) = if in_v { (0.5, t) } else { (t, 0.5) };
                let (_, su, sv) = face.surface.evaluate(u, v).unwrap();
                su.cross(sv).length()
            },
            br,
            cells,
        )
    };
    let mut cases: Vec<(String, FaceDefinition, bool, Vec<f64>)> = Vec::new();
    let u0 = 0.5 - 7.535_659_294_528_74e-4;
    cases.push((
        "midpoint a 1e5".into(),
        cubic_face(&para(1e5, u0), false, None),
        false,
        vec![0.0, u0, 1.0],
    ));
    for (a, delta, c, in_v) in [
        (1e6, 0.03, 0.3253000000000001, false),
        (1e3, 0.1, 0.9553, true),
        (1e5, 0.001, 0.28330000000000005, false),
    ] {
        let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
        let co = [0.0, a * r1 * r2, -0.5 * a * (r1 + r2), a / 3.0];
        cases.push((
            format!("two kinks a {a} delta {delta} c {c} in_v {in_v}"),
            cubic_face(&co, in_v, None),
            in_v,
            vec![0.0, r1, r2, 1.0],
        ));
    }
    let z = [
        1688.3997264262862,
        -23107.094776366353,
        1833.8591711237173,
        -17243.702287599328,
    ];
    let w = [
        0.8168441936202806,
        2.747967098896512,
        1.9498532409269198,
        1.5654883481106188,
    ];
    let mut poles = Vec::new();
    let mut ws = Vec::new();
    for i in 0..4 {
        for j in 0..2 {
            poles.push(Vec3::new(i as f64 / 3.0, f64::from(j), z[i]));
            ws.push(w[i]);
        }
    }
    let rat = square(Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: Some(ws),
        count_u: 4,
        count_v: 2,
    }));
    cases.push(("rational worst".into(), rat, false, vec![0.0, 1.0]));
    for (label, face, in_v, br) in &cases {
        let t1 = eval_truth(face, *in_v, br, 200_000);
        let t2 = eval_truth(face, *in_v, br, 1_000_000);
        let m = face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7).unwrap();
        println!(
            "VERIFY {label}: truth200k {t1:.15e} truth1M {t2:.15e} |dt| {:e} raw {:.15e} err {:e} bound {:e} ratio {:.3}",
            (t1 - t2).abs(), m.raw[0], (m.raw[0] - t2).abs(), m.bound[0], (m.raw[0] - t2).abs() / m.bound[0]
        );
    }
}
