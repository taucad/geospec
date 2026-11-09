//! The kernel-agnostic input: a body as face definitions, and the adapter trait.

use crate::nurbs::MAX_DEGREE;
use crate::refusal::{Refusal, RefusalCode};
use crate::vec::Vec3;

/// A right-handed orthonormal frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The origin.
    pub origin: Vec3,
    /// The x direction (unit).
    pub x: Vec3,
    /// The y direction (unit).
    pub y: Vec3,
    /// The z direction (unit), `x × y`.
    pub z: Vec3,
}

/// A face's surface: an analytic kind with its parameters, or a NURBS net.
///
/// The analytic parameterisations are OCCT's, so that the OCCT adapter passes
/// its `Geom_*` surfaces through unchanged, and the natural normal `S_u × S_v`
/// points away from the axis or centre:
///
/// - plane: `S(u, v) = origin + u u_axis + v v_axis` (the axes need be neither
///   unit nor orthogonal, only independent);
/// - cylinder: `S = O + r (cos u X + sin u Y) + v Z`;
/// - cone: `S = O + (r + v sin a) (cos u X + sin u Y) + v cos a Z`, `a` the
///   semi-angle;
/// - sphere: `S = O + r cos v (cos u X + sin u Y) + r sin v Z`;
/// - torus: `S = O + (R + r cos v) (cos u X + sin u Y) + r sin v Z`.
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// An affine plane.
    Plane {
        /// `S(0, 0)`.
        origin: Vec3,
        /// `S_u`.
        u_axis: Vec3,
        /// `S_v`.
        v_axis: Vec3,
    },
    /// A circular cylinder.
    Cylinder {
        /// Its frame; the axis is `z`.
        frame: Frame,
        /// Its radius.
        radius: f64,
    },
    /// A circular cone.
    Cone {
        /// Its frame; the axis is `z`.
        frame: Frame,
        /// Its radius at `v = 0`.
        radius: f64,
        /// The semi-angle in radians, in `(-pi/2, pi/2)`.
        semi_angle: f64,
    },
    /// A sphere.
    Sphere {
        /// Its frame.
        frame: Frame,
        /// Its radius.
        radius: f64,
    },
    /// A torus.
    Torus {
        /// Its frame; the axis is `z`.
        frame: Frame,
        /// The distance from the axis to the tube's centre.
        major_radius: f64,
        /// The tube's radius.
        minor_radius: f64,
    },
    /// A (rational) B-spline surface.
    Nurbs(NurbsSurface),
}

/// A (rational) B-spline surface. Evaluation clamps to the knot domain, as
/// solidcore's and OCCT's evaluators do; trims that leave it refuse.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsSurface {
    /// Degree in u, at least 1.
    pub degree_u: usize,
    /// Degree in v, at least 1.
    pub degree_v: usize,
    /// The full u knot vector: `count_u + degree_u + 1` values.
    pub knots_u: Vec<f64>,
    /// The full v knot vector: `count_v + degree_v + 1` values.
    pub knots_v: Vec<f64>,
    /// Poles in u-major order: pole `(i, j)` at `i * count_v + j`.
    pub poles: Vec<Vec3>,
    /// Weights in the same order; `None` is non-rational.
    pub weights: Option<Vec<f64>>,
    /// Poles in u.
    pub count_u: usize,
    /// Poles in v.
    pub count_v: usize,
}

/// A (rational) B-spline curve in the surface's parameter plane.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve2 {
    /// Degree, at least 1.
    pub degree: usize,
    /// The full knot vector: `poles.len() + degree + 1` values.
    pub knots: Vec<f64>,
    /// Poles `(u, v)`.
    pub poles: Vec<[f64; 2]>,
    /// Weights; `None` is non-rational.
    pub weights: Option<Vec<f64>>,
}

/// A 2D curve in the surface's parameter plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve2 {
    /// `c(t) = origin + t direction`.
    Line {
        /// `c(0)`.
        origin: [f64; 2],
        /// `c'`.
        direction: [f64; 2],
    },
    /// A B-spline curve.
    Nurbs(NurbsCurve2),
}

/// One oriented piece of a face's trim boundary: `curve` over `range`,
/// traversed from `range[1]` to `range[0]` when `reversed`.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimPiece {
    /// The curve.
    pub curve: Curve2,
    /// The parameter range, `range[0] < range[1]`.
    pub range: [f64; 2],
    /// Traversed against the curve's parameter.
    pub reversed: bool,
}

/// The direction of a face's outward normal relative to its surface's natural
/// normal `S_u × S_v`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    /// Outward is `S_u × S_v`.
    Forward,
    /// Outward is `-(S_u × S_v)`.
    Reversed,
}

/// Which sides of a face bound material, which decides how its volume terms
/// count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Material {
    /// One side (a boundary face of a solid): the volume terms count once,
    /// signed by the orientation.
    OneSide,
    /// Both sides (a face inside material): the terms count once with each
    /// sign, in that order, and cancel.
    BothSides,
    /// Neither side (a sheet face): no volume terms.
    Neither,
}

/// One face: its surface, its trim boundary, its orientation and material.
///
/// The trims are oriented so that the face lies on their left in the `(u, v)`
/// plane and form closed loops: each piece ends where the next in its loop
/// starts, and the last where the first starts (within a relative 1e-6, or a
/// whole period of a closed surface for a loop that winds around it). A face
/// has one counter-clockwise outer loop and clockwise holes, or, on a closed
/// surface, pairs of loops winding in opposite directions and clockwise holes;
/// the integrator checks all of this and refuses otherwise. The moments'
/// reference point is the body's, so that one definition can serve every
/// instance of a body.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceDefinition {
    /// The surface.
    pub surface: Surface,
    /// The oriented trim pieces, loop after loop.
    pub trims: Vec<TrimPiece>,
    /// The number of pieces in each loop, in order; they sum to
    /// `trims.len()`.
    pub loops: Vec<usize>,
    /// The outward normal against `S_u × S_v`.
    pub orientation: Orientation,
    /// Which sides bound material.
    pub material: Material,
}

/// One body: its faces, the point its moments are taken about, its size and
/// what the adapter knows about it.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyDefinition {
    /// The point the moments are taken about: the body's box centre.
    pub centre: Vec3,
    /// The body's largest box extent, the length scale of the error floors.
    pub size: f64,
    /// The body's box `[min, max]`, when known: the volume and centroid
    /// invariants check against it, and it contains `centre`.
    pub bounds: Option<[Vec3; 2]>,
    /// The faces, at least one.
    pub faces: Vec<FaceDefinition>,
    /// The file's maximum topology tolerance (mm), when known: finite and not
    /// negative.
    pub file_tolerance: Option<f64>,
    /// Whether every shell is closed, when known; a body bounding material
    /// refuses unless it is `Some(true)`.
    pub closed: Option<bool>,
}

/// A kernel adapter: the bodies of one subject as definitions.
///
/// `body` takes `&mut self` because kernels build trim curves lazily.
pub trait FaceDefinitions {
    /// The number of bodies.
    fn body_count(&self) -> usize;

    /// Body `index` as a definition, or why it cannot be one.
    ///
    /// # Errors
    ///
    /// A typed [`Refusal`]: an unsupported surface or trim kind, an export that
    /// does not reproduce the kernel's evaluation, a malformed body.
    fn body(&mut self, index: usize) -> Result<BodyDefinition, Refusal>;
}

fn invalid(detail: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDefinition, detail)
}

fn finite(values: &[f64], what: &str) -> Result<(), Refusal> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid(format!("{what} is not finite")))
    }
}

fn knots_fit(knots: &[f64], degree: usize, count: usize, what: &str) -> Result<(), Refusal> {
    if degree == 0 || degree > MAX_DEGREE {
        return Err(invalid(format!(
            "{what}: degree {degree} outside 1..={MAX_DEGREE}"
        )));
    }
    if count <= degree {
        return Err(invalid(format!(
            "{what}: {count} poles for degree {degree}"
        )));
    }
    if Some(knots.len()) != count.checked_add(degree + 1) {
        return Err(invalid(format!(
            "{what}: {} knots for {count} poles of degree {degree}",
            knots.len()
        )));
    }
    finite(knots, what)?;
    if knots.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
        return Err(invalid(format!("{what}: knots decrease")));
    }
    // A non-empty domain [knots[degree], knots[count]].
    let (Some(&lo), Some(&hi)) = (knots.get(degree), knots.get(count)) else {
        return Err(invalid(format!("{what}: empty knot domain")));
    };
    if lo >= hi {
        return Err(invalid(format!("{what}: empty knot domain")));
    }
    // No knot inside the domain with more than `degree` copies: the curve or
    // surface would be discontinuous there.
    let mut run = 0usize;
    let mut prev = f64::NAN;
    for &k in knots {
        run = if k == prev { run + 1 } else { 1 };
        prev = k;
        if k > lo && k < hi && run > degree {
            return Err(invalid(format!(
                "{what}: interior knot {k} with more than {degree} copies (discontinuous)"
            )));
        }
    }
    Ok(())
}

fn weights_fit(weights: Option<&Vec<f64>>, count: usize, what: &str) -> Result<(), Refusal> {
    let Some(w) = weights else { return Ok(()) };
    if w.len() != count {
        return Err(invalid(format!(
            "{what}: {} weights for {count} poles",
            w.len()
        )));
    }
    // A subnormal weight is refused as OCCT refuses it (`gp::Resolution`):
    // its rounding is absolute, so no relative error bound covers it.
    if w.iter().all(|w| w.is_finite() && *w >= f64::MIN_POSITIVE) {
        Ok(())
    } else {
        Err(invalid(format!(
            "{what}: a weight is not positive, normal and finite"
        )))
    }
}

impl Frame {
    /// Checks that the frame is finite, orthonormal and right-handed to 1e-9.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        let all = [self.origin, self.x, self.y, self.z];
        if !all.iter().all(|v| v.is_finite()) {
            return Err(invalid("frame is not finite"));
        }
        let unit = |v: Vec3| (v.length() - 1.0).abs() <= 1e-9;
        let right = (self.x.cross(self.y) - self.z).max_abs() <= 1e-9;
        if unit(self.x) && unit(self.y) && unit(self.z) && self.x.dot(self.y).abs() <= 1e-9 && right
        {
            Ok(())
        } else {
            Err(invalid("frame is not orthonormal and right-handed"))
        }
    }
}

impl Surface {
    /// Checks the surface's numbers.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        let positive = |r: f64, what: &str| {
            if r.is_finite() && r > 0.0 {
                Ok(())
            } else {
                Err(invalid(format!("{what} is not positive and finite")))
            }
        };
        match self {
            Self::Plane {
                origin,
                u_axis,
                v_axis,
            } => {
                if !(origin.is_finite() && u_axis.is_finite() && v_axis.is_finite()) {
                    return Err(invalid("plane is not finite"));
                }
                // Unit axes first: tiny axes would underflow the product.
                let unit = |a: Vec3| {
                    let m = a.max_abs();
                    if m > 0.0 {
                        Vec3::new(a.x / m, a.y / m, a.z / m)
                    } else {
                        a
                    }
                };
                if unit(*u_axis).cross(unit(*v_axis)).length_squared() > 0.0 {
                    Ok(())
                } else {
                    Err(invalid("plane axes are parallel"))
                }
            }
            Self::Cylinder { frame, radius } | Self::Sphere { frame, radius } => {
                frame.validate()?;
                positive(*radius, "radius")
            }
            Self::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                frame.validate()?;
                finite(&[*radius, *semi_angle], "cone")?;
                if *radius < 0.0 || semi_angle.abs() >= std::f64::consts::FRAC_PI_2 {
                    return Err(invalid("cone radius or semi-angle out of range"));
                }
                Ok(())
            }
            Self::Torus {
                frame,
                major_radius,
                minor_radius,
            } => {
                frame.validate()?;
                positive(*minor_radius, "minor radius")?;
                finite(&[*major_radius], "major radius")
            }
            Self::Nurbs(s) => s.validate(),
        }
    }
}

impl NurbsSurface {
    /// Checks that knots, poles and weights fit.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        knots_fit(&self.knots_u, self.degree_u, self.count_u, "surface u")?;
        knots_fit(&self.knots_v, self.degree_v, self.count_v, "surface v")?;
        let Some(count) = self.count_u.checked_mul(self.count_v) else {
            return Err(invalid("surface: pole count overflows"));
        };
        if self.poles.len() != count {
            return Err(invalid(format!(
                "surface: {} poles for a {} x {} net",
                self.poles.len(),
                self.count_u,
                self.count_v
            )));
        }
        if !self.poles.iter().all(|p| p.is_finite()) {
            return Err(invalid("surface poles are not finite"));
        }
        weights_fit(self.weights.as_ref(), count, "surface")
    }
}

impl NurbsCurve2 {
    /// Checks that knots, poles and weights fit.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        knots_fit(&self.knots, self.degree, self.poles.len(), "trim")?;
        if !self.poles.iter().flatten().all(|v| v.is_finite()) {
            return Err(invalid("trim poles are not finite"));
        }
        weights_fit(self.weights.as_ref(), self.poles.len(), "trim")
    }

    /// The knot domain `[knots[degree], knots[count]]`.
    #[must_use]
    pub fn domain(&self) -> [f64; 2] {
        let a = self.knots.get(self.degree).copied().unwrap_or(f64::NAN);
        let b = self
            .knots
            .get(self.poles.len())
            .copied()
            .unwrap_or(f64::NAN);
        [a, b]
    }
}

impl TrimPiece {
    /// Checks the curve and that the range is finite, ascending and, for a
    /// B-spline, inside its knot domain (to a relative 1e-12).
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        let [a, b] = self.range;
        if !(a.is_finite() && b.is_finite() && a < b) {
            return Err(invalid("trim range is not finite and ascending"));
        }
        match &self.curve {
            Curve2::Line { origin, direction } => {
                finite(origin, "trim line")?;
                finite(direction, "trim line")
            }
            Curve2::Nurbs(c) => {
                c.validate()?;
                let [lo, hi] = c.domain();
                let slack = 1e-12 * (1.0 + (hi - lo).abs());
                if a < lo - slack || b > hi + slack {
                    return Err(invalid(format!(
                        "trim range [{a}, {b}] outside its knot domain [{lo}, {hi}]"
                    )));
                }
                Ok(())
            }
        }
    }
}

impl FaceDefinition {
    /// Checks the surface, every trim, and that the loops partition the
    /// trims into non-empty loops (their chaining, closure and orientation are
    /// checked by [`FaceDefinition::check_loops`]).
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        self.surface.validate()?;
        if self.trims.is_empty() {
            return Err(invalid("face has no trims"));
        }
        let total = self.loops.iter().try_fold(0usize, |a, n| a.checked_add(*n));
        if self.loops.contains(&0) || total != Some(self.trims.len()) {
            return Err(invalid(format!(
                "loops {:?} do not partition {} trims",
                self.loops,
                self.trims.len()
            )));
        }
        self.trims.iter().try_for_each(TrimPiece::validate)
    }
}

impl BodyDefinition {
    /// The largest distance along one axis from the centre to the body box,
    /// or the size without a box: how far the moments' reference point could
    /// move within the body.
    #[must_use]
    pub fn reach(&self) -> f64 {
        match self.bounds {
            Some([lo, hi]) => {
                let c = self.centre;
                [
                    c.x - lo.x,
                    hi.x - c.x,
                    c.y - lo.y,
                    hi.y - c.y,
                    c.z - lo.z,
                    hi.z - c.z,
                ]
                .iter()
                .fold(0.0_f64, |m, d| m.max(*d))
            }
            None => self.size,
        }
    }

    /// Checks the body's numbers: at least one face, a finite centre inside
    /// the box, a positive size, a finite non-negative file tolerance (its
    /// faces are checked when integrated).
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.faces.is_empty() {
            return Err(invalid("body has no faces"));
        }
        if !self.centre.is_finite() {
            return Err(invalid("body centre is not finite"));
        }
        if !(self.size.is_finite() && self.size > 0.0) {
            return Err(invalid("body size is not positive and finite"));
        }
        if let Some(t) = self.file_tolerance {
            if !(t.is_finite() && t >= 0.0) {
                return Err(invalid("file tolerance is not finite and non-negative"));
            }
        }
        if let Some([lo, hi]) = self.bounds {
            if !(lo.is_finite() && hi.is_finite() && lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z) {
                return Err(invalid("body box is not finite and ordered"));
            }
            let slack = 1e-9 * self.size;
            let c = self.centre;
            let inside = [(c.x, lo.x, hi.x), (c.y, lo.y, hi.y), (c.z, lo.z, hi.z)]
                .iter()
                .all(|&(x, lo, hi)| x >= lo - slack && x <= hi + slack);
            if !inside {
                return Err(invalid("body centre is outside the body box"));
            }
        }
        Ok(())
    }
}
