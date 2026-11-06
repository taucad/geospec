//! Trim pieces prepared for the outer integral: evaluation, and the breaks
//! where the integrand is not smooth along the piece.

use crate::definition::{Curve2, TrimPiece};
use crate::nurbs::PreparedCurve;
use crate::refusal::{Refusal, RefusalCode};

/// The integrator's smallest step (solidcore's `SM_EFF_ZERO`).
pub const EFF_ZERO: f64 = 1e-12;

/// Where a piece is split, and the chords standing for intervals too short to
/// step (see [`PreparedTrim::breaks`]).
#[derive(Debug)]
pub struct Breaks {
    pub at: Vec<f64>,
    pub chords: Vec<([f64; 2], [f64; 2])>,
}

/// A trim piece ready to evaluate.
#[derive(Debug)]
pub struct PreparedTrim {
    curve: Curve,
    pub range: [f64; 2],
    pub reversed: bool,
}

#[derive(Debug)]
enum Curve {
    Line {
        origin: [f64; 2],
        direction: [f64; 2],
    },
    Nurbs(PreparedCurve),
}

impl PreparedTrim {
    pub fn new(t: &TrimPiece) -> Self {
        let curve = match &t.curve {
            Curve2::Line { origin, direction } => Curve::Line {
                origin: *origin,
                direction: *direction,
            },
            Curve2::Nurbs(n) => Curve::Nurbs(PreparedCurve::new(n)),
        };
        Self {
            curve,
            range: t.range,
            reversed: t.reversed,
        }
    }

    /// `(c(t), c'(t))`.
    pub fn eval(&self, t: f64) -> ([f64; 2], [f64; 2]) {
        match &self.curve {
            Curve::Line { origin, direction } => (
                [origin[0] + t * direction[0], origin[1] + t * direction[1]],
                *direction,
            ),
            Curve::Nurbs(c) => c.eval(t),
        }
    }

    /// A box containing the piece over its range: `[[u_lo, v_lo], [u_hi, v_hi]]`.
    pub fn hull(&self) -> [[f64; 2]; 2] {
        match &self.curve {
            Curve::Line { .. } => {
                let (a, _) = self.eval(self.range[0]);
                let (b, _) = self.eval(self.range[1]);
                [
                    [a[0].min(b[0]), a[1].min(b[1])],
                    [a[0].max(b[0]), a[1].max(b[1])],
                ]
            }
            Curve::Nurbs(c) => c.hull(),
        }
    }

    /// Points whose convex hull contains the piece over `[a, b]` (`a <= b`):
    /// the line's two points, or the NURBS sub-curve's control points.
    /// `None` when no such hull can be formed.
    pub fn hull_points(&self, a: f64, b: f64) -> Option<Vec<[f64; 2]>> {
        match &self.curve {
            Curve::Line { .. } => Some(vec![self.eval(a).0, self.eval(b).0]),
            Curve::Nurbs(c) => c.control_points(a, b),
        }
    }

    /// Whether the piece is a polynomial of degree at most `degree` in its
    /// parameter: a line, or a non-rational B-spline of that degree.
    pub fn polynomial_degree(&self) -> Option<usize> {
        match &self.curve {
            Curve::Line { .. } => Some(1),
            Curve::Nurbs(c) => c.is_polynomial().then_some(c.degree()),
        }
    }

    /// The parameters, ascending over the range with its ends, where the
    /// integrand may not be smooth: the curve's own distinct knots, and its
    /// crossings of the surface's u knot lines `u_knots` (found per knot span
    /// by a sign change between `4 (degree + 1)` evenly spaced points, then at
    /// most 64 bisections, as solidcore's `sm_TrimCurveBreaks`). Breaks the
    /// integrator cannot step between are merged, a curve knot kept.
    ///
    /// A merged (dropped) interval over which the curve moves beyond
    /// round-off in the parameter plane is returned as a chord `(from, to)`,
    /// which the integrator integrates into the face's bound like a gap.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`] when a dropped interval moves the
    /// curve beyond [`crate::loops::GAP_TOLERANCE`]: the piece travels over
    /// an interval too short to integrate.
    pub fn breaks(&self, u_knots: &[f64]) -> Result<Breaks, Refusal> {
        let [a, b] = self.range;
        let (mut knots, degree) = match &self.curve {
            Curve::Line { .. } => (vec![a, b], 1),
            Curve::Nurbs(c) => {
                let mut k = vec![a];
                for &x in c.knots() {
                    if x > a && x < b && k.last().is_none_or(|l| *l < x) {
                        k.push(x);
                    }
                }
                k.push(b);
                (k, c.degree())
            }
        };
        knots.dedup();
        let [lo, hi] = self.hull();
        let mut lines: Vec<f64> = u_knots
            .iter()
            .copied()
            .filter(|k| *k > lo[0] && *k < hi[0])
            .collect();
        lines.dedup();
        let mut breaks = Vec::with_capacity(knots.len());
        let samples = 4 * (degree + 1);
        let u = |t: f64| self.eval(t).0[0];
        for pair in knots.windows(2) {
            let &[a, b] = pair else { continue };
            breaks.push(a);
            for &line in &lines {
                let mut t0 = a;
                let mut u0 = u(t0);
                for i in 1..=samples {
                    let t1 = a + (b - a) * (i as f64) / (samples as f64);
                    let u1 = u(t1);
                    if (u0 - line) * (u1 - line) < 0.0 {
                        let (mut lo, mut hi, mut u_lo) = (t0, t1, u0);
                        let mut bisections = 0;
                        while bisections < 64 && hi - lo > EFF_ZERO * (1.0 + hi.abs()) {
                            let mid = 0.5 * (lo + hi);
                            let u_mid = u(mid);
                            if (u_lo - line) * (u_mid - line) <= 0.0 {
                                hi = mid;
                            } else {
                                lo = mid;
                                u_lo = u_mid;
                            }
                            bisections += 1;
                        }
                        breaks.push(0.5 * (lo + hi));
                    } else if u1 == line && i < samples {
                        breaks.push(t1);
                    }
                    t0 = t1;
                    u0 = u1;
                }
            }
        }
        if let Some(&last) = knots.last() {
            breaks.push(last);
        }
        breaks.sort_by(f64::total_cmp);
        let unsteppable = |a: f64, b: f64| 0.5 * (b - a) < EFF_ZERO * (1.0 + b.abs());
        let point = |t: f64| self.eval(t).0;
        let mut distinct: Vec<f64> = Vec::with_capacity(breaks.len());
        // Where the kept breaks stand for a dropped interval's ends.
        let mut dropped: Vec<(f64, f64)> = Vec::new();
        for brk in breaks {
            if let Some(prev) = distinct.last_mut() {
                if unsteppable(*prev, brk) {
                    dropped.push((*prev, brk));
                    if knots.contains(&brk) {
                        *prev = brk;
                    }
                    continue;
                }
            }
            distinct.push(brk);
        }
        let mut chords = Vec::new();
        for (a, b) in dropped {
            let (p, q) = (point(a), point(b));
            let scale = 1.0 + p[0].abs().max(p[1].abs()).max(q[0].abs()).max(q[1].abs());
            let moved = (q[0] - p[0]).abs().max((q[1] - p[1]).abs());
            if moved.is_nan() || moved > crate::loops::GAP_TOLERANCE * scale {
                return Err(Refusal::new(
                    RefusalCode::InvalidDefinition,
                    format!(
                        "trim moves {moved} in the parameter plane over [{a}, {b}], too short to integrate"
                    ),
                ));
            }
            if moved > 64.0 * f64::EPSILON * scale {
                chords.push((p, q));
            }
        }
        Ok(Breaks {
            at: distinct,
            chords,
        })
    }
}

impl crate::definition::Curve2 {
    /// `(c(t), c'(t))`, for adapters that check an export against their
    /// kernel (it prepares the curve on every call).
    ///
    /// # Errors
    ///
    /// [`crate::RefusalCode::InvalidDefinition`] for a malformed curve or a
    /// non-finite `t`.
    pub fn evaluate(&self, t: f64) -> Result<([f64; 2], [f64; 2]), crate::Refusal> {
        let invalid =
            |d: &str| crate::Refusal::new(crate::RefusalCode::InvalidDefinition, d.to_string());
        match self {
            Self::Nurbs(c) => c.validate()?,
            Self::Line { origin, direction } => {
                if !origin.iter().chain(direction).all(|x| x.is_finite()) {
                    return Err(invalid("trim line is not finite"));
                }
            }
        }
        if !t.is_finite() {
            return Err(invalid("trim parameter is not finite"));
        }
        let piece = TrimPiece {
            curve: self.clone(),
            range: [t, t],
            reversed: false,
        };
        Ok(PreparedTrim::new(&piece).eval(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A diagonal line from (0, 0) to (1, 1) breaks where it crosses the u
    /// knot lines 0.3 and 0.6, and nowhere else.
    #[test]
    fn breaks_isolate_u_knot_crossings() {
        let t = PreparedTrim::new(&TrimPiece {
            curve: Curve2::Line {
                origin: [0.0, 0.0],
                direction: [1.0, 1.0],
            },
            range: [0.0, 1.0],
            reversed: false,
        });
        let b = t
            .breaks(&[0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0])
            .unwrap()
            .at;
        assert_eq!(b.len(), 4, "{b:?}");
        assert!(
            (b[1] - 0.3).abs() < 1e-11 && (b[2] - 0.6).abs() < 1e-11,
            "{b:?}"
        );
    }
}
