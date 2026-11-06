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
/// plane (outer loops counter-clockwise, holes clockwise) and together form
/// closed loops. The moments' reference point is the body's, so that one
/// definition can serve every instance of a body.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceDefinition {
    /// The surface.
    pub surface: Surface,
    /// The oriented trim pieces, loop after loop.
    pub trims: Vec<TrimPiece>,
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
    /// invariants check against it.
    pub bounds: Option<[Vec3; 2]>,
    /// The faces.
    pub faces: Vec<FaceDefinition>,
    /// The file's maximum topology tolerance (mm), when known.
    pub file_tolerance: Option<f64>,
    /// Whether every shell is closed, when known.
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
    if knots.len() != count + degree + 1 {
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
    match (knots.get(degree), knots.get(count)) {
        (Some(a), Some(b)) if a < b => Ok(()),
        _ => Err(invalid(format!("{what}: empty knot domain"))),
    }
}

fn weights_fit(weights: Option<&Vec<f64>>, count: usize, what: &str) -> Result<(), Refusal> {
    let Some(w) = weights else { return Ok(()) };
    if w.len() != count {
        return Err(invalid(format!(
            "{what}: {} weights for {count} poles",
            w.len()
        )));
    }
    if w.iter().all(|w| w.is_finite() && *w > 0.0) {
        Ok(())
    } else {
        Err(invalid(format!(
            "{what}: a weight is not positive and finite"
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
                if u_axis.cross(*v_axis).length_squared() > 0.0 {
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
        let count = self.count_u * self.count_v;
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
    /// Checks the surface and every trim.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        self.surface.validate()?;
        self.trims.iter().try_for_each(TrimPiece::validate)
    }
}

impl BodyDefinition {
    /// Checks the body's numbers (its faces are checked when integrated).
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.centre.is_finite() {
            return Err(invalid("body centre is not finite"));
        }
        if !(self.size.is_finite() && self.size > 0.0) {
            return Err(invalid("body size is not positive and finite"));
        }
        if let Some([lo, hi]) = self.bounds {
            if !(lo.is_finite() && hi.is_finite() && lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z) {
                return Err(invalid("body box is not finite and ordered"));
            }
        }
        Ok(())
    }
}
