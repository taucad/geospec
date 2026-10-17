//! Direct-geometry probes of the solidcore pieces that work without topology.
//!
//! A probe builds the faces of a simple solid directly in solidcore (an
//! analytic or NURBS surface and its UV trim loop), then integrates them with
//! `mass::integrator::properties::boundary_properties`, the Green's-theorem
//! face integral behind upstream `SmFace::ComputePreciseProperties`. The B-rep
//! level entry points (`brep_mass_properties`, `face_properties`) cannot run at
//! the pinned commit: they need `SmBrep::GetFaces` (topology.brep_p1),
//! `SmFace::CreateOrientedTrimBoundary` (topology.face_p1) and the bounding
//! boxes of query.brep, which are unported.

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::mass::integrator::properties::{boundary_properties, PreciseProperties};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2::PPF_ALL;
use solidcore::{Kernel, Point2, Point3, SolidError, Vec2, Vec3};

/// One trimmed face: its surface, its UV boundary loops and the orientation
/// that makes the face normal point out of the solid.
pub struct ProbeFace {
    /// What the face is, for the report.
    pub label: String,
    /// The supporting surface.
    pub surface: Surface,
    /// The UV trim curves, one closed loop, counter-clockwise in UV.
    pub trims: Vec<Curve>,
    /// `Same` when the surface normal points out of the solid.
    pub orientation: OrientType,
}

/// The integral of one face.
#[derive(Debug, Clone)]
pub struct FaceResult {
    /// The face label.
    pub label: String,
    /// The integral, or the solidcore error.
    pub properties: Result<PreciseProperties, SolidError>,
    /// Wall time of the integral.
    pub micros: f64,
}

/// Whole-solid sums of the face integrals about the world origin.
#[derive(Debug, Clone, Default)]
pub struct SolidResult {
    /// Sum of face areas.
    pub area: f64,
    /// Enclosed volume. SMLib integrates a `Same` face use with sign -1
    /// (`SmFace::ComputePreciseProperties`), so outward faces sum to the
    /// negated volume; the sum is negated back here.
    pub volume: f64,
    /// Volume centroid, `volume first moment / volume`.
    pub center_of_mass: [f64; 3],
    /// Area centroid, `area first moment / area`.
    pub area_centroid: [f64; 3],
}

fn uv_line(a: Point2, b: Point2) -> Curve {
    Curve::from(Line::from_points(
        Point3::new(a.x, a.y, 0.0),
        Point3::new(b.x, b.y, 0.0),
        2,
    ))
}

/// The counter-clockwise rectangle bounding `domain`.
fn uv_rectangle(domain: &Extent2d) -> Vec<Curve> {
    let (lo, hi) = (domain.min(), domain.max());
    let corners = [
        Point2::new(lo.x, lo.y),
        Point2::new(hi.x, lo.y),
        Point2::new(hi.x, hi.y),
        Point2::new(lo.x, hi.y),
    ];
    (0..4)
        .map(|i| uv_line(corners[i], corners[(i + 1) % 4]))
        .collect()
}

/// A bounded plane face over `[0, a] x [0, b]` along `x_axis` and `y_axis`,
/// trimmed by its full rectangle.
fn rectangle_face(
    label: &str,
    origin: Point3,
    x_axis: Vec3,
    y_axis: Vec3,
    a: f64,
    b: f64,
    outward: Vec3,
) -> ProbeFace {
    let domain = Extent2d::from_bounds(0.0, 0.0, a, b);
    let plane = Plane::new(origin, x_axis, y_axis, Vec2::new(1.0, 1.0), &domain);
    let surface = Surface::from(plane);
    let trims = uv_rectangle(&surface.natural_uv_domain());
    let normal = x_axis.cross(y_axis);
    ProbeFace {
        label: label.into(),
        surface,
        trims,
        orientation: if normal.dot(outward) > 0.0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        },
    }
}

/// The six faces of the axis-aligned box `[0, dx] x [0, dy] x [0, dz]`.
#[must_use]
pub fn box_faces(dx: f64, dy: f64, dz: f64) -> Vec<ProbeFace> {
    let o = Point3::new(0.0, 0.0, 0.0);
    vec![
        rectangle_face("x=0", o, Vec3::Y, Vec3::Z, dy, dz, -Vec3::X),
        rectangle_face(
            "x=dx",
            Point3::new(dx, 0.0, 0.0),
            Vec3::Y,
            Vec3::Z,
            dy,
            dz,
            Vec3::X,
        ),
        rectangle_face("y=0", o, Vec3::Z, Vec3::X, dz, dx, -Vec3::Y),
        rectangle_face(
            "y=dy",
            Point3::new(0.0, dy, 0.0),
            Vec3::Z,
            Vec3::X,
            dz,
            dx,
            Vec3::Y,
        ),
        rectangle_face("z=0", o, Vec3::X, Vec3::Y, dx, dy, -Vec3::Z),
        rectangle_face(
            "z=dz",
            Point3::new(0.0, 0.0, dz),
            Vec3::X,
            Vec3::Y,
            dx,
            dy,
            Vec3::Z,
        ),
    ]
}

/// A circular cap of radius `r` centred on the axis at height `z`.
fn disc_face(
    k: &Kernel,
    label: &str,
    r: f64,
    z: f64,
    outward: Vec3,
) -> Result<ProbeFace, SolidError> {
    let domain = Extent2d::from_bounds(-r, -r, r, r);
    let plane = Plane::new(
        Point3::new(0.0, 0.0, z),
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &domain,
    );
    let surface = Surface::from(plane);
    let uv = surface.natural_uv_domain();
    let centre = uv.mid();
    // The natural UV domain may be a reparameterisation of the analytic one;
    // the circle is scaled to it so it bounds the same disc.
    let scale = (uv.max().x - uv.min().x) / (2.0 * r);
    let circle = Ellipse::new_circle(
        k,
        Point3::new(centre.x, centre.y, 0.0),
        Vec3::X,
        Vec3::Y,
        // The analytic domain of an SMLib circle is in degrees.
        Extent1d::from_min_max(0.0, 360.0),
        r * scale,
        &EllipseOptions {
            dim: 2,
            ..EllipseOptions::default()
        },
    )?;
    let normal = Vec3::X.cross(Vec3::Y);
    Ok(ProbeFace {
        label: label.into(),
        surface,
        trims: vec![Curve::from(circle)],
        orientation: if normal.dot(outward) > 0.0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        },
    })
}

/// The three faces of the solid cylinder of radius `r` from `z = 0` to `h`:
/// a surface of revolution of a line about Z, and two planar discs.
pub fn cylinder_faces(k: &Kernel, r: f64, h: f64) -> Result<Vec<ProbeFace>, SolidError> {
    let mut generator = Line::from_points(Point3::new(r, 0.0, 0.0), Point3::new(r, 0.0, h), 3);
    generator.make_nurb()?;
    let side = SurfOfRevolution::create_canonical(
        k,
        Curve::from(generator),
        Point3::new(0.0, 0.0, 0.0),
        Vec3::Z,
        false,
    )?;
    let side = Surface::from(side);
    let trims = uv_rectangle(&side.natural_uv_domain());
    // Which way the side normal points is read from the surface itself.
    let outward = side_normal_is_outward(&side)?;
    Ok(vec![
        ProbeFace {
            label: "side".into(),
            surface: side,
            trims,
            orientation: if outward {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
        },
        disc_face(k, "bottom", r, 0.0, -Vec3::Z)?,
        disc_face(k, "top", r, h, Vec3::Z)?,
    ])
}

/// Whether `dS/du x dS/dv` at the domain centre points away from the Z axis.
fn side_normal_is_outward(surface: &Surface) -> Result<bool, SolidError> {
    let mid = surface.natural_uv_domain().mid();
    let d = 1e-6;
    let p = surface.evaluate_point(mid)?;
    let pu = surface.evaluate_point(Point2::new(mid.x + d, mid.y))?;
    let pv = surface.evaluate_point(Point2::new(mid.x, mid.y + d))?;
    let n = (pu - p).cross(pv - p);
    Ok(n.dot(Vec3::new(p.x, p.y, 0.0)) > 0.0)
}

/// Integrates every face about the world origin at `accuracy`.
#[must_use]
pub fn integrate(faces: &[ProbeFace], accuracy: f64, estimated_area: f64) -> Vec<FaceResult> {
    faces
        .iter()
        .map(|face| {
            let start = std::time::Instant::now();
            let properties = boundary_properties(
                &face.surface,
                &face.trims,
                &face.surface.natural_uv_domain(),
                face.orientation,
                accuracy,
                Point3::new(0.0, 0.0, 0.0),
                estimated_area,
                0.0,
                PPF_ALL,
            );
            FaceResult {
                label: face.label.clone(),
                properties,
                micros: start.elapsed().as_secs_f64() * 1e6,
            }
        })
        .collect()
}

/// Sums face integrals into whole-solid facts; `None` if any face failed.
#[must_use]
pub fn sum(results: &[FaceResult]) -> Option<SolidResult> {
    let mut area = 0.0;
    let mut volume = 0.0;
    let mut area_moment = Vec3::new(0.0, 0.0, 0.0);
    let mut volume_moment = Vec3::new(0.0, 0.0, 0.0);
    for r in results {
        let p = r.properties.as_ref().ok()?;
        area += p.area.abs();
        volume -= p.volume;
        area_moment += p.moments[0] * p.area.signum();
        volume_moment -= p.moments[4];
    }
    Some(SolidResult {
        area,
        volume,
        center_of_mass: [
            volume_moment.x / volume,
            volume_moment.y / volume,
            volume_moment.z / volume,
        ],
        area_centroid: [
            area_moment.x / area,
            area_moment.y / area,
            area_moment.z / area,
        ],
    })
}

/// Closest point of `point` on `surface`: the point, distance and wall time.
pub fn closest(
    k: &Kernel,
    surface: &Surface,
    point: Point3,
) -> (Result<(Point3, f64), SolidError>, f64) {
    let start = std::time::Instant::now();
    let result =
        surface_closest_point(k, surface, point).map(|c| (c.closest.point, c.closest.distance));
    (result, start.elapsed().as_secs_f64() * 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_integrals_are_exact() {
        let solid = sum(&integrate(&box_faces(10.0, 20.0, 30.0), 1e-8, 14.0)).unwrap();
        assert_eq!(solid.volume, 6000.0);
        assert_eq!(solid.area, 2200.0);
        assert_eq!(solid.center_of_mass, [5.0, 10.0, 15.0]);
    }

    #[test]
    fn cylinder_integrals_meet_the_engine_tolerance_at_1e8() {
        let k = Kernel::default();
        let faces = cylinder_faces(&k, 5.0, 10.0).unwrap();
        let solid = sum(&integrate(&faces, 1e-8, 2.5)).unwrap();
        let exact = std::f64::consts::PI * 250.0;
        assert!((solid.volume - exact).abs() / exact < 1e-7);
        assert!((solid.center_of_mass[2] - 5.0).abs() < 1e-6);
    }
}
