//! Trim-consistency closure (parity item S2): the check that replaces the stitched
//! cross-check. A body built from the file's topology is closed when
//!
//! 1. every edge of its B-rep has exactly two face uses ([`Closure::edge_uses_bad`]),
//!    and the boundaries the integration pass integrates (each face's trims and the UV
//!    lines closing their gaps, mapped to 3D and turned by the face's volume sign) run
//!    it once each way ([`Closure::edges_misoriented`], probed by [`net_runs`]);
//! 2. every trim lies on its 3D edge at the file's tolerance: each face's UV trims,
//!    mapped onto its surface, are sampled at [`SAMPLES`] points, the worst refined by
//!    a golden-section search and both ends added, and measured against the file's own
//!    edge curve. A trim is off its edge when that deviation exceeds twice the larger
//!    of the edge's own gap from its file faces' surfaces and the floor
//!    ([`Limits::floor`]). This is the R3v2 `faces.rs` method the M16 census ran; a
//!    solidcore edge no file edge carries (a seam or split curve) is measured against
//!    its own 3D curve with no gap, where M16 left it out;
//! 3. its divergence-theorem volume does not depend on the origin: the signed vector
//!    area of its faces (`solidcore::mass::gauss::vector_area`, by Stokes' theorem
//!    along the trims the integrator uses) times the box's half-diagonal, relative to
//!    the volume, is at most [`Limits::origin`] ([`origin_dependence`]);
//! 4. no face of a closed solid is inverted: each face's trims bound a positive area
//!    on its surface in the integration pass ([`Body3::faces_inverted`]). A face whose
//!    loops run against its surface counts its volume with the wrong sign; when the
//!    inverted faces close up among themselves (a hole's wall and its ends), their
//!    vector area is zero and the first three tests pass a wrong volume.
//!
//! The first two are measured on the built B-rep before integration ([`check`]); the
//! last two read the integration's own pass. A body that fails one refuses its facts
//! ([`verdict`]); a stated solid with an unpaired edge is an open body, a surface model
//! stays a sheet with no volume. Nothing here repairs or rebuilds a body.

use std::collections::HashMap;
use std::time::Instant;

use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::{edgeuse, face, loopuse};
use solidcore::{Brep, EdgeId, EntityRef, Kernel, Point2};
use step_import::model::{Body, Bound};
use step_import::script::edge_point;

use crate::filegeom::{norm, sub, FSurf, V3};

/// Points per trim, as M16 ran R3v2's check (`--samples 64`).
pub const SAMPLES: usize = 64;
/// Points per edge curve for the nearest-sample start of a distance query.
const COARSE: usize = 256;
/// Below this deviation (mm) a trim cannot be off its edge at any floor this check
/// is run with, so its edge's gap is not computed: [`Closure::critical_mm`] is exact for
/// floors of at least this.
const NEGLIGIBLE_MM: f64 = 1e-6;
/// The reasons a refusal lists, at most.
const MAX_REASONS: usize = 3;

/// The closure thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The trim floor in mm.
    pub floor_mm: f64,
    /// The trim floor relative to the body's size (its box's largest extent).
    pub floor_rel: f64,
    /// The largest origin dependence a closed body may have ([`origin_dependence`]).
    pub origin: f64,
}

impl Limits {
    /// The floor for a body of `size` mm: the larger of the two.
    #[must_use]
    pub fn floor(&self, size: f64) -> f64 {
        self.floor_mm.max(self.floor_rel * size)
    }
}

/// One body's closure measurements on its built B-rep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Closure {
    /// Edges of the B-rep.
    pub edges: usize,
    /// Edges with other than two face uses (an open or non-manifold edge).
    pub edge_uses_bad: usize,
    /// Edges that the faces' integrated boundaries (trims and gap lines, mapped to 3D,
    /// each turned by its face's volume sign) do not run once each way: the faces
    /// disagree on which side is outside (a hole bound the file reverses).
    pub edges_misoriented: usize,
    /// The edge orientations could not be read.
    pub orientation_unchecked: bool,
    /// Faces whose trims could not be checked, and the first reasons.
    pub faces_unchecked: usize,
    pub unchecked: Vec<String>,
    /// Trims checked, those on solidcore's own edges, and those off their edge.
    pub trims: usize,
    pub trims_made: usize,
    pub trims_bad: usize,
    /// The largest floor (mm) at which a trim would still be off its edge: half the
    /// largest deviation among trims beyond twice their edge's gap; 0 when none.
    pub critical_mm: f64,
    /// The floor the trims were judged at (mm).
    pub floor_mm: f64,
    /// The body's size (mm) the relative floor scales with.
    pub size: f64,
    /// The worst trim off its edge, described.
    pub worst: Option<String>,
    /// The largest file gap (mm) among the edges whose gap was computed.
    pub gap_max_mm: f64,
    /// The trims were not measured: a stated solid with an unpaired edge.
    pub trims_skipped: bool,
    /// Edges whose orientation was judged: read on every face that uses it.
    pub edges_probed: usize,
    /// Edges not probed because they are shorter than [`SHORT_EDGE`] floors.
    pub edges_short: usize,
    /// Edges on a face whose volume sign is 0 (it separates no regions), not judged.
    pub edges_unsided: usize,
    /// Face loops none of whose edges was probed (all short), and the sum of their
    /// squared lengths (mm²): a bound on the area they enclose, which
    /// [`verdict`] weighs against the volume like the origin dependence.
    pub loops_unprobed: usize,
    pub unprobed_len2: f64,
    /// Wall time of the check (ms).
    pub ms: f64,
}

/// A curve sampled for distance queries.
struct Sampled {
    pts: Vec<V3>,
    /// Per segment `[j, j + 1]`: how far the curve's midpoint lies from the chord, a
    /// measure of how far the curve leaves it (zero for a straight segment).
    sag: Vec<f64>,
    lo: V3,
    hi: V3,
}

/// Samples `eval` on [0, 1] at [`COARSE`] + 1 points; `None` where it fails.
fn sample(eval: &dyn Fn(f64) -> Option<V3>) -> Option<Sampled> {
    let pts: Vec<V3> = (0..=COARSE)
        .map(|j| eval(j as f64 / COARSE as f64))
        .collect::<Option<Vec<_>>>()?;
    let sag: Vec<f64> = pts
        .windows(2)
        .enumerate()
        .map(|(j, w)| match (w, eval((j as f64 + 0.5) / COARSE as f64)) {
            ([a, b], Some(m)) => segment_distance(*a, *b, m),
            _ => f64::INFINITY,
        })
        .collect();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut len = 0.0;
    let mut prev: Option<V3> = None;
    for p in &pts {
        for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(*x);
            *h = h.max(*x);
        }
        if let Some(q) = prev {
            len += norm(sub(*p, q));
        }
        prev = Some(*p);
    }
    // a chord's sagitta bounds what the samples miss
    let pad = len / COARSE as f64;
    for (l, h) in lo.iter_mut().zip(hi.iter_mut()) {
        *l -= pad;
        *h += pad;
    }
    Some(Sampled { pts, sag, lo, hi })
}

/// Distance from `p` to the sampled curve: nearest sample, then golden-section on the
/// curve parameter between its neighbours.
fn dist(c: &Sampled, eval: &dyn Fn(f64) -> Option<V3>, p: V3) -> f64 {
    refine(c, eval, p, nearest(c, p, None))
}

/// [`dist`] for a sequence of nearby points: the nearest sample is first sought within
/// [`WINDOW`] samples of the previous point's (`seed`), and over every sample unless
/// that local nearest is interior to the window and within one sample spacing of the
/// point, where no other part of the curve can be nearer than its neighbours.
fn dist_from(
    c: &Sampled,
    eval: &dyn Fn(f64) -> Option<V3>,
    p: V3,
    seed: &mut Option<usize>,
) -> f64 {
    let j = nearest(c, p, *seed);
    *seed = Some(j);
    refine(c, eval, p, j)
}

/// Half-width of the window [`dist_from`] searches first.
const WINDOW: usize = 4;

/// The index of the sample nearest `p` (the first, among equals), searched around
/// `seed` first.
fn nearest(c: &Sampled, p: V3, seed: Option<usize>) -> usize {
    let scan = |lo: usize, hi: usize| {
        let (mut best, mut bj) = (f64::INFINITY, lo);
        for (j, q) in c.pts.iter().enumerate().take(hi + 1).skip(lo) {
            let d = norm(sub(*q, p));
            if d < best {
                best = d;
                bj = j;
            }
        }
        (bj, best)
    };
    if let Some(s) = seed {
        let (lo, hi) = (s.saturating_sub(WINDOW), (s + WINDOW).min(COARSE));
        let (j, d) = scan(lo, hi);
        let interior = (j > lo || lo == 0) && (j < hi || hi == COARSE);
        let spacing = [j.checked_sub(1), (j < COARSE).then_some(j + 1)]
            .into_iter()
            .flatten()
            .filter_map(|k| Some(norm(sub(*c.pts.get(k)?, *c.pts.get(j)?))))
            .fold(0.0, f64::max);
        if interior && d <= spacing {
            return j;
        }
    }
    scan(0, COARSE).0
}

/// Below this, a segment's sag (mm) counts as straight: its chord is the curve.
const STRAIGHT_MM: f64 = 1e-12;

/// The distance from `p` to the segment `[a, b]`.
fn segment_distance(a: V3, b: V3, p: V3) -> f64 {
    let ab = sub(b, a);
    let ap = sub(p, a);
    let l2 = ab.iter().map(|x| x * x).sum::<f64>();
    let t = if l2 > 0.0 {
        (ab.iter().zip(&ap).map(|(x, y)| x * y).sum::<f64>() / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    norm(sub(ap, [ab[0] * t, ab[1] * t, ab[2] * t]))
}

/// The distance from `p` to the curve near sample `j`: where the curve is straight on
/// both neighbouring segments, the distance to those chords; else golden-section on the
/// curve parameter between its neighbours, or the sample's own distance if smaller.
fn refine(c: &Sampled, eval: &dyn Fn(f64) -> Option<V3>, p: V3, j: usize) -> f64 {
    let best = c.pts.get(j).map_or(f64::INFINITY, |q| norm(sub(*q, p)));
    let segments = [j.checked_sub(1), (j < COARSE).then_some(j)];
    if segments
        .iter()
        .flatten()
        .all(|&k| c.sag.get(k).is_some_and(|s| *s <= STRAIGHT_MM))
    {
        return segments
            .iter()
            .flatten()
            .filter_map(|&k| Some(segment_distance(*c.pts.get(k)?, *c.pts.get(k + 1)?, p)))
            .fold(best, f64::min);
    }
    let f = |s: f64| eval(s).map_or(f64::INFINITY, |q| norm(sub(q, p)));
    let a = j.saturating_sub(1) as f64 / COARSE as f64;
    let b = (j + 1).min(COARSE) as f64 / COARSE as f64;
    best.min(golden(f, a, b, false).1)
}

/// Golden-section search for the minimum (or maximum) of `f` on [a, b].
fn golden(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64, max: bool) -> (f64, f64) {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let sgn = if max { -1.0 } else { 1.0 };
    let mut c = b - g * (b - a);
    let mut d = a + g * (b - a);
    let (mut fc, mut fd) = (sgn * f(c), sgn * f(d));
    for _ in 0..40 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = sgn * f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = sgn * f(d);
        }
    }
    let x = 0.5 * (a + b);
    (x, sgn * fc.min(fd))
}

fn in_box(c: &Sampled, p: V3, pad: f64) -> bool {
    p.iter()
        .zip(c.lo.iter().zip(&c.hi))
        .all(|(x, (l, h))| *x >= l - pad && *x <= h + pad)
}

/// A file edge's point at fraction `s` of its parameter range: `step_import`'s
/// `edge_point`, with a B-spline curve's knot vector expanded once instead of per
/// point. The de Boor steps are `edge_point`'s, in its order, so the points are its
/// bits.
struct EdgeEval<'a> {
    edge: &'a step_import::model::Edge,
    /// The B-spline's expanded knot vector; empty for other curves.
    knots: Vec<f64>,
}

/// The largest degree the fixed de Boor scratch holds; a higher degree uses
/// `edge_point` itself.
const MAX_DEGREE: usize = 32;

impl<'a> EdgeEval<'a> {
    fn new(edge: &'a step_import::model::Edge) -> Self {
        let knots = match &edge.curve {
            step_import::model::Curve::BSpline(b) if b.knots.len() == b.mults.len() => b
                .knots
                .iter()
                .zip(&b.mults)
                .flat_map(|(k, m)| std::iter::repeat_n(*k, *m as usize))
                .collect(),
            _ => Vec::new(),
        };
        Self { edge, knots }
    }

    /// The exact distance from `p` to a line or circle edge; `None` for other curves,
    /// which the sampled search measures.
    fn distance(&self, p: V3) -> Option<f64> {
        let (t0, t1) = (self.edge.t_start, self.edge.t_end);
        match &self.edge.curve {
            step_import::model::Curve::Line { .. } => Some(segment_distance(
                edge_point(self.edge, 0.0)?,
                edge_point(self.edge, 1.0)?,
                p,
            )),
            step_import::model::Curve::Circle { frame, radius } => {
                let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let q = sub(p, frame.origin);
                let (x, y, z) = (dot(q, frame.x), dot(q, frame.y), dot(q, frame.z));
                let (lo, hi) = (t0.min(t1), t0.max(t1));
                let on_arc = hi - lo >= 360.0 || {
                    let theta = y.atan2(x).to_degrees();
                    let k = ((lo - theta) / 360.0).ceil();
                    theta + 360.0 * k <= hi
                };
                if on_arc {
                    Some(((x * x + y * y).sqrt() - radius).hypot(z))
                } else {
                    let a = edge_point(self.edge, 0.0)?;
                    let b = edge_point(self.edge, 1.0)?;
                    Some(norm(sub(a, p)).min(norm(sub(b, p))))
                }
            }
            _ => None,
        }
    }

    fn point(&self, s: f64) -> Option<V3> {
        let step_import::model::Curve::BSpline(c) = &self.edge.curve else {
            return edge_point(self.edge, s);
        };
        let t = self.edge.t_start + (self.edge.t_end - self.edge.t_start) * s;
        let u = &self.knots;
        let p = c.degree as usize;
        let n = c.points.len();
        if p > MAX_DEGREE || u.len() != n + p + 1 {
            return edge_point(self.edge, s);
        }
        let lo = *u.get(p)?;
        let hi = *u.get(n)?;
        let t = t.clamp(lo, hi);
        // span k in [p, n - 1]: the last with u[k] <= t, as edge_point's scan finds it
        let below = u.get(p + 1..n)?.partition_point(|x| *x <= t);
        let k = p + below;
        let mut d = [[0.0_f64; 4]; MAX_DEGREE + 1];
        for (j, dj) in d.iter_mut().enumerate().take(p + 1) {
            let i = (k + j).checked_sub(p)?;
            let pt = *c.points.get(i)?;
            let w = match &c.weights {
                Some(ws) => *ws.get(i)?,
                None => 1.0,
            };
            *dj = [pt[0] * w, pt[1] * w, pt[2] * w, w];
        }
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = (k + j).checked_sub(p)?;
                let a_lo = *u.get(i)?;
                let a_hi = *u.get(i + p + 1 - r)?;
                let den = a_hi - a_lo;
                let a = if den == 0.0 { 0.0 } else { (t - a_lo) / den };
                let prev = *d.get(j - 1)?;
                let cur = d.get_mut(j)?;
                for (c, pv) in cur.iter_mut().zip(prev) {
                    *c = (1.0 - a) * pv + a * *c;
                }
            }
        }
        let h = *d.get(p)?;
        (h[3] != 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
    }
}

/// The file body's sampled edges, which file faces use each edge, their surfaces and
/// the edge gaps computed so far.
struct FileSide<'a> {
    body: &'a Body,
    evals: Vec<EdgeEval<'a>>,
    edges: Vec<Option<Sampled>>,
    edge_faces: Vec<Vec<usize>>,
    surfs: Vec<Option<FSurf>>,
    gaps: HashMap<(usize, usize), Option<f64>>,
}

impl<'a> FileSide<'a> {
    fn new(body: &'a Body) -> Self {
        let mut edge_faces = vec![Vec::new(); body.edges.len()];
        for shell in &body.shells {
            for &fi in &shell.faces {
                let Some(face) = body.faces.get(fi) else {
                    continue;
                };
                for l in &face.loops {
                    if let Bound::Edges(es) = &l.bound {
                        for oe in es {
                            if let Some(v) = edge_faces.get_mut(oe.edge) {
                                if !v.contains(&fi) {
                                    v.push(fi);
                                }
                            }
                        }
                    }
                }
            }
        }
        let evals: Vec<EdgeEval> = body.edges.iter().map(EdgeEval::new).collect();
        Self {
            body,
            edges: evals.iter().map(|e| sample(&|s| e.point(s))).collect(),
            evals,
            edge_faces,
            surfs: body.faces.iter().map(|f| FSurf::new(&f.surface)).collect(),
            gaps: HashMap::new(),
        }
    }

    /// The largest distance of file edge `ei` from file face `fi`'s surface (64
    /// samples, the largest refined); `None` for a surface kind without a distance.
    fn gap(&mut self, ei: usize, fi: usize) -> Option<f64> {
        if let Some(g) = self.gaps.get(&(ei, fi)) {
            return *g;
        }
        let g = self.gap_uncached(ei, fi);
        self.gaps.insert((ei, fi), g);
        g
    }

    fn gap_uncached(&self, ei: usize, fi: usize) -> Option<f64> {
        let s = self.surfs.get(fi)?.as_ref()?;
        let e = self.body.edges.get(ei)?;
        let n = 64;
        let mut seed = None;
        let mut ds = Vec::with_capacity(n + 1);
        for j in 0..=n {
            let p = edge_point(e, j as f64 / n as f64)?;
            let (d, uv) = s.dist(p, seed);
            seed = Some(uv);
            ds.push((d, uv));
        }
        let (jm, &(dm, seed)) = ds
            .iter()
            .enumerate()
            .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))?;
        let a = jm.saturating_sub(1) as f64 / n as f64;
        let b = (jm + 1).min(n) as f64 / n as f64;
        let refined = golden(
            |t| edge_point(e, t).map_or(0.0, |p| s.dist(p, Some(seed)).0),
            a,
            b,
            true,
        )
        .1;
        Some(dm.max(refined))
    }

    /// The largest gap of file edge `ei` over the file faces that use it (0 where no
    /// surface gives one).
    fn edge_gap(&mut self, ei: usize) -> f64 {
        let faces = self.edge_faces.get(ei).cloned().unwrap_or_default();
        faces
            .into_iter()
            .filter_map(|fi| self.gap(ei, fi))
            .fold(0.0, f64::max)
    }
}

/// Maps solidcore edges to file edges: `(file edge, max distance)` where one carries
/// the edge's 3D curve within `tol` (5 interior points), the nearest such.
fn map_edges(k: &Kernel, b: &Brep, file: &FileSide, tol: f64) -> HashMap<EdgeId, usize> {
    let mut out = HashMap::new();
    for &e in b.edge_list() {
        let Ok(rec) = b.edge(e) else { continue };
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let iv = rec.interval;
        let pts: Vec<V3> = (0..5)
            .filter_map(|j| {
                let t = iv.min() + (iv.max() - iv.min()) * (j as f64 + 0.5) / 5.0;
                curve.evaluate_point(k, t).ok().map(|p| [p.x, p.y, p.z])
            })
            .collect();
        if pts.len() < 5 {
            continue;
        }
        let mut best: Option<(usize, f64)> = None;
        for (fi, (fe, fedge)) in file.edges.iter().zip(&file.evals).enumerate() {
            let Some(fe) = fe else { continue };
            if !pts.iter().all(|p| in_box(fe, *p, tol)) {
                continue;
            }
            let eval = |s: f64| fedge.point(s);
            let mut dmax: f64 = 0.0;
            for p in &pts {
                dmax = dmax.max(dist(fe, &eval, *p));
                if dmax > tol {
                    break;
                }
            }
            if dmax <= tol && best.is_none_or(|(_, d)| dmax < d) {
                best = Some((fi, dmax));
            }
        }
        if let Some((fi, _)) = best {
            out.insert(e, fi);
        }
    }
    out
}

/// A solidcore edge's own 3D curve and its bounded interval.
struct OwnCurve {
    curve: solidcore::curve::curve::Curve,
    lo: f64,
    hi: f64,
}

impl OwnCurve {
    fn new(b: &Brep, e: EdgeId) -> Option<Self> {
        let rec = b.edge(e).ok()?;
        let iv = rec.interval;
        if !iv.is_bounded() {
            return None;
        }
        Some(Self {
            curve: rec.curve.clone()?,
            lo: iv.min(),
            hi: iv.max(),
        })
    }

    /// The curve on [0, 1].
    fn point(&self, k: &Kernel, s: f64) -> Option<V3> {
        self.curve
            .evaluate_point(k, self.lo + (self.hi - self.lo) * s)
            .ok()
            .map(|p| [p.x, p.y, p.z])
    }
}

/// The face's edge uses in trim-boundary order, vertex loops left out, each with the
/// index of its loop on the face.
fn face_loop_edgeuses(
    b: &Brep,
    f: solidcore::FaceId,
) -> Vec<(solidcore::topology::EdgeuseId, usize)> {
    let mut eus = Vec::new();
    if let Ok(Some(fu)) = face::p2::upward_faceuse(b, f) {
        let loops = edgeuse::p1::faceuse_loopuses(b, fu, false, None).unwrap_or_default();
        for (l, lu) in loops.into_iter().enumerate() {
            if loopuse::is_vertex_loopuse(b, lu).unwrap_or(true) {
                continue;
            }
            eus.extend(
                edgeuse::p1::loopuse_edgeuses(b, lu)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|eu| (eu, l)),
            );
        }
    }
    eus
}

/// The sign the shared pass gives `f`'s volume ([`solidcore::mass::gauss::prepare`]):
/// the sum over its face uses on a region that is not a void, -1 for a use along the
/// surface normal and +1 against it; 0 for a face that separates no regions.
fn volume_sign(b: &Brep, f: solidcore::FaceId) -> Option<i32> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    let up = face::p2::upward_faceuse(b, f).ok()??;
    let mate = b.faceuse(up).ok()?.mate?;
    let region = |fu| -> Option<_> {
        let shell = b.faceuse(fu).ok()?.shell?;
        b.shell(shell).ok()?.region
    };
    if region(up)? == region(mate)? {
        return Some(0);
    }
    let mut sign = 0;
    for fu in [up, mate] {
        if b.region(region(fu)?).ok()?.is_void == 0 {
            sign += if b.faceuse(fu).ok()?.orientation == OrientType::Same {
                -1
            } else {
                1
            };
        }
    }
    Some(sign)
}

/// A curve of the integrated boundary mapped to 3D, sampled for [`net_runs`].
struct Mapped<'a> {
    trim: &'a solidcore::curve::curve::Curve,
    /// The edge whose use the curve is; `None` for a UV line closing a gap.
    edge: Option<EdgeId>,
    lo: f64,
    hi: f64,
    pts: Vec<V3>,
    /// The samples' box, padded by the longest chord.
    lo3: V3,
    hi3: V3,
}

/// Where along an edge (fraction of its interval) its direction is probed: away from
/// its ends, where other curves of the boundary meet it.
const PROBE: f64 = 0.375;

/// Edges shorter than this many floors are not probed: the curves within the probe's
/// tolerance (4 floors) of their middle would include their neighbours.
const SHORT_EDGE: f64 = 32.0;

/// `p` when all its coordinates are finite.
fn finite_point(p: V3) -> Option<V3> {
    p.iter().all(|x| x.is_finite()).then_some(p)
}

/// Samples of each mapped trim.
const MAPPED: usize = 64;

impl<'a> Mapped<'a> {
    fn new(
        k: &Kernel,
        trim: &'a solidcore::curve::curve::Curve,
        edge: Option<EdgeId>,
        surface: &solidcore::surface::surface::Surface,
    ) -> Option<Self> {
        let iv = trim.natural_interval();
        let mut m = Self {
            trim,
            edge,
            lo: iv.min(),
            hi: iv.max(),
            pts: Vec::new(),
            lo3: [f64::INFINITY; 3],
            hi3: [f64::NEG_INFINITY; 3],
        };
        for j in 0..=MAPPED {
            // a point that is not finite leaves the curve unread
            m.pts.push(finite_point(m.point(
                k,
                surface,
                j as f64 / MAPPED as f64,
            )?)?);
        }
        let pad = m
            .pts
            .windows(2)
            .map(|w| match w {
                [a, b] => norm(sub(*b, *a)),
                _ => 0.0,
            })
            .fold(0.0, f64::max);
        for p in &m.pts {
            for ((l, h), x) in m.lo3.iter_mut().zip(m.hi3.iter_mut()).zip(p) {
                *l = l.min(*x - pad);
                *h = h.max(*x + pad);
            }
        }
        Some(m)
    }

    /// The trim's point at fraction `s` of its interval, on `surface`.
    fn point(
        &self,
        k: &Kernel,
        surface: &solidcore::surface::surface::Surface,
        s: f64,
    ) -> Option<V3> {
        let pv = self
            .trim
            .evaluate(k, self.lo + (self.hi - self.lo) * s, 0, true, true)
            .ok()?;
        let uv = pv.first()?;
        let p = surface.evaluate_point(k, Point2::new(uv.x, uv.y)).ok()?;
        Some([p.x, p.y, p.z])
    }
}

/// The edge of each curve of the integrated boundary `integrated` (the face's
/// trims with the UV lines that close their gaps): its curves are `trims` (the trims
/// alone, one per edge use in `eus`) in order, possibly shifted by a period, with
/// lines between them. A curve is the next trim when it has that trim's interval and
/// UV chord; any other is a gap line (`None`). `None` when the trims are not all found.
fn tag_uses(
    k: &Kernel,
    integrated: &[solidcore::curve::curve::Curve],
    trims: &[solidcore::curve::curve::Curve],
    eus: &[solidcore::topology::EdgeuseId],
    b: &Brep,
) -> Option<Vec<Option<EdgeId>>> {
    if trims.len() != eus.len() || integrated.is_empty() {
        return None;
    }
    let shape = |c: &solidcore::curve::curve::Curve| -> Option<(f64, f64, f64, f64)> {
        let iv = c.natural_interval();
        let p = c.evaluate_point(k, iv.min()).ok()?;
        let q = c.evaluate_point(k, iv.max()).ok()?;
        Some((iv.min(), iv.max(), q.x - p.x, q.y - p.y))
    };
    let mut tags = Vec::with_capacity(integrated.len());
    let mut j = 0;
    for c in integrated {
        let is_next = match (trims.get(j), shape(c)) {
            (Some(t), Some(a)) => shape(t).is_some_and(|b| {
                let scale = 1.0 + a.2.abs().max(a.3.abs());
                a.0 == b.0
                    && a.1 == b.1
                    && (a.2 - b.2).abs() <= 1e-9 * scale
                    && (a.3 - b.3).abs() <= 1e-9 * scale
            }),
            _ => false,
        };
        if is_next {
            tags.push(Some(b.edgeuse(*eus.get(j)?).ok()?.edge?));
            j += 1;
        } else {
            tags.push(None);
        }
    }
    (j == trims.len()).then_some(tags)
}

/// How many times the curves of `mapped` run along `tangent` through `q` (one way
/// +1, the other -1): the uses of `edge` on this face (a trim off its edge is judged
/// by the trim test), and the UV lines closing gaps that pass within `tol` of the
/// nearest of them, all within `reach`. Another edge's trim is never counted, however
/// near (a sliver face). `None` when a counted curve is neither clearly along nor
/// against `tangent`, or no use of `edge` comes within `reach`.
fn net_runs(
    k: &Kernel,
    surface: &solidcore::surface::surface::Surface,
    mapped: &[Mapped],
    (q, tangent): (V3, V3),
    edge: EdgeId,
    tol: f64,
    reach: f64,
) -> Option<i32> {
    let mut near = Vec::new();
    for m in mapped.iter().filter(|m| m.edge.is_none_or(|x| x == edge)) {
        if !q
            .iter()
            .zip(m.lo3.iter().zip(&m.hi3))
            .all(|(x, (l, h))| *x >= l - reach && *x <= h + reach)
        {
            continue;
        }
        let j = m
            .pts
            .iter()
            .enumerate()
            .map(|(j, p)| (j, norm(sub(*p, q))))
            .fold((0, f64::INFINITY), |b, x| if x.1 < b.1 { x } else { b })
            .0;
        let a = j.saturating_sub(1) as f64 / MAPPED as f64;
        let b = (j + 1).min(MAPPED) as f64 / MAPPED as f64;
        let (s, d) = golden(
            |s| {
                m.point(k, surface, s)
                    .map_or(f64::INFINITY, |p| norm(sub(p, q)))
            },
            a,
            b,
            false,
        );
        // a distance that is not finite (a point that failed or is NaN) leaves the
        // edge unread, never out of reach
        if !d.is_finite() {
            return None;
        }
        if d <= reach {
            near.push((m, s, d));
        }
    }
    let (trims, gaps): (Vec<_>, Vec<_>) = near.into_iter().partition(|x| x.0.edge.is_some());
    if trims.is_empty() {
        return None;
    }
    let dmin = trims.iter().map(|x| x.2).fold(f64::INFINITY, f64::min);
    let gaps: Vec<_> = gaps.into_iter().filter(|x| x.2 <= dmin + tol).collect();
    let (n_trims, n_gaps) = (trims.len(), gaps.len());
    let mut net = 0;
    for (m, s, _) in trims.into_iter().chain(gaps) {
        let h = 1.0 / (64.0 * MAPPED as f64);
        let chord = sub(
            m.point(k, surface, (s + h).min(1.0))?,
            m.point(k, surface, (s - h).max(0.0))?,
        );
        net += run_direction(chord, tangent)?;
    }
    face_run(net, n_trims, n_gaps)
}

/// Whether `chord` runs along `tangent` (+1) or against it (-1): `None` when they are
/// within 60 degrees of square, or either is zero or not finite (a NaN chord is never
/// read as either way).
fn run_direction(chord: V3, tangent: V3) -> Option<i32> {
    let lc = norm(chord) * norm(tangent);
    if !(lc.is_finite() && lc > 0.0) {
        return None;
    }
    let cos = chord.iter().zip(&tangent).map(|(x, y)| x * y).sum::<f64>() / lc;
    if !(cos.is_finite() && cos.abs() > 0.5) {
        return None;
    }
    Some(if cos > 0.0 { 1 } else { -1 })
}

/// The direction (+1, -1 or 0) in which one face's integrated boundary runs an edge,
/// from its net runs `net` over `n_trims` trims of the edge and `n_gaps` gap lines.
/// A net of at most one each way is the direction. A net of two is read as one only
/// when it is one trim and one gap line running the same way: a UV line closing a gap
/// across a whole period of a closed surface maps onto the full circle, so it runs the
/// edge the trim already runs (the doubling the shared pass integrates as a full
/// band, which adds nothing to the volume or vector area of a surface of revolution
/// about it). Any other count is not read (`None`).
fn face_run(net: i32, n_trims: usize, n_gaps: usize) -> Option<i32> {
    match net.abs() {
        0 | 1 => Some(net),
        2 if n_trims == 1 && n_gaps == 1 => Some(net.signum()),
        _ => None,
    }
}

/// The worst deviation of `trim` (mapped onto `surface`) from a curve, by `d`: the
/// largest of [`SAMPLES`] interior points, refined by a golden-section search between
/// its neighbours, and the trim's two ends.
pub(crate) fn trim_deviation(
    k: &Kernel,
    trim: &solidcore::curve::curve::Curve,
    surface: &solidcore::surface::surface::Surface,
    d: &dyn Fn(V3) -> f64,
) -> Option<f64> {
    let point = |s: f64| -> Option<V3> {
        let pv = trim.evaluate(k, s, 0, true, true).ok()?;
        let uv = pv.first()?;
        let p = surface.evaluate_point(k, Point2::new(uv.x, uv.y)).ok()?;
        Some([p.x, p.y, p.z])
    };
    // a distance that is not finite, or a point that cannot be evaluated, leaves the
    // trim unmeasured (`None`): it never hides behind the other samples
    let finite = |x: f64| x.is_finite().then_some(x);
    let iv = trim.natural_interval();
    let n = SAMPLES;
    let h = (iv.max() - iv.min()) / n as f64;
    let mut worst: Option<(f64, f64)> = None;
    for j in 0..n {
        let s = iv.min() + h * (j as f64 + 0.5);
        let dj = finite(d(point(s)?))?;
        if worst.is_none_or(|(_, w)| dj > w) {
            worst = Some((s, dj));
        }
    }
    let (s0, mut dmax) = worst?;
    let (lo, hi) = ((s0 - h).max(iv.min()), (s0 + h).min(iv.max()));
    let refined = golden(
        |s| point(s).and_then(|p| finite(d(p))).unwrap_or(f64::INFINITY),
        lo,
        hi,
        true,
    )
    .1;
    dmax = dmax.max(finite(refined)?);
    for s in [iv.min(), iv.max()] {
        dmax = dmax.max(finite(d(point(s)?))?);
    }
    Some(dmax)
}

/// The first trim of the first face of `brep`, with that face's surface (tests).
#[cfg(test)]
pub(crate) fn first_trim(
    k: &Kernel,
    brep: &mut Brep,
) -> Option<(
    solidcore::curve::curve::Curve,
    solidcore::surface::surface::Surface,
)> {
    let f = *solidcore::topology::brep::p1::faces(brep, None)
        .ok()?
        .first()?;
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    let tb =
        face::p1::create_oriented_trim_boundary(k, brep, f, true, false, None, &key_of).ok()?;
    let surface = brep.face(f).ok()?.surface.clone()?;
    Some((tb.curves.first()?.clone(), surface))
}

/// Measures edge uses and trims of `brep`, built from `body`, of `size` mm. The
/// trims of a stated solid with an unpaired edge are not measured: it refuses as an
/// open body whatever they are ([`Closure::trims_skipped`]).
pub fn check(
    k: &Kernel,
    body: &Body,
    brep: &mut Brep,
    size: f64,
    stated_solid: bool,
    limits: &Limits,
) -> Closure {
    let t = Instant::now();
    let floor = limits.floor(size);
    let mut c = Closure {
        edges: brep.edge_list().len(),
        floor_mm: floor,
        size,
        ..Closure::default()
    };
    let unchecked = |c: &mut Closure, why: String| {
        c.faces_unchecked += 1;
        if c.unchecked.len() < MAX_REASONS {
            c.unchecked.push(why);
        }
    };
    let faces = match solidcore::topology::brep::p1::faces(brep, None) {
        Ok(f) => f,
        Err(e) => {
            unchecked(&mut c, format!("faces: {e}"));
            c.ms = t.elapsed().as_secs_f64() * 1e3;
            return c;
        }
    };
    // 1. edge uses
    let face_loop_eus: Vec<_> = faces.iter().map(|&f| face_loop_edgeuses(brep, f)).collect();
    let face_eus: Vec<Vec<_>> = face_loop_eus
        .iter()
        .map(|v| v.iter().map(|x| x.0).collect())
        .collect();
    let mut uses: HashMap<EdgeId, usize> = HashMap::new();
    for eus in &face_eus {
        for &eu in eus {
            if let Some(e) = brep.edgeuse(eu).ok().and_then(|x| x.edge) {
                *uses.entry(e).or_default() += 1;
            }
        }
    }
    c.edge_uses_bad = brep
        .edge_list()
        .iter()
        .filter(|e| uses.get(e).copied().unwrap_or(0) != 2)
        .count();
    if stated_solid && c.edge_uses_bad > 0 {
        c.trims_skipped = true;
        c.ms = t.elapsed().as_secs_f64() * 1e3;
        return c;
    }
    // 2. trims against their 3D edges
    let mut file = FileSide::new(body);
    let emap = map_edges(k, brep, &file, 1e-5 * size);
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    let mut own: HashMap<EdgeId, Option<(OwnCurve, Sampled)>> = HashMap::new();
    let mut worst: Option<(f64, f64)> = None;
    // per edge, the net runs of the integrated boundaries along it, each turned by
    // its face's volume sign, and the faces that read it (only when every edge has
    // two uses)
    let judge_orientation = c.edge_uses_bad == 0;
    let mut runs: HashMap<EdgeId, i32> = HashMap::new();
    let mut counted: HashMap<EdgeId, usize> = HashMap::new();
    let mut short: std::collections::HashSet<EdgeId> = std::collections::HashSet::new();
    let mut unsided: std::collections::HashSet<EdgeId> = std::collections::HashSet::new();
    for (i, (&f, eus)) in faces.iter().zip(&face_eus).enumerate() {
        let loop_of = |eu| {
            face_loop_eus
                .get(i)
                .and_then(|v| v.iter().find(|x| x.0 == eu))
                .map_or(0, |x| x.1)
        };
        let sign = if judge_orientation {
            volume_sign(brep, f)
        } else {
            Some(0)
        };
        if sign.is_none() {
            c.orientation_unchecked = true;
        }
        let tb = face::p1::create_oriented_trim_boundary(k, brep, f, true, false, None, &key_of);
        let surface = brep.face(f).ok().and_then(|fc| fc.surface.clone());
        let (Ok(tb), Some(surface)) = (tb, surface) else {
            unchecked(&mut c, format!("face {i}: no trim boundary"));
            continue;
        };
        // 3. the boundary the shared pass integrates (the trims and the UV lines that
        //    close their gaps), run along each of the face's edges
        if sign == Some(0) && judge_orientation {
            unsided.extend(
                eus.iter()
                    .filter_map(|&eu| brep.edgeuse(eu).ok().and_then(|x| x.edge)),
            );
        }
        if let Some(sign @ (1.. | ..=-1)) = sign {
            let integrated =
                face::p1::create_oriented_trim_boundary(k, brep, f, true, true, None, &key_of);
            let curves = integrated.map(|ib| ib.curves).unwrap_or_default();
            let tags = tag_uses(k, &curves, &tb.curves, eus, brep);
            let mapped = tags.and_then(|tags| {
                curves
                    .iter()
                    .zip(tags)
                    .map(|(t, e)| Mapped::new(k, t, e, &surface))
                    .collect::<Option<Vec<_>>>()
            });
            match mapped {
                None => c.orientation_unchecked = true,
                Some(mapped) => {
                    // per loop: whether an edge of it was probed, and its length
                    let mut loops: HashMap<usize, (bool, f64)> = HashMap::new();
                    let mut seen = Vec::new();
                    for &eu in eus {
                        let l = loop_of(eu);
                        let Some(e) = brep.edgeuse(eu).ok().and_then(|x| x.edge) else {
                            c.orientation_unchecked = true;
                            continue;
                        };
                        let entry = own.entry(e).or_insert_with(|| own_sampled(k, brep, e));
                        // an edge with no bounded, sampled own curve is unread, never short
                        let Some(len) = own_length(entry) else {
                            c.orientation_unchecked = true;
                            continue;
                        };
                        loops.entry(l).or_default().1 += len;
                        if seen.contains(&e) {
                            continue;
                        }
                        seen.push(e);
                        if len <= SHORT_EDGE * floor {
                            // an edge not much longer than the reach of the probe has no
                            // direction to read: it is counted, and its loop is weighed
                            // below when no other edge of the loop is probed
                            short.insert(e);
                            continue;
                        }
                        let probe = entry.as_ref().and_then(|(curve, _)| {
                            let q = curve.point(k, PROBE)?;
                            let t =
                                sub(curve.point(k, PROBE + 1e-3)?, curve.point(k, PROBE - 1e-3)?);
                            Some((q, t))
                        });
                        match probe.and_then(|(q, t)| {
                            net_runs(k, &surface, &mapped, (q, t), e, 4.0 * floor, 1e-2 * size)
                        }) {
                            Some(n) => {
                                *runs.entry(e).or_default() += sign * n;
                                loops.entry(l).or_default().0 = true;
                            }
                            None => c.orientation_unchecked = true,
                        }
                        *counted.entry(e).or_default() += 1;
                    }
                    for (probed, len) in loops.into_values() {
                        if !probed {
                            c.loops_unprobed += 1;
                            c.unprobed_len2 += len * len;
                        }
                    }
                }
            }
        }
        if tb.curves.len() != eus.len() {
            unchecked(
                &mut c,
                format!(
                    "face {i}: {} trims for {} edge uses",
                    tb.curves.len(),
                    eus.len()
                ),
            );
            continue;
        }
        for (trim, &eu) in tb.curves.iter().zip(eus) {
            let Some(e) = brep.edgeuse(eu).ok().and_then(|x| x.edge) else {
                unchecked(&mut c, format!("face {i}: an edge use without an edge"));
                continue;
            };
            let (dmax, gap) = if let Some(&fe) = emap.get(&e) {
                let (Some(Some(sampled)), Some(fedge)) = (file.edges.get(fe), file.evals.get(fe))
                else {
                    unchecked(&mut c, format!("face {i}: file edge {fe} not sampled"));
                    continue;
                };
                let eval = |s: f64| fedge.point(s);
                let seed = std::cell::Cell::new(None);
                let d = |p: V3| {
                    if let Some(r) = fedge.distance(p) {
                        return r;
                    }
                    let mut s = seed.get();
                    let r = dist_from(sampled, &eval, p, &mut s);
                    seed.set(s);
                    r
                };
                let Some(dmax) = trim_deviation(k, trim, &surface, &d) else {
                    unchecked(&mut c, format!("face {i}: trim not evaluated"));
                    continue;
                };
                // a deviation no floor reaches needs no gap
                let gap = if dmax > 2.0 * NEGLIGIBLE_MM {
                    let g = file.edge_gap(fe);
                    c.gap_max_mm = c.gap_max_mm.max(g);
                    g
                } else {
                    0.0
                };
                (dmax, gap)
            } else {
                c.trims_made += 1;
                let entry = own.entry(e).or_insert_with(|| own_sampled(k, brep, e));
                let Some((curve, sampled)) = entry.as_ref() else {
                    unchecked(
                        &mut c,
                        format!("face {i}: an edge without a bounded 3D curve"),
                    );
                    continue;
                };
                let eval = |s: f64| curve.point(k, s);
                let seed = std::cell::Cell::new(None);
                let d = |p: V3| {
                    let mut s = seed.get();
                    let r = dist_from(sampled, &eval, p, &mut s);
                    seed.set(s);
                    r
                };
                let Some(dmax) = trim_deviation(k, trim, &surface, &d) else {
                    unchecked(&mut c, format!("face {i}: trim not evaluated"));
                    continue;
                };
                (dmax, 0.0)
            };
            c.trims += 1;
            if dmax > 2.0 * gap {
                c.critical_mm = c.critical_mm.max(dmax / 2.0);
            }
            if dmax > 2.0 * gap.max(floor) {
                c.trims_bad += 1;
                if worst.is_none_or(|(w, _)| dmax > w) {
                    worst = Some((dmax, gap));
                    c.worst = Some(format!(
                        "face {i}: trim {dmax:.3e} mm off its edge (gap {gap:.3e} mm, floor {floor:.1e} mm)"
                    ));
                }
            }
        }
    }
    // an edge read on every face that uses it must be run as often each way
    let mut faces_of: HashMap<EdgeId, usize> = HashMap::new();
    for eus in &face_eus {
        let mut seen = Vec::new();
        for &eu in eus {
            if let Some(e) = brep.edgeuse(eu).ok().and_then(|x| x.edge) {
                if !seen.contains(&e) {
                    seen.push(e);
                    *faces_of.entry(e).or_default() += 1;
                }
            }
        }
    }
    c.edges_misoriented = runs
        .iter()
        .filter(|&(e, &n)| n != 0 && counted.get(e) == faces_of.get(e))
        .count();
    c.edges_probed = runs
        .keys()
        .filter(|e| counted.get(*e) == faces_of.get(*e))
        .count();
    c.edges_short = short.len();
    c.edges_unsided = unsided.len();
    c.ms = t.elapsed().as_secs_f64() * 1e3;
    c
}

/// A solidcore edge's own curve, sampled.
fn own_sampled(k: &Kernel, b: &Brep, e: EdgeId) -> Option<(OwnCurve, Sampled)> {
    let curve = OwnCurve::new(b, e)?;
    let sampled = sample(&|s| curve.point(k, s))?;
    Some((curve, sampled))
}

/// The polyline length of a sampled own curve; `None` when the edge has no bounded
/// own curve or it could not be sampled (its orientation is then unread, never short).
fn own_length(entry: &Option<(OwnCurve, Sampled)>) -> Option<f64> {
    entry
        .as_ref()
        .map(|(_, s)| {
            s.pts
                .windows(2)
                .map(|w| match w {
                    [a, b] => norm(sub(*b, *a)),
                    _ => 0.0,
                })
                .sum::<f64>()
        })
        .filter(|l| l.is_finite())
}

/// How much the body's volume depends on the origin: the signed vector area of its
/// faces, `area` (zero for a closed surface), times the half-diagonal of `bounds`,
/// relative to `volume`. A divergence-theorem volume moves by at most this fraction
/// when its origin moves anywhere within the box.
#[must_use]
pub fn origin_dependence(area: [f64; 3], volume: f64, bounds: &[f64; 6]) -> f64 {
    let [x0, y0, z0, x1, y1, z1] = *bounds;
    let half_diagonal = 0.5 * norm([x1 - x0, y1 - y0, z1 - z0]);
    norm(area) * half_diagonal / volume.abs().max(f64::MIN_POSITIVE)
}

/// The same leak as M16 measured it: the relative change of solidcore's volume
/// (`∫ (z - o_z) n_z dA`) between the box centre and the world origin.
#[must_use]
pub fn world_origin_dependence(area: [f64; 3], volume: f64, bounds: &[f64; 6]) -> f64 {
    let cz = 0.5 * (bounds[2] + bounds[5]);
    (cz * area[2]).abs() / volume.abs().max(f64::MIN_POSITIVE)
}

/// What the closure test makes of a body.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Closed, with consistent trims and an origin-independent volume.
    Closed,
    /// Not a closed solid and not stated as one (a surface model or a loose face):
    /// it keeps an area and a box, never a volume.
    Shell,
    /// Refused: `(class, reason)`.
    Refused(&'static str, String),
}

/// The inputs of [`verdict`] beyond the measurements.
pub struct Body3<'a> {
    pub closure: &'a Closure,
    pub manifold: bool,
    pub stated_solid: bool,
    /// The signed vector area from the integration pass, or why it is missing.
    pub vector_area: Option<&'a Result<[f64; 3], String>>,
    /// Faces whose trims bound a negative area on their surface in the integration
    /// pass (`[area, ..]` of `solidcore::mass::gauss::run` before the volume signs).
    pub faces_inverted: usize,
    /// Faces the shared pass did not integrate (solidcore's own integrator measured
    /// the body instead): their orientation is not known.
    pub faces_unintegrated: usize,
    pub volume: Option<f64>,
    pub bounds: Option<[f64; 6]>,
}

/// Decides a body's closure, in order: an open body, a closed body whose edges are used
/// twice in one direction, faces left unchecked, an inverted
/// face in a closed body, trims off their edges, then (for a solid with a volume) the
/// origin dependence.
#[must_use]
pub fn verdict(b: &Body3, limits: &Limits) -> Verdict {
    let c = b.closure;
    let open = c.edge_uses_bad > 0 || !b.manifold;
    if open && b.stated_solid {
        return Verdict::Refused(
            "not-closed",
            if c.edge_uses_bad > 0 {
                format!(
                    "{} of {} edges not used exactly twice",
                    c.edge_uses_bad, c.edges
                )
            } else {
                "not a manifold solid".to_string()
            },
        );
    }
    if !open && c.edges_misoriented > 0 {
        return Verdict::Refused(
            "misoriented",
            format!(
                "{} of {} edges used twice in the same direction",
                c.edges_misoriented, c.edges
            ),
        );
    }
    if !open && c.orientation_unchecked {
        return Verdict::Refused(
            "closure-unchecked",
            "edge orientations not readable".to_string(),
        );
    }
    if !open && c.loops_unprobed > 0 {
        // a loop of short edges only is not probed: a reversed one changes the volume
        // by about its area times the reach of the body, so weigh the bound on that
        // area (its length squared) as the origin dependence weighs the vector area
        let weight = match (b.volume, b.bounds) {
            (Some(v), Some(bx)) => origin_dependence([c.unprobed_len2, 0.0, 0.0], v, &bx),
            _ => f64::NAN,
        };
        if weight.is_nan() || weight > limits.origin {
            return Verdict::Refused(
                "closure-unchecked",
                format!(
                    "{} loop(s) of short edges not probed: orientation unchecked, area bound \
                     {weight:.2e} of the volume (limit {:.0e})",
                    c.loops_unprobed, limits.origin
                ),
            );
        }
    }
    if c.faces_unchecked > 0 {
        return Verdict::Refused(
            "closure-unchecked",
            format!(
                "{} face(s) unchecked: {}",
                c.faces_unchecked,
                c.unchecked.join("; ")
            ),
        );
    }
    if !open && b.faces_unintegrated > 0 {
        return Verdict::Refused(
            "closure-unchecked",
            format!(
                "{} face(s) not integrated by the shared pass: orientation unchecked",
                b.faces_unintegrated
            ),
        );
    }
    if !open && b.faces_inverted > 0 {
        return Verdict::Refused(
            "face-inverted",
            format!(
                "{} face(s) bound a negative area: their trims run against the surface",
                b.faces_inverted
            ),
        );
    }
    if c.trims_bad > 0 {
        return Verdict::Refused(
            "trim-inconsistent",
            format!(
                "{} of {} trims off their edge; worst {}",
                c.trims_bad,
                c.trims,
                c.worst.as_deref().unwrap_or("?")
            ),
        );
    }
    if open {
        return Verdict::Shell;
    }
    if let (Some(volume), Some(bounds)) = (b.volume, b.bounds) {
        match b.vector_area {
            Some(Ok(area)) => {
                let od = origin_dependence(*area, volume, &bounds);
                if od.is_nan() || od > limits.origin {
                    return Verdict::Refused(
                        "origin-dependent",
                        format!(
                            "volume moves {od:.2e} of itself within its box (limit {:.0e})",
                            limits.origin
                        ),
                    );
                }
            }
            Some(Err(e)) => {
                return Verdict::Refused("closure-unchecked", format!("vector area: {e}"));
            }
            None => {
                return Verdict::Refused(
                    "closure-unchecked",
                    "vector area: not integrated".to_string(),
                );
            }
        }
    }
    Verdict::Closed
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        floor_mm: 1e-5,
        floor_rel: 0.0,
        origin: 1e-7,
    };
    const BOX: [f64; 6] = [0.0, 0.0, 0.0, 10.0, 10.0, 10.0];

    #[test]
    fn the_cached_evaluator_has_edge_point_s_bits() {
        let curve = |weights| {
            step_import::model::Curve::BSpline(step_import::model::BSplineCurve {
                degree: 3,
                points: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 2.0, 0.5],
                    [2.5, 2.0, -0.3],
                    [4.0, 0.1, 0.2],
                    [5.0, -1.0, 1.0],
                    [6.0, 0.0, 0.0],
                ],
                weights,
                knots: vec![0.0, 0.3, 0.7, 1.0],
                mults: vec![4, 1, 1, 4],
            })
        };
        for weights in [None, Some(vec![1.0, 0.7, 1.3, 0.9, 1.1, 1.0])] {
            for (t_start, t_end) in [(0.0, 1.0), (0.85, 0.12), (0.3, 0.7)] {
                let edge = step_import::model::Edge {
                    step_id: 1,
                    start: 0,
                    end: 1,
                    curve: curve(weights.clone()),
                    same_sense: t_end > t_start,
                    t_start,
                    t_end,
                };
                let eval = EdgeEval::new(&edge);
                for j in 0..=200 {
                    let s = f64::from(j) / 200.0;
                    let (a, b) = (eval.point(s), edge_point(&edge, s));
                    assert_eq!(
                        a.map(|p| p.map(f64::to_bits)),
                        b.map(|p| p.map(f64::to_bits))
                    );
                }
            }
        }
    }

    #[test]
    fn line_and_circle_distances_are_exact() {
        let frame = step_import::model::Frame {
            origin: [1.0, 2.0, 3.0],
            x: [0.0, 1.0, 0.0],
            y: [0.0, 0.0, 1.0],
            z: [1.0, 0.0, 0.0],
        };
        let edge = |curve, t_start, t_end| step_import::model::Edge {
            step_id: 1,
            start: 0,
            end: 1,
            curve,
            same_sense: t_end > t_start,
            t_start,
            t_end,
        };
        let circle = || step_import::model::Curve::Circle { frame, radius: 5.0 };
        let line = step_import::model::Curve::Line {
            origin: [0.0, 0.0, 0.0],
            dir: [0.6, 0.8, 0.0],
            magnitude: 2.0,
        };
        let edges = [
            edge(circle(), 0.0, 90.0),
            edge(circle(), 300.0, 420.0),
            edge(circle(), 250.0, -30.0),
            edge(circle(), 0.0, 360.0),
            edge(line, -1.0, 4.0),
        ];
        let points = [
            [1.0, 2.0, 3.0],
            [4.0, 7.0, 4.0],
            [-2.0, -3.0, 1.0],
            [1.5, 6.9, 3.1],
            [0.0, 0.0, 0.0],
            [9.0, -1.0, 8.0],
            [1.0, 7.0, 3.0],
        ];
        for e in &edges {
            let eval = EdgeEval::new(e);
            for p in points {
                let exact = eval.distance(p).unwrap_or(f64::NAN);
                let brute = (0..=200_000)
                    .filter_map(|j| eval.point(f64::from(j) / 200_000.0))
                    .map(|q| norm(sub(q, p)))
                    .fold(f64::INFINITY, f64::min);
                assert!(exact <= brute + 1e-12, "{exact} {brute}");
                assert!(brute - exact < 1e-4, "{exact} {brute}");
            }
        }
    }

    fn closed() -> Closure {
        Closure {
            edges: 12,
            trims: 24,
            size: 10.0,
            floor_mm: 1e-5,
            ..Closure::default()
        }
    }

    fn judge(
        c: &Closure,
        manifold: bool,
        stated: bool,
        area: Option<&Result<[f64; 3], String>>,
    ) -> Verdict {
        verdict(
            &Body3 {
                closure: c,
                manifold,
                stated_solid: stated,
                vector_area: area,
                faces_inverted: 0,
                faces_unintegrated: 0,
                volume: manifold.then_some(1000.0),
                bounds: Some(BOX),
            },
            &LIMITS,
        )
    }

    fn class(v: &Verdict) -> Option<&'static str> {
        match v {
            Verdict::Refused(c, _) => Some(c),
            _ => None,
        }
    }

    #[test]
    fn each_refusal_class_has_its_cause() {
        let zero = Ok([0.0; 3]);
        assert_eq!(judge(&closed(), true, true, Some(&zero)), Verdict::Closed);
        // an unpaired edge: a stated solid is open, a surface model a shell
        let open = Closure {
            edge_uses_bad: 4,
            ..closed()
        };
        assert_eq!(
            class(&judge(&open, false, true, Some(&zero))),
            Some("not-closed")
        );
        assert_eq!(
            class(&judge(&closed(), false, true, None)),
            Some("not-closed")
        );
        assert_eq!(judge(&open, false, false, None), Verdict::Shell);
        // a closed body whose edges are used twice in one direction; an open sheet's
        // count does not matter
        let mis = Closure {
            edges_misoriented: 4,
            ..closed()
        };
        assert_eq!(
            class(&judge(&mis, true, true, Some(&zero))),
            Some("misoriented")
        );
        assert_eq!(
            judge(
                &Closure {
                    edge_uses_bad: 1,
                    ..mis
                },
                false,
                false,
                None
            ),
            Verdict::Shell
        );
        let unread = Closure {
            orientation_unchecked: true,
            ..closed()
        };
        assert_eq!(
            class(&judge(&unread, true, true, Some(&zero))),
            Some("closure-unchecked")
        );
        // a loop of short edges only: unprobed, weighed like the origin dependence
        // (bound 1.44 mm² x 8.66 mm / 1000 mm³ refuses; 1e-12 mm² does not)
        let small = |len2| Closure {
            loops_unprobed: 2,
            unprobed_len2: len2,
            ..closed()
        };
        assert_eq!(
            class(&judge(&small(1.44), true, true, Some(&zero))),
            Some("closure-unchecked")
        );
        assert_eq!(
            judge(&small(1e-12), true, true, Some(&zero)),
            Verdict::Closed
        );
        // a face whose trims could not be checked, or no vector area
        let unchecked = Closure {
            faces_unchecked: 1,
            unchecked: vec!["face 3: no trim boundary".into()],
            ..closed()
        };
        assert_eq!(
            class(&judge(&unchecked, true, true, Some(&zero))),
            Some("closure-unchecked")
        );
        assert_eq!(
            class(&judge(&unchecked, false, false, None)),
            Some("closure-unchecked")
        );
        let failed = Err("no convergence".to_string());
        assert_eq!(
            class(&judge(&closed(), true, true, Some(&failed))),
            Some("closure-unchecked")
        );
        assert_eq!(
            class(&judge(&closed(), true, true, None)),
            Some("closure-unchecked")
        );
        // an inverted face refuses a closed body, before its trims; a sheet's area
        // does not depend on it
        let inverted = |c: &Closure, manifold: bool, stated: bool| {
            verdict(
                &Body3 {
                    closure: c,
                    manifold,
                    stated_solid: stated,
                    vector_area: Some(&zero),
                    faces_inverted: 2,
                    faces_unintegrated: 0,
                    volume: manifold.then_some(1000.0),
                    bounds: Some(BOX),
                },
                &LIMITS,
            )
        };
        assert_eq!(
            class(&inverted(&closed(), true, true)),
            Some("face-inverted")
        );
        let off = Closure {
            trims_bad: 1,
            ..closed()
        };
        assert_eq!(class(&inverted(&off, true, true)), Some("face-inverted"));
        assert_eq!(inverted(&open, false, false), Verdict::Shell);
        // a trim off its edge, in a solid or a sheet
        let bad = Closure {
            trims_bad: 1,
            ..closed()
        };
        assert_eq!(
            class(&judge(&bad, true, true, Some(&zero))),
            Some("trim-inconsistent")
        );
        assert_eq!(
            class(&judge(&bad, false, false, None)),
            Some("trim-inconsistent")
        );
        // a volume that moves with the origin: a vector area of 1e-6 mm² moves 1000 mm³
        // by 8.7e-6 mm³ within the box (8.7e-9), 1e-4 mm² by 8.7e-7
        assert_eq!(
            judge(&closed(), true, true, Some(&Ok([1e-6, 0.0, 0.0]))),
            Verdict::Closed
        );
        assert_eq!(
            class(&judge(&closed(), true, true, Some(&Ok([0.0, 1e-4, 0.0])))),
            Some("origin-dependent")
        );
        let nan = Ok([f64::NAN, 0.0, 0.0]);
        assert_eq!(
            class(&judge(&closed(), true, true, Some(&nan))),
            Some("origin-dependent")
        );
    }

    #[test]
    fn a_value_that_is_not_finite_is_never_read_as_a_direction() {
        let t = [1.0, 0.0, 0.0];
        assert_eq!(run_direction([2.0, 0.1, 0.0], t), Some(1));
        assert_eq!(run_direction([-2.0, 0.1, 0.0], t), Some(-1));
        assert_eq!(run_direction([f64::NAN, 0.0, 0.0], t), None);
        assert_eq!(run_direction([f64::INFINITY, 0.0, 0.0], t), None);
        assert_eq!(run_direction([1.0, 0.0, 0.0], [f64::NAN; 3]), None);
        assert_eq!(run_direction([0.0, 1.0, 0.0], t), None);
        assert_eq!(run_direction([0.0; 3], t), None);
        assert_eq!(finite_point([1.0, 2.0, 3.0]), Some([1.0, 2.0, 3.0]));
        assert_eq!(finite_point([1.0, f64::NAN, 3.0]), None);
    }

    #[test]
    fn a_double_run_is_read_only_as_a_trim_and_its_gap_line() {
        assert_eq!(face_run(1, 1, 0), Some(1));
        assert_eq!(face_run(-1, 1, 2), Some(-1));
        assert_eq!(face_run(0, 2, 0), Some(0));
        // a trim and the period-wide gap line over it, the same way
        assert_eq!(face_run(2, 1, 1), Some(1));
        assert_eq!(face_run(-2, 1, 1), Some(-1));
        // two trims the same way, or two gap lines over a trim, are not read
        assert_eq!(face_run(2, 2, 0), None);
        assert_eq!(face_run(-2, 0, 2), None);
        assert_eq!(face_run(3, 1, 2), None);
    }

    #[test]
    fn an_edge_without_an_own_curve_is_unread_not_short() {
        assert_eq!(own_length(&None), None);
    }

    #[test]
    fn the_floor_is_the_larger_of_its_two_terms() {
        let l = Limits {
            floor_mm: 1e-5,
            floor_rel: 1e-7,
            origin: 1e-7,
        };
        assert_eq!(l.floor(10.0), 1e-5);
        assert!((l.floor(1e3) - 1e-4).abs() < 1e-18);
    }

    #[test]
    fn the_world_origin_measure_reads_the_z_shift() {
        let b = [0.0, 0.0, 99.0, 10.0, 10.0, 101.0];
        assert!((world_origin_dependence([5.0, 5.0, 1e-3], 1000.0, &b) - 1e-4).abs() < 1e-15);
        assert_eq!(world_origin_dependence([5.0, 5.0, 0.0], 1000.0, &b), 0.0);
    }
}
