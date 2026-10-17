//! Hybrid feasibility probe: Truck's STEP reader (`truck-stepio`) supplies the
//! B-rep, solidcore integrates its faces exactly.
//!
//! Usage: solidcore-hybrid <file.step>; prints one JSON line.
//!
//! Only the narrowest bridge is written: planar faces bounded by straight
//! edges, in millimetre files, read through the flat table of manifold solids
//! without assembly placements. Any other face refuses the whole file with a
//! reason, so an admitted row is exact and a refused row is never approximated.

use serde_json::{json, Value};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent2d::Extent2d;
use solidcore::mass::integrator::properties::boundary_properties;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2::PPF_ALL;
use solidcore::{Point3, Vec2, Vec3};
use std::time::Instant;
use truck_geotrait::{BoundedCurve, ParametricCurve};
use truck_stepio::r#in::step_geometry::InnerSpace;
use truck_stepio::r#in::step_geometry::{Curve3D, ElementarySurface, Surface as StepSurface};
use truck_stepio::r#in::Table;
use truck_topology::compress::CompressedShell;

type Cs = CompressedShell<truck_stepio::r#in::step_geometry::Point3, Curve3D, StepSurface>;

/// Whether the curve stays on its chord, sampled at nine interior points.
fn is_straight(curve: &Curve3D) -> bool {
    let (t0, t1) = curve.range_tuple();
    let (a, b) = (curve.subs(t0), curve.subs(t1));
    let chord = b - a;
    let length = chord.magnitude();
    if length.is_nan() || length <= 0.0 {
        return false;
    }
    (1..10).all(|i| {
        let p = curve.subs(t0 + (t1 - t0) * f64::from(i) / 10.0);
        (p - a).cross(chord).magnitude() / length <= 1e-9 * length.max(1.0)
    })
}

fn v3(p: truck_stepio::r#in::step_geometry::Vector3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

/// The face as solidcore geometry: an outward-oriented plane with UV line
/// trims for every boundary loop.
fn planar_face(shell: &Cs, face: usize) -> Result<(Surface, Vec<Curve>), String> {
    let f = &shell.faces[face];
    let StepSurface::ElementarySurface(ElementarySurface::Plane(plane)) = &f.surface else {
        return Err("non-planar face".into());
    };
    let o = plane.origin();
    let mut normal = v3(plane.normal());
    if !f.orientation {
        normal = -normal;
    }
    let x = v3(plane.u_axis()).unitize_or_z();
    let y = normal.cross(x).unitize_or_z();
    let origin = Point3::new(o.x, o.y, o.z);
    let to_uv = |p: truck_stepio::r#in::step_geometry::Point3| {
        let d = Point3::new(p.x, p.y, p.z) - origin;
        (d.dot(x), d.dot(y))
    };
    // Loops as analytic (u, v) polygons, one per boundary wire.
    let mut loops = Vec::new();
    let mut analytic = Extent2d::new();
    for wire in &f.boundaries {
        let mut segments = Vec::new();
        for index in wire {
            let edge = &shell.edges[index.index];
            // Any edge whose curve is straight between its vertices is used as
            // a line (OCCT writers emit planar box edges as pcurves).
            if !is_straight(&edge.curve) {
                return Err(match &edge.curve {
                    Curve3D::Conic(_) => "conic edge",
                    Curve3D::BSplineCurve(_) | Curve3D::NurbsCurve(_) => "spline edge",
                    Curve3D::PCurve(_) => "curved pcurve edge",
                    Curve3D::Polyline(_) => "polyline edge",
                    Curve3D::Line(_) => "degenerate line edge",
                }
                .into());
            }
            let (a, b) = if index.orientation {
                (edge.vertices.0, edge.vertices.1)
            } else {
                (edge.vertices.1, edge.vertices.0)
            };
            let (ua, va) = to_uv(shell.vertices[a]);
            let (ub, vb) = to_uv(shell.vertices[b]);
            analytic.add_point(solidcore::Point2::new(ua, va));
            analytic.add_point(solidcore::Point2::new(ub, vb));
            segments.push(((ua, va), (ub, vb)));
        }
        loops.push(segments);
    }
    let size = analytic.max() - analytic.min();
    let bounds = Extent2d::from_bounds(
        analytic.min().x - size.x,
        analytic.min().y - size.y,
        analytic.max().x + size.x,
        analytic.max().y + size.y,
    );
    let plane = Plane::new(origin, x, y, Vec2::new(1.0, 1.0), &bounds);
    let surface = Surface::from(plane);
    // Trim curves live in the surface's natural parameters; map the analytic
    // (u, v) onto them affinely, as the plane is affine in both.
    let natural = surface.natural_uv_domain();
    let map = |(u, v): (f64, f64)| {
        Point3::new(
            natural.min().x
                + (u - bounds.min().x) * (natural.max().x - natural.min().x)
                    / (bounds.max().x - bounds.min().x),
            natural.min().y
                + (v - bounds.min().y) * (natural.max().y - natural.min().y)
                    / (bounds.max().y - bounds.min().y),
            0.0,
        )
    };
    let trims = loops
        .into_iter()
        .flatten()
        .map(|(a, b)| Curve::from(Line::from_points(map(a), map(b), 2)))
        .collect();
    Ok((surface, trims))
}

fn run(text: &str, out: &mut serde_json::Map<String, Value>) -> Result<(), String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let upper = compact.to_ascii_uppercase();
    if !upper.contains("SI_UNIT(.MILLI.,.METRE.)")
        || upper.contains("SI_UNIT(.CENTI.,.METRE.)")
        || upper.contains("SI_UNIT($,.METRE.)")
        || upper.contains("CONVERSION_BASED_UNIT('INCH'")
    {
        return Err("not a millimetre-only file".into());
    }
    let start = Instant::now();
    let table = std::panic::catch_unwind(|| Table::from_step(text))
        .map_err(|_| "truck-stepio panicked while parsing".to_string())?
        .ok_or("truck-stepio could not parse the file")?;
    let mut ids: Vec<_> = table.manifold_solid_brep.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        return Err("no manifold solid".into());
    }
    let mut shells = Vec::new();
    for id in ids {
        let solid = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            table.to_compressed_solid(&table.manifold_solid_brep[&id])
        }))
        .map_err(|_| "truck-stepio panicked while converting a solid".to_string())?
        .map_err(|e| format!("truck-stepio: {e}"))?;
        shells.extend(solid.boundaries);
    }
    out.insert("readMs".into(), json!(start.elapsed().as_secs_f64() * 1e3));
    let start = Instant::now();
    let mut faces = Vec::new();
    for shell in &shells {
        for face in 0..shell.faces.len() {
            faces.push(planar_face(shell, face)?);
        }
    }
    out.insert(
        "convertMs".into(),
        json!(start.elapsed().as_secs_f64() * 1e3),
    );
    let start = Instant::now();
    let (mut area, mut volume) = (0.0, 0.0);
    let mut moment = Vec3::new(0.0, 0.0, 0.0);
    for (surface, trims) in &faces {
        let p = boundary_properties(
            surface,
            trims,
            &surface.natural_uv_domain(),
            OrientType::Same,
            1e-8,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            0.0,
            PPF_ALL,
        )
        .map_err(|e| format!("solidcore: {e}"))?;
        area += p.area.abs();
        // SMLib integrates a Same face use with sign -1.
        volume -= p.volume;
        moment -= p.moments[4];
    }
    out.insert(
        "integrateMs".into(),
        json!(start.elapsed().as_secs_f64() * 1e3),
    );
    out.insert("solids".into(), json!(shells.len()));
    out.insert("faces".into(), json!(faces.len()));
    out.insert("area".into(), json!(area));
    out.insert("volume".into(), json!(volume));
    out.insert(
        "com".into(),
        json!([moment.x / volume, moment.y / volume, moment.z / volume]),
    );
    Ok(())
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: solidcore-hybrid <file.step>");
    let bytes = std::fs::read(&path).expect("readable file");
    let text = String::from_utf8_lossy(&bytes);
    let mut out = serde_json::Map::new();
    out.insert("kernel".into(), json!("truck-stepio+solidcore"));
    out.insert("file".into(), json!(path));
    let start = Instant::now();
    let result = run(&text, &mut out);
    out.insert("totalMs".into(), json!(start.elapsed().as_secs_f64() * 1e3));
    out.insert(
        "open".into(),
        match result {
            Ok(()) => json!("ok"),
            Err(e) => json!({"error": e}),
        },
    );
    println!("{}", Value::Object(out));
}
