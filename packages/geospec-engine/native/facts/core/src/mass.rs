//! Area, volume and centre of mass with bounds, per face and per body.
//!
//! Per face (Green's theorem, as solidcore's `SmODEIsoCurveLength` and the
//! solidcore evaluation's patch 0001): along each trim piece `c(s) = (u, v)`,
//! the outer integrand is `-u'(s) F(u, v)` with `F(u, v)` the integral over
//! `[base_v, v]` of the surface integrand at fixed `u`. The surface integrand,
//! relative to the body centre, is the area element `|n|` with `n = S_u × S_v`,
//! the volume element `z n_z` (divergence of `(0, 0, z)`), the volume first
//! moments `x z n_z`, `y z n_z`, `z^2 n_z / 2`, and the closure terms (see
//! [`crate::gk::N`]): over a closed body `n_x`, `n_y`, `n_z`, `x n_z` and `y n_z`
//! integrate to zero and `x n_x`, `y n_y` to the same volume. A body bounding
//! material refuses when they do not, beyond their bounds.
//!
//! Each loop's pieces are checked to chain and close and its orientation to fit
//! its role ([`crate::loops`]); a gap between pieces above round-off is
//! integrated as a closing segment whose magnitude and bound go into the
//! face's bound, not its value.
//!
//! Inner integrals break at the surface's v knots; on planes, cylinders and
//! cones they are integrated in closed form (patch 0014's three-point rule,
//! exact for their polynomial integrands), and on spheres and ring tori by
//! exact trigonometric interpolation ([`trig_integral`]). Outer integrals
//! break at the trim's knots and its crossings of the surface's u knot lines
//! (patch 0001). Both levels are adaptive GK15 with the patch's tolerances
//! ([`crate::gk::integrate_breaks`]): a break where the integrand may be less
//! than C1 (a knot of multiplicity at least the degree minus one) ends a
//! panel, and the spans between the other breaks start grouped into panels
//! that are split at their soft breaks before any is accepted, so every
//! accepted panel lies within one span and its grouped parent gives its
//! second error estimate.
//!
//! On NURBS surfaces each inner node carries the evaluator's round-off bound
//! ([`crate::nurbs`]), propagated through the integrand's products, into the
//! face's bound; a surface whose weights vary by more than
//! [`MAX_WEIGHT_RATIO`] within one span refuses as ill-conditioned before it
//! is integrated.
//!
//! Faces run on `threads` scoped threads and fold in face order: the facts
//! are bit-identical for every thread count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::accuracy::{FactAccuracy, Method, Scale};
use crate::definition::{BodyDefinition, FaceDefinition, Material, Orientation, Surface};
use crate::definition::{Curve2, TrimPiece};
use crate::gk::{
    integrate_breaks, Piece, AREA, CLOSURE, N, NODES, NX, NY, V, VOL, VOL_X, VOL_Y, XNZ, YNZ,
};
use crate::loops;
use crate::nurbs::{EvalBound, MAX_WEIGHT_RATIO};
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::{Breaks, PreparedTrim};
use crate::vec::Vec3;

/// A normal shorter than this fraction of `|S_u| |S_v|` (the sine of the
/// angle between them), squared, is degenerate for the closed-form rule.
const REL_ZERO_SQ: f64 = 1e-24;
/// The round-off allowance per value, in units of the unit round-off.
const ROUNDOFF_ULPS: f64 = 64.0;
/// The round-off of a surface point's coordinates relative to the centre, in
/// units of the unit round-off times the coordinates' magnitude (a point is a
/// few products and sums).
const COORD_ULPS: f64 = 8.0;
/// Without a file tolerance, the largest part of the volume that a body's
/// non-closure (beyond the integration bounds) may widen its volume bound by
/// before the body refuses as open: modelling gaps give about 1e-8, a missing
/// face about 1.
const OPEN_FRACTION: f64 = 1e-4;
/// With a file tolerance `t`, the widening may reach this multiple of the gap
/// volume `t` times the area before the body refuses as open.
const GAP_MULTIPLE: f64 = 4.0;
/// How far, relative to the domain's width, a trim may leave its surface's
/// parameter domain before it refuses. Beyond round-off the surface would be
/// evaluated (clamped) where it is not defined.
const DOMAIN_SLACK: f64 = 1e-12;
/// The largest trim degree whose outer integrand on a plane (degree
/// `4 d - 1`) the Kronrod 15-point rule integrates exactly (degree 22).
const EXACT_TRIM_DEGREE: usize = 5;

/// Integration settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The relative accuracy each piece integrates to (the inner integrals to
    /// a tenth of it); the solidcore evaluation's default is 1e-7.
    pub accuracy: f64,
    /// Threads for the per-face tasks (1 runs on the caller's thread).
    pub threads: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            accuracy: 1e-7,
            threads: 1,
        }
    }
}

/// One face's raw integrals before orientation and material signs, with
/// their bounds, about the body centre, in the order of [`crate::gk::N`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMass {
    /// The integrals.
    pub raw: [f64; N],
    /// An absolute bound on each integral's error.
    pub bound: [f64; N],
    /// Every integral exact up to round-off.
    pub exact: bool,
    /// Outer and inner panels integrated.
    pub panels: usize,
}

/// A scalar fact and its accuracy record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fact {
    /// The value.
    pub value: f64,
    /// Its accuracy record.
    pub accuracy: FactAccuracy,
}

/// A position fact: the point, a bound per axis, and the accuracy record
/// whose bound is the largest of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionFact {
    /// The point.
    pub value: Vec3,
    /// The bound on each coordinate.
    pub axis_bounds: Vec3,
    /// The record (bound: the largest axis bound).
    pub accuracy: FactAccuracy,
}

/// A body's facts.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyFacts {
    /// The sum of the faces' areas.
    pub area: Fact,
    /// The volume bounded by the faces that bound material.
    pub volume: Fact,
    /// The centre of mass; `None` without volume (a sheet body).
    pub centroid: Option<PositionFact>,
    /// The closure integral of `n_z` and its bound: zero for a closed body.
    pub closure: [f64; 2],
    /// Every closure term within its integration bound and the three volume
    /// forms agreeing within theirs (always true without volume). When false
    /// the body closes only to its modelling tolerance and the volume bound
    /// includes the forms' disagreement.
    pub closed_within_bounds: bool,
    /// Per face, in order.
    pub faces: Vec<FaceMass>,
}

fn add(a: &mut V, b: &V) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y;
    }
}

/// The powers of the length each component scales with beyond area.
const POWERS: [u8; N] = [0, 1, 2, 2, 2, 0, 1, 1, 0, 0, 1, 1];

/// The round-off allowance of an integral whose integrand's absolute integral
/// (QUADPACK's `resabs`) is `resabs`: `64 eps` times it, plus `8 eps` times
/// the area's at the component's length scale, for the coordinates' own
/// round-off relative to the centre (`length`: the size plus the centre's
/// largest coordinate).
fn roundoff(resabs: &V, length: f64, size: f64) -> V {
    let mut out = [0.0; N];
    let area = resabs[AREA];
    let scale = [1.0, length, length * size];
    for ((o, x), p) in out.iter_mut().zip(resabs).zip(POWERS) {
        let s = scale.get(usize::from(p)).copied().unwrap_or(1.0);
        *o = f64::EPSILON * (ROUNDOFF_ULPS * x + COORD_ULPS * area * s);
    }
    out
}

/// The surface integrand at `(p - centre, S_u, S_v)`.
fn integrand(p: Vec3, su: Vec3, sv: Vec3) -> V {
    let n = su.cross(sv);
    let vol = p.z * n.z;
    [
        n.length(),
        vol,
        p.x * vol,
        p.y * vol,
        p.z * vol / 2.0,
        n.z,
        p.x * n.x,
        p.y * n.y,
        n.x,
        n.y,
        p.x * n.z,
        p.y * n.z,
    ]
}

/// A bound on the error of [`integrand`] at `(p - centre, S_u, S_v)` from
/// the evaluator's round-off `bound` (point, `S_u`, `S_v`, max norm): each
/// component's factors are taken at their magnitudes plus their errors, and
/// the bound is the growth of the product, expanded so that it does not
/// cancel.
fn integrand_error(p: Vec3, su: Vec3, sv: Vec3, bound: EvalBound) -> V {
    let [ep, eu, ev] = bound;
    if ep == 0.0 && eu == 0.0 && ev == 0.0 {
        return [0.0; N];
    }
    let n = su.cross(sv);
    let l1 = |x: Vec3| x.x.abs() + x.y.abs() + x.z.abs();
    // The cross product's error per coordinate.
    let en = eu * l1(sv) + l1(su) * ev + 2.0 * eu * ev;
    // The product of factors `(value, error)`: its magnitude and error.
    let prod = |fs: &[(f64, f64)]| {
        let (mut m, mut e) = (1.0_f64, 0.0_f64);
        for &(x, ex) in fs {
            e = e * (x.abs() + ex) + m * ex;
            m *= x.abs();
        }
        e
    };
    let (x, y, z) = ((p.x, ep), (p.y, ep), (p.z, ep));
    let (nx, ny, nz) = ((n.x, en), (n.y, en), (n.z, en));
    [
        // |n| in the Euclidean norm: sqrt(3) times the max-norm error.
        2.0 * en,
        prod(&[z, nz]),
        prod(&[x, z, nz]),
        prod(&[y, z, nz]),
        0.5 * prod(&[z, z, nz]),
        en,
        prod(&[x, nx]),
        prod(&[y, ny]),
        en,
        en,
        prod(&[x, nz]),
        prod(&[y, nz]),
    ]
}

/// The surface integrand at `vs` on the row at `u`, each node with the
/// evaluator's round-off bound.
fn surface_nodes(
    surface: &Prepared,
    row: &Row,
    centre: Vec3,
    u: f64,
    vs: &[f64; NODES],
    ys: &mut [V; NODES],
    es: &mut [V; NODES],
) -> Result<(), Refusal> {
    for ((&v, y), e) in vs.iter().zip(ys.iter_mut()).zip(es.iter_mut()) {
        let ((s, su, sv), b) = surface.eval_bounded(row, v);
        *y = integrand(s - centre, su, sv);
        *e = integrand_error(s - centre, su, sv, b);
        if !e.iter().all(|x| x.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::IllConditionedSurface,
                format!("the surface's round-off bound is not finite at ({u}, {v})"),
            ));
        }
    }
    Ok(())
}

/// The inner integral over a v segment of length `l` where the surface is
/// affine in v, `S(m + t) - centre = pm + t sv`, and `S_u(m + t) = su + t dsu`
/// with `dsu` parallel to `su` or zero (planes and cylinders: zero; cones: the
/// radius varies linearly). The normal keeps its direction and its length is
/// linear, so the integrand is a polynomial of degree at most 3 and
/// three-point Gauss-Legendre is exact. Returns the integral and the integral
/// of its absolute value; `None` where the normal is degenerate (relative to
/// `|S_u| |S_v|`) or turns over within the segment.
fn ruled_integral(pm: Vec3, su: Vec3, dsu: Vec3, sv: Vec3, l: f64) -> Option<(V, V)> {
    let h = 0.5 * l;
    let (su_lo, su_hi) = (su - h * dsu, su + h * dsu);
    let (n_lo, n_hi) = (su_lo.cross(sv), su_hi.cross(sv));
    let sv2 = sv.length_squared();
    if !(n_lo.length_squared() > REL_ZERO_SQ * su_lo.length_squared() * sv2
        && n_hi.length_squared() > REL_ZERO_SQ * su_hi.length_squared() * sv2
        && n_lo.dot(n_hi) > 0.0)
    {
        return None;
    }
    let r = (0.6_f64).sqrt() * h;
    let mut out = [0.0; N];
    let mut abs = [0.0; N];
    for (t, w) in [(-r, 5.0 / 9.0), (0.0, 8.0 / 9.0), (r, 5.0 / 9.0)] {
        let y = integrand(pm + t * sv, su + t * dsu, sv);
        for ((o, a), yi) in out.iter_mut().zip(abs.iter_mut()).zip(y) {
            *o += w * yi;
            *a += w * yi.abs();
        }
    }
    for o in &mut out {
        *o *= h;
    }
    for a in &mut abs {
        *a *= h.abs();
    }
    out.iter().all(|x| x.is_finite()).then_some((out, abs))
}

/// Interpolation nodes per period of [`trig_integral`]: `2 TRIG_DEGREE + 1`.
const TRIG_NODES: usize = 9;
/// The largest degree in `cos v` and `sin v` of the surface integrand on a
/// sphere or a ring torus (the moments: a point of degree 1 times a point
/// of degree 1 times a normal of degree 2).
const TRIG_DEGREE: usize = 4;

/// On spheres and ring tori (major radius at least the minor), whose
/// integrand at fixed u is a trigonometric polynomial of degree at most
/// [`TRIG_DEGREE`] in v, the frame's largest deviation from orthonormal (the
/// Gram matrix's, with round-off) and the largest `|n|` over the surface;
/// `None` elsewhere.
fn trig_surface(surface: &Prepared) -> Option<(f64, f64)> {
    let (frame, top) = match surface {
        Prepared::Sphere { frame, radius } => (frame, radius * radius),
        Prepared::Torus {
            frame,
            major,
            minor,
        } if *major >= *minor && *minor > 0.0 => (frame, minor * (major + minor)),
        _ => return None,
    };
    let (x, y, z) = (frame.x, frame.y, frame.z);
    let gram = [
        (x.dot(x) - 1.0).abs(),
        (y.dot(y) - 1.0).abs(),
        (z.dot(z) - 1.0).abs(),
        x.dot(y).abs(),
        x.dot(z).abs(),
        y.dot(z).abs(),
    ];
    let deviation = gram.iter().fold(0.0_f64, |m, g| m.max(*g)) + 8.0 * f64::EPSILON;
    let grown = 1.0 + 4.0 * deviation;
    (deviation.is_finite() && deviation < 1e-6 && top.is_finite())
        .then_some((deviation, top * grown * grown))
}

/// The inner integral over `[lo, hi]` on a sphere or ring torus
/// ([`trig_surface`]: `deviation`, `top`), exactly up to round-off: the
/// integrand is interpolated by a trigonometric polynomial of degree
/// [`TRIG_DEGREE`] through [`TRIG_NODES`] equispaced nodes around the whole
/// period from the interval's midpoint, and the interpolant integrated in
/// closed form (`cosines`: `cos(2 pi k j / 9)`). Every component but the area
/// is such a polynomial for any frame. The area element `|n|` is one for an
/// orthonormal frame (on a sphere signed as `cos v`, so that nodes past the
/// poles continue it); within `deviation` of orthonormal it differs by at
/// most `8 deviation top`, whose interpolated integral is bounded through
/// the weights' magnitudes and added to the area's error. Returns the
/// integral, a round-off scale (the largest weight's magnitude times the sum
/// of the values' magnitudes) and the area's interpolation bound; `None` on
/// a sphere where the interval leaves `[-pi/2, pi/2]`.
#[allow(clippy::too_many_arguments)]
fn trig_integral(
    surface: &Prepared,
    row: &Row,
    centre: Vec3,
    lo: f64,
    hi: f64,
    cosines: &[[f64; TRIG_DEGREE]; TRIG_NODES],
    deviation: f64,
    top: f64,
) -> Option<(V, V, f64)> {
    let sphere = matches!(surface, Prepared::Sphere { .. });
    let half = std::f64::consts::FRAC_PI_2;
    if sphere && !(lo.abs() <= half && hi.abs() <= half) {
        return None;
    }
    let m = 0.5 * (lo + hi);
    let h = 0.5 * (hi - lo);
    let mut sines = [0.0; TRIG_DEGREE];
    let mut biggest = h.abs();
    for (k, s) in sines.iter_mut().enumerate() {
        let kf = (k + 1) as f64;
        *s = libm::sin(kf * h) / kf;
        biggest += 2.0 * s.abs();
    }
    // The nodes' own round-off moves the values by up to about
    // `eps (|m| + 2 pi)` times their derivative (at most the degree times
    // their magnitude): the scale grows with it.
    let biggest =
        2.0 / (TRIG_NODES as f64) * biggest * (1.0 + m.abs() + 2.0 * std::f64::consts::PI);
    let mut out = [0.0; N];
    let mut mags = [0.0; N];
    let mut weights = 0.0;
    for (j, cs) in cosines.iter().enumerate() {
        let mut w = h;
        for (c, s) in cs.iter().zip(&sines) {
            w += 2.0 * c * s;
        }
        let w = 2.0 / (TRIG_NODES as f64) * w;
        weights += w.abs();
        let v = m + 2.0 * std::f64::consts::PI * (j as f64) / (TRIG_NODES as f64);
        let (p, su, sv) = surface.eval(row, v);
        let mut y = integrand(p - centre, su, sv);
        if sphere && libm::cos(v) < 0.0 {
            y[AREA] = -y[AREA];
        }
        for ((o, a), yi) in out.iter_mut().zip(mags.iter_mut()).zip(y) {
            *o += w * yi;
            *a += yi.abs();
        }
    }
    for a in &mut mags {
        *a *= biggest;
    }
    let area_error = (weights + 2.0 * h.abs()) * 8.0 * deviation * top;
    (out.iter().chain(mags.iter()).all(|x| x.is_finite()) && area_error.is_finite())
        .then_some((out, mags, area_error))
}

/// `cos(2 pi k j / 9)` for `j` in `0..9`, `k` in `1..=4`.
fn trig_cosines() -> [[f64; TRIG_DEGREE]; TRIG_NODES] {
    let mut out = [[0.0; TRIG_DEGREE]; TRIG_NODES];
    for (j, row) in out.iter_mut().enumerate() {
        for (k, c) in row.iter_mut().enumerate() {
            let a = 2.0 * std::f64::consts::PI * ((k + 1) * j) as f64 / (TRIG_NODES as f64);
            *c = libm::cos(a);
        }
    }
    out
}

/// Into `out`: `(a, hard)`, the `lines` strictly between `a` and `b` in the
/// direction from `a` to `b`, and `(b, hard)`.
fn lines_between(lines: &[(f64, bool)], a: f64, b: f64, out: &mut Vec<(f64, bool)>) {
    out.clear();
    out.push((a, true));
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let first = lines.partition_point(|l| l.0 <= lo);
    let inside = lines
        .get(first..)
        .unwrap_or_default()
        .iter()
        .take_while(|l| l.0 < hi);
    if a <= b {
        out.extend(inside);
    } else {
        let at = out.len();
        out.extend(inside);
        out.get_mut(at..).unwrap_or_default().reverse();
    }
    out.push((b, true));
}

fn outside(x: f64, [lo, hi]: [f64; 2]) -> bool {
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let slack = DOMAIN_SLACK * (hi - lo);
    x < lo - slack || x > hi + slack
}

/// What every trim piece of one face integrates against.
struct Context<'a> {
    surface: &'a Prepared,
    domain: [[f64; 2]; 2],
    base_v: f64,
    centre: Vec3,
    size: f64,
    length: f64,
    accuracy: f64,
    outer_floor: f64,
    inner_floor: f64,
    plane: bool,
    /// [`trig_surface`], and the cosines of [`trig_integral`].
    trig: Option<(f64, f64)>,
    cosines: [[f64; TRIG_DEGREE]; TRIG_NODES],
}

/// One trim piece's integral (signed by its direction), bound, exactness and
/// panel count.
struct TrimMass {
    value: V,
    bound: V,
    exact: bool,
    panels: usize,
}

/// One trim piece's integral; with `probe`, only its outer nodes that every
/// integration evaluates (each run's first estimate and its splits at soft
/// breaks, which do not depend on the integrand), checked as the integration
/// checks them, with a zero integrand.
fn integrate_trim(cx: &Context<'_>, trim: &PreparedTrim, probe: bool) -> Result<TrimMass, Refusal> {
    let surface = cx.surface;
    let domain = cx.domain;
    let exact_outer = cx.plane
        && trim
            .polynomial_degree()
            .is_some_and(|d| d <= EXACT_TRIM_DEGREE);
    let mut out = TrimMass {
        value: [0.0; N],
        bound: [0.0; N],
        exact: exact_outer,
        panels: 0,
    };
    let mut row = Row::default();
    let Breaks {
        at: breaks,
        hard,
        chords,
    } = trim.breaks(surface.u_lines())?;
    // The outer integrand is smooth only between the trim's knots and its
    // crossings of the u knot lines: soft ones (where it stays C1) may fall
    // inside a panel, hard ones end one. The trim is integrated in runs
    // between hard breaks; a run of soft breaks starts as one panel and is
    // split at every soft break before a panel is accepted.
    let mut ends: Vec<(f64, bool)> = Vec::new();
    let mut run: Vec<(f64, bool)> = Vec::new();
    for (i, (&t, &h)) in breaks.iter().zip(&hard).enumerate() {
        run.push((t, h || i + 1 == breaks.len()));
        let Some(&(a, _)) = run.first() else { continue };
        if run.len() < 2 || !(h || i + 1 == breaks.len()) {
            continue;
        }
        if t.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
            run.clear();
            run.push((t, true));
            continue;
        }
        let mut inner_panels = 0;
        let all_closed = std::cell::Cell::new(true);
        let mut outer =
            |xs: &[f64; NODES], ys: &mut [V; NODES], es: &mut [V; NODES]| -> Result<(), Refusal> {
                for ((&s, y), e) in xs.iter().zip(ys.iter_mut()).zip(es.iter_mut()) {
                    let (p, d) = trim.eval(s);
                    if !(p.iter().chain(d.iter()).all(|x| x.is_finite())) {
                        return Err(Refusal::new(
                            RefusalCode::NonFinite,
                            format!("trim not finite at {s}"),
                        ));
                    }
                    if outside(p[0], domain[0]) || outside(p[1], domain[1]) {
                        return Err(Refusal::new(
                            RefusalCode::TrimOutsideDomain,
                            format!(
                                "trim point ({}, {}) outside the surface's domain {domain:?}",
                                p[0], p[1]
                            ),
                        ));
                    }
                    if probe || d[0] == 0.0 {
                        *y = [0.0; N];
                        *e = [0.0; N];
                        continue;
                    }
                    surface.row(p[0], &mut row);
                    let row = &row;
                    let mut sum = [0.0; N];
                    let mut err = [0.0; N];
                    let closed =
                        surface
                            .linear_v(row, cx.base_v, p[1])
                            .and_then(|(pm, su, dsu, sv)| {
                                ruled_integral(pm - cx.centre, su, dsu, sv, p[1] - cx.base_v)
                            });
                    let trig = match (closed.is_none(), cx.trig) {
                        (true, Some((deviation, top))) => trig_integral(
                            surface,
                            row,
                            cx.centre,
                            cx.base_v,
                            p[1],
                            &cx.cosines,
                            deviation,
                            top,
                        ),
                        _ => None,
                    };
                    if let Some((piece, abs)) = closed {
                        add(&mut sum, &piece);
                        add(&mut err, &roundoff(&abs, cx.length, cx.size));
                    } else if let Some((piece, mags, area_error)) = trig {
                        add(&mut sum, &piece);
                        add(&mut err, &roundoff(&mags, cx.length, cx.size));
                        err[AREA] += area_error;
                        all_closed.set(false);
                    } else {
                        all_closed.set(false);
                        lines_between(surface.v_lines(), cx.base_v, p[1], &mut ends);
                        let mut inner = |vs: &[f64; NODES],
                                         ys: &mut [V; NODES],
                                         es: &mut [V; NODES]|
                         -> Result<(), Refusal> {
                            surface_nodes(surface, row, cx.centre, p[0], vs, ys, es)
                        };
                        let piece = integrate_breaks(
                            &mut inner,
                            &ends,
                            cx.accuracy / 10.0,
                            cx.size,
                            cx.inner_floor,
                            200,
                            false,
                        )?;
                        inner_panels += piece.panels;
                        add(&mut sum, &piece.value);
                        add(&mut err, &piece.estimate);
                        add(&mut err, &piece.carried);
                        add(&mut err, &roundoff(&piece.resabs, cx.length, cx.size));
                    }
                    let du = d[0];
                    for ((yi, ei), (si, ri)) in y
                        .iter_mut()
                        .zip(e.iter_mut())
                        .zip(sum.iter().zip(err.iter()))
                    {
                        *yi = -(*si * du);
                        *ei = *ri * du.abs();
                    }
                }
                Ok(())
            };
        let mut piece = integrate_breaks(
            &mut outer,
            &run,
            cx.accuracy,
            cx.size,
            cx.outer_floor,
            400,
            exact_outer,
        )?;
        // The outer integrand is a polynomial only where every inner integral
        // had its closed form; otherwise integrate again as adaptive (bisected
        // at least once).
        if exact_outer && !all_closed.get() && !probe {
            piece = integrate_breaks(
                &mut outer,
                &run,
                cx.accuracy,
                cx.size,
                cx.outer_floor,
                400,
                false,
            )?;
        }
        run.clear();
        run.push((t, true));
        let Piece {
            value,
            estimate,
            carried,
            resabs,
            panels: outer_panels,
        } = piece;
        out.exact &= all_closed.get();
        out.panels += outer_panels + inner_panels;
        let sign = if trim.reversed { -1.0 } else { 1.0 };
        for (t, v) in out.value.iter_mut().zip(value) {
            *t += sign * v;
        }
        add(&mut out.bound, &estimate);
        add(&mut out.bound, &carried);
        add(&mut out.bound, &roundoff(&resabs, cx.length, cx.size));
    }
    // Intervals too short to step stand as chords: their integrals go into
    // the bound, as a gap's do.
    for (p, q) in chords {
        let m = integrate_trim(cx, &line_trim(p, q), probe)?;
        for ((b, v), mb) in out.bound.iter_mut().zip(m.value).zip(m.bound) {
            *b += v.abs() + mb;
        }
        out.exact = false;
        out.panels += m.panels;
    }
    Ok(out)
}

/// The straight piece from `p` to `q`.
fn line_trim(p: [f64; 2], q: [f64; 2]) -> PreparedTrim {
    PreparedTrim::new(&TrimPiece {
        curve: Curve2::Line {
            origin: p,
            direction: [q[0] - p[0], q[1] - p[1]],
        },
        range: [0.0, 1.0],
        reversed: false,
    })
}

/// One face's integrals about `centre`, for a body of size `size`.
///
/// # Errors
///
/// A typed [`Refusal`]: an invalid definition, an ill-conditioned rational
/// surface, trim loops that do not chain, close or fit their orientation, a
/// trim outside the surface's domain, a non-finite integrand, no
/// convergence.
pub fn face_mass(
    face: &FaceDefinition,
    centre: Vec3,
    size: f64,
    accuracy: f64,
) -> Result<FaceMass, Refusal> {
    face.validate()?;
    if !(accuracy.is_finite() && accuracy > 0.0 && size.is_finite() && size > 0.0) {
        return Err(Refusal::new(
            RefusalCode::InvalidDefinition,
            "accuracy and size must be positive and finite",
        ));
    }
    let surface = Prepared::new(&face.surface);
    let ratio = surface.weight_ratio();
    if ratio.is_nan() || ratio > MAX_WEIGHT_RATIO {
        return Err(Refusal::new(
            RefusalCode::IllConditionedSurface,
            format!("the surface's weights vary by {ratio:e} within one span (above {MAX_WEIGHT_RATIO:e})"),
        ));
    }
    let trims: Vec<PreparedTrim> = face.trims.iter().map(PreparedTrim::new).collect();
    let checked = loops::check(face, &surface, &trims)?;
    let gaps: Vec<PreparedTrim> = checked
        .gaps
        .iter()
        .map(|(p, q)| line_trim(*p, *q))
        .collect();
    let domain = surface.domain();
    let (mut v0, mut u0, mut u1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for t in trims.iter().chain(&gaps) {
        let [lo, hi] = t.hull();
        v0 = v0.min(lo[1]);
        u0 = u0.min(lo[0]);
        u1 = u1.max(hi[0]);
    }
    let [_, [dv0, _]] = domain;
    let base_v = if v0 >= dv0 { v0 } else { dv0 };
    let u_width = (u1 - u0).max(0.0);
    let outer_floor = 1e-3 * accuracy * size * size;
    let cx = Context {
        surface: &surface,
        domain,
        base_v,
        centre,
        size,
        length: size + centre.max_abs(),
        accuracy,
        outer_floor,
        inner_floor: outer_floor / u_width.max(f64::MIN_POSITIVE),
        plane: matches!(face.surface, Surface::Plane { .. }),
        trig: trig_surface(&surface),
        cosines: trig_cosines(),
    };
    // A trim outside the domain refuses before any integral: every node
    // the probe checks, the integration would check too, so only a face that
    // refuses anyway can refuse here (at most for another reason found
    // first). On a domain unbounded both ways nothing is outside.
    let bounded = domain.iter().any(|d| d.iter().all(|x| x.is_finite()));
    if bounded {
        for trim in trims.iter().chain(&gaps) {
            integrate_trim(&cx, trim, true)?;
        }
    }
    let mut total = [0.0; N];
    let mut bound = [0.0; N];
    let mut panels = 0;
    let mut exact = cx.plane;
    for trim in &trims {
        let m = integrate_trim(&cx, trim, false)?;
        add(&mut total, &m.value);
        add(&mut bound, &m.bound);
        exact &= m.exact;
        panels += m.panels;
    }
    for gap in &gaps {
        let m = integrate_trim(&cx, gap, false)?;
        for ((b, v), mb) in bound.iter_mut().zip(m.value).zip(m.bound) {
            *b += v.abs() + mb;
        }
        exact = false;
        panels += m.panels;
    }
    if !(total.iter().chain(bound.iter()).all(|x| x.is_finite())) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "face integrals not finite",
        ));
    }
    Ok(FaceMass {
        raw: total,
        bound,
        exact,
        panels,
    })
}

/// Runs `job(i)` for `i in 0..n` on up to `threads` threads, taking indices
/// in order; results come back in index order whatever the thread count. The
/// caller's thread works too, so a thread that cannot be spawned (resource
/// exhaustion, a target without threads) only means fewer workers.
pub(crate) fn pool<T: Send, F>(n: usize, threads: usize, job: F) -> Vec<Option<T>>
where
    F: Fn(usize) -> T + Sync,
{
    let threads = threads.min(n).max(1);
    if threads == 1 {
        return (0..n).map(|i| Some(job(i))).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let work = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        let Some(slot) = slots.get(i) else { break };
        let r = job(i);
        if let Ok(mut g) = slot.lock() {
            *g = Some(r);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads {
            if std::thread::Builder::new()
                .spawn_scoped(scope, work)
                .is_err()
            {
                break;
            }
        }
        work();
    });
    slots
        .into_iter()
        .map(|m| m.into_inner().ok().flatten())
        .collect()
}

/// The facts of `body`.
///
/// # Errors
///
/// A typed [`Refusal`] from a face (attributed to it) or from a body
/// invariant: a negative face area beyond its bound, a volume term larger than
/// the face's area times the body size, a volume that is not positive, larger
/// than the body box, or a centre of mass outside it.
pub fn body_facts(body: &BodyDefinition, options: Options) -> Result<BodyFacts, Refusal> {
    bodies_facts(std::slice::from_ref(body), options)
        .pop()
        .unwrap_or_else(|| Err(Refusal::new(RefusalCode::InvalidDefinition, "no body")))
}

/// The facts of every body, with every face of every body on one pool.
#[must_use]
pub fn bodies_facts(
    bodies: &[BodyDefinition],
    options: Options,
) -> Vec<Result<BodyFacts, Refusal>> {
    let index: Vec<(usize, usize)> = bodies
        .iter()
        .enumerate()
        .flat_map(|(b, body)| (0..body.faces.len()).map(move |f| (b, f)))
        .collect();
    // A body refuses with its first face's refusal, so a face after one
    // known to refuse is not integrated: per body, the first face known to
    // refuse so far. Which faces are skipped depends on the thread count;
    // the results do not.
    let refused: Vec<AtomicUsize> = bodies
        .iter()
        .map(|_| AtomicUsize::new(usize::MAX))
        .collect();
    let mut results = pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        let first = refused.get(b)?;
        if first.load(Ordering::Relaxed) < f {
            return None;
        }
        let r = body
            .validate()
            .and_then(|()| face_mass(face, body.centre, body.size, options.accuracy))
            .map_err(|r| r.at_face(f));
        if r.is_err() {
            first.fetch_min(f, Ordering::Relaxed);
        }
        Some(r)
    })
    .into_iter();
    bodies
        .iter()
        .map(|body| {
            // A skipped face follows a refusing one, whose refusal comes
            // first.
            let masses: Vec<Result<FaceMass, Refusal>> = (0..body.faces.len())
                .map(|f| {
                    results.next().flatten().flatten().unwrap_or_else(|| {
                        Err(Refusal::new(RefusalCode::NonFinite, "face task failed").at_face(f))
                    })
                })
                .collect();
            body.validate()?;
            let masses = masses.into_iter().collect::<Result<Vec<_>, _>>()?;
            fold(body, masses)
        })
        .collect()
}

/// The faces' signs as their volume terms count.
fn signs(orientation: Orientation, material: Material) -> &'static [f64] {
    let s = match orientation {
        Orientation::Forward => 1.0,
        Orientation::Reversed => -1.0,
    };
    match (material, s > 0.0) {
        (Material::OneSide, true) => &[1.0],
        (Material::OneSide, false) => &[-1.0],
        (Material::BothSides, true) => &[1.0, -1.0],
        (Material::BothSides, false) => &[-1.0, 1.0],
        (Material::Neither, _) => &[],
    }
}

fn open_body(detail: String) -> Refusal {
    Refusal::new(RefusalCode::OpenBody, detail)
}

/// Folds the faces' integrals in face order and checks the body invariants.
#[allow(clippy::too_many_lines)]
fn fold(body: &BodyDefinition, masses: Vec<FaceMass>) -> Result<BodyFacts, Refusal> {
    let size = body.size;
    let reach = body.reach();
    let (mut area, mut area_b, mut area_abs) = (0.0, 0.0, 0.0);
    let mut sum = [0.0; N];
    let mut sum_b = [0.0; N];
    let mut sum_abs = [0.0; N];
    let mut terms = 0usize;
    let mut bounding = false;
    let mut exact = true;
    for (f, (m, face)) in masses.iter().zip(&body.faces).enumerate() {
        exact &= m.exact;
        if m.raw[AREA] < -m.bound[AREA] {
            return Err(Refusal::new(
                RefusalCode::TrimOrientation,
                format!("signed area {} below -{}", m.raw[AREA], m.bound[AREA]),
            )
            .at_face(f));
        }
        if m.raw[VOL].abs() > m.raw[AREA].abs() * size + m.bound[VOL] {
            return Err(Refusal::new(
                RefusalCode::FaceTermExceedsArea,
                format!(
                    "volume term {} exceeds area {} times size {size}",
                    m.raw[VOL], m.raw[AREA]
                ),
            )
            .at_face(f));
        }
        area += m.raw[AREA].abs();
        area_b += m.bound[AREA];
        area_abs += m.raw[AREA].abs();
        for &s in signs(face.orientation, face.material) {
            bounding = true;
            terms += 1;
            let parts = sum.iter_mut().zip(sum_b.iter_mut()).zip(sum_abs.iter_mut());
            for (((acc, acc_b), acc_abs), (r, rb)) in
                parts.zip(m.raw.iter().zip(m.bound.iter())).skip(1)
            {
                *acc += s * r;
                *acc_b += rb;
                *acc_abs += r.abs();
            }
        }
    }
    // The fold's own round-off: (n - 1) eps times the sum of magnitudes.
    let fold_eps = f64::EPSILON * (masses.len().max(terms) as f64);
    area_b += fold_eps * area_abs;
    for (b, a) in sum_b.iter_mut().zip(&sum_abs).skip(1) {
        *b += fold_eps * a;
    }
    let all = sum.iter().chain(sum_b.iter());
    if !(area.is_finite() && area_b.is_finite() && all.into_iter().all(|x| x.is_finite())) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "body integrals not finite",
        ));
    }
    let at = |v: &V, i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    // A closure term with its bound: |value| + bound.
    let residual = |i: usize| at(&sum, i).abs() + at(&sum_b, i);
    // Closure: over a closed body n_x, n_y, n_z, x n_z and y n_z integrate to
    // zero and the three volume forms agree. Real B-reps close only to their
    // modelling tolerance, so a residual beyond the integration bounds widens
    // the facts' bounds by what it implies; the faces refuse when that leaves
    // no volume (a missing face) or the body is known open.
    let mut closed_within_bounds = true;
    let mut disagreement = 0.0_f64;
    if bounding {
        match body.closed {
            Some(true) => {}
            Some(false) => {
                return Err(open_body(
                    "the body bounds material but is not closed".into(),
                ))
            }
            None => {
                return Err(Refusal::new(
                    RefusalCode::ClosureUnknown,
                    "the body bounds material and its closure is unknown",
                ))
            }
        }
        for i in [CLOSURE, NX, NY, XNZ, YNZ] {
            let (v, b) = (at(&sum, i), at(&sum_b, i));
            if v.is_nan() || v.abs() > b {
                closed_within_bounds = false;
            }
        }
        for i in [VOL_X, VOL_Y] {
            let (v, b) = (at(&sum, i), at(&sum_b, i));
            let d = (sum[VOL] - v).abs();
            if d.is_nan() || d > sum_b[VOL] + b {
                closed_within_bounds = false;
                disagreement = disagreement.max(d);
            }
        }
    }
    let closure = sum[CLOSURE];
    // Moved anywhere within `reach` of the centre, the volume and moments
    // change by the closure terms (zero for a closed body) times the move.
    // The volume as each form moves with the origin by the matching closure
    // term; the reported form is z n_z.
    let volume = sum[VOL];
    let widening = reach * residual(CLOSURE).max(residual(NX)).max(residual(NY)) + disagreement;
    let volume_b = sum_b[VOL] + widening;
    if !volume_b.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "body integrals not finite",
        ));
    }
    // Closure widening beyond the integration bounds is an estimate too.
    let method = if exact && closed_within_bounds {
        Method::ExactByDegree
    } else {
        Method::Adaptive
    };
    let record = |bound: f64, scale: Scale, gap_bound: Option<f64>| FactAccuracy {
        bound: Some(bound),
        method,
        scale,
        file_tolerance: body.file_tolerance,
        gap_bound,
        closed: body.closed,
    };
    let mut centroid = None;
    if bounding {
        let limit = match body.file_tolerance {
            Some(t) => GAP_MULTIPLE * t * area,
            None => OPEN_FRACTION * volume.abs(),
        };
        if !closed_within_bounds && (widening.is_nan() || widening > limit) {
            return Err(open_body(format!(
                "the faces do not close: the volume {volume} widens by {widening}, beyond {limit} (its forms disagree by {disagreement}, the n_z closure is {closure})"
            )));
        }
        if volume - volume_b <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::NonPositiveVolume,
                format!("volume {volume} within ±{volume_b} is not positive"),
            ));
        }
        let mut axis = [0.0; 3];
        let mut point = [0.0; 3];
        let c = [body.centre.x, body.centre.y, body.centre.z];
        let moments = [sum[2], sum[3], sum[4]];
        let r2 = reach * reach;
        let moment_bounds = [
            sum_b[2] + reach * residual(XNZ) + r2 * residual(CLOSURE),
            sum_b[3] + reach * residual(YNZ) + r2 * residual(CLOSURE),
            sum_b[4] + 0.5 * r2 * residual(CLOSURE),
        ];
        for ((p, a), ((ci, m), mb)) in point
            .iter_mut()
            .zip(axis.iter_mut())
            .zip(c.iter().zip(moments).zip(moment_bounds))
        {
            let rel = m / volume;
            *p = ci + rel;
            *a = (mb + rel.abs() * volume_b) / (volume - volume_b)
                + f64::EPSILON * (ci.abs() + 2.0 * rel.abs());
        }
        let value = Vec3::new(point[0], point[1], point[2]);
        let axis_bounds = Vec3::new(axis[0], axis[1], axis[2]);
        if !(value.is_finite() && axis_bounds.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("centre of mass {value:?} ±{axis_bounds:?} not finite"),
            ));
        }
        if let Some([lo, hi]) = body.bounds {
            let e = hi - lo;
            let box_volume = e.x * e.y * e.z;
            if volume - volume_b > box_volume * (1.0 + 1e-9) {
                return Err(Refusal::new(
                    RefusalCode::VolumeExceedsBox,
                    format!("volume {volume} within ±{volume_b} exceeds the box's {box_volume}"),
                ));
            }
            let slack = 1e-9 * size;
            let out = [
                (value.x, axis_bounds.x, lo.x, hi.x),
                (value.y, axis_bounds.y, lo.y, hi.y),
                (value.z, axis_bounds.z, lo.z, hi.z),
            ]
            .iter()
            .any(|&(x, b, lo, hi)| x + b < lo - slack || x - b > hi + slack);
            if out {
                return Err(Refusal::new(
                    RefusalCode::CentroidOutsideBox,
                    format!("centre of mass {value:?} ±{axis_bounds:?} outside the box"),
                ));
            }
        }
        let gap = body.file_tolerance;
        centroid = Some(PositionFact {
            value,
            axis_bounds,
            accuracy: record(axis_bounds.max_abs(), Scale::Positional, gap),
        });
    }
    let gap_volume = body.file_tolerance.map(|t| t * area);
    Ok(BodyFacts {
        area: Fact {
            value: area,
            accuracy: record(area_b, Scale::Extensive, None),
        },
        volume: Fact {
            value: volume,
            accuracy: record(volume_b, Scale::Extensive, gap_volume),
        },
        centroid,
        closure: [closure, sum_b[CLOSURE]],
        closed_within_bounds,
        faces: masses,
    })
}
