//! Report A6-2 open point 1 and review A6-1 F6: softened kinks of the area
//! element `|n|` that an outer integral's checks miss where its hull does not
//! apply, or where it applies to the v integral of `n` rather than to `n`.
//! Each family is two close sign changes of a normal component in u
//! (`f' = a (u - r1)(u - r2)`), whose directions on both sides agree, on one
//! Bezier span with `x = u`, `y = v`, at accuracy 1e-7:
//!
//! - rational nets (random weights per u pole);
//! - curved top trims, a polynomial and a rational quadratic;
//! - outer directions that cancel (`z = f(u) + c (v - 1/2)^2`: the v
//!   integral of `n_y` is zero);
//! - F6, `z = f(u) + H v^m`: the v integral of `n` keeps the large constant
//!   `n_y` part while `|n|` dips near `v = 0`; and the same dip inside the
//!   face, `z = f(u) + H (v - 2/5)^5`.
//!
//! The truths are composite five-point Gauss-Legendre with the kinks as
//! breaks (closed-form inner integrals where there is one). The probes are
//! ignored; the regression test holds the worst case of each family on main.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    clippy::too_many_lines,
    missing_docs
)]

use geospec_facts::{
    face_mass, Curve2, FaceDefinition, Material, NurbsCurve2, NurbsSurface, Orientation, Surface,
    TrimPiece, Vec3,
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

/// The unit square of `surface`, with `top` (from `(1, 1)` to `(0, 1)`) as
/// its upper edge.
fn face(surface: Surface, top: TrimPiece) -> FaceDefinition {
    FaceDefinition {
        surface,
        loops: vec![4],
        trims: vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            top,
            line([0.0, 1.0], [0.0, 0.0]),
        ],
        orientation: Orientation::Forward,
        material: Material::OneSide,
    }
}

fn square(surface: Surface) -> FaceDefinition {
    face(surface, line([1.0, 1.0], [0.0, 1.0]))
}

const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Composite GL5 over `[breaks]` clamped to `[0, 1]`, `cells` per piece.
fn composite(g: impl Fn(f64) -> f64, breaks: &[f64], cells: usize) -> f64 {
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

/// The Bernstein coefficients on `[0, 1]` of the power polynomial `c`.
fn bezier(c: &[f64]) -> Vec<f64> {
    let d = c.len() - 1;
    (0..=d)
        .map(|i| {
            (0..=i)
                .map(|k| binomial(i, k) / binomial(d, k) * c[k])
                .sum()
        })
        .collect()
}

/// `f = a (t^3/3 - (r1 + r2) t^2/2 + r1 r2 t)`, `f' = a (t - r1)(t - r2)`.
fn two_kinks(a: f64, r1: f64, r2: f64) -> [f64; 4] {
    [0.0, a * r1 * r2, -0.5 * a * (r1 + r2), a / 3.0]
}

/// One Bezier span, degrees `(3, q)`: `x = u`, `y = v`, `z = f(u) + g(v)`
/// with `f`, `g` the power polynomials `fu` (cubic) and `gv` (degree `q`),
/// and weights `w` per u pole.
fn net(fu: &[f64; 4], gv: &[f64], w: Option<[f64; 4]>) -> Surface {
    let q = gv.len() - 1;
    let (zf, zg) = (bezier(fu), bezier(gv));
    let mut poles = Vec::new();
    let mut ws = Vec::new();
    for i in 0..4 {
        for j in 0..=q {
            poles.push(Vec3::new(
                i as f64 / 3.0,
                j as f64 / q as f64,
                zf[i] + zg[j],
            ));
            ws.push(w.map_or(1.0, |w| w[i]));
        }
    }
    Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: q,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        knots_v: [vec![0.0; q + 1], vec![1.0; q + 1]].concat(),
        poles,
        weights: w.map(|_| ws),
        count_u: 4,
        count_v: q + 1,
    })
}

/// Counts a face outside its bound (`err > bound + 1e-12 truth`) or refused.
#[derive(Default)]
struct Stats {
    total: usize,
    outside: usize,
    refused: usize,
    worst: f64,
    worst_label: String,
    panels: usize,
}

impl Stats {
    fn check(&mut self, face: &FaceDefinition, truth: f64, label: &str) {
        match face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7) {
            Ok(m) => {
                let err = (m.raw[0] - truth).abs();
                let r = err / m.bound[0];
                self.total += 1;
                self.panels += m.panels;
                if r > self.worst {
                    self.worst = r;
                    self.worst_label = label.to_string();
                }
                if err > m.bound[0] + 1e-12 * truth {
                    self.outside += 1;
                    println!(
                        "OUTSIDE {label}: raw {:.15e} truth {truth:.15e} error {err:e} bound {:e} ratio {r:.3} panels {}",
                        m.raw[0], m.bound[0], m.panels
                    );
                }
            }
            Err(r) => {
                self.refused += 1;
                println!("REFUSED {label}: {r}");
            }
        }
    }

    fn print(&self, name: &str) {
        println!(
            "{name}: {}/{} outside, {} refused, worst ratio {:.4} ({}), panels {}",
            self.outside,
            self.total + self.refused,
            self.refused,
            self.worst,
            self.worst_label,
            self.panels
        );
    }
}

/// The parameters in `[0, 1]` where `g` changes sign, by sampling and
/// bisection.
fn roots(g: impl Fn(f64) -> f64) -> Vec<f64> {
    let n = 200_000;
    let mut out = Vec::new();
    let mut prev = g(0.0);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let y = g(t);
        if (prev < 0.0) != (y < 0.0) {
            let (mut lo, mut hi) = (t - 1.0 / n as f64, t);
            for _ in 0..80 {
                let m = 0.5 * (lo + hi);
                if (g(m) < 0.0) == (prev < 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        prev = y;
    }
    out
}

/// The positions of the root pair's centre the sweeps use.
fn centres(k: usize) -> f64 {
    0.03 + 0.0947 * k as f64 + 0.0013
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A rational net with two close kinks; its area from the evaluator's
/// `|S_u x S_v|` at `v = 1/2` (`n` does not depend on v), breaking at the
/// sign changes of `n_x`.
fn rational_case(a: f64, delta: f64, c: f64, w: [f64; 4]) -> (FaceDefinition, f64) {
    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
    let f = square(net(&two_kinks(a, r1, r2), &[0.0, 0.0], Some(w)));
    let n = |u: f64| {
        let (_, su, sv) = f.surface.evaluate(u, 0.5).unwrap();
        su.cross(sv)
    };
    let mut br = roots(|u| n(u).x);
    br.extend([0.0, 1.0]);
    let t = composite(|u| n(u).length(), &br, 20_000);
    (f, t)
}

#[test]
#[ignore = "an A6b probe"]
fn probe_rational_nets_with_close_kinks() {
    let mut st = Stats::default();
    let mut rng = Lcg(29);
    for &a in &[1e3, 1e4, 1e5, 1e6, 1e7] {
        for &delta in &[1e-3, 3e-3, 1e-2, 3e-2, 0.1] {
            for k in 0..10 {
                let w = [0, 1, 2, 3].map(|_| 0.4 + 2.1 * rng.next());
                let (f, t) = rational_case(a, delta, centres(k), w);
                st.check(
                    &f,
                    t,
                    &format!("a {a} delta {delta} c {} w {w:?}", centres(k)),
                );
            }
        }
    }
    st.print("rational nets with close kinks");
}

/// A curved top edge over a net with two close kinks: from `(1, 1)` to
/// `(0, 1)` through the pole `(1/2, 2/5)` with weight `w` (1: a polynomial).
fn curved_case(a: f64, delta: f64, c: f64, w: f64) -> (FaceDefinition, f64) {
    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
    let curve = Curve2::Nurbs(NurbsCurve2 {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        poles: vec![[1.0, 1.0], [0.5, 0.4], [0.0, 1.0]],
        weights: (w != 1.0).then(|| vec![1.0, w, 1.0]),
    });
    let top = TrimPiece {
        curve: curve.clone(),
        range: [0.0, 1.0],
        reversed: false,
    };
    let f = face(net(&two_kinks(a, r1, r2), &[0.0, 0.0], None), top);
    let fp = |u: f64| a * (u - r1) * (u - r2);
    // The area is the integral over the curve's parameter of `|n(u)| v |u'|`.
    let at = |t: f64| curve.evaluate(t).unwrap();
    let mut br = roots(|t| fp(at(t).0[0]));
    br.extend([0.0, 1.0]);
    let t = composite(
        |t| {
            let (p, d) = at(t);
            (1.0 + fp(p[0]) * fp(p[0])).sqrt() * p[1] * d[0].abs()
        },
        &br,
        20_000,
    );
    (f, t)
}

#[test]
#[ignore = "an A6b probe"]
fn probe_curved_trims_with_close_kinks() {
    let mut st = Stats::default();
    for &w in &[1.0, 0.6, 2.0] {
        for &a in &[1e3, 1e4, 1e5, 1e6, 1e7] {
            for &delta in &[1e-3, 3e-3, 1e-2, 3e-2, 0.1] {
                for k in 0..10 {
                    let (f, t) = curved_case(a, delta, centres(k), w);
                    st.check(
                        &f,
                        t,
                        &format!("w {w} a {a} delta {delta} c {}", centres(k)),
                    );
                }
            }
        }
    }
    st.print("curved trims with close kinks");
}

/// `z = f(u) + s (v - 1/2)^2`: the v integral of `n_y` is zero, so where
/// `s` is large the outer node's direction is under half its area value and
/// ignored; the inner integral is closed form.
fn cancelling_case(a: f64, delta: f64, c: f64, s: f64) -> (FaceDefinition, f64) {
    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
    let f = square(net(&two_kinks(a, r1, r2), &[0.25 * s, -s, s], None));
    let fp = |u: f64| a * (u - r1) * (u - r2);
    let beta = 4.0 * s * s;
    let inner = |u: f64| {
        let al = 1.0 + fp(u) * fp(u);
        let g = |t: f64| {
            0.5 * t * (al + beta * t * t).sqrt()
                + al / (2.0 * beta.sqrt()) * libm::asinh(t * (beta / al).sqrt())
        };
        g(0.5) - g(-0.5)
    };
    (f, composite(inner, &[0.0, r1, r2, 1.0], 20_000))
}

#[test]
#[ignore = "an A6b probe"]
fn probe_cancelling_outer_directions_with_close_kinks() {
    let mut st = Stats::default();
    for &s in &[1e2, 1e3, 1e4] {
        for &a in &[1e3, 1e4, 1e5, 1e6, 1e7] {
            for &delta in &[1e-3, 3e-3, 1e-2, 3e-2, 0.1] {
                for k in 0..10 {
                    let (f, t) = cancelling_case(a, delta, centres(k), s);
                    st.check(
                        &f,
                        t,
                        &format!("s {s} a {a} delta {delta} c {}", centres(k)),
                    );
                }
            }
        }
    }
    st.print("cancelling outer directions with close kinks");
}

/// F6: `z = f(u) + H (v - v0)^m`, with `|n| = sqrt(1 + f'^2 + (m H (v -
/// v0)^(m-1))^2)` integrated over v on graded cells about `v0`.
fn dip_case(m: usize, a: f64, delta: f64, hh: f64, c: f64, v0: f64) -> (FaceDefinition, f64) {
    let (r1, r2) = (c - 0.5 * delta, c + 0.5 * delta);
    // Power coefficients of H (v - v0)^m.
    let gv: Vec<f64> = (0..=m)
        .map(|k| hh * binomial(m, k) * libm::pow(-v0, (m - k) as f64))
        .collect();
    let f = square(net(&two_kinks(a, r1, r2), &gv, None));
    let fp = |u: f64| a * (u - r1) * (u - r2);
    let mut vb = vec![0.0, v0, 1.0];
    for d in [1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.1] {
        vb.extend([v0 - d, v0 + d]);
    }
    let inner = |u: f64| {
        let f2 = 1.0 + fp(u) * fp(u);
        composite(
            |v| {
                let hp = m as f64 * hh * libm::pow(v - v0, (m - 1) as f64);
                (f2 + hp * hp).sqrt()
            },
            &vb,
            30,
        )
    };
    let mut ub = vec![0.0, r1, r2, 1.0];
    ub.extend([r1 - 0.01, r1 - 0.001, r2 + 0.001, r2 + 0.01, c]);
    (f, composite(inner, &ub, 800))
}

/// Review A6-1's sweep of F6 (`probe_outer_cone_masks_a_dip`, 1008 faces)
/// and its focus (72 faces), with the truth on the same cells.
#[test]
#[ignore = "an A6b probe (review A6-1 F6)"]
fn probe_outer_hull_masks_a_dip() {
    let mut st = Stats::default();
    for m in [3usize, 5] {
        for &a in &[1e4, 1e5, 1e6, 1e7] {
            for &delta in &[0.003, 0.01, 0.03] {
                for &hk in &[3.0, 10.0, 30.0, 100.0, 300.0, 1000.0] {
                    for k in 0..7 {
                        let c = 0.07 + 0.137 * f64::from(k);
                        let hh = hk * a * delta * delta;
                        let (f, t) = dip_case(m, a, delta, hh, c, 0.0);
                        st.check(&f, t, &format!("m {m} a {a} delta {delta} H {hh} c {c}"));
                    }
                }
            }
        }
    }
    st.print("outer hull masks a dip (review sweep)");
    let mut st = Stats::default();
    for &a in &[1e6, 1e7] {
        for &delta in &[0.005, 0.01, 0.02] {
            for &hk in &[300.0, 1000.0, 3000.0] {
                for c in [0.207, 0.344, 0.5003, 0.6551] {
                    let hh = hk * a * delta * delta;
                    let (f, t) = dip_case(5, a, delta, hh, c, 0.0);
                    st.check(&f, t, &format!("m 5 a {a} delta {delta} H {hh} c {c}"));
                }
            }
        }
    }
    st.print("outer hull masks a dip (review focus)");
}

/// The dip of F6 inside the face, at `v = 2/5`: the inner integrals' path
/// passes through it rather than starting at it.
#[test]
#[ignore = "an A6b probe"]
fn probe_dip_inside_the_face() {
    let mut st = Stats::default();
    for &a in &[1e4, 1e5, 1e6, 1e7] {
        for &delta in &[0.003, 0.01, 0.03] {
            for &hk in &[10.0, 100.0, 1000.0] {
                for k in 0..7 {
                    let c = 0.07 + 0.137 * f64::from(k);
                    let hh = hk * a * delta * delta;
                    let (f, t) = dip_case(5, a, delta, hh, c, 0.4);
                    st.check(&f, t, &format!("m 5 a {a} delta {delta} H {hh} c {c}"));
                }
            }
        }
    }
    st.print("dip inside the face");
}

/// The worst case of each family that main's outer checks miss: the curved
/// trim (22 496 times its bound on main), F6 (1.49 times) and the
/// cancelling outer directions (1.09 times). Each now stays within its bound.
#[test]
fn outer_cells_hold_main_worst_cases() {
    let mut st = Stats::default();
    let (f, t) = curved_case(1e6, 0.03, 0.5995, 0.6);
    st.check(&f, t, "curved w 0.6 a 1e6 delta 0.03 c 0.5995");
    let (f, t) = dip_case(5, 1e6, 0.01, 1e5, 0.344, 0.0);
    st.check(&f, t, "F6 m 5 a 1e6 delta 0.01 H 1e5 c 0.344");
    let (f, t) = cancelling_case(1e3, 0.1, 0.2207, 1e3);
    st.check(&f, t, "cancelling s 1e3 a 1e3 delta 0.1 c 0.2207");
    st.print("main's worst outer cases");
    assert_eq!((st.outside, st.refused, st.total), (0, 0, 3));
}
