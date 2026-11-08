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
//! loops, or between non-adjacent edges of one loop, refuses. Consecutive
//! edges (and so each edge) must have a control polygon monotone in some
//! direction, which shows their curves meet only at the joint; folds that
//! halving cannot straighten refuse. No hole may lie inside another. On a
//! face with winding loops the holes and the winding loops are separated
//! the same way at every periodic image (whole periods in u) that reaches
//! the loops' u-window, each winding loop's images chained into one open
//! polyline joined at the seam; each hole must then lie in the band between
//! one pair of winding loops and inside no other hole, and anything else
//! (more winding loops with holes, a band through the v seam, too many
//! periods) refuses. Loops winding in v are not supported and refuse.

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
    /// The periodic image's shift in u (`p` includes it).
    shift: f64,
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
                    shift: 0.0,
                });
            }
        }
    }
    Ok(out)
}

/// The points whose convex hull contains the curve that vertex `v`'s chord
/// stands for (piece `v.piece` over `[v.s0, v.s1]`), in traversal order and
/// shifted by the vertex's image shift in u, into `hull` (cleared first).
fn chord_hull_into(
    pieces: &[PreparedTrim],
    v: &Vertex,
    hull: &mut Vec<[f64; 2]>,
) -> Result<(), Refusal> {
    let (lo, hi) = if v.s0 <= v.s1 {
        (v.s0, v.s1)
    } else {
        (v.s1, v.s0)
    };
    let bounded = pieces
        .get(v.piece)
        .is_some_and(|t| t.hull_points_into(lo, hi, hull));
    if !bounded {
        return Err(orientation(format!(
            "piece {} cannot be bounded between its samples over [{lo}, {hi}]",
            v.piece
        )));
    }
    if v.s0 > v.s1 {
        hull.reverse();
    }
    for c in hull.iter_mut() {
        c[0] += v.shift;
    }
    Ok(())
}

/// A bound on the distance of the curve within `hull` from the chord
/// `p -> q`, with round-off margin: zero for a chord that is its own curve.
fn chord_radius(hull: &[[f64; 2]], p: [f64; 2], q: [f64; 2]) -> Result<f64, Refusal> {
    let mut r = 0.0_f64;
    let mut m = mag(p).max(mag(q));
    for c in hull {
        r = r.max(distance(*c, p, q));
        m = m.max(mag(*c));
    }
    if !r.is_finite() {
        return Err(orientation(
            "a trim has no finite bound between its samples".to_string(),
        ));
    }
    Ok(if r > 0.0 {
        r + 64.0 * f64::EPSILON * m
    } else {
        0.0
    })
}

/// Whether the polyline `points` is strictly monotone in some direction:
/// every edge longer than `tol` within an open half-plane of directions,
/// with a margin for the edges' round-off. A curve inside the convex hulls
/// of such a control polygon (positive weights: variation diminishing) meets
/// every line across that direction at most once, so it does not cross or
/// touch itself.
fn monotone(points: &[[f64; 2]], tol: f64, scale: f64) -> bool {
    monotone_joined(points, &[], tol, scale)
}

/// [`monotone`] of the polyline `a` followed by `b`.
fn monotone_joined(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64, scale: f64) -> bool {
    const SMALL: usize = 64;
    let n = (a.len() + b.len()).saturating_sub(1);
    let mut small = [0.0_f64; SMALL];
    let mut large = Vec::new();
    if n > SMALL {
        large.resize(n, 0.0);
    }
    let buffer: &mut [f64] = if n > SMALL { &mut large } else { &mut small };
    let mut count = 0usize;
    let mut margin = 1e-12_f64;
    let mut points = a.iter().chain(b.iter());
    let Some(mut prev) = points.next().copied() else {
        return true;
    };
    for &next in points {
        let (a, b) = (prev, next);
        prev = next;
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = mag(d);
        if !len.is_finite() {
            return false;
        }
        if len <= tol {
            continue;
        }
        margin = margin.max(1e3 * f64::EPSILON * scale / len);
        if let Some(slot) = buffer.get_mut(count) {
            *slot = libm::atan2(d[1], d[0]);
            count += 1;
        }
    }
    let angles = buffer.get_mut(..count).unwrap_or_default();
    if angles.len() < 2 {
        return true;
    }
    angles.sort_by(f64::total_cmp);
    let mut gap = 0.0_f64;
    for w in angles.windows(2) {
        if let &[x, y] = w {
            gap = gap.max(y - x);
        }
    }
    if let (Some(&f), Some(&l)) = (angles.first(), angles.last()) {
        gap = gap.max(f + std::f64::consts::TAU - l);
    }
    gap > std::f64::consts::PI + 2.0 * margin
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
    proper(a, b, c, d)
        || distance(a, c, d) <= tol
        || distance(b, c, d) <= tol
        || distance(c, a, b) <= tol
        || distance(d, a, b) <= tol
}

/// The smallest arc of directions from `j` containing every point of `pts`
/// farther than `tol` from it, as `(start, width)`, when narrower than pi
/// (and there is such a point).
fn arc(j: [f64; 2], pts: &[[f64; 2]], tol: f64) -> Option<(f64, f64)> {
    let mut angles: Vec<f64> = pts
        .iter()
        .filter(|p| mag([p[0] - j[0], p[1] - j[1]]) > tol)
        .map(|p| libm::atan2(p[1] - j[1], p[0] - j[0]))
        .collect();
    if angles.iter().any(|a| !a.is_finite()) {
        return None;
    }
    angles.sort_by(f64::total_cmp);
    let (&first, &last) = (angles.first()?, angles.last()?);
    // The largest gap between consecutive directions; the arc is the rest.
    let (mut gap, mut start) = (first + std::f64::consts::TAU - last, first);
    for w in angles.windows(2) {
        if let &[x, y] = w {
            if y - x > gap {
                gap = y - x;
                start = y;
            }
        }
    }
    let width = std::f64::consts::TAU - gap;
    (width < std::f64::consts::PI).then_some((start, width))
}

/// Whether a line through the joint `j` separates `closed` (every control
/// point on it or on its non-positive side) from `strict` (traversed away
/// from the joint; every control point past its first two strictly on the
/// positive side, its tangent point not below, and its first point strictly
/// above when it starts across a gap): `strict` then leaves the line at the
/// joint and the two curves meet only there, up to the round-off `eps` of
/// a tangent cusp.
fn line_separates(
    j: [f64; 2],
    normal: [f64; 2],
    closed: &[[f64; 2]],
    strict: &[[f64; 2]],
    tol: f64,
    eps: f64,
) -> bool {
    let d = |p: &[f64; 2]| normal[0] * (p[0] - j[0]) + normal[1] * (p[1] - j[1]);
    if !closed.iter().all(|p| d(p) <= eps) {
        return false;
    }
    strict.iter().enumerate().all(|(i, p)| match i {
        0 => mag([p[0] - j[0], p[1] - j[1]]) <= tol || d(p) > eps,
        1 => d(p) >= -eps,
        _ => d(p) > eps,
    }) && strict.len() > 2
}

/// Whether consecutive edges with hulls `a` (ending at the joint) and `b`
/// (starting there, or across a gap) meet only at their joint, by one of:
/// their joined control polygon (with the gap) is monotone; each is
/// monotone and seen from the joint they lie in disjoint wedges narrower
/// than pi (a corner, or a cusp whose sides part; beyond a gap between the
/// pieces the curves may meet only within a thousand times the gap of the
/// joint, the scale of the accepted closing segment); or each is monotone
/// and a line through the joint along either tangent separates them (a
/// tangent cusp, up to round-off at the joint). A tangent cusp whose sides
/// curve the same way (a crescent) cannot be decided from hulls and fails.
fn joint_simple(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64, scale: f64) -> bool {
    if monotone_joined(a, b, tol, scale) {
        return true;
    }
    if !(monotone(a, tol, scale) && monotone(b, tol, scale)) {
        return false;
    }
    let Some(&j) = a.last() else {
        return true;
    };
    // Seen from the joint, the two hulls lie in cones whose directions part
    // by `delta`: they meet only at the joint. Or, leaving out points within
    // the gap `g` to `b`'s start (the closing segment's own length), the
    // cones thickened by `g` meet only within `2 g / sin(delta / 2)` of the
    // joint, which must stay within a thousand times the gap and tolerance.
    let g = b
        .first()
        .map_or(0.0, |b0| mag([b0[0] - j[0], b0[1] - j[1]]));
    for excluded in [0.0, g] {
        let (Some((sa, wa)), Some((sb, wb))) = (arc(j, a, excluded), arc(j, b, excluded)) else {
            continue;
        };
        let tau = std::f64::consts::TAU;
        let delta = ((sb - sa).rem_euclid(tau) - wa).min((sa - sb).rem_euclid(tau) - wb);
        let margin = 1e-12 + 1e3 * f64::EPSILON;
        let reach = 2.0 * excluded / libm::sin(0.5 * delta.min(std::f64::consts::PI));
        if delta > margin && (excluded == 0.0 || reach <= 1e3 * (excluded + tol)) {
            return true;
        }
    }
    // The tangent lines: towards the joint along `a`, away from it along
    // `b`.
    let far = |pts: &[[f64; 2]]| {
        pts.iter()
            .find(|p| mag([p[0] - j[0], p[1] - j[1]]) > tol)
            .map(|p| [p[0] - j[0], p[1] - j[1]])
    };
    let eps = 16.0 * f64::EPSILON * scale;
    let back: Vec<[f64; 2]> = a.iter().rev().copied().collect();
    let tangents = [far(&back), far(b)];
    for t in tangents.into_iter().flatten() {
        let len = (t[0] * t[0] + t[1] * t[1]).sqrt();
        if len.is_nan() || len <= 0.0 {
            continue;
        }
        let n = [-t[1] / len, t[0] / len];
        for normal in [n, [-n[0], -n[1]]] {
            if line_separates(j, normal, a, b, tol, eps)
                || line_separates(j, normal, b, &back, tol, eps)
            {
                return true;
            }
        }
    }
    false
}

/// How many times the chords in contact may be halved before the loops
/// refuse (up to 256 chords per sampled chord).
const MAX_ROUNDS: usize = 8;

/// A loop to separate: its pieces, whether it is open (a winding loop: no
/// edge from its end back to its start) and the u shifts of its periodic
/// images. A closed loop has one image per ring; an open one chains its
/// images in order, each ending where the next begins (their shared seam
/// joint is an ordinary joint between consecutive edges).
struct Ring<'a> {
    pieces: &'a [PreparedTrim],
    open: bool,
    shifts: Vec<f64>,
}

/// The most periodic images a loop may need before the face refuses.
const MAX_IMAGES: usize = 16;

/// One ring's sampled vertices, and per vertex with an outgoing edge its
/// hull and radius.
struct Sampled<'a> {
    verts: &'a [Vertex],
    hulls: &'a Hulls,
    edges: usize,
    open: bool,
}

impl Sampled<'_> {
    /// Vertex `i`'s hull, if it has an outgoing edge.
    fn hull(&self, i: usize) -> Option<&[[f64; 2]]> {
        if i >= self.edges {
            return None;
        }
        let &(start, len) = self.hulls.at.get(i)?.as_ref()?;
        self.hulls.points.get(start..start + len)
    }

    /// Vertex `i`'s radius (zero without an outgoing edge).
    fn radius(&self, i: usize) -> f64 {
        if i >= self.edges {
            return 0.0;
        }
        self.hulls.radii.get(i).copied().unwrap_or(0.0)
    }
}

/// One ring's per-vertex hulls (`(start, len)` into `points`, `None` until
/// computed) and radii, kept across rounds.
#[derive(Default)]
struct Hulls {
    points: Vec<[f64; 2]>,
    at: Vec<Option<(usize, usize)>>,
    radii: Vec<f64>,
}

/// A capsule edge: the chord `a -> b` of ring `poly`, standing for curve
/// within `r` of it, and the vertices merged into it.
struct Seg {
    poly: usize,
    index: usize,
    len: usize,
    open: bool,
    a: [f64; 2],
    b: [f64; 2],
    r: f64,
    vertices: Vec<usize>,
}

impl Seg {
    /// Whether `t` follows `self` along one ring.
    fn followed_by(&self, t: &Self) -> bool {
        self.poly == t.poly
            && if self.open {
                self.index + 1 == t.index
            } else {
                (self.index + 1) % self.len == t.index
            }
    }
}

/// The capsule edges of the rings: consecutive vertices within `tol` of each
/// other merge (the merged chord's radius grows by `tol`), so that
/// consecutive edges are the adjacent ones.
fn capsules(rings: &[Sampled<'_>], tol: f64) -> Vec<Seg> {
    let mut segs = Vec::new();
    for (pi, ring) in rings.iter().enumerate() {
        // (point, radius, merged vertices) per kept vertex.
        let mut kept: Vec<([f64; 2], f64, Vec<usize>)> = Vec::new();
        for (i, v) in ring.verts.iter().enumerate() {
            let r = ring.radius(i);
            match kept.last_mut() {
                Some((q, kr, vs)) if mag([v.p[0] - q[0], v.p[1] - q[1]]) <= tol => {
                    *kr = kr.max(r) + tol;
                    vs.push(i);
                }
                _ => kept.push((v.p, r, vec![i])),
            }
        }
        while !ring.open && kept.len() > 1 {
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
        let edges = if ring.open { n - 1 } else { n };
        for i in 0..edges {
            let Some(b) = kept.get((i + 1) % n).map(|b| b.0) else {
                continue;
            };
            let Some(a) = kept.get_mut(i) else { continue };
            segs.push(Seg {
                poly: pi,
                index: i,
                len: n,
                open: ring.open,
                a: a.0,
                b,
                r: a.1,
                vertices: std::mem::take(&mut a.2),
            });
        }
    }
    segs
}

/// Whether segments `ab` and `cd` cross properly, as [`contact`] decides it.
fn proper(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0
}

/// Every pair of capsule edges in contact (the chords within the sum of
/// their radii plus `tol`) that are not consecutive edges of one ring; a
/// uniform grid keeps it near linear. Each pair is tested once, in the first
/// cell the two share. A pair whose boxes are apart by more than the sum of
/// their radii, the tolerance and a margin above the distances' round-off
/// cannot be within that distance, so only [`contact`]'s proper-crossing
/// test (the same expression) is evaluated for it.
fn contacts(segs: &[Seg], tol: f64) -> Vec<(usize, usize)> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for s in segs {
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
    // capsules in contact share a cell; and its plain box with the margin.
    let mut ranges: Vec<[usize; 4]> = Vec::with_capacity(segs.len());
    let mut boxes: Vec<([f64; 2], [f64; 2], f64)> = Vec::with_capacity(segs.len());
    // The grid's lists, one after another: `starts[c]..starts[c + 1]` of
    // `members` are cell `c`'s segments, in index order.
    let mut starts = vec![0usize; cells * cells + 1];
    for s in segs {
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
                if let Some(c) = starts.get_mut(x * cells + y + 1) {
                    *c += 1;
                }
            }
        }
        ranges.push([x0, x1, y0, y1]);
        boxes.push((
            [s.a[0].min(s.b[0]), s.a[1].min(s.b[1])],
            [s.a[0].max(s.b[0]), s.a[1].max(s.b[1])],
            64.0 * f64::EPSILON * (mag(s.a).max(mag(s.b)) + s.r + tol),
        ));
    }
    for c in 1..starts.len() {
        let prev = starts.get(c - 1).copied().unwrap_or(0);
        if let Some(x) = starts.get_mut(c) {
            *x += prev;
        }
    }
    let mut fill: Vec<usize> = starts.clone();
    let mut members = vec![0usize; starts.last().copied().unwrap_or(0)];
    for (si, r) in ranges.iter().enumerate() {
        for x in r[0]..=r[1] {
            for y in r[2]..=r[3] {
                if let Some(at) = fill.get_mut(x * cells + y) {
                    if let Some(m) = members.get_mut(*at) {
                        *m = si;
                    }
                    *at += 1;
                }
            }
        }
    }
    let mut out = Vec::new();
    for c in 0..cells * cells {
        let (Some(&lo), Some(&hi)) = (starts.get(c), starts.get(c + 1)) else {
            continue;
        };
        let list = members.get(lo..hi).unwrap_or_default();
        let (cx, cy) = (c / cells, c % cells);
        for (k, &i) in list.iter().enumerate() {
            let (Some(s), Some(ri), Some(bi)) = (segs.get(i), ranges.get(i), boxes.get(i)) else {
                continue;
            };
            for &j in list.iter().skip(k + 1) {
                let Some(rj) = ranges.get(j) else { continue };
                // Tested in the first cell the two share only.
                if ri[0].max(rj[0]) != cx || ri[2].max(rj[2]) != cy {
                    continue;
                }
                let (Some(t), Some(bj)) = (segs.get(j), boxes.get(j)) else {
                    continue;
                };
                let reach = s.r + t.r + tol;
                let gap = (bi.0[0] - bj.1[0])
                    .max(bj.0[0] - bi.1[0])
                    .max(bi.0[1] - bj.1[1])
                    .max(bj.0[1] - bi.1[1]);
                let touch = if gap > reach + bi.2 + bj.2 {
                    proper(s.a, s.b, t.a, t.b)
                } else {
                    contact(s.a, s.b, t.a, t.b, reach)
                };
                // Consecutive edges share their joint; whether their curves
                // meet elsewhere is decided by [`monotone`].
                if touch && !(s.followed_by(t) || t.followed_by(s)) {
                    out.push((i, j));
                }
            }
        }
    }
    out
}

/// Whether the rings' curves (not only their sampled polygons) are simple
/// and separate. Each chord is a capsule whose radius bounds its curve's
/// distance from it; capsules of non-consecutive edges must not touch, and
/// each pair of consecutive edges (and so each edge) must have a monotone
/// control polygon ([`monotone`]), so their curves meet only at the joint.
/// Edges that fail are halved, up to [`MAX_ROUNDS`] times. Returns the final
/// polygons' points (shifted), or the rings `(x, y)` that still touch, or
/// `(x, x)` for a ring that cannot be shown simple.
#[allow(clippy::type_complexity)]
fn separate(rings: &[Ring<'_>]) -> Result<Result<Vec<Vec<[f64; 2]>>, (usize, usize)>, Refusal> {
    let mut polys: Vec<Vec<Vertex>> = Vec::with_capacity(rings.len());
    for ring in rings {
        let base = polygon(ring.pieces)?;
        let mut verts = Vec::new();
        for (k, &shift) in ring.shifts.iter().enumerate() {
            verts.extend(base.iter().map(|v| Vertex {
                p: [v.p[0] + shift, v.p[1]],
                shift,
                ..*v
            }));
            if ring.open && k + 1 == ring.shifts.len() {
                // The end of the last piece closes the polyline.
                if let Some(last) = base.last().copied() {
                    let end = last.s1;
                    let p = ring
                        .pieces
                        .get(last.piece)
                        .map_or(last.p, |t| t.eval(end).0);
                    verts.push(Vertex {
                        p: [p[0] + shift, p[1]],
                        piece: last.piece,
                        s0: end,
                        s1: end,
                        shift,
                    });
                }
            }
        }
        polys.push(verts);
    }
    let mut cache: Vec<Hulls> = polys.iter().map(|_| Hulls::default()).collect();
    let mut scratch: Vec<[f64; 2]> = Vec::new();
    for round in 0..=MAX_ROUNDS {
        let scale = polys.iter().flatten().fold(0.0_f64, |m, v| m.max(mag(v.p)));
        let tol = CONTACT_TOLERANCE * scale;
        // Each edge's hull and radius, kept from the round before unless its
        // vertex was split (an edge's next vertex keeps its point when that
        // vertex is split).
        for ((verts, ring), hulls) in polys.iter().zip(rings).zip(cache.iter_mut()) {
            let n = verts.len();
            let edges = if ring.open { n.saturating_sub(1) } else { n };
            hulls.at.resize(n, None);
            hulls.radii.resize(n, 0.0);
            for (i, v) in verts.iter().enumerate().take(edges) {
                if hulls.at.get(i).is_some_and(Option::is_some) {
                    continue;
                }
                let q = verts.get((i + 1) % n).map_or(v.p, |w| w.p);
                chord_hull_into(ring.pieces, v, &mut scratch)?;
                let r = chord_radius(&scratch, v.p, q)?;
                let start = hulls.points.len();
                hulls.points.extend_from_slice(&scratch);
                if let (Some(a), Some(rr)) = (hulls.at.get_mut(i), hulls.radii.get_mut(i)) {
                    *a = Some((start, scratch.len()));
                    *rr = r;
                }
            }
        }
        let sampled: Vec<Sampled<'_>> = polys
            .iter()
            .zip(rings)
            .zip(&cache)
            .map(|((verts, ring), hulls)| {
                let n = verts.len();
                Sampled {
                    verts,
                    hulls,
                    edges: if ring.open { n.saturating_sub(1) } else { n },
                    open: ring.open,
                }
            })
            .collect();
        let segs = capsules(&sampled, tol);
        let found = contacts(&segs, tol);
        // Each capsule edge's hull: its merged vertices' hulls in order.
        let mut flat: Vec<[f64; 2]> = Vec::new();
        let mut offsets: Vec<usize> = Vec::with_capacity(segs.len() + 1);
        offsets.push(0);
        for s in &segs {
            let ring = sampled.get(s.poly);
            for &v in &s.vertices {
                if let Some(h) = ring.and_then(|r| r.hull(v)) {
                    flat.extend_from_slice(h);
                }
            }
            offsets.push(flat.len());
        }
        // Consecutive edges whose joined control polygon is not monotone.
        let mut folds = Vec::new();
        for (i, s) in segs.iter().enumerate() {
            let next = segs
                .get(i + 1)
                .filter(|t| s.followed_by(t))
                .map(|_| i + 1)
                .or_else(|| {
                    // A closed ring's last edge is followed by its first.
                    (!s.open && s.index + 1 == s.len)
                        .then(|| segs.iter().position(|t| s.followed_by(t)))
                        .flatten()
                });
            let hull = |k: usize| -> &[[f64; 2]] {
                let (Some(&lo), Some(&hi)) = (offsets.get(k), offsets.get(k + 1)) else {
                    return &[];
                };
                flat.get(lo..hi).unwrap_or_default()
            };
            let simple = match next {
                Some(k) => joint_simple(hull(i), hull(k), tol, scale),
                None => monotone(hull(i), tol, scale),
            };
            if !simple {
                folds.push((i, next.unwrap_or(i)));
            }
        }
        if found.is_empty() && folds.is_empty() {
            return Ok(Ok(polys
                .iter()
                .map(|p| p.iter().map(|v| v.p).collect())
                .collect()));
        }
        let pair = |i: usize, j: usize| {
            (
                segs.get(i).map_or(0, |s| s.poly),
                segs.get(j).map_or(0, |s| s.poly),
            )
        };
        let all: Vec<(usize, usize)> = found.iter().chain(&folds).copied().collect();
        // Straight chords in contact or folding back: the polygons themselves
        // meet, and halving cannot help.
        let straight = |k: usize| segs.get(k).is_some_and(|s| s.r == 0.0);
        if let Some(&(i, j)) = all.iter().find(|&&(i, j)| straight(i) && straight(j)) {
            return Ok(Err(pair(i, j)));
        }
        if round == MAX_ROUNDS {
            if let Some(&(i, j)) = all.first() {
                return Ok(Err(pair(i, j)));
            }
        }
        let mut split: Vec<Vec<bool>> = polys.iter().map(|p| vec![false; p.len()]).collect();
        for &(i, j) in &all {
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
        drop(sampled);
        drop(segs);
        for (((poly, marks), ring), hulls) in polys
            .iter_mut()
            .zip(&split)
            .zip(rings)
            .zip(cache.iter_mut())
        {
            let mut next = Vec::with_capacity(poly.len());
            let mut kept = Hulls {
                points: Vec::with_capacity(hulls.points.len()),
                at: Vec::with_capacity(poly.len()),
                radii: Vec::with_capacity(poly.len()),
            };
            for (i, (v, &m)) in poly.iter().zip(marks).enumerate() {
                if !m || v.s0 == v.s1 {
                    next.push(*v);
                    let at = hulls.at.get(i).copied().flatten().and_then(|(start, len)| {
                        let h = hulls.points.get(start..start + len)?;
                        let at = kept.points.len();
                        kept.points.extend_from_slice(h);
                        Some((at, len))
                    });
                    kept.at.push(at);
                    kept.radii.push(hulls.radii.get(i).copied().unwrap_or(0.0));
                    continue;
                }
                kept.at.extend([None, None]);
                kept.radii.extend([0.0, 0.0]);
                let mid = 0.5 * (v.s0 + v.s1);
                let p = ring.pieces.get(v.piece).map_or(v.p, |t| {
                    let p = t.eval(mid).0;
                    [p[0] + v.shift, p[1]]
                });
                next.push(Vertex { s1: mid, ..*v });
                next.push(Vertex { p, s0: mid, ..*v });
            }
            *poly = next;
            *hulls = kept;
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
    // (signed area, degenerate, pieces) per closed loop; (direction, sign,
    // face loop, pieces) per winding loop.
    let mut closed: Vec<(f64, bool, &[PreparedTrim])> = Vec::new();
    let mut winding: Vec<(usize, f64, usize, &[PreparedTrim])> = Vec::new();
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
            Some((dir, sign)) => winding.push((dir, sign, l, pieces)),
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
            let rings: Vec<Ring<'_>> = live
                .iter()
                .map(|&(i, _)| Ring {
                    pieces: closed.get(i).map_or(&[][..], |c| c.2),
                    open: false,
                    shifts: vec![0.0],
                })
                .collect();
            let polys = match separate(&rings)? {
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
        // The holes and the winding loops must be simple and separate on the
        // surface, not only in the parameter plane: every loop stands at each
        // of its periodic images (shifts of whole periods in u) that reach
        // the u-window of all the loops, so any two meet in the plane wherever
        // they meet on the surface. A winding loop's images chain into one
        // open polyline joined at the seam. Only u-winding loops reach here.
        let period = periods.first().copied().flatten().unwrap_or(f64::NAN);
        let extent = |pieces: &[PreparedTrim]| {
            pieces
                .iter()
                .fold([f64::INFINITY, f64::NEG_INFINITY], |e, t| {
                    let [lo, hi] = t.hull();
                    [e[0].min(lo[0]), e[1].max(hi[0])]
                })
        };
        let holes: Vec<(usize, &[PreparedTrim])> = live
            .iter()
            .map(|&(i, _)| (i, closed.get(i).map_or(&[][..], |c| c.2)))
            .collect();
        let mut window = [f64::INFINITY, f64::NEG_INFINITY];
        for pieces in holes.iter().map(|h| h.1).chain(winding.iter().map(|w| w.3)) {
            let [a, b] = extent(pieces);
            window = [window[0].min(a), window[1].max(b)];
        }
        // The images `k` (shift `k * step`) of a loop spanning `[a, b]` in u
        // that reach the window, in order.
        // `pad`: how far beyond the window the images must reach.
        let images = |[a, b]: [f64; 2], step: f64, pad: f64| -> Result<Vec<f64>, Refusal> {
            let slack = 1e-9 * period + pad;
            let (k0, k1) = (
                (window[0] - slack - b) / step,
                (window[1] + slack - a) / step,
            );
            let (lo, hi) = (k0.min(k1).ceil(), k0.max(k1).floor());
            let count = hi - lo + 1.0;
            if !(count.is_finite() && period > 0.0 && (1.0..=MAX_IMAGES as f64).contains(&count)) {
                return Err(orientation(format!(
                    "the loops span u [{}, {}], beyond {MAX_IMAGES} periods of {period}",
                    window[0], window[1]
                )));
            }
            Ok((0..count as usize)
                .map(|k| (lo + k as f64) * step)
                .collect())
        };
        let mut rings: Vec<Ring<'_>> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        // (hole, its ring at shift 0) and (hole, ring) for every image.
        let mut originals: Vec<(usize, usize)> = Vec::new();
        let mut hole_rings: Vec<(usize, usize)> = Vec::new();
        for &(i, pieces) in &holes {
            for shift in images(extent(pieces), period, 0.0)? {
                if shift == 0.0 {
                    originals.push((i, rings.len()));
                }
                hole_rings.push((i, rings.len()));
                rings.push(Ring {
                    pieces,
                    open: false,
                    shifts: vec![shift],
                });
                labels.push(format!("closed loop {i} (shifted by {shift} in u)"));
            }
        }
        // (sign, ring) per winding loop. Its images reach a period beyond the
        // window on either side, so that the band they bound is closed far
        // from every hole.
        let mut chains: Vec<(f64, usize)> = Vec::new();
        for &(_, sign, l, pieces) in &winding {
            // Each image starts where the previous one ends: one period on in
            // the loop's own direction.
            let mut shifts = images(extent(pieces), -sign * period, period)?;
            shifts.sort_by(|x, y| (sign * y).total_cmp(&(sign * x)));
            chains.push((sign, rings.len()));
            rings.push(Ring {
                pieces,
                open: true,
                shifts,
            });
            labels.push(format!("winding loop {l} (with its periodic images)"));
        }
        let polys = match separate(&rings)? {
            Ok(polys) => polys,
            Err((x, y)) => {
                let name = |k: usize| labels.get(k).cloned().unwrap_or_default();
                return Err(orientation(if x == y {
                    format!("{} crosses or touches itself", name(x))
                } else {
                    format!("{} and {} cross or touch", name(x), name(y))
                }));
            }
        };
        if !originals.is_empty() {
            band_holds_holes(&polys, &chains, &originals, &hole_rings, window, period)?;
        }
    }
    let orientation = if live.is_empty() {
        LoopOrientation::Undecided
    } else {
        LoopOrientation::Decided
    };
    Ok(Loops { gaps, orientation })
}

/// On a face with winding loops and holes, whose curves are known to be
/// separate (so each hole lies wholly inside or outside any other loop, and
/// a polygon vertex, on its curve and away from every other capsule, decides
/// which): each hole lies in the band between the winding loops and inside
/// no other hole. The band is decidable here only for one pair of winding
/// loops: the chain running towards +u and, above it, the chain running
/// back, closed by joining their ends a period or more beyond every hole.
/// `polys`: the separated rings' polygons; `chains`: (sign, ring) per
/// winding loop; `originals`: (hole, ring) at shift 0; `hole_rings`: (hole,
/// ring) for every image; `window`: the holes' and loops' u-window.
fn band_holds_holes(
    polys: &[Vec<[f64; 2]>],
    chains: &[(f64, usize)],
    originals: &[(usize, usize)],
    hole_rings: &[(usize, usize)],
    window: [f64; 2],
    period: f64,
) -> Result<(), Refusal> {
    let undecided = |why: &str| {
        Err(orientation(format!(
            "the holes' places on a face with winding loops cannot be decided: {why}"
        )))
    };
    // A loop running towards +u has sign -1 (its closing junction steps back
    // by a period).
    let (Some(&(_, up)), Some(&(_, down))) = (
        chains.iter().find(|c| c.0 < 0.0),
        chains.iter().find(|c| c.0 > 0.0),
    ) else {
        return undecided("not one pair of winding loops");
    };
    if chains.len() != 2 {
        return undecided("not one pair of winding loops");
    }
    let (Some(lower), Some(upper)) = (polys.get(up), polys.get(down)) else {
        return undecided("no band");
    };
    // The joins between the chains' ends must lie beyond the window.
    let ends = [lower.first(), lower.last(), upper.first(), upper.last()];
    let [Some(l0), Some(l1), Some(u0), Some(u1)] = ends else {
        return undecided("an empty winding loop");
    };
    let margin = 0.5 * period;
    if !(l0[0].max(u1[0]) < window[0] - margin && l1[0].min(u0[0]) > window[1] + margin) {
        return undecided("the winding loops do not reach beyond the holes");
    }
    let band: Vec<[f64; 2]> = lower.iter().chain(upper.iter()).copied().collect();
    let mut twice = 0.0;
    for (i, p) in band.iter().enumerate() {
        let q = band.get((i + 1) % band.len()).unwrap_or(p);
        twice += p[0] * q[1] - q[0] * p[1];
    }
    if twice.is_nan() || twice <= 0.0 {
        return undecided("the band between the winding loops wraps through the v seam");
    }
    // The two joins are the band's only edges that are not loop curves:
    // they must meet no other edge of it (the chains are separate and
    // simple already).
    let n = band.len();
    let scale = band.iter().fold(0.0_f64, |m, &p| m.max(mag(p)));
    let tol = CONTACT_TOLERANCE * scale;
    for join in [lower.len() - 1, n - 1] {
        let (Some(&a), Some(&b)) = (band.get(join), band.get((join + 1) % n)) else {
            return undecided("no band");
        };
        for k in 0..n {
            if k == join || (k + 1) % n == join || (join + 1) % n == k {
                continue;
            }
            let (Some(&c), Some(&d)) = (band.get(k), band.get((k + 1) % n)) else {
                continue;
            };
            if contact(a, b, c, d, tol) {
                return undecided("a join between the winding loops' ends meets a loop");
            }
        }
    }
    for &(i, ring) in originals {
        let Some(&p) = polys.get(ring).and_then(|p| p.first()) else {
            return undecided("an empty hole");
        };
        if !inside(p, &band) {
            return Err(orientation(format!(
                "hole {i} lies outside the band between the winding loops"
            )));
        }
        for &(j, other) in hole_rings {
            if j != i && polys.get(other).is_some_and(|q| inside(p, q)) {
                return Err(orientation(format!("hole {i} lies inside hole {j}")));
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The grid search before it tested each pair once (every pair sharing a
    /// cell, in cell order, with the repeats a pair met in later cells
    /// dropped): [`contacts`] must return exactly this.
    fn contacts_reference(segs: &[Seg], tol: f64) -> Vec<(usize, usize)> {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for s in segs {
            for p in [s.a, s.b] {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        let side = (segs.len() as f64).sqrt().ceil().max(1.0);
        let cells = side as usize;
        let cell = |x: f64, k: usize| -> usize {
            let w = hi[k] - lo[k];
            if w.is_nan() || w <= 0.0 {
                return 0;
            }
            (((x - lo[k]) / w * side).max(0.0) as usize).min(cells - 1)
        };
        let mut grid: Vec<Vec<usize>> = vec![Vec::new(); cells * cells];
        for (si, s) in segs.iter().enumerate() {
            let e = s.r + tol;
            for x in cell(s.a[0].min(s.b[0]) - e, 0)..=cell(s.a[0].max(s.b[0]) + e, 0) {
                for y in cell(s.a[1].min(s.b[1]) - e, 1)..=cell(s.a[1].max(s.b[1]) + e, 1) {
                    grid[x * cells + y].push(si);
                }
            }
        }
        let mut out: Vec<(usize, usize)> = Vec::new();
        for list in &grid {
            for (k, &i) in list.iter().enumerate() {
                for &j in list.iter().skip(k + 1) {
                    let (s, t) = (&segs[i], &segs[j]);
                    let adjacent = s.followed_by(t) || t.followed_by(s);
                    if !adjacent
                        && contact(s.a, s.b, t.a, t.b, s.r + t.r + tol)
                        && !out.contains(&(i, j))
                    {
                        out.push((i, j));
                    }
                }
            }
        }
        out
    }

    fn seg(poly: usize, index: usize, len: usize, a: [f64; 2], b: [f64; 2], r: f64) -> Seg {
        Seg {
            poly,
            index,
            len,
            open: false,
            a,
            b,
            r,
            vertices: vec![index],
        }
    }

    /// A deterministic generator (a 64-bit linear congruential one).
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

    /// Clustered rings (many short edges crowded into a few grid cells, as
    /// on a face with many small holes), touching and crossing rings, and
    /// slanted collinear edges (whose orientation tests are round-off):
    /// the contacts and their order match the reference exactly.
    #[test]
    fn contacts_match_the_grid_search_exactly() {
        let mut rng = Lcg(7);
        let mut found = 0;
        for case in 0..40 {
            let mut segs = Vec::new();
            let rings = 2 + case % 7;
            for poly in 0..rings {
                let n = 3 + (rng.next() * 60.0) as usize;
                // Most rings small and crowded near the origin, one large.
                let big = poly == 0;
                let (cx, cy) = if big {
                    (0.0, 0.0)
                } else {
                    (rng.next() * 0.02, rng.next() * 0.02)
                };
                let radius = if big { 10.0 } else { 0.001 + rng.next() * 0.01 };
                let pts: Vec<[f64; 2]> = (0..n)
                    .map(|k| {
                        let a = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
                        [cx + radius * libm::cos(a), cy + radius * libm::sin(a)]
                    })
                    .collect();
                for k in 0..n {
                    let r = if rng.next() < 0.5 {
                        0.0
                    } else {
                        rng.next() * 1e-4
                    };
                    segs.push(seg(poly, k, n, pts[k], pts[(k + 1) % n], r));
                }
            }
            // Slanted collinear edges, apart and overlapping.
            for k in 0..6 {
                let t0 = 0.1 * f64::from(k);
                let p = |t: f64| [1.0 + t * 0.7, 2.0 + t * 0.3];
                segs.push(seg(100 + k as usize, 0, 1, p(t0), p(t0 + 0.05), 0.0));
            }
            let tol = CONTACT_TOLERANCE * 10.0;
            let got = contacts(&segs, tol);
            let want = contacts_reference(&segs, tol);
            assert_eq!(got, want, "case {case}");
            found += want.len();
        }
        assert!(found > 100, "{found}");
    }
}
