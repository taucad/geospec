//! B-spline basis functions and rational evaluation with first derivatives.
//!
//! The surface is evaluated a row at a time: at a fixed u the homogeneous net
//! is contracted once with the u basis and its derivative ([`SurfaceRow`]), so
//! each v node of the inner integral costs one curve evaluation of degree
//! `degree_v`. This is the batching of the solidcore evaluation's patch 0012
//! (one u basis shared by every v node of a panel) in its cheapest form.
//!
//! Every sum runs in a fixed order with separate multiply and add.
//!
//! Surface evaluation also returns a bound on its own round-off: every sum
//! of products is accumulated a second time over absolute values (the basis
//! functions are non-negative, the derivative's terms are bounded by their
//! two parts' magnitudes), and the bound is a multiple of the unit round-off
//! times those magnitudes divided by the (positive) weight, propagated
//! through the rational quotient. Large weight ratios show up as magnitudes
//! far above the derivatives themselves, and the bound grows with them.
//! A net whose weights vary by more than [`MAX_WEIGHT_RATIO`] within one
//! span's poles is not evaluated: its parametrization can change over a
//! parameter interval below the floating-point resolution of the parameter,
//! where no sampled integral sees it.
//!
//! Preconditions, established by the definition's `validate`: degrees in
//! `1..=MAX_DEGREE`, `knots.len() == count + degree + 1`, non-decreasing knots
//! and a non-empty domain. Indexing below relies on them.
#![allow(clippy::indexing_slicing)]

use crate::definition::{NurbsCurve2, NurbsSurface};
use crate::vec::Vec3;

/// The largest degree accepted.
pub const MAX_DEGREE: usize = 25;
const MAXP1: usize = MAX_DEGREE + 1;
/// The largest ratio of the largest to the smallest weight among the poles
/// of one span (the `(p + 1) x (q + 1)` poles that define it). Within it the
/// rational basis changes over at least about a millionth of the span, and
/// the evaluator's error growth (the round-off bound) stays near `1e6` unit
/// round-offs, below the default accuracy of `1e-7`.
pub const MAX_WEIGHT_RATIO: f64 = 1e6;

/// The span `i` with `knots[i] <= t < knots[i + 1]`, clamped to
/// `degree..=count - 1` (`count - 1` at the domain's upper end), and never an
/// empty span: at a domain end with extra knot multiplicity the nearest
/// non-empty span inside the domain (the domain itself is non-empty).
fn find_span(knots: &[f64], degree: usize, count: usize, t: f64) -> usize {
    let n = count - 1;
    if t >= knots[n + 1] {
        let mut span = n;
        while span > degree && knots[span] >= knots[span + 1] {
            span -= 1;
        }
        return span;
    }
    if t <= knots[degree] {
        let mut span = degree;
        while span < n && knots[span] >= knots[span + 1] {
            span += 1;
        }
        return span;
    }
    let (mut lo, mut hi) = (degree, n + 1);
    let mut mid = (lo + hi) / 2;
    while t < knots[mid] || t >= knots[mid + 1] {
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// The `degree + 1` non-zero basis functions at `t` in span `span`, their
/// first derivatives (Piegl and Tiller A2.2 with the derivative from the
/// degree `p - 1` functions) and a bound on each derivative's magnitude
/// before cancellation (`p (|a| + |b|)`, the scale of its round-off).
/// The non-zero basis functions at `t` in `span` (as [`basis1`]'s `n`).
fn basis0(knots: &[f64], span: usize, degree: usize, t: f64, n: &mut [f64; MAXP1]) {
    let mut left = [0.0; MAXP1];
    let mut right = [0.0; MAXP1];
    n[0] = 1.0;
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
}

fn basis1(
    knots: &[f64],
    span: usize,
    degree: usize,
    t: f64,
    n: &mut [f64; MAXP1],
    d: &mut [f64; MAXP1],
    da: &mut [f64; MAXP1],
) {
    let mut left = [0.0; MAXP1];
    let mut right = [0.0; MAXP1];
    let mut lower = [0.0; MAXP1];
    n[0] = 1.0;
    for j in 1..=degree {
        if j == degree {
            lower[..degree].copy_from_slice(&n[..degree]);
        }
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    let p = degree as f64;
    for j in 0..=degree {
        let i = span - degree + j;
        let a = if j > 0 {
            let den = knots[i + degree] - knots[i];
            if den > 0.0 {
                lower[j - 1] / den
            } else {
                0.0
            }
        } else {
            0.0
        };
        let b = if j < degree {
            let den = knots[i + degree + 1] - knots[i + 1];
            if den > 0.0 {
                lower[j] / den
            } else {
                0.0
            }
        } else {
            0.0
        };
        d[j] = p * (a - b);
        da[j] = p * (a.abs() + b.abs());
    }
}

/// `t` clamped to `[lo, hi]`.
fn clamp(t: f64, lo: f64, hi: f64) -> f64 {
    if t < lo {
        lo
    } else if t > hi {
        hi
    } else {
        t
    }
}

/// A surface prepared for evaluation: the homogeneous net.
#[derive(Debug)]
pub struct PreparedSurface {
    degree_u: usize,
    degree_v: usize,
    knots_u: Vec<f64>,
    knots_v: Vec<f64>,
    count_u: usize,
    count_v: usize,
    /// `(w x, w y, w z, w)` about `origin`, u-major.
    net: Vec<[f64; 4]>,
    /// The centre of the poles' box: the net is evaluated relative to it so
    /// that a small surface far from the origin keeps its derivatives (the
    /// rational quotient would otherwise cancel them away).
    origin: Vec3,
    /// The largest ratio of weights among one span's poles.
    weight_ratio: f64,
}

/// One iso-u row: the net contracted with the u basis (`q`) and its derivative
/// (`dq`), one homogeneous point per v pole, and the same contractions over
/// absolute values (`qa`, `dqa`) for the round-off bound.
#[derive(Debug, Default)]
pub struct SurfaceRow {
    q: Vec<[f64; 4]>,
    dq: Vec<[f64; 4]>,
    qa: Vec<[f64; 4]>,
    dqa: Vec<[f64; 4]>,
}

/// The round-off bound of one evaluation, in the max norm: the point (about
/// the poles' box centre), `S_u` and `S_v`.
pub type EvalBound = [f64; 3];

/// The largest ratio of the largest to the smallest weight among the poles
/// of any non-empty span.
fn weight_ratio(s: &NurbsSurface) -> f64 {
    let Some(w) = s.weights.as_ref() else {
        return 1.0;
    };
    let (p, q) = (s.degree_u, s.degree_v);
    let mut worst = 1.0_f64;
    for i in p..s.count_u {
        if s.knots_u[i] >= s.knots_u[i + 1] {
            continue;
        }
        for j in q..s.count_v {
            if s.knots_v[j] >= s.knots_v[j + 1] {
                continue;
            }
            let (mut lo, mut hi) = (f64::INFINITY, 0.0_f64);
            for k in i - p..=i {
                for l in j - q..=j {
                    let x = w[k * s.count_v + l];
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            worst = worst.max(hi / lo);
        }
    }
    worst
}

impl PreparedSurface {
    pub fn new(s: &NurbsSurface) -> Self {
        let (mut lo, mut hi) = (s.poles[0], s.poles[0]);
        for p in &s.poles {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        let origin = 0.5 * (lo + hi);
        let net = s
            .poles
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let w = s.weights.as_ref().map_or(1.0, |w| w[k]);
                let q = *p - origin;
                [q.x * w, q.y * w, q.z * w, w]
            })
            .collect();
        Self {
            degree_u: s.degree_u,
            degree_v: s.degree_v,
            knots_u: s.knots_u.clone(),
            knots_v: s.knots_v.clone(),
            count_u: s.count_u,
            count_v: s.count_v,
            net,
            origin,
            weight_ratio: weight_ratio(s),
        }
    }

    /// The largest ratio of weights among one span's poles.
    pub const fn weight_ratio(&self) -> f64 {
        self.weight_ratio
    }

    /// `[[u_min, u_max], [v_min, v_max]]`.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        [
            [self.knots_u[self.degree_u], self.knots_u[self.count_u]],
            [self.knots_v[self.degree_v], self.knots_v[self.count_v]],
        ]
    }

    pub fn knots_u(&self) -> &[f64] {
        &self.knots_u
    }

    pub fn knots_v(&self) -> &[f64] {
        &self.knots_v
    }

    /// Fills `row` for `u` (clamped to the domain).
    pub fn row(&self, u: f64, row: &mut SurfaceRow) {
        let [[lo, hi], _] = self.domain();
        let u = clamp(u, lo, hi);
        let p = self.degree_u;
        let span = find_span(&self.knots_u, p, self.count_u, u);
        let mut n = [0.0; MAXP1];
        let mut d = [0.0; MAXP1];
        let mut da = [0.0; MAXP1];
        basis1(&self.knots_u, span, p, u, &mut n, &mut d, &mut da);
        row.q.clear();
        row.dq.clear();
        row.qa.clear();
        row.dqa.clear();
        for j in 0..self.count_v {
            let mut q = [0.0; 4];
            let mut dq = [0.0; 4];
            let mut qa = [0.0; 4];
            let mut dqa = [0.0; 4];
            for k in 0..=p {
                let pw = self.net[(span - p + k) * self.count_v + j];
                for c in 0..4 {
                    q[c] += n[k] * pw[c];
                    dq[c] += d[k] * pw[c];
                    qa[c] += n[k] * pw[c].abs();
                    dqa[c] += da[k] * pw[c].abs();
                }
            }
            row.q.push(q);
            row.dq.push(dq);
            row.qa.push(qa);
            row.dqa.push(dqa);
        }
    }

    /// `(S, S_u, S_v)` at `v` (clamped) on `row`.
    pub fn eval(&self, row: &SurfaceRow, v: f64) -> (Vec3, Vec3, Vec3) {
        self.eval_bounded(row, v).0
    }

    /// `(S, S_u, S_v)` at `v` (clamped) on `row`, and the bound on their
    /// round-off.
    pub fn eval_bounded(&self, row: &SurfaceRow, v: f64) -> ((Vec3, Vec3, Vec3), EvalBound) {
        let [_, [lo, hi]] = self.domain();
        let v = clamp(v, lo, hi);
        let q = self.degree_v;
        let span = find_span(&self.knots_v, q, self.count_v, v);
        let mut n = [0.0; MAXP1];
        let mut d = [0.0; MAXP1];
        let mut da = [0.0; MAXP1];
        basis1(&self.knots_v, span, q, v, &mut n, &mut d, &mut da);
        let mut a = [0.0; 4];
        let mut au = [0.0; 4];
        let mut av = [0.0; 4];
        let mut aa = [0.0; 4];
        let mut aua = [0.0; 4];
        let mut ava = [0.0; 4];
        for l in 0..=q {
            let k = span - q + l;
            let (pq, pdq) = (row.q[k], row.dq[k]);
            let (pqa, pdqa) = (row.qa[k], row.dqa[k]);
            for c in 0..4 {
                a[c] += n[l] * pq[c];
                au[c] += n[l] * pdq[c];
                av[c] += d[l] * pq[c];
                aa[c] += n[l] * pqa[c];
                aua[c] += n[l] * pdqa[c];
                ava[c] += da[l] * pqa[c];
            }
        }
        let (p, su, sv) = rational(a, au, av);
        // Each homogeneous sum has at most `2 (p + q + 2)` rounded operations
        // after the basis functions' own `2 (p + q)`; the margin covers the
        // quotient's.
        let g = 4.0 * (self.degree_u + q + 4) as f64 * f64::EPSILON;
        let w = a[3];
        let s = [p.x, p.y, p.z];
        let mut es = 0.0_f64;
        for c in 0..3 {
            es = es.max(g * (aa[c] + s[c].abs() * aa[3]) / w);
        }
        let derivative = |d: Vec3, ad: &[f64; 4], ada: &[f64; 4]| {
            let d = [d.x, d.y, d.z];
            let mut e = 0.0_f64;
            for c in 0..3 {
                e = e.max(
                    (g * (ada[c] + s[c].abs() * ada[3]) + ad[3].abs() * es) / w
                        + d[c].abs() * (g * aa[3] / w + f64::EPSILON),
                );
            }
            e
        };
        let bound = [es, derivative(su, &au, &aua), derivative(sv, &av, &ava)];
        ((p + self.origin, su, sv), bound)
    }
}

/// The Euclidean point and first derivatives from homogeneous ones.
fn rational(a: [f64; 4], au: [f64; 4], av: [f64; 4]) -> (Vec3, Vec3, Vec3) {
    let w = a[3];
    let s = Vec3::new(a[0] / w, a[1] / w, a[2] / w);
    let su = Vec3::new(
        (au[0] - au[3] * s.x) / w,
        (au[1] - au[3] * s.y) / w,
        (au[2] - au[3] * s.z) / w,
    );
    let sv = Vec3::new(
        (av[0] - av[3] * s.x) / w,
        (av[1] - av[3] * s.y) / w,
        (av[2] - av[3] * s.z) / w,
    );
    (s, su, sv)
}

/// A 2D curve prepared for evaluation: the homogeneous poles.
#[derive(Debug)]
pub struct PreparedCurve {
    degree: usize,
    knots: Vec<f64>,
    /// `(w u, w v, w)`.
    poles: Vec<[f64; 3]>,
}

impl PreparedCurve {
    pub fn new(c: &NurbsCurve2) -> Self {
        let poles = c
            .poles
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let w = c.weights.as_ref().map_or(1.0, |w| w[k]);
                [p[0] * w, p[1] * w, w]
            })
            .collect();
        Self {
            degree: c.degree,
            knots: c.knots.clone(),
            poles,
        }
    }

    pub const fn degree(&self) -> usize {
        self.degree
    }

    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Whether every weight is equal: the curve is a polynomial.
    pub fn is_polynomial(&self) -> bool {
        let first = self.poles.first().map(|p| p[2]);
        self.poles.iter().all(|p| Some(p[2]) == first)
    }

    /// The point at `t` (clamped to the knot domain), as [`Self::eval`]'s.
    pub fn point(&self, t: f64) -> [f64; 2] {
        let count = self.poles.len();
        let t = clamp(t, self.knots[self.degree], self.knots[count]);
        let span = find_span(&self.knots, self.degree, count, t);
        let mut n = [0.0; MAXP1];
        basis0(&self.knots, span, self.degree, t, &mut n);
        let mut a = [0.0; 3];
        for (k, nk) in n.iter().enumerate().take(self.degree + 1) {
            let pw = self.poles[span - self.degree + k];
            for c in 0..3 {
                a[c] += nk * pw[c];
            }
        }
        [a[0] / a[2], a[1] / a[2]]
    }

    /// The point and first derivative at `t` (clamped to the knot domain).
    pub fn eval(&self, t: f64) -> ([f64; 2], [f64; 2]) {
        let count = self.poles.len();
        let t = clamp(t, self.knots[self.degree], self.knots[count]);
        let span = find_span(&self.knots, self.degree, count, t);
        let mut n = [0.0; MAXP1];
        let mut d = [0.0; MAXP1];
        let mut da = [0.0; MAXP1];
        basis1(&self.knots, span, self.degree, t, &mut n, &mut d, &mut da);
        let mut a = [0.0; 3];
        let mut da = [0.0; 3];
        for k in 0..=self.degree {
            let pw = self.poles[span - self.degree + k];
            for c in 0..3 {
                a[c] += n[k] * pw[c];
                da[c] += d[k] * pw[c];
            }
        }
        let w = a[2];
        let p = [a[0] / w, a[1] / w];
        let dp = [(da[0] - da[2] * p[0]) / w, (da[1] - da[2] * p[1]) / w];
        (p, dp)
    }

    /// Writes into `out` (cleared first) the control points of the curve
    /// over `[a, b]` (clamped to the domain, `a <= b`): the Bézier points of each knot span it covers, from
    /// the blossom (de Boor's algorithm with `a` at the first levels and `b`
    /// at the rest). With positive weights the curve over `[a, b]` lies in
    /// their convex hull. `false` (and `out` empty) when a sub-weight is not
    /// positive and finite (no hull bound).
    pub fn control_points_into(&self, a: f64, b: f64, out: &mut Vec<[f64; 2]>) -> bool {
        out.clear();
        let count = self.poles.len();
        let (lo, hi) = (self.knots[self.degree], self.knots[count]);
        let (a, b) = (clamp(a, lo, hi), clamp(b, lo, hi));
        if a > b || a.is_nan() || b.is_nan() {
            return false;
        }
        let p = self.degree;
        // The distinct knots strictly inside `(a, b)` cut it into spans.
        let mut s0 = a;
        let mut ks = self.knots.iter().copied().filter(|&k| k > a && k < b);
        loop {
            let s1 = match ks.next() {
                Some(k) if k > s0 => k,
                Some(_) => continue,
                None => b,
            };
            let span = find_span(&self.knots, p, count, 0.5 * (s0 + s1));
            // The blossom with `s0` at the first `p - k` levels and `s1` at
            // the rest.
            for k in 0..=p {
                let mut d = [[0.0; 3]; MAXP1];
                for (j, dj) in d.iter_mut().enumerate().take(p + 1) {
                    *dj = self.poles[span - p + j];
                }
                for r in 1..=p {
                    let t = if r <= p - k { s0 } else { s1 };
                    for j in (r..=p).rev() {
                        let i = span - p + j;
                        let den = self.knots[i + p + 1 - r] - self.knots[i];
                        let alpha = if den > 0.0 {
                            (t - self.knots[i]) / den
                        } else {
                            0.0
                        };
                        for c in 0..3 {
                            d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
                        }
                    }
                }
                let q = d[p];
                if !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())) {
                    out.clear();
                    return false;
                }
                out.push([q[0] / q[2], q[1] / q[2]]);
            }
            if s1 >= b {
                break;
            }
            s0 = s1;
        }
        true
    }

    /// The smallest and largest v of the control polygon (a hull of the curve).
    pub fn hull(&self) -> [[f64; 2]; 2] {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in &self.poles {
            for c in 0..2 {
                let x = p[c] / p[2];
                lo[c] = lo[c].min(x);
                hi[c] = hi[c].max(x);
            }
        }
        [lo, hi]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_partitions_unity_and_derivatives_sum_to_zero() {
        let knots = [0.0, 0.0, 0.0, 0.0, 0.3, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
        let count = knots.len() - 4;
        for &t in &[0.0, 0.1, 0.3, 0.42, 0.5, 0.77, 1.0] {
            let span = find_span(&knots, 3, count, t);
            let mut n = [0.0; MAXP1];
            let mut d = [0.0; MAXP1];
            let mut da = [0.0; MAXP1];
            basis1(&knots, span, 3, t, &mut n, &mut d, &mut da);
            let s: f64 = n[..4].iter().sum();
            let ds: f64 = d[..4].iter().sum();
            assert!((s - 1.0).abs() < 1e-15, "{t}: {s}");
            assert!(ds.abs() < 1e-12, "{t}: {ds}");
        }
    }

    #[test]
    fn rational_quarter_circle_is_a_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let c = PreparedCurve::new(&NurbsCurve2 {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            poles: vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            weights: Some(vec![1.0, w, 1.0]),
        });
        for i in 0..=10 {
            let (p, d) = c.eval(f64::from(i) / 10.0);
            assert!((p[0] * p[0] + p[1] * p[1] - 1.0).abs() < 1e-15);
            // The tangent is perpendicular to the radius.
            assert!((p[0] * d[0] + p[1] * d[1]).abs() < 1e-14);
        }
    }
}
