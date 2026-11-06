//! A face's trim loops: chaining, closure and orientation.
//!
//! Within a loop each piece must end where the next starts and the last where
//! the first starts, to a relative [`GAP_TOLERANCE`]; the closing junction may
//! instead differ by one whole period of a closed surface (a loop winding
//! around a cylinder, cone, sphere, torus or closed net) when the surface
//! points at both ends coincide. A gap above round-off is returned as a
//! closing segment, which the integrator integrates into the face's bound.
//!
//! Orientation, from each closed loop's signed area in the parameter plane
//! (Kronrod 15 per trim span, relative to the loop's first point, with the
//! closing segments): without winding loops the largest loop is the outer one
//! and is counter-clockwise, every other loop clockwise; with winding loops
//! they pair up in opposite directions around each period and every closed
//! loop is a clockwise hole. A loop whose signed area is within ten times its
//! quadrature estimate plus round-off is degenerate and takes no role; a loop
//! whose area is not finite refuses. Without winding loops each hole must lie
//! inside the outer loop: at least one of its pieces' midpoints inside a
//! polygon of 17 samples per span of the outer loop (a hole wholly outside
//! refuses). The face's closed loops must not meet except where consecutive
//! edges of one loop share their joint. Each chord of the sampled polygons is
//! a capsule whose radius bounds the distance of the curve it stands for
//! (the convex hull of the curve's control points over the chord's interval;
//! zero for lines), so a crossing between samples is seen: capsules in
//! contact are halved up to eight times, and any contact that remains (a
//! proper crossing, a vertex on an edge, edges touching or overlapping
//! collinearly, or curves the halving cannot separate) between different
//! loops, or between non-adjacent edges of one loop, refuses. No hole may lie
//! inside another. Loops winding in v are not supported and refuse.

use crate::definition::FaceDefinition;
use crate::gk::gk15_scalar;
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::PreparedTrim;

/// The largest gap between consecutive pieces, relative to `1 +` the
/// magnitude of the junction's coordinates (solidcore fills gaps above about
/// `1e-6 sqrt(1 + |p|)` with lines).
pub(crate) const GAP_TOLERANCE: f64 = 1e-6;
/// Gaps at or below this many unit round-offs (relative) are representation
/// noise, not closing segments.
const NOISE_ULPS: f64 = 64.0;
/// Surface points at a winding loop's ends coincide to this relative
/// distance.
const WIND_TOLERANCE: f64 = 1e-9;
/// Sampled polygon edges closer than this fraction of the polygons' largest
/// coordinate magnitude are in contact (a thousand times the round-off of a
/// point-to-segment distance).
const CONTACT_TOLERANCE: f64 = 1e-12;

/// Whether the loops decided the face's orientation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoopOrientation {
    /// At least one closed loop is not degenerate and every loop fits its
    /// role.
    Decided,
    /// Every closed loop is degenerate, or the face has only winding loops:
    /// the loops fit either orientation, and the face's signed area decides.
    Undecided,
}

/// The loops' closing segments `(from, to)` and their orientation.
#[derive(Debug)]
pub struct Loops {
    pub gaps: Vec<([f64; 2], [f64; 2])>,
    pub orientation: LoopOrientation,
}

fn ends(t: &PreparedTrim) -> ([f64; 2], [f64; 2]) {
    let (a, _) = t.eval(t.range[0]);
    let (b, _) = t.eval(t.range[1]);
    if t.reversed {
        (b, a)
    } else {
        (a, b)
    }
}

fn mag(p: [f64; 2]) -> f64 {
    p[0].abs().max(p[1].abs())
}

/// The signed area `1/2 ∮ (u - u0) dv - (v - v0) du` of one piece and its
/// error estimate and round-off scale.
fn piece_area(t: &PreparedTrim, o: [f64; 2]) -> Result<[f64; 3], Refusal> {
    let breaks = t.breaks(&[])?.at;
    let mut out = [0.0; 3];
    for w in breaks.windows(2) {
        let &[a, b] = w else { continue };
        let r = gk15_scalar(
            |s| {
                let (p, d) = t.eval(s);
                0.5 * ((p[0] - o[0]) * d[1] - (p[1] - o[1]) * d[0])
            },
            a,
            b,
        );
        for (x, y) in out.iter_mut().zip(r) {
            *x += y;
        }
    }
    if t.reversed {
        out[0] = -out[0];
    }
    Ok(out)
}

/// A vertex of a loop's sampled polygon: the point, and the piece and
/// parameter interval (in either order) of the curve that the chord from it
/// to the next vertex stands for.
#[derive(Clone, Copy)]
struct Vertex {
    p: [f64; 2],
    piece: usize,
    s0: f64,
    s1: f64,
}

/// The loop's polygon, 17 points per trim span, in traversal order: each
/// piece from its traversal start (included) towards its end (the next
/// piece's start).
fn polygon(pieces: &[PreparedTrim]) -> Result<Vec<Vertex>, Refusal> {
    let mut out = Vec::new();
    for (piece, t) in pieces.iter().enumerate() {
        let breaks = t.breaks(&[])?.at;
        let spans: Vec<(f64, f64)> = breaks
            .windows(2)
            .filter_map(|w| match *w {
                [a, b] => Some(if t.reversed { (b, a) } else { (a, b) }),
                _ => None,
            })
            .collect();
        let ordered: Box<dyn Iterator<Item = &(f64, f64)>> = if t.reversed {
            Box::new(spans.iter().rev())
        } else {
            Box::new(spans.iter())
        };
        for &(from, to) in ordered {
            // The span's start, then 16 points at half-step offsets.
            let mut at = vec![from];
            for k in 0..16 {
                at.push(from + (to - from) * (f64::from(k) + 0.5) / 16.0);
            }
            at.push(to);
            for w in at.windows(2) {
                let &[s0, s1] = w else { continue };
                out.push(Vertex {
                    p: t.eval(s0).0,
                    piece,
                    s0,
                    s1,
                });
            }
        }
    }
    Ok(out)
}

/// A bound on the distance of the curve that the chord `p -> q` stands for
/// (piece `v.piece` over `[v.s0, v.s1]`, from the convex hull of its control
/// points) from the chord, with round-off margin. Lines give the distance of
/// their end from the chord (zero unless the next piece starts across a gap).
fn chord_radius(pieces: &[PreparedTrim], v: &Vertex, q: [f64; 2]) -> Result<f64, Refusal> {
    let (lo, hi) = if v.s0 <= v.s1 {
        (v.s0, v.s1)
    } else {
        (v.s1, v.s0)
    };
    let hull = pieces
        .get(v.piece)
        .and_then(|t| t.hull_points(lo, hi))
        .ok_or_else(|| {
            orientation(format!(
                "piece {} cannot be bounded between its samples over [{lo}, {hi}]",
                v.piece
            ))
        })?;
    let mut r = 0.0_f64;
    let mut m = mag(v.p).max(mag(q));
    for c in &hull {
        r = r.max(distance(*c, v.p, q));
        m = m.max(mag(*c));
    }
    if !r.is_finite() {
        return Err(orientation(format!(
            "piece {} has no finite bound between its samples",
            v.piece
        )));
    }
    Ok(if r > 0.0 {
        r + 64.0 * f64::EPSILON * m
    } else {
        0.0
    })
}

/// The sign of the turn `a -> b -> c` (exact inputs, rounded product).
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// The max-norm distance from `p` to the segment `ab`.
fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    mag([p[0] - (a[0] + t * d[0]), p[1] - (a[1] + t * d[1])])
}

/// Whether segments `ab` and `cd` meet: they cross properly, or an end of one
/// lies within `tol` of the other (a touch, a vertex on an edge or a
/// collinear overlap). Two segments that do not cross are nearest at an end
/// of one of them, so this is their distance against `tol`.
fn contact(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], tol: f64) -> bool {
    let proper = orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0;
    proper
        || distance(a, c, d) <= tol
        || distance(b, c, d) <= tol
        || distance(c, a, b) <= tol
        || distance(d, a, b) <= tol
}

/// How many times the chords in contact may be halved before the loops
/// refuse (up to 256 chords per sampled chord).
const MAX_ROUNDS: usize = 8;

/// A capsule edge: the chord `a -> b` of polygon `poly`, standing for curve
/// within `r` of it, and the vertices merged into it.
struct Seg {
    poly: usize,
    index: usize,
    len: usize,
    a: [f64; 2],
    b: [f64; 2],
    r: f64,
    vertices: Vec<usize>,
}

/// The capsule edges of the polygons with chord radii `radii`: consecutive
/// vertices within `tol` of each other merge (the merged chord's radius grows
/// by `tol`), so that consecutive edges are the adjacent ones.
fn capsules(polys: &[Vec<Vertex>], radii: &[Vec<f64>], tol: f64) -> Vec<Seg> {
    let mut segs = Vec::new();
    for (pi, (poly, rs)) in polys.iter().zip(radii).enumerate() {
        // (point, radius, merged vertices) per kept vertex.
        let mut kept: Vec<([f64; 2], f64, Vec<usize>)> = Vec::new();
        for (i, (v, &r)) in poly.iter().zip(rs).enumerate() {
            match kept.last_mut() {
                Some((q, kr, vs)) if mag([v.p[0] - q[0], v.p[1] - q[1]]) <= tol => {
                    *kr = kr.max(r) + tol;
                    vs.push(i);
                }
                _ => kept.push((v.p, r, vec![i])),
            }
        }
        while kept.len() > 1 {
            let close = matches!((kept.first(), kept.last()), (Some(f), Some(l)) if mag([f.0[0] - l.0[0], f.0[1] - l.0[1]]) <= tol);
            if !close {
                break;
            }
            if let (Some((_, r, vs)), Some(first)) = (kept.pop(), kept.first_mut()) {
                first.1 = first.1.max(r) + tol;
                first.2.extend(vs);
            }
        }
        let n = kept.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let (Some(a), Some(b)) = (kept.get(i), kept.get((i + 1) % n)) else {
                continue;
            };
            segs.push(Seg {
                poly: pi,
                index: i,
                len: n,
                a: a.0,
                b: b.0,
                r: a.1,
                vertices: a.2.clone(),
            });
        }
    }
    segs
}

/// Every pair of capsule edges in contact (the chords within the sum of
/// their radii plus `tol`) that are not consecutive edges of one polygon; a
/// uniform grid keeps it near linear.
fn contacts(segs: &[Seg], tol: f64) -> Vec<(usize, usize)> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut rmax = 0.0_f64;
    for s in segs {
        rmax = rmax.max(s.r);
        for p in [s.a, s.b] {
            for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *l = l.min(x);
                *h = h.max(x);
            }
        }
    }
    let side = (segs.len() as f64).sqrt().ceil().max(1.0);
    let cells = side as usize;
    let cell = |x: f64, k: usize| -> usize {
        let (l, h) = (
            lo.get(k).copied().unwrap_or(0.0),
            hi.get(k).copied().unwrap_or(0.0),
        );
        let w = h - l;
        if w.is_nan() || w <= 0.0 {
            return 0;
        }
        let c = ((x - l) / w * side).max(0.0) as usize;
        c.min(cells - 1)
    };
    // Each segment's box, widened by its radius and the tolerance, so that
    // capsules in contact share a cell.
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); cells * cells];
    for (si, s) in segs.iter().enumerate() {
        let e = s.r + tol;
        let (x0, x1) = (
            cell(s.a[0].min(s.b[0]) - e, 0),
            cell(s.a[0].max(s.b[0]) + e, 0),
        );
        let (y0, y1) = (
            cell(s.a[1].min(s.b[1]) - e, 1),
            cell(s.a[1].max(s.b[1]) + e, 1),
        );
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(g) = grid.get_mut(x * cells + y) {
                    g.push(si);
                }
            }
        }
    }
    let mut out = Vec::new();
    for list in &grid {
        for (k, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(k + 1) {
                let (Some(s), Some(t)) = (segs.get(i), segs.get(j)) else {
                    continue;
                };
                // Consecutive edges share their joint; a triangle's edges are
                // all consecutive.
                let adjacent = s.poly == t.poly
                    && ((s.index + 1) % s.len == t.index || (t.index + 1) % t.len == s.index);
                if !adjacent && contact(s.a, s.b, t.a, t.b, s.r + t.r + tol) {
                    out.push((i, j));
                }
            }
        }
    }
    out
}

/// Whether the closed loops' curves (not only their sampled polygons) are
/// separate: each chord is a capsule whose radius bounds its curve's
/// distance from it, and capsules in contact are halved, up to
/// [`MAX_ROUNDS`] times, until none touch. Returns the final polygons'
/// points, or the loops `(x, y)` (indices into `loops`) whose capsules still
/// touch: exact chords in contact, or curved ones the halving could not
/// separate.
#[allow(clippy::type_complexity)]
fn separate(
    loops: &[&[PreparedTrim]],
) -> Result<Result<Vec<Vec<[f64; 2]>>, (usize, usize)>, Refusal> {
    let mut polys = loops
        .iter()
        .map(|pieces| polygon(pieces))
        .collect::<Result<Vec<_>, _>>()?;
    for round in 0..=MAX_ROUNDS {
        let mut radii = Vec::with_capacity(polys.len());
        for (poly, pieces) in polys.iter().zip(loops) {
            let n = poly.len();
            let mut rs = Vec::with_capacity(n);
            for (i, v) in poly.iter().enumerate() {
                let q = poly.get((i + 1) % n).map_or(v.p, |w| w.p);
                rs.push(chord_radius(pieces, v, q)?);
            }
            radii.push(rs);
        }
        let scale = polys.iter().flatten().fold(0.0_f64, |m, v| m.max(mag(v.p)));
        let tol = CONTACT_TOLERANCE * scale;
        let segs = capsules(&polys, &radii, tol);
        let found = contacts(&segs, tol);
        let Some(&(i, j)) = found.first() else {
            return Ok(Ok(polys
                .iter()
                .map(|p| p.iter().map(|v| v.p).collect())
                .collect()));
        };
        let pair = |i: usize, j: usize| {
            (
                segs.get(i).map_or(0, |s| s.poly),
                segs.get(j).map_or(0, |s| s.poly),
            )
        };
        // Exact chords in contact: the polygons themselves meet.
        if let Some(&(i, j)) = found.iter().find(|&&(i, j)| {
            segs.get(i).is_some_and(|s| s.r == 0.0) && segs.get(j).is_some_and(|s| s.r == 0.0)
        }) {
            return Ok(Err(pair(i, j)));
        }
        if round == MAX_ROUNDS {
            return Ok(Err(pair(i, j)));
        }
        let mut split: Vec<Vec<bool>> = polys.iter().map(|p| vec![false; p.len()]).collect();
        for &(i, j) in &found {
            for s in [segs.get(i), segs.get(j)].into_iter().flatten() {
                if s.r > 0.0 {
                    if let Some(marks) = split.get_mut(s.poly) {
                        for &v in &s.vertices {
                            if let Some(m) = marks.get_mut(v) {
                                *m = true;
                            }
                        }
                    }
                }
            }
        }
        for ((poly, marks), pieces) in polys.iter_mut().zip(&split).zip(loops) {
            let mut next = Vec::with_capacity(poly.len());
            for (v, &m) in poly.iter().zip(marks) {
                if !m {
                    next.push(*v);
                    continue;
                }
                let mid = 0.5 * (v.s0 + v.s1);
                let p = pieces.get(v.piece).map_or(v.p, |t| t.eval(mid).0);
                next.push(Vertex { s1: mid, ..*v });
                next.push(Vertex {
                    p,
                    piece: v.piece,
                    s0: mid,
                    s1: v.s1,
                });
            }
            *poly = next;
        }
    }
    Ok(Err((0, 0)))
}

/// Whether `p` is inside the closed polygon `poly` (crossing number).
fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else {
            continue;
        };
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                c = !c;
            }
        }
    }
    c
}

/// The midpoints of a loop's pieces.
fn midpoints(pieces: &[PreparedTrim]) -> Vec<[f64; 2]> {
    pieces
        .iter()
        .map(|t| t.eval(0.5 * (t.range[0] + t.range[1])).0)
        .collect()
}

fn segment_area(p: [f64; 2], q: [f64; 2], o: [f64; 2]) -> f64 {
    let (a, b) = ([p[0] - o[0], p[1] - o[1]], [q[0] - o[0], q[1] - o[1]]);
    0.5 * (a[0] * b[1] - a[1] * b[0])
}

fn open(detail: String) -> Refusal {
    Refusal::new(RefusalCode::OpenTrimLoop, detail)
}

fn orientation(detail: String) -> Refusal {
    Refusal::new(RefusalCode::TrimOrientation, detail)
}

/// The periods of the directions in which `surface` may be closed: `2 pi`
/// around the axis of the kinds of revolution (and around the torus's tube),
/// the domain's width for a net. A winding loop must also meet itself on the
/// surface.
fn periods(surface: &Prepared) -> [Option<f64>; 2] {
    let tau = Some(std::f64::consts::TAU);
    match surface {
        Prepared::Plane { .. } => [None, None],
        Prepared::Ruled { .. } | Prepared::Sphere { .. } => [tau, None],
        Prepared::Torus { .. } => [tau, tau],
        Prepared::Nurbs(_) => {
            let [u, v] = surface.domain();
            [Some(u[1] - u[0]), Some(v[1] - v[0])]
        }
    }
}

fn point(surface: &Prepared, row: &mut Row, p: [f64; 2]) -> crate::vec::Vec3 {
    surface.row(p[0], row);
    surface.eval(row, p[1]).0
}

/// Checks the loops of a validated face.
pub fn check(
    face: &FaceDefinition,
    surface: &Prepared,
    trims: &[PreparedTrim],
) -> Result<Loops, Refusal> {
    let periods = periods(surface);
    let mut gaps = Vec::new();
    let mut row = Row::default();
    // (signed area, degenerate, pieces) per closed loop; (direction, sign) per
    // winding loop.
    let mut closed: Vec<(f64, bool, &[PreparedTrim])> = Vec::new();
    let mut winding: Vec<(usize, f64)> = Vec::new();
    let mut start = 0usize;
    for (l, &count) in face.loops.iter().enumerate() {
        let pieces = trims.get(start..start + count).unwrap_or(&[]);
        start += count;
        let Some(first) = pieces.first() else {
            continue;
        };
        let o = ends(first).0;
        let mut area = [0.0; 3];
        let mut winds = None;
        for (i, t) in pieces.iter().enumerate() {
            let a = piece_area(t, o)?;
            for (x, y) in area.iter_mut().zip(a) {
                *x += y;
            }
            let last = i + 1 == pieces.len();
            let next = pieces.get(if last { 0 } else { i + 1 }).unwrap_or(first);
            let (p, q) = (ends(t).1, ends(next).0);
            let scale = 1.0 + mag(p).max(mag(q));
            let mut target = q;
            let diff = [q[0] - p[0], q[1] - p[1]];
            if mag(diff) > GAP_TOLERANCE * scale {
                let wrap = periods.iter().enumerate().find_map(|(dir, period)| {
                    let period = (*period)?;
                    let other = diff.get(1 - dir).copied().unwrap_or(f64::NAN);
                    let along = diff.get(dir).copied().unwrap_or(f64::NAN);
                    let residual = (along.abs() - period).abs();
                    (last
                        && other.abs() <= GAP_TOLERANCE * scale
                        && residual <= GAP_TOLERANCE * scale)
                        .then_some((dir, along.signum() * period))
                });
                let Some((dir, shift)) = wrap else {
                    return Err(open(format!(
                        "loop {l}: piece {i} ends at {p:?}, the next starts at {q:?}"
                    )));
                };
                let (sp, sq) = (point(surface, &mut row, p), point(surface, &mut row, q));
                let d = (sp - sq).max_abs();
                if d.is_nan() || d > WIND_TOLERANCE * (1.0 + sp.max_abs().max(sq.max_abs())) {
                    return Err(open(format!(
                        "loop {l} shifts by a period in direction {dir} but its ends are {d} apart on the surface"
                    )));
                }
                if let Some(x) = target.get_mut(dir) {
                    *x -= shift;
                }
                if dir == 1 {
                    // The boundary integral runs along u; the cut a v-winding
                    // pair needs is not integrated.
                    return Err(Refusal::new(
                        RefusalCode::UnsupportedTrim,
                        format!("loop {l} winds around the surface in v"),
                    ));
                }
                winds = Some((dir, shift.signum()));
            }
            let gap = [target[0] - p[0], target[1] - p[1]];
            if mag(gap) > NOISE_ULPS * f64::EPSILON * scale {
                gaps.push((p, target));
            }
            if winds.is_none() || !last {
                area[0] += segment_area(p, target, o);
            }
        }
        match winds {
            Some(w) => winding.push(w),
            None => {
                let [a, est, abs] = area;
                if !(a.is_finite() && est.is_finite() && abs.is_finite()) {
                    return Err(Refusal::new(
                        RefusalCode::NonFinite,
                        format!("loop {l}: signed parameter area {a} is not finite"),
                    ));
                }
                let degenerate = a.abs() <= 10.0 * est + NOISE_ULPS * f64::EPSILON * abs;
                closed.push((a, degenerate, pieces));
            }
        }
    }
    let live: Vec<(usize, f64)> = closed
        .iter()
        .enumerate()
        .filter(|(_, (_, d, _))| !d)
        .map(|(i, (a, _, _))| (i, *a))
        .collect();
    if winding.is_empty() {
        let outer = live
            .iter()
            .max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))
            .map(|(i, _)| *i);
        for &(i, a) in &live {
            let is_outer = Some(i) == outer;
            if is_outer != (a > 0.0) {
                return Err(orientation(format!(
                    "closed loop {i} of {} has signed parameter area {a}: {}",
                    closed.len(),
                    if is_outer {
                        "the outer loop is clockwise"
                    } else {
                        "a hole is counter-clockwise"
                    }
                )));
            }
        }
        if let Some(o) = outer {
            let loops: Vec<&[PreparedTrim]> = live
                .iter()
                .map(|&(i, _)| closed.get(i).map_or(&[][..], |c| c.2))
                .collect();
            let polys = match separate(&loops)? {
                Ok(polys) => polys,
                Err((x, y)) => {
                    let (x, y) = (
                        live.get(x).map_or(x, |l| l.0),
                        live.get(y).map_or(y, |l| l.0),
                    );
                    return Err(orientation(if x == y {
                        format!("closed loop {x} crosses or touches itself")
                    } else {
                        format!("closed loops {x} and {y} cross or touch")
                    }));
                }
            };
            let poly_of = |i: usize| {
                live.iter()
                    .position(|l| l.0 == i)
                    .and_then(|k| polys.get(k))
            };
            let outer_poly = poly_of(o).map_or(&[][..], |p| &p[..]);
            for &(i, _) in &live {
                if i == o {
                    continue;
                }
                let pieces = closed.get(i).map_or(&[][..], |c| c.2);
                let mids = midpoints(pieces);
                if !mids.iter().any(|p| inside(*p, outer_poly)) {
                    return Err(orientation(format!(
                        "hole {i} lies outside the outer loop {o}"
                    )));
                }
                for &(j, _) in &live {
                    if j == o || j == i {
                        continue;
                    }
                    let other = poly_of(j).map_or(&[][..], |p| &p[..]);
                    if !mids.is_empty() && mids.iter().all(|p| inside(*p, other)) {
                        return Err(orientation(format!("hole {i} lies inside hole {j}")));
                    }
                }
            }
        }
    } else {
        for dir in 0..2 {
            let net: f64 = winding.iter().filter(|w| w.0 == dir).map(|w| w.1).sum();
            if net != 0.0 {
                return Err(orientation(format!(
                    "loops winding in direction {dir} do not pair (net {net})"
                )));
            }
        }
        if let Some(&(i, a)) = live.iter().find(|(_, a)| *a > 0.0) {
            return Err(orientation(format!(
                "closed loop {i} on a face with winding loops is counter-clockwise (area {a})"
            )));
        }
    }
    let orientation = if live.is_empty() {
        LoopOrientation::Undecided
    } else {
        LoopOrientation::Decided
    };
    Ok(Loops { gaps, orientation })
}

impl FaceDefinition {
    /// Checks the face and its loops' chaining, closure and orientation (the
    /// integrator does this for every face); adapters call it to orient their
    /// loops.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`], [`RefusalCode::OpenTrimLoop`] or
    /// [`RefusalCode::TrimOrientation`].
    pub fn check_loops(&self) -> Result<LoopOrientation, Refusal> {
        self.validate()?;
        let surface = Prepared::new(&self.surface);
        let trims: Vec<PreparedTrim> = self.trims.iter().map(PreparedTrim::new).collect();
        check(self, &surface, &trims).map(|l| l.orientation)
    }
}
