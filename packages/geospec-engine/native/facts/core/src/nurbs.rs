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
//! The inner integrals of one face meet the same v nodes on every outer node
//! wherever they cover a whole v span (the same panels, so the same nodes,
//! bit for bit): the integrator takes the v basis through a [`BasisCache`],
//! a fixed-size table per face that reuses a node's basis while the node
//! stays in it. Up to degree 5 in v the evaluation runs with
//! arrays and loops fitted to the degree. Neither changes an operation or
//! its order, so every result keeps its bits.
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

/// The non-zero basis functions at `t` in `span`, their first derivatives
/// and the derivatives' magnitudes, as described above [`basis0`]. The
/// arrays hold `M >= degree + 1` values; every `M` computes the same bits
/// (the evaluator fits `M` to small degrees).
#[inline(always)]
fn basis1<const M: usize>(
    knots: &[f64],
    span: usize,
    degree: usize,
    t: f64,
    n: &mut [f64; M],
    d: &mut [f64; M],
    da: &mut [f64; M],
) {
    let mut left = [0.0; M];
    let mut right = [0.0; M];
    let mut lower = [0.0; M];
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

/// The distinct values of `knots`, ascending, each with whether a B-spline
/// of `degree` may be less than twice continuously differentiable across it
/// (its multiplicity at least `degree - 1`: C1 or C0 there, so the first
/// derivatives the integrand takes are at best continuous, not smooth). The
/// integrand stays C1 across the others.
pub fn knot_lines(knots: &[f64], degree: usize) -> Vec<(f64, bool)> {
    let mut out: Vec<(f64, bool)> = Vec::new();
    let mut run = 0usize;
    for (i, &k) in knots.iter().enumerate() {
        run += 1;
        if knots.get(i + 1) != Some(&k) {
            out.push((k, run + 1 >= degree));
            run = 0;
        }
    }
    out
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
    /// [`knot_lines`] of each direction.
    lines_u: Vec<(f64, bool)>,
    lines_v: Vec<(f64, bool)>,
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
            lines_u: knot_lines(&s.knots_u, s.degree_u),
            lines_v: knot_lines(&s.knots_v, s.degree_v),
        }
    }

    /// The largest ratio of weights among one span's poles.
    pub const fn weight_ratio(&self) -> f64 {
        self.weight_ratio
    }

    /// The degrees in u and v.
    pub const fn degrees(&self) -> [usize; 2] {
        [self.degree_u, self.degree_v]
    }

    /// `[[u_min, u_max], [v_min, v_max]]`.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        [
            [self.knots_u[self.degree_u], self.knots_u[self.count_u]],
            [self.knots_v[self.degree_v], self.knots_v[self.count_v]],
        ]
    }

    /// [`knot_lines`] in u.
    pub fn lines_u(&self) -> &[(f64, bool)] {
        &self.lines_u
    }

    /// [`knot_lines`] in v.
    pub fn lines_v(&self) -> &[(f64, bool)] {
        &self.lines_v
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
        let (first, bound, _) = self.eval_weighted(row, v);
        (first, bound)
    }

    /// [`Self::eval_bounded`] and the weight `w` at `v`.
    pub fn eval_weighted(&self, row: &SurfaceRow, v: f64) -> ((Vec3, Vec3, Vec3), EvalBound, f64) {
        self.eval_degree(row, v, None)
    }

    /// [`Self::eval_weighted`], with the v basis at `v` taken from `cache`
    /// when it holds `v` (the same bits as computing it: the basis is a
    /// function of `v` alone) and stored there otherwise. One cache serves
    /// one surface at a time.
    pub fn eval_weighted_cached<'s>(
        &'s self,
        row: &SurfaceRow,
        v: f64,
        cache: &mut BasisCache<'s>,
    ) -> ((Vec3, Vec3, Vec3), EvalBound, f64) {
        self.eval_degree(row, v, Some(cache))
    }

    /// [`Self::eval_at`] with its capacity fitted to `degree_v`: the small
    /// degrees get loops of a fixed length and arrays no longer than they
    /// need. The arithmetic and its order are the same at every capacity.
    fn eval_degree<'s>(
        &'s self,
        row: &SurfaceRow,
        v: f64,
        cache: Option<&mut BasisCache<'s>>,
    ) -> ((Vec3, Vec3, Vec3), EvalBound, f64) {
        match self.degree_v {
            1 => self.eval_at::<2>(row, v, cache),
            2 => self.eval_at::<3>(row, v, cache),
            3 => self.eval_at::<4>(row, v, cache),
            4 => self.eval_at::<5>(row, v, cache),
            5 => self.eval_at::<6>(row, v, cache),
            _ => self.eval_at::<MAXP1>(row, v, cache),
        }
    }

    /// The evaluation at `v` with basis arrays of `M`: below [`MAXP1`],
    /// `degree_v` is `M - 1`.
    #[inline(always)]
    fn eval_at<'s, const M: usize>(
        &'s self,
        row: &SurfaceRow,
        v: f64,
        cache: Option<&mut BasisCache<'s>>,
    ) -> ((Vec3, Vec3, Vec3), EvalBound, f64) {
        let q = if M < MAXP1 { M - 1 } else { self.degree_v };
        let mut n = [0.0; M];
        let mut d = [0.0; M];
        let mut da = [0.0; M];
        let Some(cache) = cache else {
            let span = self.basis_v(q, v, &mut n, &mut d, &mut da);
            return self.contract(q, row, span, &n, &d, &da);
        };
        let len = q + 1;
        if !cache.surface.is_some_and(|s| std::ptr::eq(s, self)) {
            cache.reset(self, len);
        }
        let key = v.to_bits();
        let slot = BasisCache::slot(key);
        let at = slot * 3 * len;
        let values = &mut cache.values[at..at + 3 * len];
        let span = if cache.spans[slot] != usize::MAX && cache.keys[slot] == key {
            n[..len].copy_from_slice(&values[..len]);
            d[..len].copy_from_slice(&values[len..2 * len]);
            da[..len].copy_from_slice(&values[2 * len..]);
            cache.spans[slot]
        } else {
            let span = self.basis_v(q, v, &mut n, &mut d, &mut da);
            values[..len].copy_from_slice(&n[..len]);
            values[len..2 * len].copy_from_slice(&d[..len]);
            values[2 * len..].copy_from_slice(&da[..len]);
            cache.keys[slot] = key;
            cache.spans[slot] = span;
            span
        };
        self.contract(q, row, span, &n, &d, &da)
    }

    /// The v span at `v` (clamped) and its basis functions, their
    /// derivatives and the derivatives' magnitudes ([`basis1`]), for
    /// `q = degree_v`.
    #[inline(always)]
    fn basis_v<const M: usize>(
        &self,
        q: usize,
        v: f64,
        n: &mut [f64; M],
        d: &mut [f64; M],
        da: &mut [f64; M],
    ) -> usize {
        let [_, [lo, hi]] = self.domain();
        let v = clamp(v, lo, hi);
        let span = find_span(&self.knots_v, q, self.count_v, v);
        basis1(&self.knots_v, span, q, v, n, d, da);
        span
    }

    /// The point, first derivatives, round-off bound and weight from `row`
    /// and the v basis of `span` (`q = degree_v`).
    #[inline(always)]
    fn contract<const M: usize>(
        &self,
        q: usize,
        row: &SurfaceRow,
        span: usize,
        n: &[f64; M],
        d: &[f64; M],
        da: &[f64; M],
    ) -> ((Vec3, Vec3, Vec3), EvalBound, f64) {
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
        ((p + self.origin, su, sv), bound, w)
    }
}

/// The log2 of [`BasisCache`]'s slot count.
const CACHE_BITS: u32 = 11;

/// The v basis functions of one surface at the v nodes met last, one per
/// slot (direct-mapped by the node's bits, a later node replacing an
/// earlier one in its slot). The inner integrals of a face's outer nodes
/// share their v nodes over every whole v span (the same panels, so the
/// same nodes, bit for bit), and [`PreparedSurface::eval_weighted_cached`]
/// reuses a node's basis while the node keeps its slot. A value from the cache has the bits a
/// fresh computation has, so facts do not depend on it.
#[derive(Debug, Default)]
pub struct BasisCache<'s> {
    /// The surface the slots hold (none before the first); another surface
    /// empties them.
    surface: Option<&'s PreparedSurface>,
    keys: Vec<u64>,
    /// The span per slot, `usize::MAX` where the slot is empty.
    spans: Vec<usize>,
    /// Per slot, `n`, `d` and `da` ([`basis1`]), `degree_v + 1` each.
    values: Vec<f64>,
}

impl<'s> BasisCache<'s> {
    /// Empties every slot, for `surface` with basis vectors of `len`.
    fn reset(&mut self, surface: &'s PreparedSurface, len: usize) {
        let slots = 1usize << CACHE_BITS;
        self.surface = Some(surface);
        self.keys.clear();
        self.keys.resize(slots, 0);
        self.spans.clear();
        self.spans.resize(slots, usize::MAX);
        self.values.clear();
        self.values.resize(slots * 3 * len, 0.0);
    }

    /// The slot of a node's bits.
    const fn slot(key: u64) -> usize {
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - CACHE_BITS)) as usize
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
    /// Per parameter, a bound on the rounding of the points
    /// [`Self::control_points_into`] and [`Self::hull`] compute (infinite
    /// when none is shown).
    rounding: [f64; 2],
    /// The box of the definition's own poles, without rounding: the curve
    /// lies in it.
    pole_box: [[f64; 2]; 2],
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
            .collect::<Vec<[f64; 3]>>();
        // Within a span the blossom's factors are a convex pair, so each
        // homogeneous coordinate rounds by at most `4 p EPS` of the largest
        // pole's, plus the products `w u`; the projection adds its own.
        let mut hmax = [0.0_f64; 3];
        let mut wmin = f64::INFINITY;
        let mut pole_box = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for (h, p) in poles.iter().zip(&c.poles) {
            for k in 0..3 {
                hmax[k] = hmax[k].max(h[k].abs());
            }
            wmin = wmin.min(h[2]);
            for (k, x) in p.iter().enumerate() {
                pole_box[0][k] = pole_box[0][k].min(*x);
                pole_box[1][k] = pole_box[1][k].max(*x);
            }
        }
        let f = 8.0 * (c.degree + 1) as f64 * f64::EPSILON;
        let ew = f * hmax[2];
        let mut rounding = [f64::INFINITY; 2];
        for k in 0..2 {
            let x = pole_box[0][k].abs().max(pole_box[1][k].abs());
            let r = (f * hmax[k] + x * ew) / (wmin - ew) + 2.0 * f64::EPSILON * x;
            if wmin - ew > 0.0 && r.is_finite() {
                rounding[k] = r;
            }
        }
        Self {
            degree: c.degree,
            knots: c.knots.clone(),
            poles,
            rounding,
            pole_box,
        }
    }

    /// Per parameter, a bound on how far the points
    /// [`Self::control_points_into`] and [`Self::hull`] compute lie from
    /// their exact values (infinite when none is shown).
    pub const fn rounding(&self) -> [f64; 2] {
        self.rounding
    }

    /// The box of the definition's poles, exact: the curve lies in it.
    pub const fn pole_box(&self) -> [[f64; 2]; 2] {
        self.pole_box
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

/// The cosine of the largest angle between a cell's coefficients and their
/// axis ([`in_cone`]): 60 degrees, as the facts integrator's hull of one
/// variable.
const CELL_COS: f64 = 0.5;
/// The cosine of the largest angle between a transversal cell's v
/// derivative coefficients and their axis ([`NormalNet::transversal`]):
/// about 41 degrees, so the derivative's part across the axis stays below
/// its part along it.
const DERIVATIVE_COS: f64 = 0.75;

/// The Bézier points over `[a, b]` (within one knot span, `a < b`) of the
/// homogeneous curve whose `degree + 1` de Boor points for `span` are `pts`:
/// the blossom with `a` at the first levels and `b` at the rest.
fn bezier_points(
    knots: &[f64],
    degree: usize,
    span: usize,
    a: f64,
    b: f64,
    pts: &[[f64; 4]],
) -> Vec<[f64; 4]> {
    let p = degree;
    let mut out = Vec::with_capacity(p + 1);
    for k in 0..=p {
        let mut d: Vec<[f64; 4]> = pts.to_vec();
        for r in 1..=p {
            let t = if r <= p - k { a } else { b };
            for j in (r..=p).rev() {
                let i = span - p + j;
                let den = knots[i + p + 1 - r] - knots[i];
                let alpha = if den > 0.0 { (t - knots[i]) / den } else { 0.0 };
                for c in 0..4 {
                    d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
                }
            }
        }
        out.push(d[p]);
    }
    out
}

fn binomial(n: usize, k: usize) -> f64 {
    let mut x = 1.0;
    for i in 0..k {
        x = x * (n - i) as f64 / (i + 1) as f64;
    }
    x
}

/// A value of `D` coordinates with a bound on each coordinate's round-off.
#[derive(Clone, Copy, Debug)]
struct Bounded<const D: usize> {
    x: [f64; D],
    e: [f64; D],
}

impl<const D: usize> Default for Bounded<D> {
    fn default() -> Self {
        Self {
            x: [0.0; D],
            e: [0.0; D],
        }
    }
}

/// A tensor-product Bernstein polynomial over one cell: `(du + 1) (dv + 1)`
/// coefficients, u-major.
#[derive(Clone, Debug)]
struct Grid<T> {
    du: usize,
    dv: usize,
    c: Vec<T>,
}

impl<T: Copy> Grid<T> {
    fn at(&self, i: usize, j: usize) -> T {
        self.c[i * (self.dv + 1) + j]
    }
}

/// `a × b` with its round-off bound per coordinate.
fn cross(a: Bounded<4>, b: Bounded<4>) -> Bounded<3> {
    let mut out = Bounded::default();
    for (k, (i, j)) in [(1, 2), (2, 0), (0, 1)].into_iter().enumerate() {
        let (p, q) = (a.x[i] * b.x[j], a.x[j] * b.x[i]);
        out.x[k] = p - q;
        let carried =
            |s: usize, t: usize| a.x[s].abs() * b.e[t] + a.e[s] * b.x[t].abs() + a.e[s] * b.e[t];
        out.e[k] = carried(i, j) + carried(j, i) + 2.0 * f64::EPSILON * (p.abs() + q.abs());
    }
    out
}

/// `s a` (the weight `s.x[3]` times the vector `a`) with its round-off bound.
fn scaled(s: Bounded<4>, a: Bounded<3>) -> Bounded<3> {
    let mut out = Bounded::default();
    let (w, ew) = (s.x[3], s.e[3]);
    for k in 0..3 {
        out.x[k] = w * a.x[k];
        out.e[k] =
            w.abs() * a.e[k] + ew * a.x[k].abs() + ew * a.e[k] + f64::EPSILON * out.x[k].abs();
    }
    out
}

/// The Bernstein coefficients of the product of two tensor polynomials,
/// each pair of coefficients combined by `f`, with round-off bounds: the
/// pairs' (the weights sum to one) and the sums' own.
fn product<A: Copy, B: Copy>(
    a: &Grid<A>,
    b: &Grid<B>,
    f: impl Fn(A, B) -> Bounded<3>,
) -> Grid<Bounded<3>> {
    let (du, dv) = (a.du + b.du, a.dv + b.dv);
    let mut c = vec![Bounded::<3>::default(); (du + 1) * (dv + 1)];
    let mut size = vec![[0.0_f64; 3]; c.len()];
    let bu: Vec<Vec<f64>> = (0..=a.du)
        .map(|i| {
            (0..=b.du)
                .map(|k| binomial(a.du, i) * binomial(b.du, k) / binomial(du, i + k))
                .collect()
        })
        .collect();
    let bv: Vec<Vec<f64>> = (0..=a.dv)
        .map(|j| {
            (0..=b.dv)
                .map(|l| binomial(a.dv, j) * binomial(b.dv, l) / binomial(dv, j + l))
                .collect()
        })
        .collect();
    for (i, bui) in bu.iter().enumerate() {
        for (k, buk) in bui.iter().enumerate() {
            for (j, bvj) in bv.iter().enumerate() {
                for (l, bvl) in bvj.iter().enumerate() {
                    let w = buk * bvl;
                    let x = f(a.at(i, j), b.at(k, l));
                    let at = (i + k) * (dv + 1) + j + l;
                    for m in 0..3 {
                        let t = w * x.x[m];
                        c[at].x[m] += t;
                        c[at].e[m] += w * x.e[m];
                        size[at][m] += t.abs();
                    }
                }
            }
        }
    }
    // Each sum of at most `(a.du + 1) (a.dv + 1)` terms, each term's weight
    // computed in a few operations.
    let n = ((a.du + 1) * (a.dv + 1)) as f64 + 8.0;
    for (o, s) in c.iter_mut().zip(&size) {
        for (e, z) in o.e.iter_mut().zip(s) {
            *e += 2.0 * n * f64::EPSILON * z;
        }
    }
    Grid { du, dv, c }
}

/// The coefficients over `[s0, s1]` (within `[0, 1]`) of the polynomial
/// with coefficients `c`, by de Casteljau's algorithm. Each new coefficient
/// is a convex combination of the old, so the same combination of their
/// bounds and magnitudes bounds its carried error and its size.
fn sub_range<const D: usize>(c: &mut [[f64; D]], s0: f64, s1: f64) {
    let d = c.len().saturating_sub(1);
    // The part over [0, s1]: the first point of each level.
    if s1 < 1.0 {
        for k in 1..=d {
            for j in (k..=d).rev() {
                for x in 0..D {
                    c[j][x] = (1.0 - s1) * c[j - 1][x] + s1 * c[j][x];
                }
            }
        }
    }
    // Of that, the part over [s0 / s1, 1]: the last point of each level.
    if s0 > 0.0 {
        let t = s0 / s1;
        for k in 1..=d {
            for j in 0..=d - k {
                for x in 0..D {
                    c[j][x] = (1.0 - t) * c[j][x] + t * c[j + 1][x];
                }
            }
        }
    }
}

fn norm(c: [f64; 3]) -> f64 {
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

/// Whether the vectors `cs`, each within its radius `rs` of its exact
/// value, lie in one convex cone of half-angle `acos(cos)` about the sum of
/// their unit vectors, and that axis (zero where every vector is within its
/// radius of zero). With `exempt`, a vector within its radius of zero
/// (round-off of a normal that vanishes) is not held against the cone, and
/// if all are, they pass.
fn in_cone(cs: &[[f64; 3]], rs: &[f64], cos: f64, exempt: bool) -> Option<[f64; 3]> {
    let mut axis = [0.0; 3];
    let mut small = true;
    for (c, &r) in cs.iter().zip(rs) {
        let l = norm(*c);
        if !(l.is_finite() && r.is_finite()) {
            return None;
        }
        if l > r {
            small = false;
            for (a, x) in axis.iter_mut().zip(c) {
                *a += x / l;
            }
        }
    }
    let al = norm(axis);
    if al == 0.0 {
        return (exempt && small).then_some(axis);
    }
    let e = [axis[0] / al, axis[1] / al, axis[2] / al];
    cs.iter()
        .zip(rs)
        .all(|(c, &r)| {
            let l = norm(*c);
            let along = c[0] * e[0] + c[1] * e[1] + c[2] * e[2];
            (exempt && l <= r) || along - r >= cos * (l + r)
        })
        .then_some(e)
}

/// The Bernstein coefficients, over one knot span's cell of a NURBS
/// surface, of a vector polynomial along its natural normal `n = S_u x S_v`
/// (in the span's own parameters, a positive multiple of `n`): with the
/// homogeneous net `A` and its weight `w`, `w A_u x A_v - w_v A_u x A -
/// w_u A x A_v` (`w^3 n`, degree `3 p - 1` by `3 q - 1`), or `A_u x A_v`
/// where the weight is constant (degree `2 p - 1` by `2 q - 1`), each with
/// a bound on its round-off. Built once per cell from the net;
/// [`Self::cone_over`] and [`Self::strips_hold`] take its sub-cells by de
/// Casteljau's algorithm.
#[derive(Debug)]
pub struct NormalNet {
    /// Per coefficient: the vector, its round-off radius and its size (the
    /// 1-norm before cancellation), for the subdivisions' own round-off.
    h: Grid<[f64; 5]>,
    /// Whether the whole cell passes [`Self::cone_over`]: then so does every
    /// part of it.
    pub full: bool,
}

/// A sub-cell's coefficients and their round-off radii.
struct Part {
    du: usize,
    dv: usize,
    c: Vec<[f64; 3]>,
    r: Vec<f64>,
}

impl NormalNet {
    /// The coefficients over the part `[s0, s1] x [t0, t1]` of the cell (in
    /// its own parameters on `[0, 1]`).
    fn sub(&self, s: [f64; 2], t: [f64; 2]) -> Part {
        let mut g = self.h.clone();
        let (du, dv) = (g.du, g.dv);
        if s != [0.0, 1.0] {
            let mut col = vec![[0.0; 5]; du + 1];
            for j in 0..=dv {
                for (i, x) in col.iter_mut().enumerate() {
                    *x = g.c[i * (dv + 1) + j];
                }
                sub_range(&mut col, s[0], s[1]);
                for (i, x) in col.iter().enumerate() {
                    g.c[i * (dv + 1) + j] = *x;
                }
            }
        }
        if t != [0.0, 1.0] {
            for i in 0..=du {
                sub_range(&mut g.c[i * (dv + 1)..(i + 1) * (dv + 1)], t[0], t[1]);
            }
        }
        // Four de Casteljau passes, each of at most `du` or `dv` levels of
        // one product and one sum per coordinate.
        let levels = (4 * (du + dv) + 4) as f64;
        Part {
            du,
            dv,
            c: g.c.iter().map(|x| [x[0], x[1], x[2]]).collect(),
            r: g.c
                .iter()
                .map(|x| (x[3] + 3.0 * levels * f64::EPSILON * x[4]) * (1.0 + 4.0 * f64::EPSILON))
                .collect(),
        }
    }

    /// Whether the normal lies in one cone of 60 degrees' half-angle over
    /// the part `[s0, s1] x [t0, t1]` of the cell ([`in_cone`]): it then
    /// does not vanish there beyond round-off, and `|n|` is analytic.
    pub fn cone_over(&self, s: [f64; 2], t: [f64; 2]) -> bool {
        let g = self.sub(s, t);
        in_cone(&g.c, &g.r, CELL_COS, true).is_some()
    }

    /// Whether the part is transversal: along one axis `e` the normal's
    /// component is strictly monotone in v (its v derivative's coefficients
    /// all within [`DERIVATIVE_COS`]'s cone about `e`), and its part across
    /// `e` lies in one cone of 60 degrees. A softened kink of `|n|` there
    /// crosses every v line once, so the zeros of `n . n` in v stay apart
    /// as u varies: they do not pinch the inner integral's path.
    fn transversal(g: &Part) -> bool {
        let (du, dv) = (g.du, g.dv);
        if dv == 0 {
            return false;
        }
        let mut d = Vec::with_capacity((du + 1) * dv);
        let mut rd = Vec::with_capacity((du + 1) * dv);
        for i in 0..=du {
            for j in 0..dv {
                let (k0, k1) = (i * (dv + 1) + j, i * (dv + 1) + j + 1);
                let (a, b) = (g.c[k0], g.c[k1]);
                let x = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                rd.push(
                    (g.r[k0] + g.r[k1]) * (1.0 + 4.0 * f64::EPSILON) + 2.0 * f64::EPSILON * norm(x),
                );
                d.push(x);
            }
        }
        let Some(e) = in_cone(&d, &rd, DERIVATIVE_COS, false) else {
            return false;
        };
        let across: Vec<[f64; 3]> =
            g.c.iter()
                .map(|c| {
                    let k = c[0] * e[0] + c[1] * e[1] + c[2] * e[2];
                    [c[0] - k * e[0], c[1] - k * e[1], c[2] - k * e[2]]
                })
                .collect();
        // The projection does not lengthen an error; its own rounding is a
        // few units of the vector's size (the axis is a unit vector to
        // within a few units too).
        let ra: Vec<f64> =
            g.c.iter()
                .zip(&g.r)
                .map(|(c, &r)| 2.0 * r + 16.0 * f64::EPSILON * norm(*c))
                .collect();
        in_cone(&across, &ra, CELL_COS, true).is_some()
    }

    /// Whether every v strip of the part `[s0, s1] x [t0, t1]`, bisected in
    /// v at most `depth` times, passes [`Self::cone_over`] or is
    /// transversal ([`Self::transversal`]).
    pub fn strips_hold(&self, s: [f64; 2], t: [f64; 2], depth: u32) -> bool {
        let g = self.sub(s, t);
        if in_cone(&g.c, &g.r, CELL_COS, true).is_some() || Self::transversal(&g) {
            return true;
        }
        if depth == 0 {
            return false;
        }
        let m = 0.5 * (t[0] + t[1]);
        if !(t[0] < m && m < t[1]) {
            return false;
        }
        self.strips_hold(s, [t[0], m], depth - 1) && self.strips_hold(s, [m, t[1]], depth - 1)
    }
}

impl PreparedSurface {
    /// The [`NormalNet`] of the cell `[u0, u1] x [v0, v1]` (one knot span
    /// each way, `u0 < u1`, `v0 < v1`); `None` where a value is not finite.
    pub fn normal_net(&self, [u0, u1]: [f64; 2], [v0, v1]: [f64; 2]) -> Option<NormalNet> {
        let (p, q) = (self.degree_u, self.degree_v);
        let su = find_span(&self.knots_u, p, self.count_u, 0.5 * (u0 + u1));
        let sv = find_span(&self.knots_v, q, self.count_v, 0.5 * (v0 + v1));
        // The cell's homogeneous Bézier net, v first.
        let mut rows = Vec::with_capacity(p + 1);
        for k in 0..=p {
            let base = (su - p + k) * self.count_v + sv - q;
            rows.push(bezier_points(
                &self.knots_v,
                q,
                sv,
                v0,
                v1,
                &self.net[base..=base + q],
            ));
        }
        let mut b = vec![[0.0; 4]; (p + 1) * (q + 1)];
        let mut col = Vec::with_capacity(p + 1);
        for l in 0..=q {
            col.clear();
            col.extend(rows.iter().map(|r| r[l]));
            for (k, x) in bezier_points(&self.knots_u, p, su, u0, u1, &col)
                .into_iter()
                .enumerate()
            {
                b[k * (q + 1) + l] = x;
            }
        }
        // The extraction's affine combinations (convex within the span)
        // round each coordinate by at most a few units per level of its
        // largest value over the span's poles; a difference doubles that.
        let mut big = [0.0_f64; 4];
        for x in &b {
            for (m, y) in big.iter_mut().zip(x) {
                *m = m.max(y.abs());
            }
        }
        let f = 8.0 * (p + q + 2) as f64 * f64::EPSILON;
        let ea = big.map(|m| f * m);
        let at = |i: usize, j: usize| Bounded {
            x: b[i * (q + 1) + j],
            e: ea,
        };
        let diff = |x: Bounded<4>, y: Bounded<4>| {
            let mut o = Bounded::<4>::default();
            for k in 0..4 {
                o.x[k] = y.x[k] - x.x[k];
                o.e[k] = x.e[k] + y.e[k] + f64::EPSILON * o.x[k].abs();
            }
            o
        };
        let grid = |du: usize, dv: usize, f: &dyn Fn(usize, usize) -> Bounded<4>| Grid {
            du,
            dv,
            c: (0..=du)
                .flat_map(|i| (0..=dv).map(move |j| (i, j)))
                .map(|(i, j)| f(i, j))
                .collect::<Vec<_>>(),
        };
        let a = grid(p, q, &at);
        let au = grid(p - 1, q, &|i, j| diff(at(i, j), at(i + 1, j)));
        let av = grid(p, q - 1, &|i, j| diff(at(i, j), at(i, j + 1)));
        let xuv = product(&au, &av, cross);
        let h = if self.weight_ratio == 1.0 {
            xuv
        } else {
            let t1 = product(&a, &xuv, scaled);
            let t2 = product(&av, &product(&au, &a, cross), scaled);
            let t3 = product(&au, &product(&a, &av, cross), scaled);
            let mut h = t1;
            for ((o, y), z) in h.c.iter_mut().zip(&t2.c).zip(&t3.c) {
                for k in 0..3 {
                    let s = o.x[k] - y.x[k];
                    let x = s - z.x[k];
                    o.e[k] = o.e[k] + y.e[k] + z.e[k] + f64::EPSILON * (s.abs() + x.abs());
                    o.x[k] = x;
                }
            }
            h
        };
        let mut c = Vec::with_capacity(h.c.len());
        for x in &h.c {
            // Radius: the 1-norm of the coordinates' bounds; size: the
            // 1-norm of the vector and its bound (a bound on the sizes the
            // subdivisions combine).
            let r = x.e.iter().sum::<f64>();
            let size = x.x.iter().map(|y| y.abs()).sum::<f64>() + r;
            if !(r.is_finite() && size.is_finite()) {
                return None;
            }
            c.push([x.x[0], x.x[1], x.x[2], r, size]);
        }
        let mut net = NormalNet {
            h: Grid {
                du: h.du,
                dv: h.dv,
                c,
            },
            full: false,
        };
        net.full = net.cone_over([0.0, 1.0], [0.0, 1.0]);
        Some(net)
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

    /// The value at `(s, t)` of a Bernstein grid.
    fn value(g: &Part, s: f64, t: f64) -> [f64; 3] {
        let mut out = [0.0; 3];
        for i in 0..=g.du {
            let bu =
                binomial(g.du, i) * libm::pow(s, i as f64) * libm::pow(1.0 - s, (g.du - i) as f64);
            for j in 0..=g.dv {
                let bv = binomial(g.dv, j)
                    * libm::pow(t, j as f64)
                    * libm::pow(1.0 - t, (g.dv - j) as f64);
                for (o, x) in out.iter_mut().zip(g.c[i * (g.dv + 1) + j]) {
                    *o += bu * bv * x;
                }
            }
        }
        out
    }

    /// On every knot-span cell, polynomial and rational, the normal net's
    /// polynomial and its sub-cells are positive multiples of the
    /// evaluator's `S_u x S_v` (inside the cell: the test nets have C0
    /// and, at degree 1, discontinuous knots).
    #[test]
    fn normal_nets_follow_the_normal() {
        for (p, q) in [(1, 1), (2, 3), (3, 3), (3, 1), (5, 2)] {
            for rational in [false, true] {
                let s = surface(p, q, rational);
                let us: Vec<f64> = s.lines_u().iter().map(|l| l.0).collect();
                let vs: Vec<f64> = s.lines_v().iter().map(|l| l.0).collect();
                let mut row = SurfaceRow::default();
                for u in us.windows(2) {
                    for v in vs.windows(2) {
                        let net = s.normal_net([u[0], u[1]], [v[0], v[1]]).unwrap();
                        for (a, b) in [(0.01, 0.02), (0.3, 0.7), (0.99, 0.45), (0.9, 0.98)] {
                            let (x, y) = (u[0] + a * (u[1] - u[0]), v[0] + b * (v[1] - v[0]));
                            s.row(x, &mut row);
                            let (_, su, sv) = s.eval(&row, y);
                            let n = su.cross(sv);
                            let n = [n.x, n.y, n.z];
                            for (h, (ss, tt)) in [
                                (value(&net.sub([0.0, 1.0], [0.0, 1.0]), a, b), (a, b)),
                                (
                                    value(
                                        &net.sub([0.2, 0.9], [0.1, 0.8]),
                                        (a - 0.2) / 0.7,
                                        (b - 0.1) / 0.7,
                                    ),
                                    (a, b),
                                ),
                            ] {
                                if (ss, tt) != (a, b) {
                                    continue;
                                }
                                let dot = h[0] * n[0] + h[1] * n[1] + h[2] * n[2];
                                let cos = dot / (norm(h) * norm(n));
                                assert!(
                                    cos > 1.0 - 1e-9,
                                    "{p} {q} {rational} ({x}, {y}): {h:?} {n:?}"
                                );
                            }
                        }
                    }
                }
            }
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

    /// A clamped knot vector of `count` poles and `degree`, with uneven
    /// interior knots and one double knot.
    fn knots(count: usize, degree: usize, lo: f64, hi: f64) -> Vec<f64> {
        let inner = count - degree - 1;
        let mut k = vec![lo; degree + 1];
        for i in 1..=inner {
            let t = i as f64 / (inner + 1) as f64;
            k.push(lo + (hi - lo) * t * t);
        }
        if inner >= 2 {
            k[degree + 2] = k[degree + 1];
        }
        k.extend(std::iter::repeat_n(hi, degree + 1));
        k
    }

    fn surface(p: usize, q: usize, rational: bool) -> PreparedSurface {
        let (cu, cv) = (p + 4, q + 5);
        let mut poles = Vec::new();
        let mut weights = Vec::new();
        for i in 0..cu {
            for j in 0..cv {
                let (x, y) = (i as f64, j as f64);
                poles.push(Vec3::new(
                    1e3 + x,
                    -7.0 + 1.3 * y,
                    0.1 * x * y - libm::sin(x + 2.0 * y),
                ));
                weights.push(1.0 + 0.4 * libm::cos(3.0 * x + y).abs());
            }
        }
        PreparedSurface::new(&NurbsSurface {
            degree_u: p,
            degree_v: q,
            knots_u: knots(cu, p, -1.0, 2.0),
            knots_v: knots(cv, q, 0.25, 4.0),
            poles,
            weights: rational.then_some(weights),
            count_u: cu,
            count_v: cv,
        })
    }

    fn bits(e: ((Vec3, Vec3, Vec3), EvalBound, f64)) -> Vec<u64> {
        let ((a, b, c), bound, w) = e;
        [a, b, c]
            .iter()
            .flat_map(|x| [x.x, x.y, x.z])
            .chain(bound)
            .chain([w])
            .map(|x| {
                if x.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    x.to_bits()
                }
            })
            .collect()
    }

    #[test]
    fn the_cached_and_fitted_evaluations_have_the_bits_of_the_general_one() {
        for q in 1..=8 {
            for (p, rational) in [(1, false), (3, true), (2, false), (5, true)] {
                let a = surface(p, q, rational);
                let b = surface(p + 1, q, !rational);
                let mut cache = BasisCache::default();
                let mut row = SurfaceRow::default();
                // Repeated nodes on several rows, both surfaces through one
                // cache, the domain's ends and points outside it (clamped).
                let mut vs: Vec<f64> = (0..300)
                    .map(|i| -0.5 + 5.0 * f64::from(i % 97) / 96.0)
                    .collect();
                vs.extend([0.25, 4.0, a.knots_v[q + 2], a.knots_v[q + 3], f64::NAN]);
                for (k, s) in [&a, &a, &b, &a].into_iter().enumerate() {
                    let mut other = BasisCache::default();
                    for &u in &[-1.0, 0.3 + 0.1 * k as f64, 2.0] {
                        s.row(u, &mut row);
                        for &v in &vs {
                            let general = bits(s.eval_at::<MAXP1>(&row, v, None));
                            assert_eq!(bits(s.eval_weighted(&row, v)), general, "{p} {q} {v}");
                            assert_eq!(bits(s.eval_weighted_cached(&row, v, &mut cache)), general);
                            assert_eq!(bits(s.eval_weighted_cached(&row, v, &mut other)), general);
                        }
                    }
                }
            }
        }
    }
}
