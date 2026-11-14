//! Surfaces prepared for the integrand: evaluation a row (fixed u) at a time,
//! the knot lines the integrand is not smooth across, the parameter domain,
//! and where the inner integral has a closed form.

use crate::definition::{Frame, Surface};
use crate::nurbs::{BasisCache, EvalBound, PreparedSurface, SurfaceRow};
use crate::vec::Vec3;

/// A surface ready to evaluate.
#[derive(Debug)]
pub enum Prepared {
    Plane {
        origin: Vec3,
        su: Vec3,
        sv: Vec3,
    },
    /// A cylinder (`sin_a = 0`, `cos_a = 1`) or cone.
    Ruled {
        frame: Frame,
        radius: f64,
        sin_a: f64,
        cos_a: f64,
    },
    Sphere {
        frame: Frame,
        radius: f64,
    },
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    Nurbs(PreparedSurface),
}

/// The surface's state at one u.
#[derive(Debug, Default)]
pub struct Row {
    u: f64,
    /// `cos u X + sin u Y` and its u derivative, for the kinds of revolution.
    radial: Vec3,
    tangent: Vec3,
    nurbs: SurfaceRow,
}

/// `(S, S_u, S_v)`.
pub type First = (Vec3, Vec3, Vec3);

impl Prepared {
    pub fn new(s: &Surface) -> Self {
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => Self::Plane {
                origin: *origin,
                su: *u_axis,
                sv: *v_axis,
            },
            Surface::Cylinder { frame, radius } => Self::Ruled {
                frame: *frame,
                radius: *radius,
                sin_a: 0.0,
                cos_a: 1.0,
            },
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => Self::Ruled {
                frame: *frame,
                radius: *radius,
                sin_a: libm::sin(*semi_angle),
                cos_a: libm::cos(*semi_angle),
            },
            Surface::Sphere { frame, radius } => Self::Sphere {
                frame: *frame,
                radius: *radius,
            },
            Surface::Torus {
                frame,
                major_radius,
                minor_radius,
            } => Self::Torus {
                frame: *frame,
                major: *major_radius,
                minor: *minor_radius,
            },
            Surface::Nurbs(n) => Self::Nurbs(PreparedSurface::new(n)),
        }
    }

    /// The parameter domain `[[u_min, u_max], [v_min, v_max]]` where the surface
    /// is defined; infinite where it is not bounded.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        let all = [f64::NEG_INFINITY, f64::INFINITY];
        match self {
            Self::Nurbs(n) => n.domain(),
            Self::Sphere { .. } => [
                all,
                [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
            ],
            Self::Plane { .. } | Self::Ruled { .. } | Self::Torus { .. } => [all, all],
        }
    }

    /// The distinct u knots, each with whether the integrand may be less than
    /// C1 across it ([`crate::nurbs::knot_lines`]).
    pub fn u_lines(&self) -> &[(f64, bool)] {
        match self {
            Self::Nurbs(n) => n.lines_u(),
            _ => &[],
        }
    }

    /// The distinct v knots, as [`Self::u_lines`].
    pub fn v_lines(&self) -> &[(f64, bool)] {
        match self {
            Self::Nurbs(n) => n.lines_v(),
            _ => &[],
        }
    }

    /// Fills `row` for `u`.
    pub fn row(&self, u: f64, row: &mut Row) {
        row.u = u;
        match self {
            Self::Ruled { frame, .. } | Self::Sphere { frame, .. } | Self::Torus { frame, .. } => {
                let (c, s) = (libm::cos(u), libm::sin(u));
                row.radial = c * frame.x + s * frame.y;
                row.tangent = c * frame.y - s * frame.x;
            }
            Self::Nurbs(n) => n.row(u, &mut row.nurbs),
            Self::Plane { .. } => {}
        }
    }

    /// `(S, S_u, S_v)` at `(row.u, v)`.
    pub fn eval(&self, row: &Row, v: f64) -> First {
        match self {
            Self::Plane { origin, su, sv } => (*origin + row.u * *su + v * *sv, *su, *sv),
            Self::Ruled {
                frame,
                radius,
                sin_a,
                cos_a,
            } => {
                let r = *radius + v * *sin_a;
                (
                    frame.origin + r * row.radial + (v * *cos_a) * frame.z,
                    r * row.tangent,
                    *sin_a * row.radial + *cos_a * frame.z,
                )
            }
            Self::Sphere { frame, radius } => {
                let (c, s) = (libm::cos(v), libm::sin(v));
                (
                    frame.origin + (*radius * c) * row.radial + (*radius * s) * frame.z,
                    (*radius * c) * row.tangent,
                    (-*radius * s) * row.radial + (*radius * c) * frame.z,
                )
            }
            Self::Torus {
                frame,
                major,
                minor,
            } => {
                let (c, s) = (libm::cos(v), libm::sin(v));
                let r = *major + *minor * c;
                (
                    frame.origin + r * row.radial + (*minor * s) * frame.z,
                    r * row.tangent,
                    (-*minor * s) * row.radial + (*minor * c) * frame.z,
                )
            }
            Self::Nurbs(n) => n.eval(&row.nurbs, v),
        }
    }

    /// `(S, S_u, S_v)` at `(row.u, v)` and a bound on their round-off beyond
    /// the coordinates' own (zero for the analytic kinds, whose evaluation is
    /// a few products of the frame).
    pub fn eval_bounded(&self, row: &Row, v: f64) -> (First, EvalBound) {
        match self {
            Self::Nurbs(n) => n.eval_bounded(&row.nurbs, v),
            _ => (self.eval(row, v), [0.0; 3]),
        }
    }

    /// [`Self::eval_bounded`] and the weight at `(row.u, v)` (1 for the
    /// analytic kinds).
    pub fn eval_weighted(&self, row: &Row, v: f64) -> (First, EvalBound, f64) {
        match self {
            Self::Nurbs(n) => n.eval_weighted(&row.nurbs, v),
            _ => (self.eval(row, v), [0.0; 3], 1.0),
        }
    }

    /// [`Self::eval_weighted`], on a NURBS surface with the v basis through
    /// `cache` ([`PreparedSurface::eval_weighted_cached`]: the same bits).
    pub fn eval_weighted_cached<'s>(
        &'s self,
        row: &Row,
        v: f64,
        cache: &mut BasisCache<'s>,
    ) -> (First, EvalBound, f64) {
        match self {
            Self::Nurbs(n) => n.eval_weighted_cached(&row.nurbs, v, cache),
            _ => self.eval_weighted(row, v),
        }
    }

    /// On a NURBS surface, its degrees in u and v and whether its weight
    /// varies within a span (it is constant on every span where all the
    /// span's poles have the same weight); `None` on the analytic kinds.
    pub fn nurbs_degrees(&self) -> Option<([usize; 2], bool)> {
        match self {
            Self::Nurbs(n) => Some((n.degrees(), n.weight_ratio() != 1.0)),
            _ => None,
        }
    }

    /// Whether `|S_u x S_v|` is an analytic function of `(u, v)` over the
    /// whole domain: on planes, cylinders, spheres (`r^2 cos v` on
    /// `[-pi/2, pi/2]`) and tori with major radius at least the minor
    /// (`r (R + r cos v)`, never negative). Not on cones, whose `|n|` has a
    /// kink at the apex, nor on NURBS surfaces.
    pub fn smooth_area(&self) -> bool {
        match self {
            Self::Plane { .. } | Self::Sphere { .. } => true,
            Self::Ruled { sin_a, .. } => *sin_a == 0.0,
            Self::Torus { major, minor, .. } => *major >= *minor && *minor > 0.0,
            Self::Nurbs(_) => false,
        }
    }

    /// The largest ratio of weights among one span's poles (1 for the
    /// analytic kinds).
    pub fn weight_ratio(&self) -> f64 {
        match self {
            Self::Nurbs(n) => n.weight_ratio(),
            _ => 1.0,
        }
    }

    /// Where the surface is affine in v with `S_u` affine along a fixed
    /// direction (planes, cylinders, cones): `(S(u, m), S_u(u, m), dS_u/dv, S_v)`
    /// at the midpoint `m` of `[lo, hi]`, for [`ruled_integral`].
    pub fn linear_v(&self, row: &Row, lo: f64, hi: f64) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let m = 0.5 * (lo + hi);
        match self {
            Self::Plane { su, sv, .. } => {
                let (p, _, _) = self.eval(row, m);
                Some((p, *su, Vec3::ZERO, *sv))
            }
            Self::Ruled { sin_a, .. } => {
                let (p, su, sv) = self.eval(row, m);
                Some((p, su, *sin_a * row.tangent, sv))
            }
            _ => None,
        }
    }
}

impl Surface {
    /// `(S, S_u, S_v)` at `(u, v)`, for adapters that check an export against
    /// their kernel (it prepares the surface on every call).
    ///
    /// # Errors
    ///
    /// [`crate::RefusalCode::InvalidDefinition`] for a malformed surface.
    pub fn evaluate(&self, u: f64, v: f64) -> Result<First, crate::Refusal> {
        self.validate()?;
        let p = Prepared::new(self);
        let mut row = Row::default();
        p.row(u, &mut row);
        Ok(p.eval(&row, v))
    }
}
