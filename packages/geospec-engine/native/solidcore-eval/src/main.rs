//! `solidcore-facts`: GeoSpec's admission facts through solidcore.
//!
//! Usage: `geospec-solidcore-eval [--open-only] [--threads N] [--accuracy A] <file.step>`
//!
//! Prints one JSON line: per-phase wall times (read, Part 21 parse and map, B-rep
//! build, facts), the importer's findings, and per-file facts (solids, faces,
//! volume, area, centroid, bounds, manifold state). A failure is a typed reason in
//! the JSON, never a panic; the process exit code is 0 whenever the JSON was
//! written.
//!
//! `--threads N` builds and measures bodies on N scoped worker threads, each with
//! its own `Kernel` (solidcore's kernel is single-threaded by design). Results are
//! gathered in body order, so the output is identical for every N.
//!
//! By default volume, area and centre of mass come from one shared pass of the
//! fixed-rule integrator in `gauss.rs`, run per face across the same N threads.
//! `--reference` keeps solidcore's own per-query integrator for comparison.

mod fast;
mod gauss;

use std::time::Instant;

use serde_json::{json, Map, Value};
use solidcore::mass::integrator::properties::{
    brep_area, brep_mass_properties_default, brep_volume_default,
};
use solidcore::query::api_queries::{
    brep_bounding_box, brep_is_manifold_solid, brep_material_census,
};
use solidcore::topology::integrity;
use solidcore::{Brep, Kernel, Point3};
use step_brep::{build_body_with_file_pcurves, BuildOptions, BuildPath};
use step_import::model::{Body, BodyKind, FilePcurves};
use step_import::Limits;

struct Args {
    open_only: bool,
    reference: bool,
    threads: usize,
    accuracy: f64,
    path: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        open_only: std::env::var_os("PARITY_OPEN_ONLY").is_some_and(|v| v == "1"),
        reference: false,
        threads: 1,
        accuracy: 1e-7,
        path: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--open-only" => args.open_only = true,
            "--reference" => args.reference = true,
            "--threads" => {
                args.threads = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&n: &usize| n > 0)
                    .ok_or("--threads needs a positive integer")?;
            }
            "--accuracy" => {
                args.accuracy = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|a: &f64| a.is_finite() && *a > 0.0)
                    .ok_or("--accuracy needs a positive number")?;
            }
            _ => args.path = arg,
        }
    }
    if args.path.is_empty() {
        return Err("missing STEP path".into());
    }
    Ok(args)
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e3
}

/// One body's build and facts, in a form that crosses threads.
#[derive(Default)]
struct BodyResult {
    built: bool,
    path_topology: bool,
    error: Option<String>,
    face_failures: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
    manifold: Option<bool>,
    solids: usize,
    voids: usize,
    volume: Option<f64>,
    area: Option<f64>,
    centroid: Option<[f64; 3]>,
    bounds: Option<[f64; 6]>,
    facts_error: Option<String>,
    volume_only: Option<f64>,
    integrity: Option<bool>,
    integrity_error: Option<String>,
    build_ms: f64,
    facts_ms: f64,
    check_ms: [f64; 6],
    /// Fast path: trim construction for the shared pass.
    prepare_ms: f64,
    /// Fast path: this body's faces did not all converge and solidcore's own
    /// integrator produced its values.
    fallback: Option<String>,
}

/// The GeoSpec whole-subject checks timed one by one, in the order the OCCT
/// child evaluates its claims.
const CHECKS: [&str; 6] = [
    "topologyCounts",
    "boundingBox",
    "volume",
    "surfaceArea",
    "centerOfMass",
    "validBrep",
];

fn body_facts(k: &Kernel, brep: &mut Brep, accuracy: f64, out: &mut BodyResult) {
    // topologyCounts: entity counts are read at build; add the manifold and
    // material census the engine reports with them.
    let t = Instant::now();
    let manifold = brep_is_manifold_solid(brep);
    out.manifold = manifold.as_ref().ok().copied();
    if let Ok((solids, voids)) = brep_material_census(brep) {
        out.solids = solids;
        out.voids = voids;
    }
    out.check_ms[0] = ms(t);
    let t = Instant::now();
    let bounds = brep_bounding_box(k, brep, true).or_else(|_| brep_bounding_box(k, brep, false));
    out.check_ms[1] = ms(t);
    let is_solid = matches!(manifold, Ok(true));
    if is_solid {
        let t = Instant::now();
        out.volume_only = brep_volume_default(k, brep, accuracy).ok();
        out.check_ms[2] = ms(t);
    }
    let t = Instant::now();
    match brep_area(k, brep, accuracy) {
        Ok(a) => out.area = Some(a),
        Err(e) => out.facts_error = Some(format!("area: {e}")),
    }
    out.check_ms[3] = ms(t);
    let t = Instant::now();
    if is_solid {
        match brep_mass_properties_default(k, brep, accuracy, 1.0, Point3::new(0.0, 0.0, 0.0)) {
            Ok(p) => {
                out.volume = Some(p.properties.volume);
                out.centroid = Some([p.centroid.x, p.centroid.y, p.centroid.z]);
            }
            Err(e) => out.facts_error = Some(format!("mass: {e}")),
        }
    }
    out.check_ms[4] = ms(t);
    let t = Instant::now();
    match integrity::check(brep) {
        Ok(()) => out.integrity = Some(true),
        Err(e) => {
            out.integrity = Some(false);
            out.integrity_error = Some(e.to_string());
        }
    }
    out.check_ms[5] = ms(t);
    match bounds {
        Ok(b) if b.is_bounded() => {
            let (m, s) = (b.mid(), b.size());
            out.bounds = Some([
                m.x - s.x / 2.0,
                m.y - s.y / 2.0,
                m.z - s.z / 2.0,
                m.x + s.x / 2.0,
                m.y + s.y / 2.0,
                m.z + s.z / 2.0,
            ]);
        }
        Ok(_) => out.facts_error = Some("bounds: unbounded".into()),
        Err(e) => {
            out.facts_error
                .get_or_insert_with(|| format!("bounds: {e}"));
        }
    }
}

fn run_body(
    k: &Kernel,
    body: &Body,
    pcurves: &FilePcurves,
    open_only: bool,
    accuracy: f64,
) -> BodyResult {
    let mut out = BodyResult::default();
    let t = Instant::now();
    let built = build_body_with_file_pcurves(body, pcurves, BuildOptions::default());
    out.build_ms = ms(t);
    let mut built = match built {
        Ok(b) => b,
        Err(e) => {
            out.error = Some(e.message);
            return out;
        }
    };
    out.built = true;
    out.path_topology = built.path == BuildPath::Topology;
    out.face_failures = built.face_failures.len();
    out.faces = built.brep.face_list().len();
    out.edges = built.brep.edge_list().len();
    out.vertices = built.brep.vertex_list().len();
    if !open_only {
        let t = Instant::now();
        body_facts(k, &mut built.brep, accuracy, &mut out);
        out.facts_ms = ms(t);
    }
    out
}

fn run_bodies(bodies: &[Body], pcurves: &FilePcurves, args: &Args) -> Vec<BodyResult> {
    let threads = args.threads.min(bodies.len()).max(1);
    if threads == 1 {
        let k = Kernel::default();
        return bodies
            .iter()
            .map(|b| run_body(&k, b, pcurves, args.open_only, args.accuracy))
            .collect();
    }
    // Static interleaved partition: worker w takes bodies w, w+threads, ...
    let mut slots: Vec<Option<BodyResult>> = bodies.iter().map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|w| {
                scope.spawn(move || {
                    let k = Kernel::default();
                    bodies
                        .iter()
                        .enumerate()
                        .skip(w)
                        .step_by(threads)
                        .map(|(i, b)| (i, run_body(&k, b, pcurves, args.open_only, args.accuracy)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            if let Ok(results) = h.join() {
                for (i, r) in results {
                    if let Some(slot) = slots.get_mut(i) {
                        *slot = Some(r);
                    }
                }
            }
        }
    });
    slots
        .into_iter()
        .map(|s| {
            s.unwrap_or_else(|| BodyResult {
                error: Some("worker thread panicked".into()),
                ..BodyResult::default()
            })
        })
        .collect()
}

/// Phase timings of the shared pass.
#[derive(Default)]
struct PassStats {
    build_phase_ms: f64,
    integrate_phase_ms: f64,
    tasks: usize,
}

fn main() {
    let row = match parse_args() {
        Ok(args) => run(&args),
        Err(e) => json!({ "open": { "error": e } }),
    };
    println!("{row}");
}

fn run(args: &Args) -> Value {
    let total = Instant::now();
    let t = Instant::now();
    let bytes = match std::fs::read(&args.path) {
        Ok(b) => b,
        Err(e) => return json!({ "open": { "error": format!("read: {e}") } }),
    };
    let read_ms = ms(t);
    let t = Instant::now();
    let model = match step_import::import(&bytes, &Limits::default()) {
        Ok(m) => m,
        Err(e) => {
            return json!({
                "open": { "error": format!("part21: line {}: {}", e.line, e.message) },
                "phases": { "readMs": read_ms, "parseMs": ms(t) },
            })
        }
    };
    let parse_ms = ms(t);
    let t = Instant::now();
    let mut pass = PassStats::default();
    let results = if args.reference || args.open_only {
        run_bodies(&model.bodies, &model.file_pcurves, args)
    } else {
        fast::run_bodies(&model.bodies, &model.file_pcurves, args, &mut pass)
    };
    let bodies_ms = ms(t);

    let solid_bodies = model
        .bodies
        .iter()
        .filter(|b| {
            matches!(
                b.kind,
                BodyKind::ManifoldSolid | BodyKind::SolidWithVoids | BodyKind::Faceted
            )
        })
        .count();
    let built = results.iter().filter(|r| r.built).count();
    let build_errors: Vec<&str> = results.iter().filter_map(|r| r.error.as_deref()).collect();
    let facts_errors: Vec<&str> = results
        .iter()
        .filter_map(|r| r.facts_error.as_deref())
        .collect();
    let mut row = Map::new();
    row.insert(
        "phases".into(),
        json!({
            "readMs": read_ms,
            "parseMs": parse_ms,
            "bodiesMs": bodies_ms,
            "buildMsSum": results.iter().map(|r| r.build_ms).sum::<f64>(),
            "factsMsSum": results.iter().map(|r| r.facts_ms).sum::<f64>(),
            "checkMs": CHECKS
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let total: f64 = results
                        .iter()
                        .map(|r| r.check_ms.get(i).copied().unwrap_or(0.0))
                        .sum();
                    ((*name).to_string(), json!(total))
                })
                .collect::<Map<String, Value>>(),
            "totalMs": ms(total),
            "threads": args.threads,
            "engine": if args.reference { "reference" } else { "shared-gauss" },
            "buildPhaseMs": pass.build_phase_ms,
            "preparePhaseMsSum": results.iter().map(|r| r.prepare_ms).sum::<f64>(),
            "integratePhaseMs": pass.integrate_phase_ms,
            "faceTasks": pass.tasks,
            "fallbackBodies": results.iter().filter(|r| r.fallback.is_some()).count(),
            "fallbackReasons": results.iter().filter_map(|r| r.fallback.as_deref()).take(3).collect::<Vec<_>>(),
        }),
    );
    row.insert(
        "import".into(),
        json!({
            "bodies": model.bodies.len(),
            "solidBodies": solid_bodies,
            "built": built,
            "topologyPath": results.iter().filter(|r| r.path_topology).count(),
            "faceFailures": results.iter().map(|r| r.face_failures).sum::<usize>(),
            "skipped": model.skipped.iter().map(|s| s.reason.clone()).collect::<Vec<_>>(),
            "diagnostics": model.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>(),
            "assemblyInstances": model.assembly_instances,
            "buildErrors": build_errors,
            "factsErrors": facts_errors,
            "integrityErrors": results
                .iter()
                .filter_map(|r| r.integrity_error.as_deref())
                .take(3)
                .collect::<Vec<_>>(),
        }),
    );
    let solids: usize = results.iter().map(|r| r.solids).sum();
    let manifold = results.iter().filter(|r| r.manifold == Some(true)).count();
    row.insert("solids".into(), json!(solids));
    row.insert("manifoldBodies".into(), json!(manifold));
    row.insert(
        "faces".into(),
        json!(results.iter().map(|r| r.faces).sum::<usize>()),
    );
    row.insert(
        "edges".into(),
        json!(results.iter().map(|r| r.edges).sum::<usize>()),
    );
    row.insert(
        "vertices".into(),
        json!(results.iter().map(|r| r.vertices).sum::<usize>()),
    );
    if args.open_only {
        row.insert(
            "open".into(),
            json!(if built > 0 { "ok" } else { "no-body" }),
        );
        return Value::Object(row);
    }
    // Volume, area and centroid over manifold bodies; a file-level value only when
    // every manifold body produced one.
    let solids_with_volume: Vec<&BodyResult> = results
        .iter()
        .filter(|r| r.manifold == Some(true))
        .collect();
    if !solids_with_volume.is_empty() && solids_with_volume.iter().all(|r| r.volume.is_some()) {
        let volume: f64 = solids_with_volume.iter().filter_map(|r| r.volume).sum();
        let mut c = [0.0; 3];
        for r in &solids_with_volume {
            if let (Some(v), Some(rc)) = (r.volume, r.centroid) {
                for (ci, x) in c.iter_mut().zip(rc) {
                    *ci += v * x;
                }
            }
        }
        if volume != 0.0 {
            row.insert("centroid".into(), json!(c.map(|x| x / volume)));
        }
        row.insert("volume".into(), json!(volume));
    }
    if results.iter().any(|r| r.built)
        && results.iter().filter(|r| r.built).all(|r| r.area.is_some())
    {
        row.insert(
            "area".into(),
            json!(results.iter().filter_map(|r| r.area).sum::<f64>()),
        );
    }
    let mut bounds: Option<[f64; 6]> = None;
    for b in results.iter().filter_map(|r| r.bounds) {
        bounds = Some(match bounds {
            None => b,
            Some(a) => [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].min(b[2]),
                a[3].max(b[3]),
                a[4].max(b[4]),
                a[5].max(b[5]),
            ],
        });
    }
    if let Some(b) = bounds {
        row.insert("bounds".into(), json!(b));
    }
    row.insert("valid".into(), json!(manifold > 0 && manifold == built));
    row.insert(
        "integrity".into(),
        json!(results
            .iter()
            .filter(|r| r.built)
            .all(|r| r.integrity == Some(true))),
    );
    // The stand-alone volume query must agree with the mass-properties volume.
    let stand_alone: Vec<f64> = results.iter().filter_map(|r| r.volume_only).collect();
    if !stand_alone.is_empty() {
        row.insert("volumeQuery".into(), json!(stand_alone.iter().sum::<f64>()));
    }
    Value::Object(row)
}
