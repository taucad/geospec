//! Review A4-R2 F2: a bicubic-by-linear net on `[0, 1]^2` with `x = u`,
//! `y = v` and `z` from five poles of amplitude up to 3000, u knots
//! `[0, 0, 0, 0, s, 1, 1, 1, 1]`, at accuracy 1e-7. Its area element
//! `sqrt(1 + z'(u)^2)` has a softened kink of width about `1 / |z''|` where
//! `z'` changes sign. The truth is composite five-point Gauss-Legendre over
//! each span.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::inconsistent_digit_grouping,
    clippy::needless_range_loop
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

fn net(s: f64, z: &[f64; 5]) -> FaceDefinition {
    let ku = vec![0.0, 0.0, 0.0, 0.0, s, 1.0, 1.0, 1.0, 1.0];
    let g = |i: usize| ku[i + 1..i + 4].iter().sum::<f64>() / 3.0;
    let mut poles = Vec::new();
    for (i, zi) in z.iter().enumerate() {
        for j in 0..2 {
            poles.push(Vec3::new(g(i), f64::from(j), *zi));
        }
    }
    FaceDefinition {
        surface: Surface::Nurbs(NurbsSurface {
            degree_u: 3,
            degree_v: 1,
            knots_u: ku,
            knots_v: vec![0.0, 0.0, 1.0, 1.0],
            poles,
            weights: None,
            count_u: 5,
            count_v: 2,
        }),
        loops: vec![4],
        trims: vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
        orientation: Orientation::Forward,
        material: Material::OneSide,
    }
}

/// Composite five-point Gauss-Legendre over each u span in `cells` cells:
/// the area and the volume term `z n_z` (about a centre at `z = 0`).
fn truth(face: &FaceDefinition, s: f64, cells: usize) -> [f64; 2] {
    let x = [
        0.0,
        0.538_469_310_105_683_1,
        -0.538_469_310_105_683_1,
        0.906_179_845_938_664,
        -0.906_179_845_938_664,
    ];
    let w = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_5,
        0.478_628_670_499_366_5,
        0.236_926_885_056_189_1,
        0.236_926_885_056_189_1,
    ];
    let mut total = [0.0; 2];
    for (a, b) in [(0.0, s), (s, 1.0)] {
        let h = (b - a) / cells as f64;
        for c in 0..cells {
            let m = a + (c as f64 + 0.5) * h;
            for (xk, wk) in x.iter().zip(&w) {
                let (p, su, sv) = face.surface.evaluate(m + 0.5 * h * xk, 0.5).unwrap();
                let n = su.cross(sv);
                total[0] += 0.5 * h * wk * n.length();
                total[1] += 0.5 * h * wk * p.z * n.z;
            }
        }
    }
    total
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

/// Counterexamples outside their bounds before this rule: the review's
/// worst net (13.6 times), two more of its probe's nets (3.3 and 1.7 times),
/// the transposed probe's worst (4.4 times: the kink inside the inner
/// integrals) and two kinks in the gap between a knot and a span's nearest
/// node (226 and 2.3 times). The area bound now contains the error.
#[test]
fn steep_nets_keep_the_area_error_within_the_bound() {
    let mut cases: Vec<(String, FaceDefinition, f64)> = Vec::new();
    for (s, z) in [
        (
            0.01,
            [
                -1460.2836059491142,
                -1944.777031284189,
                2414.6497049749805,
                510.90483922971066,
                2317.1897556315225,
            ],
        ),
        (
            0.01,
            [
                219.9669131245383,
                -14.429992540744063,
                286.3617829550742,
                139.89363433160477,
                292.62346181814036,
            ],
        ),
        (
            0.1,
            [
                -1914.315947396006,
                -2994.275439089542,
                9.097134911218774,
                -384.50918262406475,
                392.2224518309152,
            ],
        ),
    ] {
        let face = net(s, &z);
        let t = truth(&face, s, 40_000)[0];
        cases.push((format!("net s {s} z {z:?}"), face, t));
    }
    let (s, z) = (
        0.001,
        [
            -2019.7713873909934,
            1461.5309930833048,
            1448.77518335233,
            -1846.3711825241044,
            -2529.0957713249772,
        ],
    );
    let face = net_in_v(s, &z);
    let t = truth_in_v(&face, s);
    cases.push((format!("net in v s {s} z {z:?}"), face, t));
    for (s, a, u0) in [(0.01, 1e6, 0.011), (0.01, 1e8, 0.011)] {
        let (face, t) = parabola(s, a, u0);
        cases.push((format!("parabola s {s} a {a} u0 {u0}"), face, t));
    }
    for (label, face, t) in &cases {
        let m = face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7).unwrap();
        let err = (m.raw[0] - t).abs();
        assert!(
            err <= m.bound[0],
            "{label}: error {err:e} > bound {:e}",
            m.bound[0]
        );
    }
}

/// The review's whole probe: 720 nets (four knot positions, three
/// amplitudes, 60 random pole sets each). Prints every face outside its
/// bound and the totals; fails if any. Run with `--ignored --nocapture`.
#[test]
#[ignore = "the review's 720-net probe (about a minute in release)"]
fn review_probe_steep_nets() {
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let mut rng = Lcg(env("PROBE_SEED", 7));
    let per = env("PROBE_N", 60);
    let (mut outside, mut total, mut refused, mut panels) = (0, 0, 0, 0usize);
    let mut volume_outside = 0;
    let mut worst = 0.0_f64;
    for &s in &[0.5, 0.1, 0.01, 0.001] {
        for &amp in &[30.0, 300.0, 3000.0] {
            for _ in 0..per {
                let mut z = [0.0; 5];
                for zi in &mut z {
                    *zi = amp * (2.0 * rng.next() - 1.0);
                }
                let face = net(s, &z);
                let m = face_mass(&face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7);
                let Ok(m) = m else {
                    refused += 1;
                    println!("REFUSED s {s} z {z:?}: {:?}", m.unwrap_err());
                    continue;
                };
                total += 1;
                panels += m.panels;
                let [t, tv] = truth(&face, s, 40_000);
                let err = (m.raw[0] - t).abs();
                worst = worst.max(err / m.bound[0]);
                let verr = (m.raw[1] - tv).abs();
                if verr > m.bound[1] + 1e-12 * t * 3000.0 {
                    volume_outside += 1;
                    println!(
                        "VOLUME OUTSIDE s {s} amp {amp} z {z:?}: error {verr:e} bound {:e}",
                        m.bound[1]
                    );
                }
                if err > m.bound[0] + 1e-12 * t {
                    outside += 1;
                    println!(
                        "OUTSIDE s {s} amp {amp} z {z:?}: error {err:e} bound {:e} ratio {:.3} panels {}",
                        m.bound[0],
                        err / m.bound[0],
                        m.panels
                    );
                }
            }
        }
    }
    println!(
        "probe: {outside}/{total} outside, {refused} refused, worst ratio {worst:.4}, panels {panels}; volume outside {volume_outside}"
    );
    assert_eq!(outside, 0);
    assert_eq!(volume_outside, 0);
}

const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Composite five-point Gauss-Legendre of `g` between each pair of
/// consecutive `breaks`, `cells` cells each.
fn composite(g: impl Fn(f64) -> f64, breaks: &[f64], cells: usize) -> f64 {
    let mut total = 0.0;
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let h = (b - a) / cells as f64;
        for c in 0..cells {
            let m = a + (c as f64 + 0.5) * h;
            for (x, wt) in GL5 {
                total += 0.5 * h * wt * g(m + 0.5 * h * x);
            }
        }
    }
    total
}

fn square(surface: Surface) -> FaceDefinition {
    FaceDefinition {
        surface,
        loops: vec![4],
        trims: vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
        orientation: Orientation::Forward,
        material: Material::OneSide,
    }
}

/// The face's area error against `truth`, or `None` if it refuses.
fn area_check(face: &FaceDefinition, truth: f64, label: &str) -> Option<(f64, f64, usize)> {
    match face_mass(face, Vec3::new(0.5, 0.5, 0.0), 1.0, 1e-7) {
        Ok(m) => {
            let err = (m.raw[0] - truth).abs();
            if err > m.bound[0] + 1e-12 * truth {
                println!(
                    "OUTSIDE {label}: error {err:e} bound {:e} ratio {:.3} panels {}",
                    m.bound[0],
                    err / m.bound[0],
                    m.panels
                );
            }
            Some((err, m.bound[0], m.panels))
        }
        Err(r) => {
            println!("REFUSED {label}: {r:?}");
            None
        }
    }
}

/// The review's net transposed: `x = u`, `y` from the v knots' Greville
/// points, `z` from the poles in v.
fn net_in_v(s: f64, z: &[f64; 5]) -> FaceDefinition {
    let kv = vec![0.0, 0.0, 0.0, 0.0, s, 1.0, 1.0, 1.0, 1.0];
    let g = |j: usize| kv[j + 1..j + 4].iter().sum::<f64>() / 3.0;
    let mut poles = Vec::new();
    for i in 0..2 {
        for (j, zj) in z.iter().enumerate() {
            poles.push(Vec3::new(f64::from(i), g(j), *zj));
        }
    }
    square(Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 3,
        knots_u: vec![0.0, 0.0, 1.0, 1.0],
        knots_v: kv,
        poles,
        weights: None,
        count_u: 2,
        count_v: 5,
    }))
}

fn truth_in_v(face: &FaceDefinition, s: f64) -> f64 {
    composite(
        |v| {
            let (_, su, sv) = face.surface.evaluate(0.5, v).unwrap();
            su.cross(sv).length()
        },
        &[0.0, s, 1.0],
        40_000,
    )
}

/// The probe transposed: the steep cubic runs in v, so the softened kink
/// is inside the inner integrals.
#[test]
#[ignore = "a 720-net probe with the kink in the inner integrals"]
fn probe_steep_nets_in_v() {
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let mut rng = Lcg(env("PROBE_SEED", 11));
    let per = env("PROBE_N", 60);
    let (mut outside, mut total, mut panels) = (0, 0, 0usize);
    let mut worst = 0.0_f64;
    for &s in &[0.5, 0.1, 0.01, 0.001] {
        for &amp in &[30.0, 300.0, 3000.0] {
            for _ in 0..per {
                let mut z = [0.0; 5];
                for zi in &mut z {
                    *zi = amp * (2.0 * rng.next() - 1.0);
                }
                let face = net_in_v(s, &z);
                let t = truth_in_v(&face, s);
                let Some((err, bound, p)) =
                    area_check(&face, t, &format!("s {s} amp {amp} z {z:?}"))
                else {
                    continue;
                };
                total += 1;
                panels += p;
                worst = worst.max(err / bound);
                outside += usize::from(err > bound + 1e-12 * t);
            }
        }
    }
    println!("probe in v: {outside}/{total} outside, worst ratio {worst:.4}, panels {panels}");
    assert_eq!(outside, 0);
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// A bicubic patch `x = u`, `y = v`, `z = f(u + c v)` with `f` cubic and
/// `f'` vanishing at `r1` and `r2`: the softened kinks run slanted across
/// both parameters, so they move along the inner integrals with u.
#[test]
#[ignore = "a 720-net probe with slanted kinks"]
fn probe_slanted_steep_nets() {
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let mut rng = Lcg(env("PROBE_SEED", 13));
    let per = env("PROBE_N", 60);
    let (mut outside, mut total, mut panels) = (0, 0, 0usize);
    let mut worst = 0.0_f64;
    for &c in &[0.05, 0.3, 0.7, 0.95] {
        for &amp in &[30.0, 300.0, 3000.0] {
            for _ in 0..per {
                let a = amp * (2.0 * rng.next() - 1.0);
                let (r1, r2) = ((1.0 + c) * rng.next(), (1.0 + c) * rng.next());
                // f(t) = a (t^3 - 1.5 (r1 + r2) t^2 + 3 r1 r2 t).
                let b = [0.0, 3.0 * a * r1 * r2, -1.5 * a * (r1 + r2), a];
                // Power coefficients of f(u + c v): coef[i][j] of u^i v^j.
                let mut coef = [[0.0; 4]; 4];
                for (k, bk) in b.iter().enumerate() {
                    for i in 0..=k {
                        let j = k - i;
                        coef[i][j] += bk * binomial(k, i) * (0..j).fold(1.0, |p, _| p * c);
                    }
                }
                let mut poles = Vec::new();
                for i in 0..4 {
                    for j in 0..4 {
                        let mut z = 0.0;
                        for (p, row) in coef.iter().enumerate().take(i + 1) {
                            for (q, cpq) in row.iter().enumerate().take(j + 1) {
                                z += binomial(i, p) / binomial(3, p) * binomial(j, q)
                                    / binomial(3, q)
                                    * cpq;
                            }
                        }
                        poles.push(Vec3::new(i as f64 / 3.0, j as f64 / 3.0, z));
                    }
                }
                let face = square(Surface::Nurbs(NurbsSurface {
                    degree_u: 3,
                    degree_v: 3,
                    knots_u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    knots_v: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    poles,
                    weights: None,
                    count_u: 4,
                    count_v: 4,
                }));
                // |n| = sqrt(1 + (1 + c^2) f'(t)^2) at t = u + c v, weighted by
                // the length of {v in [0, 1] : 0 <= t - c v <= 1}.
                let fp = |t: f64| 3.0 * a * (t - r1) * (t - r2);
                let m = |t: f64| ((t / c).min(1.0) - ((t - 1.0) / c).max(0.0)).max(0.0);
                let mut br = vec![0.0, c, 1.0, 1.0 + c, r1, r2];
                br.sort_by(f64::total_cmp);
                let t = composite(
                    |t| (1.0 + (1.0 + c * c) * fp(t) * fp(t)).sqrt() * m(t),
                    &br,
                    20_000,
                );
                let label = format!("c {c} a {a} r {r1} {r2}");
                let Some((err, bound, p)) = area_check(&face, t, &label) else {
                    continue;
                };
                total += 1;
                panels += p;
                worst = worst.max(err / bound);
                outside += usize::from(err > bound + 1e-12 * t);
            }
        }
    }
    println!("probe slanted: {outside}/{total} outside, worst ratio {worst:.4}, panels {panels}");
    assert_eq!(outside, 0);
}

/// `x = u`, `y = v`, `z = A (u - u0)^2 / 2` on the probe's knots
/// `[0, 0, 0, 0, s, 1, 1, 1, 1]` (poles by blossoming): one softened kink of
/// width `1 / A` at `u0`, placed at and near the knot `s`, where a span's
/// nodes may all lie on one side of it.
fn parabola(s: f64, a: f64, u0: f64) -> (FaceDefinition, f64) {
    let ku = [0.0, 0.0, 0.0, 0.0, s, 1.0, 1.0, 1.0, 1.0];
    let blossom = |x: &[f64]| {
        let e1 = (x[0] + x[1] + x[2]) / 3.0;
        let e2 = (x[0] * x[1] + x[0] * x[2] + x[1] * x[2]) / 3.0;
        0.5 * a * (e2 - 2.0 * u0 * e1 + u0 * u0)
    };
    let mut poles = Vec::new();
    for i in 0..5 {
        let x = &ku[i + 1..i + 4];
        for j in 0..2 {
            poles.push(Vec3::new(
                x.iter().sum::<f64>() / 3.0,
                f64::from(j),
                blossom(x),
            ));
        }
    }
    let face = square(Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: ku.to_vec(),
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 5,
        count_v: 2,
    }));
    let f = |t: f64| 0.5 * (t * (1.0 + a * a * t * t).sqrt() + libm::asinh(a * t) / a);
    (face, f(1.0 - u0) - f(-u0))
}

#[test]
#[ignore = "kinks at and near a knot"]
fn probe_kink_at_a_knot() {
    let (mut outside, mut total, mut panels) = (0, 0, 0usize);
    let mut worst = 0.0_f64;
    for &s in &[0.5, 0.1, 0.01] {
        for &a in &[1e2, 1e4, 1e6, 1e8] {
            for k in 0..=40 {
                for side in [-1.0, 1.0] {
                    let d = if k == 40 {
                        0.0
                    } else {
                        s * libm::pow(10.0, -1.0 - 0.2 * f64::from(k))
                    };
                    let u0 = s + side * d;
                    let (face, t) = parabola(s, a, u0);
                    let label = format!("s {s} a {a} u0 {u0} (knot {side:+} {d:e})");
                    let Some((err, bound, p)) = area_check(&face, t, &label) else {
                        continue;
                    };
                    total += 1;
                    panels += p;
                    worst = worst.max(err / bound);
                    outside += usize::from(err > bound + 1e-12 * t);
                }
            }
        }
    }
    println!("probe knot: {outside}/{total} outside, worst ratio {worst:.4}, panels {panels}");
    assert_eq!(outside, 0);
}
