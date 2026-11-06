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
//! Preconditions, established by the definition's `validate`: degrees in
//! `1..=MAX_DEGREE`, `knots.len() == count + degree + 1`, non-decreasing knots
//! and a non-empty domain. Indexing below relies on them.
#![allow(clippy::indexing_slicing)]

use crate::definition::{NurbsCurve2, NurbsSurface};
use crate::vec::Vec3;

/// The largest degree accepted.
pub const MAX_DEGREE: usize = 25;
const MAXP1: usize = MAX_DEGREE + 1;

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

/// The `degree + 1` non-zero basis functions at `t` in span `span`, and their
/// first derivatives (Piegl and Tiller A2.2 with the derivative from the
/// degree `p - 1` functions).
fn basis1(
    knots: &[f64],
    span: usize,
    degree: usize,
    t: f64,
    n: &mut [f64; MAXP1],
    d: &mut [f64; MAXP1],
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
    /// `(w x, w y, w z, w)`, u-major.
    net: Vec<[f64; 4]>,
}

/// One iso-u row: the net contracted with the u basis (`q`) and its derivative
/// (`dq`), one homogeneous point per v pole.
#[derive(Debug, Default)]
pub struct SurfaceRow {
    q: Vec<[f64; 4]>,
    dq: Vec<[f64; 4]>,
}

impl PreparedSurface {
    pub fn new(s: &NurbsSurface) -> Self {
        let net = s
            .poles
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let w = s.weights.as_ref().map_or(1.0, |w| w[k]);
                [p.x * w, p.y * w, p.z * w, w]
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
        }
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
        basis1(&self.knots_u, span, p, u, &mut n, &mut d);
        row.q.clear();
        row.dq.clear();
        for j in 0..self.count_v {
            let mut q = [0.0; 4];
            let mut dq = [0.0; 4];
            for k in 0..=p {
                let pw = self.net[(span - p + k) * self.count_v + j];
                for c in 0..4 {
                    q[c] += n[k] * pw[c];
                    dq[c] += d[k] * pw[c];
                }
            }
            row.q.push(q);
            row.dq.push(dq);
        }
    }

    /// `(S, S_u, S_v)` at `v` (clamped) on `row`.
    pub fn eval(&self, row: &SurfaceRow, v: f64) -> (Vec3, Vec3, Vec3) {
        let [_, [lo, hi]] = self.domain();
        let v = clamp(v, lo, hi);
        let q = self.degree_v;
        let span = find_span(&self.knots_v, q, self.count_v, v);
        let mut n = [0.0; MAXP1];
        let mut d = [0.0; MAXP1];
        basis1(&self.knots_v, span, q, v, &mut n, &mut d);
        let mut a = [0.0; 4];
        let mut au = [0.0; 4];
        let mut av = [0.0; 4];
        for l in 0..=q {
            let k = span - q + l;
            let (pq, pdq) = (row.q[k], row.dq[k]);
            for c in 0..4 {
                a[c] += n[l] * pq[c];
                au[c] += n[l] * pdq[c];
                av[c] += d[l] * pq[c];
            }
        }
        rational(a, au, av)
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

    /// The point and first derivative at `t` (clamped to the knot domain).
    pub fn eval(&self, t: f64) -> ([f64; 2], [f64; 2]) {
        let count = self.poles.len();
        let t = clamp(t, self.knots[self.degree], self.knots[count]);
        let span = find_span(&self.knots, self.degree, count, t);
        let mut n = [0.0; MAXP1];
        let mut d = [0.0; MAXP1];
        basis1(&self.knots, span, self.degree, t, &mut n, &mut d);
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
            basis1(&knots, span, 3, t, &mut n, &mut d);
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
