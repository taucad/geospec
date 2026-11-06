//! Area, volume and centre of mass with bounds, per face and per body.
//!
//! Per face (Green's theorem, as solidcore's `SmODEIsoCurveLength` and the
//! solidcore evaluation's patch 0001): along each trim piece `c(s) = (u, v)`,
//! the outer integrand is `-u'(s) F(u, v)` with `F(u, v)` the integral over
//! `[base_v, v]` of the surface integrand at fixed `u`. The surface integrand,
//! relative to the body centre, is the area element `|S_u × S_v|`, the volume
//! element `|S_u × S_v| z n_z` (divergence of `(0, 0, z)`), the volume first
//! moments `x dV`, `y dV`, `z dV / 2`, and the closure element `(S_u × S_v)_z`,
//! whose integral over a closed body is zero.
//!
//! Inner integrals split at the surface's v knots; on planes, cylinders and
//! cones they are integrated in closed form (patch 0014's three-point rule,
//! exact for their polynomial integrands). Outer integrals split at the trim's
//! knots and its crossings of the surface's u knot lines (patch 0001). Both
//! levels are adaptive GK15 with the patch's tolerances.
//!
//! Faces run on `threads` scoped threads and fold in face order: the facts
//! are bit-identical for every thread count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::accuracy::{FactAccuracy, Method, Scale};
use crate::definition::{BodyDefinition, FaceDefinition, Material, Orientation, Surface};
use crate::gk::{integrate_piece, Piece, CLOSURE, N, NODES, V};
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::{PreparedTrim, EFF_ZERO};
use crate::vec::Vec3;

const EFF_ZERO_SQ: f64 = 1e-24;
/// The round-off allowance per value, in units of the unit round-off.
const ROUNDOFF_ULPS: f64 = 64.0;
/// How far, relative to the domain's width, a trim may leave its surface's
/// parameter domain before it refuses.
const DOMAIN_SLACK: f64 = 1e-7;
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
/// their bounds: `[area, volume, Mx, My, Mz, closure]` about the body centre.
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
    /// The closure integral and its bound: zero for a closed body.
    pub closure: [f64; 2],
    /// Per face, in order.
    pub faces: Vec<FaceMass>,
}

fn add(a: &mut V, b: &V) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y;
    }
}

/// The powers of the size each component scales with beyond area.
const POWERS: [u8; N] = [0, 1, 2, 2, 2, 0];

/// The round-off allowance of a computed value `v`.
fn roundoff(v: &V, size: f64) -> V {
    let mut out = [0.0; N];
    let area = v[0].abs();
    let scale = [1.0, size, size * size];
    for ((o, x), p) in out.iter_mut().zip(v).zip(POWERS) {
        let s = scale.get(usize::from(p)).copied().unwrap_or(1.0);
        *o = ROUNDOFF_ULPS * f64::EPSILON * (x.abs() + area * s);
    }
    out
}

/// The surface integrand at `(p - centre, S_u, S_v)`.
fn integrand(p: Vec3, su: Vec3, sv: Vec3) -> V {
    let norm = su.cross(sv);
    let area = norm.length();
    let unit_z = if norm.length_squared() > EFF_ZERO_SQ {
        norm.z / area
    } else {
        norm.z
    };
    let vol = area * p.z * unit_z;
    [area, vol, p.x * vol, p.y * vol, p.z * vol / 2.0, norm.z]
}

/// The inner integral over a v segment of length `l` where the surface is
/// affine in v, `S(m + t) - centre = pm + t sv`, and `S_u(m + t) = su + t dsu`
/// with `dsu` parallel to `su` or zero (planes and cylinders: zero; cones: the
/// radius varies linearly). The normal keeps its direction and its length is
/// linear, so the integrand is a polynomial of degree at most 3 and
/// three-point Gauss-Legendre is exact. `None` where the normal reaches the
/// integrand's zero guard or turns over within the segment.
fn ruled_integral(pm: Vec3, su: Vec3, dsu: Vec3, sv: Vec3, l: f64) -> Option<V> {
    let h = 0.5 * l;
    let n_lo = (su - h * dsu).cross(sv);
    let n_hi = (su + h * dsu).cross(sv);
    if !(n_lo.length_squared() > EFF_ZERO_SQ
        && n_hi.length_squared() > EFF_ZERO_SQ
        && n_lo.dot(n_hi) > 0.0)
    {
        return None;
    }
    let r = (0.6_f64).sqrt() * h;
    let mut out = [0.0; N];
    for (t, w) in [(-r, 5.0 / 9.0), (0.0, 8.0 / 9.0), (r, 5.0 / 9.0)] {
        let y = integrand(pm + t * sv, su + t * dsu, sv);
        for (o, yi) in out.iter_mut().zip(y) {
            *o += w * yi;
        }
    }
    for o in &mut out {
        *o *= h;
    }
    out.iter().all(|x| x.is_finite()).then_some(out)
}

/// The distinct knots strictly between `a` and `b`, in the direction from `a`
/// to `b`.
fn knots_between(knots: &[f64], a: f64, b: f64) -> Vec<f64> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut out: Vec<f64> = Vec::new();
    for &k in knots {
        if k > lo && k < hi && out.last().is_none_or(|l| *l < k) {
            out.push(k);
        }
    }
    if a > b {
        out.reverse();
    }
    out
}

fn outside(x: f64, [lo, hi]: [f64; 2]) -> bool {
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let slack = DOMAIN_SLACK * (hi - lo);
    x < lo - slack || x > hi + slack
}

/// One face's integrals about `centre`, for a body of size `size`.
///
/// # Errors
///
/// A typed [`Refusal`]: an invalid definition, a trim outside the surface's
/// domain, a non-finite integrand, no convergence.
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
    let trims: Vec<PreparedTrim> = face.trims.iter().map(PreparedTrim::new).collect();
    let domain = surface.domain();
    let (mut v0, mut u0, mut u1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for t in &trims {
        let [lo, hi] = t.hull();
        v0 = v0.min(lo[1]);
        u0 = u0.min(lo[0]);
        u1 = u1.max(hi[0]);
    }
    let [_, [dv0, _]] = domain;
    let base_v = if trims.is_empty() {
        0.0
    } else if v0 >= dv0 {
        v0
    } else {
        dv0
    };
    let u_width = (u1 - u0).max(0.0);
    let inner_accuracy = accuracy / 10.0;
    let outer_floor = 1e-3 * accuracy * size * size;
    let inner_floor = outer_floor / u_width.max(f64::MIN_POSITIVE);
    let mut exact = matches!(face.surface, Surface::Plane { .. })
        && trims.iter().all(|t| {
            t.polynomial_degree()
                .is_some_and(|d| d <= EXACT_TRIM_DEGREE)
        });
    let mut total = [0.0; N];
    let mut bound = [0.0; N];
    let mut panels = 0;
    let mut row = Row::default();
    for trim in &trims {
        let breaks = trim.breaks(surface.u_knots());
        for pair in breaks.windows(2) {
            let &[a, b] = pair else { continue };
            if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let mut inner_panels = 0;
            let mut all_closed = true;
            let mut outer = |xs: &[f64; NODES],
                             ys: &mut [V; NODES],
                             es: &mut [V; NODES]|
             -> Result<(), Refusal> {
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
                    if d[0].abs() <= EFF_ZERO {
                        *y = [0.0; N];
                        *e = [0.0; N];
                        continue;
                    }
                    surface.row(p[0], &mut row);
                    let row = &row;
                    let mut ends = knots_between(surface.v_knots(), base_v, p[1]);
                    ends.insert(0, base_v);
                    ends.push(p[1]);
                    let mut sum = [0.0; N];
                    let mut err = [0.0; N];
                    for w in ends.windows(2) {
                        let &[lo, hi] = w else { continue };
                        let closed = surface.linear_v(row, lo, hi).and_then(|(pm, su, dsu, sv)| {
                            ruled_integral(pm - centre, su, dsu, sv, hi - lo)
                        });
                        if let Some(piece) = closed {
                            add(&mut sum, &piece);
                            add(&mut err, &roundoff(&piece, size));
                            continue;
                        }
                        all_closed = false;
                        let mut inner = |vs: &[f64; NODES],
                                         ys: &mut [V; NODES],
                                         _: &mut [V; NODES]|
                         -> Result<(), Refusal> {
                            for (&v, y) in vs.iter().zip(ys.iter_mut()) {
                                let (s, su, sv) = surface.eval(row, v);
                                *y = integrand(s - centre, su, sv);
                            }
                            Ok(())
                        };
                        let piece = integrate_piece(
                            &mut inner,
                            lo,
                            hi,
                            inner_accuracy,
                            size,
                            inner_floor,
                            200,
                        )?;
                        inner_panels += piece.panels;
                        add(&mut sum, &piece.value);
                        add(&mut err, &piece.estimate);
                        add(&mut err, &roundoff(&piece.value, size));
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
            let Piece {
                value,
                estimate,
                carried,
                panels: outer_panels,
            } = integrate_piece(&mut outer, a, b, accuracy, size, outer_floor, 400)?;
            exact &= all_closed;
            panels += outer_panels + inner_panels;
            let sign = if trim.reversed { -1.0 } else { 1.0 };
            for (t, v) in total.iter_mut().zip(value) {
                *t += sign * v;
            }
            add(&mut bound, &estimate);
            add(&mut bound, &carried);
            add(&mut bound, &roundoff(&value, size));
        }
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

/// Runs `job(i)` for `i in 0..n` on `threads` scoped threads, taking indices
/// in order; results come back in index order whatever the thread count.
fn pool<T: Send, F>(n: usize, threads: usize, job: F) -> Vec<Option<T>>
where
    F: Fn(usize) -> T + Sync,
{
    let threads = threads.min(n).max(1);
    if threads == 1 {
        return (0..n).map(|i| Some(job(i))).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(slot) = slots.get(i) else { break };
                let r = job(i);
                if let Ok(mut g) = slot.lock() {
                    *g = Some(r);
                }
            });
        }
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
    let mut results = pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        Some(
            body.validate()
                .and_then(|()| face_mass(face, body.centre, body.size, options.accuracy))
                .map_err(|r| r.at_face(f)),
        )
    })
    .into_iter();
    bodies
        .iter()
        .map(|body| {
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

/// Folds the faces' integrals in face order and checks the body invariants.
fn fold(body: &BodyDefinition, masses: Vec<FaceMass>) -> Result<BodyFacts, Refusal> {
    let size = body.size;
    let (mut area, mut area_b, mut area_abs) = (0.0, 0.0, 0.0);
    let mut sum = [0.0; N];
    let mut sum_b = [0.0; N];
    let mut sum_abs = [0.0; N];
    let mut terms = 0usize;
    let mut bounding = false;
    let mut exact = true;
    for (f, (m, face)) in masses.iter().zip(&body.faces).enumerate() {
        exact &= m.exact;
        if m.raw[0] < -m.bound[0] {
            return Err(Refusal::new(
                RefusalCode::TrimOrientation,
                format!("signed area {} below -{}", m.raw[0], m.bound[0]),
            )
            .at_face(f));
        }
        if m.raw[1].abs() > m.raw[0].abs() * size + m.bound[1] {
            return Err(Refusal::new(
                RefusalCode::FaceTermExceedsArea,
                format!(
                    "volume term {} exceeds area {} times size {size}",
                    m.raw[1], m.raw[0]
                ),
            )
            .at_face(f));
        }
        area += m.raw[0].abs();
        area_b += m.bound[0];
        area_abs += m.raw[0].abs();
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
    let closure = sum[CLOSURE];
    // About another origin in the body's box the volume differs by up to
    // size / 2 times the closure residual: zero for a closed body.
    let volume = sum[1];
    let volume_b = sum_b[1] + 0.5 * size * closure.abs();
    if !(area.is_finite() && volume.is_finite() && area_b.is_finite() && volume_b.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "body integrals not finite",
        ));
    }
    let method = if exact {
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
        let moment_bounds = [sum_b[2], sum_b[3], sum_b[4]];
        for ((p, a), ((ci, m), mb)) in point
            .iter_mut()
            .zip(axis.iter_mut())
            .zip(c.iter().zip(moments).zip(moment_bounds))
        {
            let rel = m / volume;
            *p = ci + rel;
            *a = (mb + rel.abs() * volume_b) / (volume - volume_b);
        }
        let value = Vec3::new(point[0], point[1], point[2]);
        let axis_bounds = Vec3::new(axis[0], axis[1], axis[2]);
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
        faces: masses,
    })
}
