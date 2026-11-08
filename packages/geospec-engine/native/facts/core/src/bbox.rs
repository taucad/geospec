//! A body's box with a bracket (program item P5).
//!
//! For each of the box's six sides the result holds two numbers: the `inner`
//! value, which a point of the body attains (so the true side lies at or
//! beyond it), and the `outer` value, which no point of the body exceeds. The
//! true box lies between the inner and the outer box; `bound`, the largest
//! difference over the six sides, is the bracket's width. Both numbers are
//! rigorous for the definition's geometry (the surfaces over their trims),
//! with every rounding error carried into them.
//!
//! Per face, the extreme of a coordinate over the face lies on the face's
//! boundary or at an interior point where the coordinate's gradient in the
//! parameter plane vanishes:
//!
//! - **Boundary.** Each trim piece (and each closing chord between pieces
//!   that do not meet) is bisected in its parameter: an interval's enclosure
//!   is the surface's enclosure over the box of the piece's control points
//!   there (on a plane or a flat bilinear net, the affine image of those
//!   control points), and its end points are attained. Where the surface's
//!   and the trim's first and second derivatives are bounded (analytic kinds;
//!   a net within its domain and away from C0 knot lines), an interval is
//!   also bounded by its end values plus `k h^2 / 8`, `k` bounding the
//!   coordinate's second derivative along the piece: near an extreme this
//!   converges quadratically. A straight piece on an affine surface has its
//!   extremes at its ends. An iso-parametric line on an analytic surface is
//!   a segment or a circular arc whose extremes are in closed form.
//! - **Interior, analytic kinds.** On planes, cylinders and cones a
//!   coordinate is affine in v, and on a bilinear net (degree 1 by 1, equal
//!   weights) harmonic, so its extremes lie on the boundary. On
//!   spheres and tori the critical points are in closed form; each is
//!   classified against the trims (every periodic image in the trims' box)
//!   and counts when it is inside (attained) or cannot be shown outside
//!   (outer only).
//! - **Interior, B-spline nets.** The trims' box is split at the knots and
//!   bisected; a sub-patch's Bezier net (from the blossom) encloses it. A cell
//!   is dropped when it lies outside the trims, when its enclosure cannot
//!   beat the side's attained value by more than `epsilon`, or when the
//!   coordinate is monotone over it in u or in v (the rational hodograph's
//!   interval excludes zero) and the edge it increases towards is not a knot
//!   line of C0 continuity: there it has no interior extreme. A cell inside
//!   the trims makes its corners (where the net interpolates the surface)
//!   attained.
//!
//! **Classification** against the trims is exact or undecided, never wrong: a
//! query box that the trims' control-point boxes (refined by bisection near
//! it) do not touch takes the parity of an upward ray against the chords of
//! those boxes, which are homotopic to the trims away from the query. It
//! relies on the loops' structure, so it runs only on faces whose loops pass
//! the facts layer's own loop check ([`FaceDefinition::check_loops`]). A face
//! whose loops do not pass is bounded over the box of its trims in the
//! parameter plane instead (the UV-rectangle hull, a rigorous over-bound of
//! the face; nothing inside it counts as attained), and the body reports it in
//! `fallback_faces`: its bracket is wider.
//!
//! The body's sides fold the faces' in face order. A first pass evaluates the
//! trims' end points of every face; the second refines each face only until
//! its outer value is within `epsilon` of the larger of its own attained value
//! and the first pass's, so faces away from a side cost little. Both passes
//! are per-face tasks whose results do not depend on the order or the thread
//! that runs them, so the box is bit-identical for every thread count. A face
//! whose refinement reaches its work cap keeps the outer values it has (still
//! rigorous) and is reported in `capped_faces`.
//!
//! A trim point outside a bounded domain (a net's knots, a sphere's
//! latitudes) by up to a relative 1e-6 is evaluated at the nearest point of
//! the domain, and the face's inner and outer values widen by twice the
//! surface's derivative bound times the trims' excursion; the body reports
//! it in `extrapolated_faces`. Beyond that it refuses
//! ([`RefusalCode::TrimOutsideDomain`]): the definition does not say where
//! such a face lies. The box encloses the definition's
//! geometry; a kernel whose edges lie off their trims by up to their
//! tolerance needs that tolerance added to the outer box.
//!
//! Preconditions, established by `validate`: degrees within the crate's
//! limits, knot vectors that fit their nets, positive weights. Indexing below
//! relies on them.
// Coordinate loops index several arrays by axis.
#![allow(clippy::indexing_slicing, clippy::needless_range_loop)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::definition::{
    BodyDefinition, Curve2, FaceDefinition, Frame, NurbsSurface, Surface, TrimPiece,
};
use crate::loops;
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::PreparedTrim;
use crate::vec::Vec3;

const EPS: f64 = f64::EPSILON;
/// How far, relative to the domain's width, a trim may leave its surface's
/// parameter domain before the face refuses. Within it the surface is
/// evaluated clamped (the definition's semantics) and both ends of the
/// bracket move by a bound on the surface's derivative times the excursion,
/// so the bracket also holds a kernel that extrapolates the net instead.
const DOMAIN_TOLERANCE: f64 = 1e-6;
/// Bisections of one trim interval.
const MAX_T_DEPTH: u32 = 64;
/// Bisections of one parameter-plane cell.
const MAX_CELL_DEPTH: u32 = 56;
/// Bisections of one trim segment while classifying.
const MAX_CLASSIFY_DEPTH: u32 = 52;
/// Enclosures per face before its refinement stops.
const WORK_CAP: usize = 250_000;
/// A cell over at most this many knot spans is split at its knots.
const SPLIT_SPANS: usize = 16;
/// Query boxes are widened by this relative distance before the trims'
/// boxes are tested against them.
const CLASSIFY_TOLERANCE: f64 = 1e-12;
/// Samples along a line of critical points.
const LINE_SAMPLES: usize = 16;

/// The six sides in the order `-x, -y, -z, +x, +y, +z`; side `s` is the
/// largest value of `-coordinate` (`s < 3`) or `coordinate` (`s >= 3`).
pub const SIDES: usize = 6;

/// Box settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxOptions {
    /// The bracket's target width relative to the body's size: each side is
    /// refined until its width is under `accuracy` times the size.
    pub accuracy: f64,
    /// Threads for the per-face tasks (1 runs on the caller's thread).
    pub threads: usize,
}

impl Default for BoxOptions {
    fn default() -> Self {
        Self {
            accuracy: 1e-7,
            threads: 1,
        }
    }
}

/// One face's or body's sides: each as the largest value of `-x, -y, -z, x,
/// y, z` that a point attains (`inner`) and that no point exceeds (`outer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides {
    /// Values attained by points of the face or body.
    pub inner: [f64; SIDES],
    /// Values no point of the face or body exceeds.
    pub outer: [f64; SIDES],
}

impl Sides {
    const EMPTY: Self = Self {
        inner: [f64::NEG_INFINITY; SIDES],
        outer: [f64::NEG_INFINITY; SIDES],
    };

    /// The box spanned by the attained values: inside the true box.
    #[must_use]
    pub fn inner_box(&self) -> [Vec3; 2] {
        to_box(&self.inner)
    }

    /// The box spanned by the outer values: around the true box.
    #[must_use]
    pub fn outer_box(&self) -> [Vec3; 2] {
        to_box(&self.outer)
    }

    /// The largest width `outer - inner` over the sides.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.inner
            .iter()
            .zip(&self.outer)
            .fold(0.0_f64, |m, (i, o)| m.max(o - i))
    }
}

fn to_box(s: &[f64; SIDES]) -> [Vec3; 2] {
    [Vec3::new(-s[0], -s[1], -s[2]), Vec3::new(s[3], s[4], s[5])]
}

/// A face's bracket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    /// The sides.
    pub sides: Sides,
    /// Bounded over its trims' parameter box: its loops did not pass the
    /// loop check, so nothing inside the trims was classified.
    pub fallback: bool,
    /// Its refinement reached the work cap.
    pub capped: bool,
    /// Its trims leave the surface's domain (within the tolerance): both
    /// ends of every side moved by `excursion`.
    pub extrapolated: bool,
    /// How far each side's ends moved for trims outside the domain.
    pub excursion: f64,
    /// Enclosures computed.
    pub work: usize,
}

/// A body's box with its bracket.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyBox {
    /// The sides, folded over the faces in face order.
    pub sides: Sides,
    /// The bracket's width: the largest `outer - inner` over the sides.
    pub bound: f64,
    /// The target width (`accuracy` times the body's size).
    pub epsilon: f64,
    /// Faces bounded over their trims' parameter box (loops not classified).
    pub fallback_faces: Vec<usize>,
    /// Faces whose refinement reached the work cap.
    pub capped_faces: Vec<usize>,
    /// Faces whose trims leave their surface's domain (within the tolerance).
    pub extrapolated_faces: Vec<usize>,
    /// Enclosures computed over every face.
    pub work: usize,
}

impl BodyBox {
    /// The box around the body: `[min, max]` of the outer values.
    #[must_use]
    pub fn outer(&self) -> [Vec3; 2] {
        self.sides.outer_box()
    }

    /// The box the body's attained points span: `[min, max]` of the inner
    /// values.
    #[must_use]
    pub fn inner(&self) -> [Vec3; 2] {
        self.sides.inner_box()
    }
}

fn coord(p: Vec3, c: usize) -> f64 {
    match c {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

fn side_value(p: Vec3, s: usize) -> f64 {
    let c = coord(p, s % 3);
    if s < 3 {
        -c
    } else {
        c
    }
}

/// An enclosure of a set of points: per axis `[lo, hi]`, rounding included,
/// and `m`, the rounding margin included in them.
#[derive(Clone, Copy, Debug)]
struct Enc {
    lo: [f64; 3],
    hi: [f64; 3],
    m: [f64; 3],
}

impl Enc {
    const EMPTY: Self = Self {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
        m: [0.0; 3],
    };

    fn upper(&self, s: usize) -> f64 {
        if s < 3 {
            -self.lo[s]
        } else {
            self.hi[s - 3]
        }
    }

    fn add_point(&mut self, p: [f64; 3]) {
        for c in 0..3 {
            self.lo[c] = self.lo[c].min(p[c]);
            self.hi[c] = self.hi[c].max(p[c]);
        }
    }

    fn widen(&mut self, m: [f64; 3]) {
        for c in 0..3 {
            self.lo[c] -= m[c];
            self.hi[c] += m[c];
            self.m[c] = self.m[c].max(m[c]);
        }
    }

    fn union(&mut self, o: &Self) {
        for c in 0..3 {
            self.lo[c] = self.lo[c].min(o.lo[c]);
            self.hi[c] = self.hi[c].max(o.hi[c]);
            self.m[c] = self.m[c].max(o.m[c]);
        }
    }

    fn unbounded() -> Self {
        Self {
            lo: [f64::NEG_INFINITY; 3],
            hi: [f64::INFINITY; 3],
            m: [0.0; 3],
        }
    }
}

/// The range of `a cos t + b sin t` over `[t0, t1]`, without rounding margin.
fn range_cs(a: f64, b: f64, t0: f64, t1: f64) -> [f64; 2] {
    let rho = libm::hypot(a, b);
    if t1 - t0 >= TAU || t1.is_nan() || t0.is_nan() {
        return [-rho, rho];
    }
    let e0 = a * libm::cos(t0) + b * libm::sin(t0);
    let e1 = a * libm::cos(t1) + b * libm::sin(t1);
    let (mut lo, mut hi) = (e0.min(e1), e0.max(e1));
    if rho > 0.0 {
        let phi = libm::atan2(b, a);
        let within = |x: f64| x + libm::ceil((t0 - x) / TAU) * TAU <= t1;
        if within(phi) {
            hi = rho;
        }
        if within(phi + PI) {
            lo = -rho;
        }
    }
    [lo, hi]
}

/// A parameter-plane box `[[u0, v0], [u1, v1]]`.
type UvBox = [[f64; 2]; 2];

fn box_of(points: &[[f64; 2]]) -> UvBox {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        for c in 0..2 {
            b[0][c] = b[0][c].min(p[c]);
            b[1][c] = b[1][c].max(p[c]);
        }
    }
    b
}

fn boxes_meet(a: &UvBox, b: &UvBox) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

// ---------------------------------------------------------------------------
// B-spline nets: Bezier sub-patches and local hulls.

/// A net prepared for enclosures: homogeneous poles about the poles' box
/// centre.
#[derive(Debug)]
struct Net {
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
    ku: Vec<f64>,
    kv: Vec<f64>,
    h: Vec<[f64; 4]>,
    origin: Vec3,
    dom: [[f64; 2]; 2],
    /// Interior knots of C0 continuity (multiplicity at least the degree).
    c0u: Vec<f64>,
    c0v: Vec<f64>,
    /// The largest magnitude of a homogeneous coordinate.
    hmax: f64,
    /// The rounding factor of a sub-patch's coordinates.
    gamma: f64,
}

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

/// The non-empty spans meeting `[lo, hi]` (the span holding `lo` when the
/// interval is a point).
fn spans(knots: &[f64], degree: usize, count: usize, lo: f64, hi: f64) -> Vec<usize> {
    if lo >= hi {
        return vec![find_span(knots, degree, count, lo)];
    }
    (degree..count)
        .filter(|&i| knots[i] < knots[i + 1] && knots[i] < hi && knots[i + 1] > lo)
        .collect()
}

fn c0_knots(knots: &[f64], degree: usize, dom: [f64; 2]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut run = 0usize;
    let mut prev = f64::NAN;
    for &k in knots {
        run = if k == prev { run + 1 } else { 1 };
        prev = k;
        if run == degree && k > dom[0] && k < dom[1] {
            out.push(k);
        }
    }
    out
}

/// Blossom points of the curve over the span: Bezier point `m` over `[a, b]`
/// is the blossom at `a` (`degree - m` times) and `b` (`m` times), by de
/// Boor's algorithm with the argument per level.
fn blossom(
    knots: &[f64],
    degree: usize,
    span: usize,
    a: f64,
    b: f64,
    poles: &[[f64; 4]],
    out: &mut [[f64; 4]],
) {
    let p = degree;
    let mut d = [[0.0; 4]; crate::nurbs::MAX_DEGREE + 1];
    for (m, o) in out.iter_mut().enumerate().take(p + 1) {
        d[..=p].copy_from_slice(&poles[..=p]);
        for r in 1..=p {
            let t = if r <= p - m { a } else { b };
            for j in (r..=p).rev() {
                let i = span - p + j;
                let den = knots[i + p + 1 - r] - knots[i];
                let alpha = if den > 0.0 { (t - knots[i]) / den } else { 0.0 };
                for c in 0..4 {
                    d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
                }
            }
        }
        *o = d[p];
    }
}

/// A sub-patch's Bezier net and what it tells.
struct Bezier {
    /// `(p + 1) x (q + 1)` homogeneous points, u-major.
    pts: Vec<[f64; 4]>,
}

impl Net {
    fn new(s: &NurbsSurface) -> Self {
        let (mut lo, mut hi) = (s.poles[0], s.poles[0]);
        for p in &s.poles {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        let origin = 0.5 * (lo + hi);
        let mut hmax = 0.0_f64;
        let h: Vec<[f64; 4]> = s
            .poles
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let w = s.weights.as_ref().map_or(1.0, |w| w[k]);
                let q = *p - origin;
                let r = [q.x * w, q.y * w, q.z * w, w];
                for x in r {
                    hmax = hmax.max(x.abs());
                }
                r
            })
            .collect();
        let dom = [
            [s.knots_u[s.degree_u], s.knots_u[s.count_u]],
            [s.knots_v[s.degree_v], s.knots_v[s.count_v]],
        ];
        Self {
            p: s.degree_u,
            q: s.degree_v,
            nu: s.count_u,
            nv: s.count_v,
            c0u: c0_knots(&s.knots_u, s.degree_u, dom[0]),
            c0v: c0_knots(&s.knots_v, s.degree_v, dom[1]),
            ku: s.knots_u.clone(),
            kv: s.knots_v.clone(),
            h,
            origin,
            dom,
            hmax,
            gamma: 8.0 * (s.degree_u + s.degree_v + 2) as f64 * EPS,
        }
    }

    fn clamp(&self, b: &UvBox) -> UvBox {
        let cl = |x: f64, d: [f64; 2]| x.max(d[0]).min(d[1]);
        [
            [cl(b[0][0], self.dom[0]), cl(b[0][1], self.dom[1])],
            [cl(b[1][0], self.dom[0]), cl(b[1][1], self.dom[1])],
        ]
    }

    fn spans_u(&self, lo: f64, hi: f64) -> Vec<usize> {
        spans(&self.ku, self.p, self.nu, lo, hi)
    }

    fn spans_v(&self, lo: f64, hi: f64) -> Vec<usize> {
        spans(&self.kv, self.q, self.nv, lo, hi)
    }

    /// The Bezier net of span `(i, j)` over `b` (inside the span).
    fn bezier(&self, i: usize, j: usize, b: &UvBox) -> Bezier {
        let (p, q) = (self.p, self.q);
        let mut tmp = vec![[0.0; 4]; (p + 1) * (q + 1)];
        let mut col = vec![[0.0; 4]; p + 1];
        let mut outc = vec![[0.0; 4]; p + 1];
        for l in 0..=q {
            for (k, c) in col.iter_mut().enumerate() {
                *c = self.h[(i - p + k) * self.nv + (j - q + l)];
            }
            blossom(&self.ku, p, i, b[0][0], b[1][0], &col, &mut outc);
            for m in 0..=p {
                tmp[m * (q + 1) + l] = outc[m];
            }
        }
        let mut pts = vec![[0.0; 4]; (p + 1) * (q + 1)];
        let mut row = vec![[0.0; 4]; q + 1];
        let mut outr = vec![[0.0; 4]; q + 1];
        for m in 0..=p {
            row.copy_from_slice(&tmp[m * (q + 1)..(m + 1) * (q + 1)]);
            blossom(&self.kv, q, j, b[0][1], b[1][1], &row, &mut outr);
            pts[m * (q + 1)..(m + 1) * (q + 1)].copy_from_slice(&outr);
        }
        Bezier { pts }
    }

    fn project(&self, h: [f64; 4]) -> [f64; 3] {
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// The enclosure of a Bezier net (its projected points' box, rounding
    /// included).
    fn bezier_enc(&self, bz: &Bezier) -> Enc {
        let mut wmin = f64::INFINITY;
        let mut rel = Enc::EMPTY;
        for h in &bz.pts {
            wmin = wmin.min(h[3]);
            rel.add_point(self.project(*h));
        }
        let err = self.gamma * self.hmax;
        let w = wmin - err;
        if w.is_nan() || w <= 0.0 || !rel.lo.iter().chain(&rel.hi).all(|x| x.is_finite()) {
            return Enc::unbounded();
        }
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        for c in 0..3 {
            let xmax = rel.lo[c].abs().max(rel.hi[c].abs());
            m[c] = err * (1.0 + xmax) / w + 4.0 * EPS * (xmax + o[c].abs());
            e.lo[c] = rel.lo[c] + o[c];
            e.hi[c] = rel.hi[c] + o[c];
        }
        e.widen(m);
        e
    }

    /// The corners of a Bezier net, which the surface attains (up to the
    /// returned rounding per axis).
    fn corners(&self, bz: &Bezier) -> ([[f64; 3]; 4], [f64; 3]) {
        let (p, q) = (self.p, self.q);
        let idx = [0, q, p * (q + 1), p * (q + 1) + q];
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut out = [[0.0; 3]; 4];
        let mut m = [0.0_f64; 3];
        let err = self.gamma * self.hmax;
        for (k, &i) in idx.iter().enumerate() {
            let h = bz.pts[i];
            let x = self.project(h);
            let w = (h[3] - err).max(f64::MIN_POSITIVE);
            for c in 0..3 {
                out[k][c] = x[c] + o[c];
                m[c] =
                    m[c].max(err * (1.0 + x[c].abs()) / w + 4.0 * EPS * (x[c].abs() + o[c].abs()));
            }
        }
        (out, m)
    }

    /// Per direction (u, v) and axis: `1` when the coordinate increases over
    /// the whole sub-patch in that direction, `-1` when it decreases, `0`
    /// when the hodograph's interval does not decide.
    fn monotone(&self, bz: &Bezier) -> [[i8; 3]; 2] {
        let (p, q) = (self.p, self.q);
        let at = |m: usize, n: usize| bz.pts[m * (q + 1) + n];
        let mut f = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for h in &bz.pts {
            let x = self.project(*h);
            for c in 0..3 {
                f[c][0] = f[c][0].min(x[c]);
                f[c][1] = f[c][1].max(x[c]);
            }
        }
        let mut out = [[0i8; 3]; 2];
        for (dir, o) in out.iter_mut().enumerate() {
            // Hodograph coefficients (up to the positive degree factor).
            let mut a = [[f64::INFINITY, f64::NEG_INFINITY]; 4];
            let (mm, nn) = if dir == 0 { (p, q + 1) } else { (p + 1, q) };
            for m in 0..mm {
                for n in 0..nn {
                    let (x0, x1) = if dir == 0 {
                        (at(m, n), at(m + 1, n))
                    } else {
                        (at(m, n), at(m, n + 1))
                    };
                    for c in 0..4 {
                        let d = x1[c] - x0[c];
                        a[c][0] = a[c][0].min(d);
                        a[c][1] = a[c][1].max(d);
                    }
                }
            }
            let b = a[3];
            for c in 0..3 {
                let prods = [
                    f[c][0] * b[0],
                    f[c][0] * b[1],
                    f[c][1] * b[0],
                    f[c][1] * b[1],
                ];
                let pmin = prods.iter().fold(f64::INFINITY, |m, x| m.min(*x));
                let pmax = prods.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x));
                let lo = a[c][0] - pmax;
                let hi = a[c][1] - pmin;
                let fmag = f[c][0].abs().max(f[c][1].abs());
                let tol = 8.0 * self.gamma * self.hmax * (1.0 + fmag);
                if !(lo.is_finite() && hi.is_finite()) {
                    continue;
                }
                if lo > tol {
                    o[c] = 1;
                } else if hi < -tol {
                    o[c] = -1;
                }
            }
        }
        out
    }

    /// Per direction (u, v) and axis, a bound on the coordinate's derivative
    /// over the domain: `(|A'| + |x| |W'|) / min W` from the homogeneous
    /// hodograph's control points.
    fn lipschitz(&self) -> [[f64; 3]; 2] {
        let mut wmin = f64::INFINITY;
        let mut xmax = [0.0_f64; 3];
        for h in &self.h {
            wmin = wmin.min(h[3]);
            for c in 0..3 {
                xmax[c] = xmax[c].max((h[c] / h[3]).abs());
            }
        }
        let mut out = [[0.0; 3]; 2];
        for (dir, o) in out.iter_mut().enumerate() {
            let (deg, knots, count) = if dir == 0 {
                (self.p, &self.ku, self.nu)
            } else {
                (self.q, &self.kv, self.nv)
            };
            let mut d = [0.0_f64; 4];
            for i in 0..count - 1 {
                let den = knots[i + deg + 1] - knots[i + 1];
                if den <= 0.0 {
                    continue;
                }
                let others = if dir == 0 { self.nv } else { self.nu };
                for j in 0..others {
                    let (a, b) = if dir == 0 {
                        (self.h[i * self.nv + j], self.h[(i + 1) * self.nv + j])
                    } else {
                        (self.h[j * self.nv + i], self.h[j * self.nv + i + 1])
                    };
                    for c in 0..4 {
                        d[c] = d[c].max(deg as f64 * (b[c] - a[c]).abs() / den);
                    }
                }
            }
            for c in 0..3 {
                o[c] = (d[c] + xmax[c] * d[3]) / wmin * (1.0 + 1e-12);
            }
        }
        out
    }

    /// The hull of the poles that define the surface over spans `su x sv`.
    fn local_hull(&self, su: &[usize], sv: &[usize]) -> Enc {
        let (Some(&i0), Some(&i1), Some(&j0), Some(&j1)) =
            (su.first(), su.last(), sv.first(), sv.last())
        else {
            return Enc::unbounded();
        };
        let mut rel = Enc::EMPTY;
        for i in i0 - self.p..=i1 {
            for j in j0 - self.q..=j1 {
                rel.add_point(self.project(self.h[i * self.nv + j]));
            }
        }
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        for c in 0..3 {
            let xmax = rel.lo[c].abs().max(rel.hi[c].abs());
            m[c] = 4.0 * EPS * (xmax + o[c].abs());
            e.lo[c] = rel.lo[c] + o[c];
            e.hi[c] = rel.hi[c] + o[c];
        }
        e.widen(m);
        e
    }

    /// An enclosure of the surface over `b`: Bezier nets of each span it
    /// meets when they are few, else the hull of the poles that define it.
    fn enclose(&self, b: &UvBox) -> (Enc, usize) {
        let b = self.clamp(b);
        let su = self.spans_u(b[0][0], b[1][0]);
        let sv = self.spans_v(b[0][1], b[1][1]);
        if su.len() * sv.len() > 4 {
            return (self.local_hull(&su, &sv), 1);
        }
        let mut e = Enc::EMPTY;
        let mut work = 0;
        for &i in &su {
            for &j in &sv {
                let sub = [
                    [b[0][0].max(self.ku[i]), b[0][1].max(self.kv[j])],
                    [b[1][0].min(self.ku[i + 1]), b[1][1].min(self.kv[j + 1])],
                ];
                let bz = self.bezier(i, j, &sub);
                e.union(&self.bezier_enc(&bz));
                work += 1;
            }
        }
        (e, work)
    }
}

// ---------------------------------------------------------------------------
// Surfaces: enclosures over parameter boxes.

#[derive(Debug)]
enum Geo {
    Plane { o: Vec3, su: Vec3, sv: Vec3 },
    Ruled { f: Frame, r: f64, s: f64, c: f64 },
    Sphere { f: Frame, r: f64 },
    Torus { f: Frame, big: f64, r: f64 },
    Nurbs(Net),
}

impl Geo {
    fn new(s: &Surface) -> Self {
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => Self::Plane {
                o: *origin,
                su: *u_axis,
                sv: *v_axis,
            },
            Surface::Cylinder { frame, radius } => Self::Ruled {
                f: *frame,
                r: *radius,
                s: 0.0,
                c: 1.0,
            },
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => Self::Ruled {
                f: *frame,
                r: *radius,
                s: libm::sin(*semi_angle),
                c: libm::cos(*semi_angle),
            },
            Surface::Sphere { frame, radius } => Self::Sphere {
                f: *frame,
                r: *radius,
            },
            Surface::Torus {
                frame,
                major_radius,
                minor_radius,
            } => Self::Torus {
                f: *frame,
                big: *major_radius,
                r: *minor_radius,
            },
            Surface::Nurbs(n) => Self::Nurbs(Net::new(n)),
        }
    }

    /// The periods in u and v of the analytic kinds that close.
    fn periods(&self) -> [Option<f64>; 2] {
        match self {
            Self::Ruled { .. } | Self::Sphere { .. } => [Some(TAU), None],
            Self::Torus { .. } => [Some(TAU), Some(TAU)],
            Self::Plane { .. } | Self::Nurbs(_) => [None, None],
        }
    }

    /// An enclosure of the surface over `b`, exact up to rounding for the
    /// analytic kinds, and the enclosures computed.
    fn enclose(&self, b: &UvBox) -> (Enc, usize) {
        let [[u0, v0], [u1, v1]] = *b;
        let umax = u0.abs().max(u1.abs());
        let vmax = v0.abs().max(v1.abs());
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        match self {
            Self::Nurbs(n) => return n.enclose(b),
            Self::Plane { o, su, sv } => {
                for c in 0..3 {
                    let (oc, a, bb) = (coord(*o, c), coord(*su, c), coord(*sv, c));
                    for (u, v) in [(u0, v0), (u0, v1), (u1, v0), (u1, v1)] {
                        let x = oc + u * a + v * bb;
                        e.lo[c] = e.lo[c].min(x);
                        e.hi[c] = e.hi[c].max(x);
                    }
                    m[c] = 8.0 * EPS * (oc.abs() + umax * a.abs() + vmax * bb.abs());
                }
            }
            Self::Ruled { f, r, s, c: ca } => {
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = range_cs(a, bb, u0, u1);
                    for vk in [v0, v1] {
                        let rho = *r + vk * *s;
                        let base = oc + vk * *ca * z;
                        let (lo, hi) = if rho >= 0.0 {
                            (base + rho * g[0], base + rho * g[1])
                        } else {
                            (base + rho * g[1], base + rho * g[0])
                        };
                        e.lo[c] = e.lo[c].min(lo);
                        e.hi[c] = e.hi[c].max(hi);
                    }
                    m[c] = 32.0
                        * EPS
                        * (oc.abs() + (r.abs() + vmax * s.abs()) * (1.0 + umax) + vmax * z.abs());
                }
            }
            Self::Sphere { f, r } => {
                let va = v0.clamp(-FRAC_PI_2, FRAC_PI_2);
                let vb = v1.clamp(-FRAC_PI_2, FRAC_PI_2);
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = range_cs(a, bb, u0, u1);
                    e.lo[c] = oc + *r * range_cs(g[0], z, va, vb)[0];
                    e.hi[c] = oc + *r * range_cs(g[1], z, va, vb)[1];
                    m[c] = 32.0 * EPS * (oc.abs() + *r * (1.0 + umax + vmax));
                }
            }
            Self::Torus { f, big, r } => {
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = range_cs(a, bb, u0, u1);
                    if *big >= *r {
                        e.lo[c] = oc + *big * g[0] + *r * range_cs(g[0], z, v0, v1)[0];
                        e.hi[c] = oc + *big * g[1] + *r * range_cs(g[1], z, v0, v1)[1];
                    } else {
                        // A spindle torus: the coefficient of g changes sign.
                        let cv = range_cs(1.0, 0.0, v0, v1);
                        let sv = range_cs(0.0, 1.0, v0, v1);
                        let k = [*big + *r * cv[0], *big + *r * cv[1]];
                        let prods = [k[0] * g[0], k[0] * g[1], k[1] * g[0], k[1] * g[1]];
                        let zs = [*r * z * sv[0], *r * z * sv[1]];
                        e.lo[c] = oc
                            + prods.iter().fold(f64::INFINITY, |m, x| m.min(*x))
                            + zs[0].min(zs[1]);
                        e.hi[c] = oc
                            + prods.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x))
                            + zs[0].max(zs[1]);
                    }
                    m[c] = 32.0 * EPS * (oc.abs() + (big.abs() + *r) * (1.0 + umax + vmax));
                }
            }
        }
        e.widen(m);
        (e, 1)
    }
}

// ---------------------------------------------------------------------------
// The face's boundary pieces and its classifier.

/// A boundary piece: a trim (attained) or a closing chord (bounds only).
/// The coefficients of a B-spline's derivative (degree `deg - 1`, knots
/// without their ends); a zero-length span's coefficient is zero.
fn dcoef<const D: usize>(cs: &[[f64; D]], deg: usize, knots: &[f64]) -> (Vec<[f64; D]>, Vec<f64>) {
    let mut out = Vec::with_capacity(cs.len().saturating_sub(1));
    if deg == 0 || cs.len() < 2 {
        return (out, knots.to_vec());
    }
    for i in 0..cs.len() - 1 {
        let den = knots[i + deg + 1] - knots[i + 1];
        let mut d = [0.0; D];
        if den > 0.0 {
            for c in 0..D {
                d[c] = deg as f64 * (cs[i + 1][c] - cs[i][c]) / den;
            }
        }
        out.push(d);
    }
    (out, knots[1..knots.len() - 1].to_vec())
}

fn max_abs<const D: usize>(cs: &[[f64; D]]) -> [f64; D] {
    let mut m = [0.0_f64; D];
    for x in cs {
        for c in 0..D {
            m[c] = m[c].max(x[c].abs());
        }
    }
    m
}

/// Bounds on `|c'|` and `|c''|` per parameter (u, v) of a trim curve over
/// its whole range, from its homogeneous derivatives' coefficients:
/// `c' = (A' - c w') / w` and `c'' = (A'' - 2 c' w' - c w'') / w`.
fn curve_derivatives(t: &TrimPiece) -> Option<[[f64; 2]; 2]> {
    match &t.curve {
        Curve2::Line { direction, .. } => {
            Some([[direction[0].abs(), direction[1].abs()], [0.0; 2]])
        }
        Curve2::Nurbs(c) => {
            let h: Vec<[f64; 3]> = c
                .poles
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let w = c.weights.as_ref().map_or(1.0, |w| w[k]);
                    [p[0] * w, p[1] * w, w]
                })
                .collect();
            let wmin = h.iter().fold(f64::INFINITY, |m, x| m.min(x[2]));
            if wmin.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return None;
            }
            let x = max_abs(&c.poles);
            let (d1, k1) = dcoef(&h, c.degree, &c.knots);
            let (d2, _) = dcoef(&d1, c.degree.saturating_sub(1), &k1);
            let (a1, a2) = (max_abs(&d1), max_abs(&d2));
            let mut out = [[0.0; 2]; 2];
            for k in 0..2 {
                out[0][k] = (a1[k] + x[k] * a1[2]) / wmin;
                out[1][k] = (a2[k] + 2.0 * out[0][k] * a1[2] + x[k] * a2[2]) / wmin;
            }
            let ok = out.iter().flatten().all(|v| v.is_finite());
            ok.then(|| out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
        }
    }
}

/// Bounds per axis on `|S_u|, |S_v|, |S_uu|, |S_uv|, |S_vv|` of a net over
/// its whole domain (its homogeneous derivatives' coefficients, as for a
/// curve).
fn net_derivatives(n: &Net) -> Option<[[f64; 3]; 5]> {
    let grid: Vec<Vec<[f64; 4]>> = (0..n.nu)
        .map(|i| n.h[i * n.nv..(i + 1) * n.nv].to_vec())
        .collect();
    let wmin = n.h.iter().fold(f64::INFINITY, |m, x| m.min(x[3]));
    if wmin.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let mut xmax = [0.0_f64; 3];
    for h in &n.h {
        for c in 0..3 {
            xmax[c] = xmax[c].max((h[c] / h[3]).abs());
        }
    }
    // Along u: each column j over i; along v: each row i over j.
    let du =
        |g: &Vec<Vec<[f64; 4]>>, deg: usize, knots: &[f64]| -> (Vec<Vec<[f64; 4]>>, Vec<f64>) {
            let cols = g.first().map_or(0, Vec::len);
            let mut out: Vec<Vec<[f64; 4]>> =
                vec![Vec::with_capacity(cols); g.len().saturating_sub(1)];
            let mut ks = Vec::new();
            for j in 0..cols {
                let col: Vec<[f64; 4]> = g.iter().map(|r| r[j]).collect();
                let (d, k) = dcoef(&col, deg, knots);
                for (i, x) in d.into_iter().enumerate() {
                    out[i].push(x);
                }
                ks = k;
            }
            (out, ks)
        };
    let dv = |g: &Vec<Vec<[f64; 4]>>, deg: usize, knots: &[f64]| -> Vec<Vec<[f64; 4]>> {
        g.iter().map(|r| dcoef(r, deg, knots).0).collect()
    };
    let flat = |g: &Vec<Vec<[f64; 4]>>| max_abs(&g.iter().flatten().copied().collect::<Vec<_>>());
    let (gu, ku1) = du(&grid, n.p, &n.ku);
    let (guu, _) = du(&gu, n.p.saturating_sub(1), &ku1);
    let gv = dv(&grid, n.q, &n.kv);
    let kv1 = n.kv[1..n.kv.len() - 1].to_vec();
    let gvv = dv(&gv, n.q.saturating_sub(1), &kv1);
    let guv = dv(&gu, n.q, &n.kv);
    let (au, av, auu, auv, avv) = (flat(&gu), flat(&gv), flat(&guu), flat(&guv), flat(&gvv));
    let mut out = [[0.0; 3]; 5];
    for c in 0..3 {
        let su = (au[c] + xmax[c] * au[3]) / wmin;
        let sv = (av[c] + xmax[c] * av[3]) / wmin;
        out[0][c] = su;
        out[1][c] = sv;
        out[2][c] = (auu[c] + 2.0 * su * au[3] + xmax[c] * auu[3]) / wmin;
        out[3][c] = (auv[c] + su * av[3] + sv * au[3] + xmax[c] * auv[3]) / wmin;
        out[4][c] = (avv[c] + 2.0 * sv * av[3] + xmax[c] * avv[3]) / wmin;
    }
    let ok = out.iter().flatten().all(|v| v.is_finite());
    ok.then(|| out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
}

struct Piece {
    trim: PreparedTrim,
    /// The range's ends and the curve's distinct knots inside it; empty for
    /// a line (its range alone).
    cuts: Vec<f64>,
    attained: bool,
    straight: bool,
    /// An iso-parametric line (`u` or `v` constant), exact on analytic kinds.
    iso: bool,
    /// Bounds on `|c'|` and `|c''|` per parameter, when known.
    deriv: Option<[[f64; 2]; 2]>,
}

/// A trim's range ends and its curve's distinct knots strictly inside.
fn cuts_of(t: &TrimPiece) -> Vec<f64> {
    let [a, b] = t.range;
    let Curve2::Nurbs(c) = &t.curve else {
        return Vec::new();
    };
    let mut out = vec![a];
    for &k in &c.knots {
        if k > a && k < b && out.last().is_some_and(|l| *l < k) {
            out.push(k);
        }
    }
    out.push(b);
    out
}

impl Piece {
    /// The piece's parameter intervals between its cuts.
    fn intervals(&self) -> Vec<(f64, f64)> {
        if self.cuts.is_empty() {
            return vec![(self.trim.range[0], self.trim.range[1])];
        }
        self.cuts
            .windows(2)
            .filter_map(|w| match *w {
                [a, b] => Some((a, b)),
                _ => None,
            })
            .collect()
    }
}

fn line_piece(a: [f64; 2], b: [f64; 2]) -> Piece {
    Piece {
        trim: PreparedTrim::new(&TrimPiece {
            curve: Curve2::Line {
                origin: a,
                direction: [b[0] - a[0], b[1] - a[1]],
            },
            range: [0.0, 1.0],
            reversed: false,
        }),
        cuts: Vec::new(),
        attained: false,
        straight: true,
        iso: a[0] == b[0] || a[1] == b[1],
        deriv: Some([[(b[0] - a[0]).abs(), (b[1] - a[1]).abs()], [0.0; 2]]),
    }
}

fn trim_ends(t: &PreparedTrim) -> ([f64; 2], [f64; 2]) {
    let (a, _) = t.eval(t.range[0]);
    let (b, _) = t.eval(t.range[1]);
    if t.reversed {
        (b, a)
    } else {
        (a, b)
    }
}

/// The face's pieces: its trims, then a chord for every junction where a
/// piece does not end exactly where the next starts (shifted by whole periods
/// to the nearest image).
fn pieces(face: &FaceDefinition, periods: [Option<f64>; 2]) -> Vec<Piece> {
    let mut out: Vec<Piece> = face
        .trims
        .iter()
        .map(|t| {
            let iso = match &t.curve {
                Curve2::Line { direction, .. } => direction[0] == 0.0 || direction[1] == 0.0,
                Curve2::Nurbs(_) => false,
            };
            Piece {
                trim: PreparedTrim::new(t),
                cuts: cuts_of(t),
                attained: true,
                straight: matches!(t.curve, Curve2::Line { .. }),
                iso,
                deriv: curve_derivatives(t),
            }
        })
        .collect();
    let mut chords = Vec::new();
    let mut start = 0usize;
    for &count in &face.loops {
        let lo = start;
        start += count;
        for k in lo..start {
            let next = if k + 1 == start { lo } else { k + 1 };
            let (Some(a), Some(b)) = (out.get(k), out.get(next)) else {
                continue;
            };
            let p = trim_ends(&a.trim).1;
            let mut q = trim_ends(&b.trim).0;
            for (c, period) in periods.iter().enumerate() {
                if let Some(t) = period {
                    let n = libm::round((p[c] - q[c]) / t);
                    q[c] += n * t;
                }
            }
            if p != q {
                chords.push(line_piece(p, q));
            }
        }
    }
    out.extend(chords);
    out
}

/// One segment of a piece over `[t0, t1]`: its control-point box and end
/// points.
#[derive(Clone, Copy)]
struct Seg {
    piece: usize,
    t0: f64,
    t1: f64,
    b: UvBox,
    a: [f64; 2],
    z: [f64; 2],
}

/// Classifies parameter-plane points and boxes against the face's pieces.
struct Classifier<'a> {
    pieces: &'a [Piece],
    segs: Vec<Seg>,
    period_u: Option<f64>,
    period_v: Option<f64>,
    /// The pieces' box.
    window: UvBox,
}

fn seg_of(pieces: &[Piece], piece: usize, t0: f64, t1: f64) -> Option<Seg> {
    let t = &pieces.get(piece)?.trim;
    let pts = t.hull_points(t0, t1)?;
    let mut b = box_of(&pts);
    let a = t.eval(t0).0;
    let z = t.eval(t1).0;
    for p in [a, z] {
        for c in 0..2 {
            b[0][c] = b[0][c].min(p[c]);
            b[1][c] = b[1][c].max(p[c]);
        }
    }
    let finite = b
        .iter()
        .flatten()
        .chain(&a)
        .chain(&z)
        .all(|x| x.is_finite());
    finite.then_some(Seg {
        piece,
        t0,
        t1,
        b,
        a,
        z,
    })
}

/// Whether segment `a`-`z` meets box `q` (Liang-Barsky clipping).
fn line_meets_box(a: [f64; 2], z: [f64; 2], q: &UvBox) -> bool {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for c in 0..2 {
        let d = z[c] - a[c];
        for (p, r) in [(-d, a[c] - q[0][c]), (d, q[1][c] - a[c])] {
            if p == 0.0 {
                if r < 0.0 {
                    return false;
                }
            } else {
                let t = r / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
    }
    t0 <= t1
}

/// The whole-period shifts `k` with `[lo, hi] + k period` meeting `[a, b]`.
fn shifts(period: Option<f64>, lo: f64, hi: f64, a: f64, b: f64) -> std::ops::RangeInclusive<i64> {
    match period {
        None if hi >= a && lo <= b => 0..=0,
        #[allow(clippy::reversed_empty_ranges)]
        None => 1..=0,
        Some(t) => {
            let k0 = libm::ceil((a - hi) / t);
            let k1 = libm::floor((b - lo) / t);
            if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 64.0 {
                #[allow(clippy::reversed_empty_ranges)]
                return 1..=0;
            }
            (k0 as i64)..=(k1 as i64)
        }
    }
}

impl<'a> Classifier<'a> {
    fn new(pieces: &'a [Piece], periods: [Option<f64>; 2]) -> Option<Self> {
        let mut segs = Vec::new();
        for (i, p) in pieces.iter().enumerate() {
            for (t0, t1) in p.intervals() {
                if p.straight {
                    segs.push(seg_of(pieces, i, t0, t1)?);
                } else {
                    let m = 0.5 * (t0 + t1);
                    segs.push(seg_of(pieces, i, t0, m)?);
                    segs.push(seg_of(pieces, i, m, t1)?);
                }
            }
        }
        let mut window = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for s in &segs {
            for c in 0..2 {
                window[0][c] = window[0][c].min(s.b[0][c]);
                window[1][c] = window[1][c].max(s.b[1][c]);
            }
        }
        Some(Self {
            pieces,
            segs,
            period_u: periods[0],
            period_v: periods[1],
            window,
        })
    }

    /// `Some(true)` when `q` lies inside the face, `Some(false)` outside,
    /// `None` when the trims come too close to decide.
    fn classify(&self, q: &UvBox) -> Option<bool> {
        let mut reps = Vec::new();
        match self.period_v {
            None => reps.push(*q),
            Some(t) => {
                for k in shifts(
                    Some(t),
                    q[0][1],
                    q[1][1],
                    self.window[0][1],
                    self.window[1][1],
                ) {
                    let d = k as f64 * t;
                    reps.push([[q[0][0], q[0][1] + d], [q[1][0], q[1][1] + d]]);
                }
            }
        }
        let mut unknown = false;
        for r in &reps {
            match self.classify_one(r) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => unknown = true,
            }
        }
        if unknown {
            None
        } else {
            Some(false)
        }
    }

    fn classify_one(&self, q: &UvBox) -> Option<bool> {
        if q[1][1] < self.window[0][1] || q[0][1] > self.window[1][1] {
            return Some(false);
        }
        if self.period_u.is_none() && (q[1][0] < self.window[0][0] || q[0][0] > self.window[1][0]) {
            return Some(false);
        }
        let mag = q.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
        let tol = CLASSIFY_TOLERANCE * (1.0 + mag);
        let wide = [
            [q[0][0] - tol, q[0][1] - tol],
            [q[1][0] + tol, q[1][1] + tol],
        ];
        let c = [0.5 * (q[0][0] + q[1][0]), 0.5 * (q[0][1] + q[1][1])];
        let mut crossings = 0usize;
        for s in &self.segs {
            if !self.resolve(*s, &wide, c, 0, &mut crossings) {
                return None;
            }
        }
        Some(crossings % 2 == 1)
    }

    /// Counts `s`'s upward-ray crossings from `c` after bisecting it until no
    /// part of it meets `wide`; `false` when that cannot be done.
    fn resolve(
        &self,
        s: Seg,
        wide: &UvBox,
        c: [f64; 2],
        depth: u32,
        crossings: &mut usize,
    ) -> bool {
        let tu = self.period_u;
        let straight = self.pieces.get(s.piece).is_some_and(|p| p.straight);
        for k in shifts(tu, s.b[0][0], s.b[1][0], wide[0][0], wide[1][0]) {
            let d = tu.map_or(0.0, |t| k as f64 * t);
            let b = [[s.b[0][0] + d, s.b[0][1]], [s.b[1][0] + d, s.b[1][1]]];
            if !boxes_meet(&b, wide) {
                continue;
            }
            if straight {
                if line_meets_box([s.a[0] + d, s.a[1]], [s.z[0] + d, s.z[1]], wide) {
                    return false;
                }
                continue;
            }
            if depth >= MAX_CLASSIFY_DEPTH
                || s.t1.partial_cmp(&s.t0) != Some(std::cmp::Ordering::Greater)
            {
                return false;
            }
            let m = 0.5 * (s.t0 + s.t1);
            let (Some(l), Some(r)) = (
                seg_of(self.pieces, s.piece, s.t0, m),
                seg_of(self.pieces, s.piece, m, s.t1),
            ) else {
                return false;
            };
            return self.resolve(l, wide, c, depth + 1, crossings)
                && self.resolve(r, wide, c, depth + 1, crossings);
        }
        // No image of the segment meets the query: count its chord.
        for k in shifts(tu, s.a[0].min(s.z[0]), s.a[0].max(s.z[0]), c[0], c[0]) {
            let d = tu.map_or(0.0, |t| k as f64 * t);
            let (a, z) = ([s.a[0] + d, s.a[1]], [s.z[0] + d, s.z[1]]);
            if (a[0] <= c[0]) != (z[0] <= c[0]) {
                let v = a[1] + (c[0] - a[0]) * (z[1] - a[1]) / (z[0] - a[0]);
                if v > c[1] {
                    *crossings += 1;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// One face.

/// What one face's refinement keeps.
struct State {
    sides: Sides,
    /// The side values the first pass attained over the body: refinement
    /// stops within `eps` of the larger of these and the face's own.
    floor: [f64; SIDES],
    /// Side values of surface points in the region being bounded that are
    /// not known to lie on the face (a fallback's rectangle, closing
    /// chords): refinement stops near them too, but they are never inner.
    reach: [f64; SIDES],
    /// Bounding a fallback rectangle: its points count towards `reach`.
    loose: bool,
    /// Refining (the second pass); the first pass encloses each piece and
    /// the face's interior once.
    refine: bool,
    eps: f64,
    work: usize,
    capped: bool,
}

impl State {
    fn new(floor: [f64; SIDES], eps: f64) -> Self {
        Self {
            sides: Sides::EMPTY,
            floor,
            reach: [f64::NEG_INFINITY; SIDES],
            loose: false,
            refine: true,
            eps,
            work: 0,
            capped: false,
        }
    }

    fn target(&self, s: usize) -> f64 {
        self.sides.inner[s].max(self.floor[s]).max(self.reach[s]) + self.eps
    }

    fn attain(&mut self, p: [f64; 3], m: [f64; 3]) {
        for s in 0..SIDES {
            let c = s % 3;
            let v = if s < 3 { -p[c] } else { p[c] } - m[c];
            if v > self.sides.inner[s] {
                self.sides.inner[s] = v;
            }
        }
    }

    fn reach(&mut self, p: [f64; 3]) {
        for s in 0..SIDES {
            let v = if s < 3 { -p[s % 3] } else { p[s % 3] };
            if v > self.reach[s] {
                self.reach[s] = v;
            }
        }
    }

    fn bound(&mut self, s: usize, upper: f64) {
        if upper > self.sides.outer[s] {
            self.sides.outer[s] = upper;
        }
    }

    /// Records each side of `e` that is resolved (cannot beat the target)
    /// and returns the sides still open.
    fn settle(&mut self, e: &Enc, skip: [bool; SIDES]) -> [bool; SIDES] {
        let mut open = [false; SIDES];
        for s in 0..SIDES {
            if skip[s] {
                continue;
            }
            let u = e.upper(s);
            if u > self.target(s) {
                open[s] = true;
            } else {
                self.bound(s, u);
            }
        }
        open
    }
}

/// A surface `o + a su + b sv + a b twist` with `a = (u - u0) / du` and
/// `b = (v - v0) / dv`. The affine map stands for the surface only where
/// the twist is negligible, and its margin then covers the twist.
struct Affine {
    o: Vec3,
    su: Vec3,
    sv: Vec3,
    u0: f64,
    du: f64,
    v0: f64,
    dv: f64,
    /// The twist's magnitude per axis; the map adds it to every margin.
    twist: [f64; 3],
    /// Whether the twist is small enough for the affine map.
    flat: bool,
}

impl Affine {
    fn of(s: &Surface) -> Option<Self> {
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => Some(Self {
                o: *origin,
                su: *u_axis,
                sv: *v_axis,
                u0: 0.0,
                du: 1.0,
                v0: 0.0,
                dv: 1.0,
                twist: [0.0; 3],
                flat: true,
            }),
            Surface::Nurbs(n)
                if n.degree_u == 1 && n.degree_v == 1 && n.count_u == 2 && n.count_v == 2 =>
            {
                if let Some(w) = &n.weights {
                    if w.iter().any(|x| *x != w[0]) {
                        return None;
                    }
                }
                let [u0, u1] = [n.knots_u[1], n.knots_u[2]];
                let [v0, v1] = [n.knots_v[1], n.knots_v[2]];
                if !(u1 > u0 && v1 > v0) {
                    return None;
                }
                // Pole (i, j) at i * 2 + j.
                let (p00, p01, p10, p11) = (n.poles[0], n.poles[1], n.poles[2], n.poles[3]);
                let d = p11 - p10 - p01 + p00;
                let size = [p00, p01, p10, p11]
                    .iter()
                    .fold(0.0_f64, |m, p| m.max(p.max_abs()));
                let twist = [
                    d.x.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                    d.y.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                    d.z.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                ];
                Some(Self {
                    o: p00,
                    su: p10 - p00,
                    sv: p01 - p00,
                    u0,
                    du: u1 - u0,
                    v0,
                    dv: v1 - v0,
                    flat: d.max_abs() <= 1e-9 * size,
                    twist,
                })
            }
            _ => None,
        }
    }

    /// The affine part at `uv` and its margin per axis, the twist included.
    fn map(&self, uv: [f64; 2]) -> ([f64; 3], [f64; 3]) {
        let a = (uv[0] - self.u0) / self.du;
        let b = (uv[1] - self.v0) / self.dv;
        let x = self.o + a * self.su + b * self.sv;
        let mut m = [0.0; 3];
        for c in 0..3 {
            // The bilinear term's reach, also just outside the unit square.
            m[c] = 16.0
                * EPS
                * (coord(self.o, c).abs()
                    + (a.abs() + 1.0) * coord(self.su, c).abs()
                    + (b.abs() + 1.0) * coord(self.sv, c).abs())
                + self.twist[c] * (a.abs().max(1.0) * b.abs().max(1.0)).max(1.0) * 1.01;
        }
        ([x.x, x.y, x.z], m)
    }
}

/// The surface prepared both ways: for enclosures and for evaluation.
struct FaceGeo {
    geo: Geo,
    prepared: Prepared,
    domain: [[f64; 2]; 2],
    nurbs: bool,
    /// A plane, or a bilinear net (degree 1 by 1, two by two poles, equal
    /// weights): a coordinate's extremes over a region lie on its boundary.
    affine: Option<Affine>,
    /// A net's derivative bounds over its domain (see [`net_derivatives`]).
    net_d: Option<[[f64; 3]; 5]>,
    /// The analytic kinds' coordinate scale for rounding margins: the
    /// origin's magnitude plus the axes' or radii.
    scale: f64,
}

fn outside(x: f64, [lo, hi]: [f64; 2]) -> bool {
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let slack = DOMAIN_TOLERANCE * (hi - lo);
    x < lo - slack || x > hi + slack
}

impl FaceGeo {
    fn new(s: &Surface) -> Self {
        let prepared = Prepared::new(s);
        let domain = prepared.domain();
        Self {
            scale: match s {
                Surface::Plane {
                    origin,
                    u_axis,
                    v_axis,
                } => origin.max_abs() + u_axis.max_abs() + v_axis.max_abs(),
                Surface::Cylinder { frame, radius } | Surface::Sphere { frame, radius } => {
                    frame.origin.max_abs() + radius.abs() + 1.0
                }
                Surface::Cone { frame, radius, .. } => frame.origin.max_abs() + radius.abs() + 1.0,
                Surface::Torus {
                    frame,
                    major_radius,
                    minor_radius,
                } => frame.origin.max_abs() + major_radius.abs() + minor_radius.abs(),
                Surface::Nurbs(_) => 0.0,
            },
            net_d: match s {
                Surface::Nurbs(n) => net_derivatives(&Net::new(n)),
                _ => None,
            },
            geo: Geo::new(s),
            prepared,
            domain,
            nurbs: matches!(s, Surface::Nurbs(_)),
            affine: Affine::of(s),
        }
    }

    /// The surface point at `uv` and its rounding per axis.
    fn point(&self, uv: [f64; 2], row: &mut Row) -> Result<([f64; 3], [f64; 3]), Refusal> {
        if !(uv[0].is_finite() && uv[1].is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("trim point {uv:?}"),
            ));
        }
        if outside(uv[0], self.domain[0]) || outside(uv[1], self.domain[1]) {
            return Err(Refusal::new(
                RefusalCode::TrimOutsideDomain,
                format!(
                    "trim point ({}, {}) outside the surface's domain {:?}",
                    uv[0], uv[1], self.domain
                ),
            ));
        }
        let cl = |x: f64, d: [f64; 2]| {
            if x < d[0] {
                d[0]
            } else if x > d[1] {
                d[1]
            } else {
                x
            }
        };
        let uv = [cl(uv[0], self.domain[0]), cl(uv[1], self.domain[1])];
        self.prepared.row(uv[0], row);
        let ((p, _, _), err) = self.prepared.eval_bounded(row, uv[1]);
        if !p.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("surface point at {uv:?}"),
            ));
        }
        let mag = match &self.geo {
            Geo::Plane { .. } => {
                // A plane's rounding grows with u and v linearly.
                let m = self.affine.as_ref().map_or([0.0; 3], |a| a.map(uv).1);
                return Ok(([p.x, p.y, p.z], m));
            }
            Geo::Nurbs(n) => n.origin.max_abs() + p.max_abs(),
            _ => self.scale * (1.0 + uv[0].abs()) * (1.0 + uv[1].abs()),
        };
        let m = err[0] + 32.0 * EPS * mag;
        Ok(([p.x, p.y, p.z], [m; 3]))
    }

    /// Bounds per axis on `|S_u|, |S_v|, |S_uu|, |S_uv|, |S_vv|` over `hull`,
    /// `None` where unknown; a net's hold only inside its domain.
    fn derivatives(&self, hull: UvBox) -> Option<[[f64; 3]; 5]> {
        let mut out = [[0.0; 3]; 5];
        let vmax = hull[0][1].abs().max(hull[1][1].abs());
        for c in 0..3 {
            let rho_z = |f: &Frame| {
                (
                    libm::hypot(coord(f.x, c), coord(f.y, c)),
                    coord(f.z, c).abs(),
                )
            };
            out[0][c] = match &self.geo {
                Geo::Plane { su, sv, .. } => {
                    out[1][c] = coord(*sv, c).abs();
                    coord(*su, c).abs()
                }
                Geo::Ruled { f, r, s, c: k } => {
                    let (rho, z) = rho_z(f);
                    let big = (r.abs() + s.abs() * vmax) * rho;
                    out[1][c] = s.abs() * rho + k.abs() * z;
                    out[2][c] = big;
                    out[3][c] = s.abs() * rho;
                    big
                }
                Geo::Sphere { f, r } => {
                    let (rho, z) = rho_z(f);
                    let a = r.abs() * (rho + z);
                    for d in 1..5 {
                        out[d][c] = a;
                    }
                    a
                }
                Geo::Torus { f, big, r } => {
                    let (rho, z) = rho_z(f);
                    let a = (big.abs() + r.abs()) * rho;
                    out[1][c] = r.abs() * (rho + z);
                    out[2][c] = a;
                    out[3][c] = r.abs() * rho;
                    out[4][c] = r.abs() * (rho + z);
                    a
                }
                Geo::Nurbs(n) => {
                    // Not across a knot line where the first derivatives
                    // jump; the caller keeps to the domain.
                    let kink = n.c0u.iter().any(|&x| x > hull[0][0] && x < hull[1][0])
                        || n.c0v.iter().any(|&x| x > hull[0][1] && x < hull[1][1]);
                    return if kink { None } else { self.net_d };
                }
            };
        }
        // The frames are unit only to rounding.
        Some(out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
    }

    /// An enclosure of one piece over `[a, b]`.
    fn piece_enc(&self, piece: &Piece, a: f64, b: f64) -> (Enc, usize) {
        let pts = piece.trim.hull_points(a, b).unwrap_or_else(|| {
            let h = piece.trim.hull();
            vec![h[0], h[1], [h[0][0], h[1][1]], [h[1][0], h[0][1]]]
        });
        if let Some(af) = self.affine.as_ref().filter(|a| a.flat) {
            // An affine map takes the control points' hull onto the piece's
            // hull.
            let mut e = Enc::EMPTY;
            let mut m = [0.0_f64; 3];
            for p in &pts {
                let (x, mx) = af.map(*p);
                e.add_point(x);
                for c in 0..3 {
                    m[c] = m[c].max(mx[c]);
                }
            }
            e.widen(m);
            return (e, 1);
        }
        self.geo.enclose(&box_of(&pts))
    }
}

/// Refines one boundary piece.
fn bound_piece(g: &FaceGeo, piece: &Piece, st: &mut State, row: &mut Row) -> Result<(), Refusal> {
    let mut eval = |t: f64, st: &mut State| -> Result<([f64; 3], [f64; 3]), Refusal> {
        let uv = piece.trim.eval(t).0;
        let (p, m) = g.point(uv, row)?;
        if piece.attained {
            st.attain(p, m);
        } else {
            st.reach(p);
        }
        Ok((p, m))
    };
    let [a, b] = piece.trim.range;
    if let Some(af) = g.affine.as_ref().filter(|a| piece.straight && a.flat) {
        // A segment of an affine surface: its end points are its extremes.
        for t in [a, b] {
            let uv = piece.trim.eval(t).0;
            let (p, m) = g.point(uv, row)?;
            if piece.attained {
                st.attain(p, m);
            } else {
                st.reach(p);
            }
            let (_, ma) = af.map(uv);
            for s in 0..SIDES {
                st.bound(
                    s,
                    side_value(Vec3::new(p[0], p[1], p[2]), s) + m[s % 3] + 2.0 * ma[s % 3],
                );
            }
        }
        st.work += 1;
        return Ok(());
    }
    if piece.straight && piece.iso && !g.nurbs {
        // An iso line of an analytic surface: a segment or an arc whose
        // enclosure is its own extremes, which it attains.
        if g.domain.iter().flatten().any(|x| x.is_finite()) {
            eval(a, st)?;
            eval(b, st)?;
        }
        let (e, w) = g.piece_enc(piece, a, b);
        st.work += w;
        let mut p = [0.0; 3];
        let mut q = [0.0; 3];
        for c in 0..3 {
            p[c] = e.hi[c] - e.m[c];
            q[c] = e.lo[c] + e.m[c];
        }
        if piece.attained {
            st.attain(p, e.m);
            st.attain(q, e.m);
        } else {
            st.reach(p);
            st.reach(q);
        }
        for s in 0..SIDES {
            st.bound(s, e.upper(s));
        }
        return Ok(());
    }
    let intervals = piece.intervals();
    // A second-order bound: with `|f''| <= k` per axis along the piece,
    // `f <= max(f(t0), f(t1)) + k h^2 / 8` over `[t0, t1]`.
    let curv = piece.deriv.and_then(|[d1, d2]| {
        let sd = g.derivatives(piece.trim.hull())?;
        let mut k = [0.0; 3];
        for c in 0..3 {
            k[c] = (sd[2][c] * d1[0] * d1[0]
                + 2.0 * sd[3][c] * d1[0] * d1[1]
                + sd[4][c] * d1[1] * d1[1]
                + sd[0][c] * d2[0]
                + sd[1][c] * d2[1])
                * (1.0 + 1e-9);
        }
        k.iter().all(|x| x.is_finite()).then_some(k)
    });
    let mut ends = Vec::with_capacity(intervals.len() + 1);
    ends.push(eval(a, st)?);
    for &(_, t1) in &intervals {
        ends.push(eval(t1, st)?);
    }
    type End = ([f64; 3], [f64; 3]);
    let mut stack: Vec<(f64, f64, u32, End, End)> = intervals
        .iter()
        .enumerate()
        .rev()
        .map(|(i, &(t0, t1))| (t0, t1, 0, ends[i], ends[i + 1]))
        .collect();
    while let Some((t0, t1, depth, e0, e1)) = stack.pop() {
        let (mut e, w) = g.piece_enc(piece, t0, t1);
        st.work += w;
        // A net's bounds hold where the sub-curve stays in its domain (a
        // clamped point beyond it is not smooth along the piece).
        let smooth = match &g.geo {
            Geo::Nurbs(n) if curv.is_some() => piece.trim.hull_points(t0, t1).is_some_and(|pts| {
                pts.iter()
                    .all(|p| (0..2).all(|k| p[k] >= n.dom[k][0] && p[k] <= n.dom[k][1]))
            }),
            _ => true,
        };
        if let Some(k) = curv.filter(|_| smooth) {
            let h = t1 - t0;
            for c in 0..3 {
                let q = k[c] * h * h * 0.125 * (1.0 + 1e-9);
                let hi = (e0.0[c] + e0.1[c]).max(e1.0[c] + e1.1[c]) + q;
                let lo = (e0.0[c] - e0.1[c]).min(e1.0[c] - e1.1[c]) - q;
                if hi < e.hi[c] {
                    e.hi[c] = hi;
                }
                if lo > e.lo[c] {
                    e.lo[c] = lo;
                }
            }
        }
        let open = st.settle(&e, [false; SIDES]);
        if !open.iter().any(|o| *o) {
            continue;
        }
        let m = 0.5 * (t0 + t1);
        if !st.refine || depth >= MAX_T_DEPTH || st.work >= WORK_CAP || !(m > t0 && m < t1) {
            st.capped |= st.refine && (depth >= MAX_T_DEPTH || st.work >= WORK_CAP);
            for (s, o) in open.iter().enumerate() {
                if *o {
                    st.bound(s, e.upper(s));
                }
            }
            continue;
        }
        let em = eval(m, st)?;
        stack.push((m, t1, depth + 1, em, e1));
        stack.push((t0, m, depth + 1, e0, em));
    }
    Ok(())
}

/// The critical points of each coordinate on a sphere or torus: isolated
/// points `(u, v)`, and lines `v = const` (every u) with the sides they
/// count for and their value per side.
struct Critical {
    points: Vec<[f64; 2]>,
    lines: Vec<(f64, [Option<f64>; SIDES])>,
}

fn critical(geo: &Geo) -> Critical {
    let mut out = Critical {
        points: Vec::new(),
        lines: Vec::new(),
    };
    let line_value = |c: usize, v: f64, oc: f64, value: f64| {
        let mut sides = [None; SIDES];
        sides[c] = Some(-(oc + value));
        sides[c + 3] = Some(oc + value);
        (v, sides)
    };
    match geo {
        Geo::Sphere { f, r } => {
            for c in 0..3 {
                let (a, b, z) = (coord(f.x, c), coord(f.y, c), coord(f.z, c));
                let rho = libm::hypot(a, b);
                if rho > 0.0 {
                    let phi = libm::atan2(b, a);
                    let v = libm::atan2(z, rho);
                    out.points.push([phi, v]);
                    out.points.push([phi + PI, -v]);
                } else {
                    let oc = coord(f.origin, c);
                    out.lines.push(line_value(c, FRAC_PI_2, oc, *r * z));
                    out.lines.push(line_value(c, -FRAC_PI_2, oc, -*r * z));
                }
            }
        }
        Geo::Torus { f, big, r } => {
            for c in 0..3 {
                let (a, b, z) = (coord(f.x, c), coord(f.y, c), coord(f.z, c));
                let rho = libm::hypot(a, b);
                let oc = coord(f.origin, c);
                if rho > 0.0 {
                    let phi = libm::atan2(b, a);
                    let v1 = libm::atan2(z, rho);
                    let v2 = libm::atan2(z, -rho);
                    out.points.push([phi, v1]);
                    out.points.push([phi, v1 + PI]);
                    out.points.push([phi + PI, v2]);
                    out.points.push([phi + PI, v2 + PI]);
                } else {
                    out.lines.push(line_value(c, FRAC_PI_2, oc, *r * z));
                    out.lines.push(line_value(c, -FRAC_PI_2, oc, -*r * z));
                }
            }
            if *big < *r {
                // A spindle torus: its apexes, where the u derivative vanishes
                // on a whole line, are points on the axis.
                let v = libm::acos(-*big / *r);
                for v in [v, -v] {
                    let p = f.origin + (*r * libm::sin(v)) * f.z;
                    let mut sides = [None; SIDES];
                    for (s, x) in sides.iter_mut().enumerate() {
                        *x = Some(side_value(p, s));
                    }
                    out.lines.push((v, sides));
                }
            }
        }
        _ => {}
    }
    out
}

/// Counts the interior critical points of a sphere or torus.
fn bound_critical(
    g: &FaceGeo,
    cls: &Classifier<'_>,
    st: &mut State,
    row: &mut Row,
) -> Result<(), Refusal> {
    let crit = critical(&g.geo);
    let w = cls.window;
    let (tu, tv) = (cls.period_u, cls.period_v);
    for p in &crit.points {
        let mut inside = false;
        let mut maybe = false;
        for ku in shifts(tu, p[0], p[0], w[0][0], w[1][0]) {
            for kv in shifts(tv, p[1], p[1], w[0][1], w[1][1]) {
                let q = [
                    p[0] + tu.map_or(0.0, |t| ku as f64 * t),
                    p[1] + tv.map_or(0.0, |t| kv as f64 * t),
                ];
                match cls.classify(&[q, q]) {
                    Some(true) => inside = true,
                    None => maybe = true,
                    Some(false) => {}
                }
            }
        }
        if !(inside || maybe) {
            continue;
        }
        st.work += 1;
        let v = p[1].max(g.domain[1][0]).min(g.domain[1][1]);
        g.prepared.row(p[0], row);
        let ((x, _, _), err) = g.prepared.eval_bounded(row, v);
        let m = err[0] + 64.0 * EPS * (x.max_abs() + 1.0);
        let pt = [x.x, x.y, x.z];
        if inside {
            st.attain(pt, [m; 3]);
        }
        for s in 0..SIDES {
            st.bound(s, side_value(x, s) + m);
        }
    }
    for (v, sides) in &crit.lines {
        // Outer: the line meets the trims' v range; inner: a sample on it
        // lies inside.
        let reps: Vec<f64> = shifts(tv, *v, *v, w[0][1], w[1][1])
            .map(|k| *v + tv.map_or(0.0, |t| k as f64 * t))
            .collect();
        if reps.is_empty() {
            continue;
        }
        let mag = g.prepared_mag();
        let m = 64.0 * EPS * mag;
        let mut inside = false;
        'samples: for &vr in &reps {
            for i in 0..LINE_SAMPLES {
                let u = w[0][0] + (w[1][0] - w[0][0]) * (i as f64 + 0.5) / LINE_SAMPLES as f64;
                st.work += 1;
                if cls.classify(&[[u, vr], [u, vr]]) == Some(true) {
                    inside = true;
                    break 'samples;
                }
            }
        }
        for (s, x) in sides.iter().enumerate() {
            if let Some(x) = x {
                st.bound(s, x + m);
                if inside && x - m > st.sides.inner[s] {
                    st.sides.inner[s] = x - m;
                }
            }
        }
    }
    Ok(())
}

impl FaceGeo {
    /// Per axis, how far the surface can move over the trims' excursion
    /// outside its domain: twice the derivative bound times the excursion of
    /// the trims' control points per knot span (zero inside the domain).
    fn excursion_widening(&self, pieces: &[Piece]) -> [f64; 3] {
        let bounded = self.domain.iter().flatten().all(|x| x.is_finite())
            || matches!(self.geo, Geo::Sphere { .. });
        if !bounded {
            return [0.0; 3];
        }
        let mut delta = [0.0_f64; 2];
        for piece in pieces {
            for (t0, t1) in piece.intervals() {
                self.excursion(&piece.trim, t0, t1, 0, &mut delta);
            }
        }
        if delta == [0.0, 0.0] {
            return [0.0; 3];
        }
        let l = match &self.geo {
            Geo::Nurbs(n) => n.lipschitz(),
            Geo::Sphere { r, .. } => [[*r; 3], [*r; 3]],
            _ => [[f64::INFINITY; 3]; 2],
        };
        let mut w = [0.0; 3];
        for c in 0..3 {
            let mut x = 0.0;
            for d in 0..2 {
                if delta[d] > 0.0 {
                    x += 2.0 * l[d][c] * delta[d];
                }
            }
            w[c] = x * (1.0 + 1e-12);
        }
        w
    }

    /// Raises `delta` (per parameter) to a bound on how far the trim leaves
    /// the domain over `[t0, t1]`: its control points' excursion, bisected
    /// while that is more than twice the excursion of the interval's end
    /// points and above round-off.
    fn excursion(&self, t: &PreparedTrim, t0: f64, t1: f64, depth: u32, delta: &mut [f64; 2]) {
        let beyond = |p: [f64; 2], c: usize| {
            let [lo, hi] = self.domain[c];
            if lo.is_finite() && hi.is_finite() {
                (lo - p[c]).max(p[c] - hi).max(0.0)
            } else {
                0.0
            }
        };
        let pts = t.hull_points(t0, t1).unwrap_or_else(|| {
            let h = t.hull();
            vec![h[0], h[1]]
        });
        let mut hull = [0.0_f64; 2];
        for p in &pts {
            for c in 0..2 {
                hull[c] = hull[c].max(beyond(*p, c));
            }
        }
        if hull[0] <= delta[0] && hull[1] <= delta[1] {
            return;
        }
        let (a, b) = (t.eval(t0).0, t.eval(t1).0);
        let mut ends = [0.0_f64; 2];
        for c in 0..2 {
            ends[c] = beyond(a, c).max(beyond(b, c));
        }
        let m = 0.5 * (t0 + t1);
        let settled = (0..2).all(|c| {
            let [lo, hi] = self.domain[c];
            hull[c] <= 2.0 * ends[c] || hull[c] <= 1e-13 * (hi - lo).abs()
        });
        if settled || depth >= 48 || !(m > t0 && m < t1) {
            for c in 0..2 {
                delta[c] = delta[c].max(hull[c]);
            }
            return;
        }
        self.excursion(t, t0, m, depth + 1, delta);
        self.excursion(t, m, t1, depth + 1, delta);
    }

    fn prepared_mag(&self) -> f64 {
        match &self.geo {
            Geo::Sphere { f, r } => f.origin.max_abs() + r,
            Geo::Torus { f, big, r } => f.origin.max_abs() + big.abs() + r,
            _ => 0.0,
        }
    }
}

/// Refines the interior cells of a net over `window` (clamped to its
/// domain). With a classifier, cells outside the trims drop, cells inside
/// attain their corners, and monotone cells drop; without one (the
/// fallback), `window`'s edges are pieces of their own and only monotone
/// cells drop.
fn bound_cells(net: &Net, window: &UvBox, cls: Option<&Classifier<'_>>, st: &mut State) {
    let mut stack: Vec<(UvBox, u32)> = vec![(net.clamp(window), 0)];
    while let Some((cell, depth)) = stack.pop() {
        let su = net.spans_u(cell[0][0], cell[1][0]);
        let sv = net.spans_v(cell[0][1], cell[1][1]);
        let n = su.len() * sv.len();
        if n > 1 {
            if n <= SPLIT_SPANS {
                for &i in su.iter().rev() {
                    for &j in sv.iter().rev() {
                        let sub = [
                            [cell[0][0].max(net.ku[i]), cell[0][1].max(net.kv[j])],
                            [cell[1][0].min(net.ku[i + 1]), cell[1][1].min(net.kv[j + 1])],
                        ];
                        stack.push((sub, depth));
                    }
                }
                continue;
            }
            st.work += 1;
            let e = net.local_hull(&su, &sv);
            let open = st.settle(&e, [false; SIDES]);
            if !open.iter().any(|o| *o) {
                continue;
            }
            if st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH {
                st.capped = true;
                for (s, o) in open.iter().enumerate() {
                    if *o {
                        st.bound(s, e.upper(s));
                    }
                }
                continue;
            }
            // Split at the knot nearest the middle of the longer span run.
            let (dir, list, knots) = if su.len() >= sv.len() {
                (0, &su, &net.ku)
            } else {
                (1, &sv, &net.kv)
            };
            let k = list.len() / 2;
            let at = knots[list[k]];
            let mut lo = cell;
            let mut hi = cell;
            lo[1][dir] = at;
            hi[0][dir] = at;
            stack.push((hi, depth + 1));
            stack.push((lo, depth + 1));
            continue;
        }
        let (Some(&i), Some(&j)) = (su.first(), sv.first()) else {
            continue;
        };
        st.work += 1;
        let bz = net.bezier(i, j, &cell);
        let e = net.bezier_enc(&bz);
        // Sides whose coordinate is monotone over the cell, towards an edge
        // that is not a C0 knot line, have no extreme inside it.
        let mono = net.monotone(&bz);
        let mut skip = [false; SIDES];
        for (s, sk) in skip.iter_mut().enumerate() {
            let c = s % 3;
            let sign: i8 = if s < 3 { -1 } else { 1 };
            for (dir, c0) in [(0usize, &net.c0u), (1, &net.c0v)] {
                let d = mono[dir][c] * sign;
                if d == 0 {
                    continue;
                }
                let edge = if d > 0 { cell[1][dir] } else { cell[0][dir] };
                if !c0.contains(&edge) {
                    *sk = true;
                }
            }
        }
        let mut open = st.settle(&e, skip);
        if !open.iter().any(|o| *o) {
            continue;
        }
        // Only a cell that could still move a side is classified.
        let class = cls.map(|c| {
            st.work += 1;
            c.classify(&cell)
        });
        if class == Some(Some(false)) {
            continue;
        }
        let inside = class == Some(Some(true));
        if inside || st.loose {
            let (corners, m) = net.corners(&bz);
            for p in corners {
                if inside {
                    st.attain(p, m);
                } else {
                    st.reach(p);
                }
            }
            for (s, o) in open.iter_mut().enumerate() {
                if *o && e.upper(s) <= st.target(s) {
                    *o = false;
                    st.bound(s, e.upper(s));
                }
            }
            if !open.iter().any(|o| *o) {
                continue;
            }
        }
        let mu = 0.5 * (cell[0][0] + cell[1][0]);
        let mv = 0.5 * (cell[0][1] + cell[1][1]);
        let can_u = mu > cell[0][0] && mu < cell[1][0];
        let can_v = mv > cell[0][1] && mv < cell[1][1];
        if st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH || !(can_u || can_v) {
            st.capped |= st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH;
            for (s, o) in open.iter().enumerate() {
                if *o {
                    st.bound(s, e.upper(s));
                }
            }
            continue;
        }
        // Split along the parameter whose edge polygons are longer (the
        // polygons, not the corners: an edge may close on itself or shrink
        // to a pole).
        let span = |a: usize, b: usize| {
            let (x, y) = (bz.pts[a], bz.pts[b]);
            let (p, q) = (net.project(x), net.project(y));
            (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs()
        };
        let (p, q) = (net.p, net.q);
        let at = |m: usize, n: usize| m * (q + 1) + n;
        let lu: f64 = (0..p)
            .map(|m| span(at(m, 0), at(m + 1, 0)) + span(at(m, q), at(m + 1, q)))
            .sum();
        let lv: f64 = (0..q)
            .map(|n| span(at(0, n), at(0, n + 1)) + span(at(p, n), at(p, n + 1)))
            .sum();
        let split_u = can_u && (!can_v || lu >= lv);
        let (mut a, mut b) = (cell, cell);
        if split_u {
            a[1][0] = mu;
            b[0][0] = mu;
        } else {
            a[1][1] = mv;
            b[0][1] = mv;
        }
        stack.push((b, depth + 1));
        stack.push((a, depth + 1));
    }
}

/// The bracket of one face, refined until each side is within `eps` of the
/// larger of its attained value and `floor`'s.
///
/// # Errors
///
/// A typed [`Refusal`]: a malformed definition, a trim point outside a net's
/// domain or a value that is not finite.
fn bracket_face(
    face: &FaceDefinition,
    floor: [f64; SIDES],
    eps: f64,
    refine: bool,
) -> Result<FaceBox, Refusal> {
    face.validate()?;
    let g = FaceGeo::new(&face.surface);
    let periods = g.geo.periods();
    let ps = pieces(face, periods);
    let mut st = State::new(floor, eps);
    st.refine = refine;
    let mut row = Row::default();
    for piece in &ps {
        bound_piece(&g, piece, &mut st, &mut row)?;
    }
    let mut fallback = false;
    match &g.geo {
        Geo::Plane { .. } | Geo::Ruled { .. } => {}
        // A bilinear coordinate is harmonic: its extremes lie on the
        // boundary.
        Geo::Nurbs(_) if g.affine.is_some() => {}
        Geo::Sphere { .. } | Geo::Torus { .. } | Geo::Nurbs(_) if !refine => {
            // The first pass: the surface over the trims' box bounds the
            // interior.
            let (e, n) = g.geo.enclose(&pieces_box(&ps));
            st.work += n;
            for s in 0..SIDES {
                st.bound(s, e.upper(s));
            }
        }
        Geo::Sphere { .. } | Geo::Torus { .. }
            if !could_move(&g.geo.enclose(&pieces_box(&ps)).0, &mut st) => {}
        Geo::Sphere { .. } | Geo::Torus { .. } => {
            let classified = loops_pass(face, &g)
                .then(|| Classifier::new(&ps, periods))
                .flatten();
            match classified {
                Some(cls) => bound_critical(&g, &cls, &mut st, &mut row)?,
                None => {
                    fallback = true;
                    let w = pieces_box(&ps);
                    let (e, n) = g.geo.enclose(&w);
                    st.work += n;
                    for s in 0..SIDES {
                        st.bound(s, e.upper(s));
                    }
                }
            }
        }
        Geo::Nurbs(net) if !could_move(&net.enclose(&pieces_box(&ps)).0, &mut st) => {}
        Geo::Nurbs(net) => {
            let classified = loops_pass(face, &g)
                .then(|| Classifier::new(&ps, periods))
                .flatten();
            let w = pieces_box(&ps);
            match classified {
                Some(cls) => bound_cells(net, &w, Some(&cls), &mut st),
                None => {
                    fallback = true;
                    st.loose = true;
                    // The window's edges bound the fallback region.
                    let w = net.clamp(&w);
                    let corners = [
                        [w[0][0], w[0][1]],
                        [w[1][0], w[0][1]],
                        [w[1][0], w[1][1]],
                        [w[0][0], w[1][1]],
                    ];
                    for k in 0..4 {
                        let edge = line_piece(corners[k], corners[(k + 1) % 4]);
                        bound_piece(&g, &edge, &mut st, &mut row)?;
                    }
                    bound_cells(net, &w, None, &mut st);
                }
            }
        }
    }
    let w = g.excursion_widening(&ps);
    for s in 0..SIDES {
        st.sides.inner[s] -= w[s % 3];
        st.sides.outer[s] += w[s % 3];
    }
    if !st
        .sides
        .inner
        .iter()
        .chain(&st.sides.outer)
        .all(|x| !x.is_nan())
    {
        return Err(Refusal::new(RefusalCode::NonFinite, "face box not finite"));
    }
    let excursion = w[0].max(w[1]).max(w[2]);
    Ok(FaceBox {
        sides: st.sides,
        fallback,
        capped: st.capped,
        extrapolated: excursion > 0.0,
        excursion,
        work: st.work,
    })
}

/// Whether the surface over the trims' box (enclosure `e`) could move a side
/// beyond its target; when it cannot, `e` bounds every side and the face's
/// interior needs no classification.
fn could_move(e: &Enc, st: &mut State) -> bool {
    st.work += 1;
    if (0..SIDES).any(|s| e.upper(s) > st.target(s)) {
        return true;
    }
    for s in 0..SIDES {
        st.bound(s, e.upper(s));
    }
    false
}

fn pieces_box(ps: &[Piece]) -> UvBox {
    let mut w = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in ps {
        let h = p.trim.hull();
        for c in 0..2 {
            w[0][c] = w[0][c].min(h[0][c]);
            w[1][c] = w[1][c].max(h[1][c]);
        }
    }
    w
}

fn loops_pass(face: &FaceDefinition, g: &FaceGeo) -> bool {
    let trims: Vec<PreparedTrim> = face.trims.iter().map(PreparedTrim::new).collect();
    loops::check(face, &g.prepared, &trims).is_ok()
}

/// The bracket of one face on its own, each side refined until its width is
/// under `epsilon` (absolute, in millimetres).
///
/// # Errors
///
/// A typed [`Refusal`]: a malformed definition, a trim point outside a net's
/// domain or a value that is not finite.
pub fn face_box(face: &FaceDefinition, epsilon: f64) -> Result<FaceBox, Refusal> {
    if !(epsilon.is_finite() && epsilon > 0.0) {
        return Err(Refusal::new(
            RefusalCode::InvalidDefinition,
            "epsilon must be positive and finite",
        ));
    }
    bracket_face(face, [f64::NEG_INFINITY; SIDES], epsilon, true)
}

/// The box of `body` with its bracket.
///
/// # Errors
///
/// A typed [`Refusal`] from a face (attributed to it) or a malformed body.
pub fn body_box(body: &BodyDefinition, options: BoxOptions) -> Result<BodyBox, Refusal> {
    bodies_boxes(std::slice::from_ref(body), options)
        .pop()
        .unwrap_or_else(|| Err(Refusal::new(RefusalCode::InvalidDefinition, "no body")))
}

fn fb_eps(body: &BodyDefinition, accuracy: f64) -> f64 {
    accuracy * body.size
}

/// The boxes of every body, with every face of every body on one pool.
#[must_use]
pub fn bodies_boxes(
    bodies: &[BodyDefinition],
    options: BoxOptions,
) -> Vec<Result<BodyBox, Refusal>> {
    let index: Vec<(usize, usize)> = bodies
        .iter()
        .enumerate()
        .flat_map(|(b, body)| (0..body.faces.len()).map(move |f| (b, f)))
        .collect();
    let valid = |b: &BodyDefinition| -> Result<f64, Refusal> {
        b.validate()?;
        let a = options.accuracy;
        if !(a.is_finite() && a > 0.0) {
            return Err(Refusal::new(
                RefusalCode::InvalidDefinition,
                "accuracy must be positive and finite",
            ));
        }
        Ok(a * b.size)
    };
    // First pass: every face's pieces and interior enclosed once.
    let first = crate::mass::pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        let eps = match valid(body) {
            Ok(e) => e,
            Err(r) => return Some(Err(r)),
        };
        Some(bracket_face(face, [f64::NEG_INFINITY; SIDES], eps, false).map_err(|r| r.at_face(f)))
    });
    let mut floors: Vec<[f64; SIDES]> = vec![[f64::NEG_INFINITY; SIDES]; bodies.len()];
    for (&(b, _), r) in index.iter().zip(&first) {
        if let (Some(Some(Ok(fb))), Some(fl)) = (r, floors.get_mut(b)) {
            for k in 0..SIDES {
                fl[k] = fl[k].max(fb.sides.inner[k]);
            }
        }
    }
    // Second pass: the faces whose outer values could move a side, refined
    // against their body's first-pass values.
    let second = crate::mass::pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        let floor = *floors.get(b)?;
        match first.get(i)?.as_ref()? {
            Some(Ok(fb)) => {
                let eps = fb_eps(body, options.accuracy);
                if (0..SIDES).all(|s| fb.sides.outer[s] <= floor[s] + eps) {
                    return Some(Ok(*fb));
                }
                Some(bracket_face(face, floor, eps, true).map_err(|r| r.at_face(f)))
            }
            Some(Err(r)) => Some(Err(r.clone())),
            None => None,
        }
    });
    let mut results = second.into_iter();
    bodies
        .iter()
        .zip(&floors)
        .map(|(body, floor)| {
            let faces: Vec<Result<FaceBox, Refusal>> = (0..body.faces.len())
                .map(|f| {
                    results.next().flatten().flatten().unwrap_or_else(|| {
                        Err(Refusal::new(RefusalCode::NonFinite, "face task failed").at_face(f))
                    })
                })
                .collect();
            let epsilon = valid(body)?;
            let faces = faces.into_iter().collect::<Result<Vec<_>, _>>()?;
            let mut sides = Sides::EMPTY;
            sides.inner = *floor;
            let mut out = BodyBox {
                sides,
                bound: 0.0,
                epsilon,
                fallback_faces: Vec::new(),
                capped_faces: Vec::new(),
                extrapolated_faces: Vec::new(),
                work: 0,
            };
            for (f, fb) in faces.iter().enumerate() {
                for s in 0..SIDES {
                    out.sides.inner[s] = out.sides.inner[s].max(fb.sides.inner[s]);
                    out.sides.outer[s] = out.sides.outer[s].max(fb.sides.outer[s]);
                }
                if fb.fallback {
                    out.fallback_faces.push(f);
                }
                if fb.capped {
                    out.capped_faces.push(f);
                }
                if fb.extrapolated {
                    out.extrapolated_faces.push(f);
                }
                out.work += fb.work;
            }
            out.bound = out.sides.width();
            if !(out.bound.is_finite()
                && out
                    .sides
                    .inner
                    .iter()
                    .chain(&out.sides.outer)
                    .all(|x| x.is_finite()))
            {
                return Err(Refusal::new(RefusalCode::NonFinite, "body box not finite"));
            }
            Ok(out)
        })
        .collect()
}
