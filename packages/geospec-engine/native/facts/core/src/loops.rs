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
//! polygon of 16 samples per span of the outer loop (a hole wholly outside
//! refuses; one that only grazes the outer boundary is accepted, since a
//! sampled polygon cannot place points near a curved boundary exactly).

use crate::definition::FaceDefinition;
use crate::gk::gk15_scalar;
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::PreparedTrim;

/// The largest gap between consecutive pieces, relative to `1 +` the
/// magnitude of the junction's coordinates (solidcore fills gaps above about
/// `1e-6 sqrt(1 + |p|)` with lines).
pub const GAP_TOLERANCE: f64 = 1e-6;
/// Gaps at or below this many unit round-offs (relative) are representation
/// noise, not closing segments.
const NOISE_ULPS: f64 = 64.0;
/// Surface points at a winding loop's ends coincide to this relative
/// distance.
const WIND_TOLERANCE: f64 = 1e-9;

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
fn piece_area(t: &PreparedTrim, o: [f64; 2]) -> [f64; 3] {
    let breaks = t.breaks(&[]);
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
    out
}

/// The loop's points, 16 per trim span, in traversal order.
fn polygon(pieces: &[PreparedTrim]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for t in pieces {
        let breaks = t.breaks(&[]);
        let mut pts = Vec::new();
        for w in breaks.windows(2) {
            let &[a, b] = w else { continue };
            for k in 0..16 {
                pts.push(t.eval(a + (b - a) * f64::from(k) / 16.0).0);
            }
        }
        if t.reversed {
            pts.reverse();
        }
        out.extend(pts);
    }
    out
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
            let a = piece_area(t, o);
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
                // The loop's net travel around the period, against the face on
                // its left: travelling +u keeps it at larger v.
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
            let poly = closed.get(o).map(|c| polygon(c.2)).unwrap_or_default();
            for &(i, _) in &live {
                if i == o {
                    continue;
                }
                let pieces = closed.get(i).map(|c| c.2).unwrap_or(&[]);
                if !midpoints(pieces).iter().any(|p| inside(*p, &poly)) {
                    return Err(orientation(format!(
                        "hole {i} lies outside the outer loop {o}"
                    )));
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
