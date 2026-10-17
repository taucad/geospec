//! Runs one BRep connector over one STEP file through the engine's seam and
//! prints one JSON line of facts and timings. The driver runs each file in its
//! own process under a timeout, so a hang or abort is recorded per file.
//!
//! Usage: truck-parity <truck|occt> <file.step> [tessellation-deflection-mm]

use geospec_engine_native_core::backend::brep::{
    BrepConnector, BrepEntity, BrepSubject, SurfaceFacts, TessellationProfile,
};
use serde_json::{json, Value};
use std::time::Instant;

fn surface_json(surface: &SurfaceFacts) -> Value {
    match surface {
        SurfaceFacts::Plane { origin, normal } => {
            json!({"kind": "plane", "origin": origin, "normal": normal})
        }
        SurfaceFacts::Cylinder {
            origin,
            axis,
            radius,
        } => json!({"kind": "cylinder", "origin": origin, "axis": axis, "radius": radius}),
        SurfaceFacts::Cone {
            origin,
            axis,
            reference_radius,
            semi_angle,
        } => {
            json!({"kind": "cone", "origin": origin, "axis": axis, "radius": reference_radius, "semiAngle": semi_angle})
        }
        SurfaceFacts::Sphere { center, radius } => {
            json!({"kind": "sphere", "origin": center, "radius": radius})
        }
        SurfaceFacts::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
        } => {
            json!({"kind": "torus", "origin": center, "axis": axis, "radius": major_radius, "minor": minor_radius})
        }
        SurfaceFacts::Bezier { .. } => json!({"kind": "bezier"}),
        SurfaceFacts::Bspline { u_rational, .. } => {
            json!({"kind": "bspline", "rational": u_rational})
        }
        SurfaceFacts::Revolution => json!({"kind": "revolution"}),
        SurfaceFacts::Extrusion => json!({"kind": "extrusion"}),
        SurfaceFacts::Offset => json!({"kind": "offset"}),
        SurfaceFacts::Other => json!({"kind": "other"}),
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn err(e: geospec_engine_native_core::backend::BackendError) -> Value {
    json!({"error": format!("{:?}: {}", e.kind, e.message)})
}

fn connector(kernel: &str) -> Box<dyn BrepConnector> {
    match kernel {
        "truck" => Box::new(geospec_engine_native_truck::TruckConnector),
        #[cfg(feature = "occt")]
        "occt" => Box::new(geospec_engine_native_occt::OcctConnector),
        other => panic!("unknown kernel {other}"),
    }
}

fn run(subject: &dyn BrepSubject, deflection: f64, out: &mut serde_json::Map<String, Value>) {
    let t = Instant::now();
    let admission = subject.admission_facts();
    out.insert("admissionMs".into(), json!(ms(t)));
    eprintln!("admissionMs {:.0}", ms(t));
    out.insert(
        "admission".into(),
        match admission {
            Ok(a) => json!({"unit": a.source_length_unit, "toMm": a.source_unit_to_millimeters, "occurrences": a.occurrence_count, "surfaceless": a.surfaceless_faces}),
            Err(e) => err(e),
        },
    );

    let t = Instant::now();
    let faces = subject.faces();
    out.insert("facesMs".into(), json!(ms(t)));
    eprintln!("facesMs {:.0}", ms(t));
    out.insert(
        "faces".into(),
        match faces {
            Ok(faces) => Value::Array(
                faces
                    .iter()
                    .map(|f| json!({"area": f.facts.area, "com": f.facts.center_of_mass, "min": f.bounds.min, "max": f.bounds.max, "reversed": f.reversed, "surface": surface_json(&f.facts.surface)}))
                    .collect(),
            ),
            Err(e) => err(e),
        },
    );

    let t = Instant::now();
    let shape = subject.reported_shape();
    out.insert("shapeMs".into(), json!(ms(t)));
    eprintln!("shapeMs {:.0}", ms(t));
    out.insert(
        "shape".into(),
        match shape {
            Ok(s) => json!({
                "volume": s.volume, "area": s.surface_area, "com": s.center_of_mass,
                "min": s.bounds.min, "max": s.bounds.max,
                "counts": {"solids": s.topology.solids, "shells": s.topology.shells, "faces": s.topology.faces, "wires": s.topology.wires, "edges": s.topology.edges, "vertices": s.topology.vertices}
            }),
            Err(e) => err(e),
        },
    );

    let t = Instant::now();
    let validity = subject.validity();
    out.insert("validityMs".into(), json!(ms(t)));
    eprintln!("validityMs {:.0}", ms(t));
    out.insert(
        "validity".into(),
        match validity {
            Ok(v) => json!({"valid": v.valid, "openEdges": v.open_edge_count, "nonmanifoldEdges": v.nonmanifold_edge_count, "solids": v.solid_count, "invalidSolids": v.invalid_solid_count, "closedShells": v.closed_shells, "checks": v.checks.as_ref().map(|c| c.iter().take(4).map(|c| format!("{}: {}", c.shape, c.status)).collect::<Vec<_>>())}),
            Err(e) => err(e),
        },
    );

    let t = Instant::now();
    let occurrences = subject.source_occurrences();
    out.insert("occurrencesMs".into(), json!(ms(t)));
    eprintln!("occurrencesMs {:.0}", ms(t));
    out.insert(
        "occurrences".into(),
        match occurrences {
            Ok(rows) => Value::Array(rows.iter().map(|o| json!({"path": o.path, "parent": o.parent, "faces": o.face_count, "placement": o.placement, "min": o.bounds.min, "max": o.bounds.max})).collect()),
            Err(e) => err(e),
        },
    );

    let profile = TessellationProfile {
        linear_deflection_mm: deflection,
        angular_deflection_rad: 0.5,
    };
    let t = Instant::now();
    let mesh = subject.tessellate(BrepEntity::Whole, profile);
    out.insert("tessellateMs".into(), json!(ms(t)));
    eprintln!("tessellateMs {:.0}", ms(t));
    out.insert(
        "tessellation".into(),
        match mesh {
            Ok(m) => json!({"positions": m.positions.len(), "triangles": m.triangles.len()}),
            Err(e) => err(e),
        },
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kernel = args.get(1).expect("kernel");
    let path = args.get(2).expect("file");
    let deflection = args.get(3).map_or(0.1, |d| d.parse().expect("deflection"));
    let bytes = std::fs::read(path).expect("readable file");
    let mut out = serde_json::Map::new();
    out.insert("kernel".into(), json!(kernel));
    out.insert("file".into(), json!(path));
    out.insert("bytes".into(), json!(bytes.len()));
    let connector = connector(kernel);
    let t = Instant::now();
    let subject = connector.open_step(&bytes);
    out.insert("openMs".into(), json!(ms(t)));
    eprintln!("open {:.0} ms", ms(t));
    match subject {
        Ok(subject) => {
            out.insert("open".into(), json!("ok"));
            run(subject.as_ref(), deflection, &mut out);
        }
        Err(e) => {
            out.insert("open".into(), err(e));
        }
    }
    println!("{}", Value::Object(out));
}
