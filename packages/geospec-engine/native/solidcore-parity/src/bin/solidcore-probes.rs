//! Exercises the solidcore pieces that work without a STEP reader on geometry
//! built directly to match two corpus parts, and prints one JSON document with
//! the values, the OCCT reference values and their differences.
//!
//! Usage: solidcore-probes <occt.jsonl> [repeats]
//!
//! `occt.jsonl` is the OCCT run of the Truck spike (`truck-parity occt`); its
//! rows for `box-10x20x30.step` and `06-full-cylinder.step` are the oracle.

use geospec_engine_native_solidcore::probe::{self, FaceResult, ProbeFace};
use serde_json::{json, Value};
use solidcore::geom::extent2d::Extent2d;
use solidcore::query::api_queries::{brep_bounding_box, face_classify_uv, get_faces};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::{Brep, LoopEdge};
use solidcore::{Kernel, Point2, Point3, Vec2, Vec3};
use std::time::Instant;

const BOX_FILE: &str = "docs-site/public/demo/box-10x20x30.step";
const CYLINDER_FILE: &str =
    "packages/geospec-engine/native/occt/rust/tests/fixtures/edge-treatments/06-full-cylinder.step";

fn occt_row(path: &str, file: &str) -> Value {
    let text = std::fs::read_to_string(path).expect("readable OCCT results");
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|row| row["file"] == file)
        .unwrap_or_else(|| panic!("no OCCT row for {file}"))
}

fn rel(a: f64, b: f64) -> f64 {
    let scale = a.abs().max(b.abs());
    if scale == 0.0 {
        0.0
    } else {
        (a - b).abs() / scale
    }
}

fn face_json(results: &[FaceResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|r| match &r.properties {
                Ok(p) => json!({
                    "face": r.label, "area": p.area, "volume": p.volume,
                    "areaMoment": [p.moments[0].x, p.moments[0].y, p.moments[0].z],
                    "volumeMoment": [p.moments[4].x, p.moments[4].y, p.moments[4].z],
                    "micros": r.micros
                }),
                Err(e) => json!({"face": r.label, "error": e.to_string(), "micros": r.micros}),
            })
            .collect(),
    )
}

/// Integrates `faces` at each accuracy, `repeats` times, and compares the sums
/// with the OCCT whole-shape row.
fn solid_probe(name: &str, faces: &[ProbeFace], occt: &Value, repeats: usize) -> Value {
    let shape = &occt["shape"];
    let occt_volume = shape["volume"].as_f64().expect("OCCT volume");
    let occt_area = shape["area"].as_f64().expect("OCCT area");
    let occt_com: Vec<f64> = shape["com"]
        .as_array()
        .expect("OCCT centre of mass")
        .iter()
        .map(|v| v.as_f64().expect("number"))
        .collect();
    let diagonal = {
        let min = shape["min"].as_array().expect("min");
        let max = shape["max"].as_array().expect("max");
        min.iter()
            .zip(max)
            .map(|(a, b)| (b.as_f64().expect("n") - a.as_f64().expect("n")).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let estimated_area = diagonal * diagonal / 100.0;
    let mut runs = Vec::new();
    for accuracy in [1e-4, 1e-6, 1e-8] {
        let mut micros = Vec::new();
        let mut last = Vec::new();
        for _ in 0..repeats {
            let start = Instant::now();
            last = probe::integrate(faces, accuracy, estimated_area);
            micros.push(start.elapsed().as_secs_f64() * 1e6);
        }
        micros.sort_by(f64::total_cmp);
        let median = micros[micros.len() / 2];
        let summary = match probe::sum(&last) {
            Some(s) => {
                let com_error = s
                    .center_of_mass
                    .iter()
                    .zip(&occt_com)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                json!({
                    "area": s.area, "volume": s.volume, "com": s.center_of_mass,
                    "areaCentroid": s.area_centroid,
                    "volumeRelError": rel(s.volume, occt_volume),
                    "areaRelError": rel(s.area, occt_area),
                    "comErrorMm": com_error,
                    "comErrorOverDiagonal": com_error / diagonal,
                })
            }
            None => json!({"error": "a face integral failed"}),
        };
        runs.push(json!({
            "accuracy": accuracy, "medianMicros": median, "repeats": repeats,
            "solid": summary, "faces": face_json(&last)
        }));
    }
    json!({
        "probe": name,
        "occt": {"volume": occt_volume, "area": occt_area, "com": occt_com,
                 "shapeMs": occt["shapeMs"], "facesMs": occt["facesMs"]},
        "runs": runs
    })
}

/// Closest-point projection onto a plane and onto the cylinder side, against
/// the exact distance.
fn closest_probe(k: &Kernel, cylinder: &[ProbeFace]) -> Value {
    let plane = Surface::from(Plane::new(
        Point3::new(0.0, 0.0, 30.0),
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 10.0, 20.0),
    ));
    let side = &cylinder[0].surface;
    let cases = [
        ("box top plane", &plane, Point3::new(3.0, 7.0, 41.5), 11.5),
        (
            "cylinder side, outside",
            side,
            Point3::new(9.0, 0.0, 4.0),
            4.0,
        ),
        (
            "cylinder side, inside",
            side,
            Point3::new(1.0, 2.0, 7.5),
            5.0 - 5.0_f64.sqrt(),
        ),
        (
            "cylinder side, off-axis",
            side,
            Point3::new(-6.0, 8.0, 2.0),
            5.0,
        ),
    ];
    Value::Array(
        cases
            .iter()
            .map(|(label, surface, point, exact)| {
                let (result, micros) = probe::closest(k, surface, *point);
                match result {
                    Ok((p, d)) => json!({
                        "case": label, "point": [p.x, p.y, p.z], "distance": d,
                        "exact": exact, "absError": (d - exact).abs(), "micros": micros
                    }),
                    Err(e) => json!({"case": label, "error": e.to_string(), "micros": micros}),
                }
            })
            .collect(),
    )
}

/// Builds a one-face B-rep with the public builders and calls the B-rep level
/// queries the engine would need; at the pinned commit they report what is
/// unported.
fn topology_probe(k: &Kernel) -> Value {
    let mut b = Brep::new();
    let built = (|| -> Result<_, solidcore::SolidError> {
        let shell = b.add_shell()?;
        let corners = [
            Point3::new(0.0, 0.0, 30.0),
            Point3::new(10.0, 0.0, 30.0),
            Point3::new(10.0, 20.0, 30.0),
            Point3::new(0.0, 20.0, 30.0),
        ];
        let vertices = corners
            .iter()
            .map(|p| b.add_vertex(*p))
            .collect::<Result<Vec<_>, _>>()?;
        let edges = (0..4)
            .map(|i| b.add_edge(vertices[i], vertices[(i + 1) % 4]))
            .collect::<Result<Vec<_>, _>>()?;
        let face = b.add_face(shell)?;
        b.add_loop(
            face,
            edges
                .iter()
                .map(|&edge| LoopEdge {
                    edge,
                    forward: true,
                })
                .collect(),
        )?;
        let domain = Extent2d::from_bounds(0.0, 0.0, 10.0, 20.0);
        let record = b.face_mut(face)?;
        record.surface = Some(Surface::from(Plane::new(
            corners[0],
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &domain,
        )));
        record.uv_domain = domain;
        Ok(face)
    })();
    let face = match built {
        Ok(face) => face,
        Err(e) => return json!({"build": e.to_string()}),
    };
    let outcome = |r: Result<String, solidcore::SolidError>| match r {
        Ok(v) => json!({"ok": v}),
        Err(e) => json!({"error": e.to_string()}),
    };
    json!({
        "build": "ok: 1 shell, 1 face, 1 loop, 4 edges, 4 vertices",
        "faceClassifyUv(5,10)": outcome(face_classify_uv(k, &b, face, Point2::new(5.0, 10.0)).map(|c| format!("{c:?}"))),
        "getFaces": outcome(get_faces(&b).map(|f| format!("{} faces", f.len()))),
        "brepBoundingBox": outcome(brep_bounding_box(k, &b, true).map(|e| format!("{e:?}"))),
        "faceArea": outcome(solidcore::mass::integrator::properties::face_area(k, &b, face, 1e-6).map(|a| a.to_string())),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let occt_path = args
        .get(1)
        .expect("usage: solidcore-probes <occt.jsonl> [repeats]");
    let repeats = args.get(2).map_or(21, |r| r.parse().expect("repeats"));
    let k = Kernel::default();
    let box_faces = probe::box_faces(10.0, 20.0, 30.0);
    let cylinder = probe::cylinder_faces(&k, 5.0, 10.0).expect("cylinder faces");
    let out = json!({
        "solidcoreRev": geospec_engine_native_solidcore::SOLIDCORE_REV,
        "solids": [
            solid_probe("box-10x20x30", &box_faces, &occt_row(occt_path, BOX_FILE), repeats),
            solid_probe("full-cylinder r5 h10", &cylinder, &occt_row(occt_path, CYLINDER_FILE), repeats),
        ],
        "closestPoint": closest_probe(&k, &cylinder),
        "topology": topology_probe(&k),
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
