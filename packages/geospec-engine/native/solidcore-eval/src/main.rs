//! `solidcore-facts`: GeoSpec's admission facts through solidcore.
//!
//! Usage: `geospec-solidcore-eval [--open-only] [--threads N] [--accuracy A]
//! [--claim-tolerance T] [--lenient-admission] <file.step>`
//!
//! Prints one JSON line: per-phase wall times (read, Part 21 parse and map, B-rep
//! build, facts), the importer's findings, and per-file facts (solids, faces,
//! volume, area, centroid, bounds, manifold state). A failure is a typed reason in
//! the JSON, never a panic; the process exit code is 0 whenever the JSON was
//! written.
//!
//! Admission is fail-closed ([`admit`]): a file the admission profile refuses
//! (Part 21 and header defects, entity parameters OCCT's reader refuses, dangling or
//! external references, units the file does not state, assembly placements the
//! importer does not apply, a subject over the size limit) prints
//! `open.refusal` and no facts. A body whose facts cannot describe a solid (an
//! extent at the infinity marker or not finite, a volume not finite and positive or
//! larger than its box, a centre of mass outside its box, a tolerance raised above
//! `--claim-tolerance`) refuses the file's facts (`refusal`). `manifold` is closure
//! (every built body a manifold solid) plus trim consistency (no face left out or
//! untrimmed, no tolerance raised above the zone tolerance); `oracle` adds that every
//! body kept the file's topology unrepaired (no `BuildPath::Stitch`, no repaired or
//! healed build): only such files serve as a differential oracle.
//! `--lenient-admission` reports the admission findings without refusing, for
//! measurement only.
//!
//! `--threads N` builds and measures bodies on N scoped worker threads, each with
//! its own `Kernel` (solidcore's kernel is single-threaded by design). Results are
//! gathered in body order, so the output is identical for every N.
//!
//! By default volume, area and centre of mass come from one shared pass of the
//! adaptive Gauss-Kronrod integrator (`solidcore::mass::gauss`, from `patches/`),
//! run per face across the same N threads; where the build's closed-solid check
//! already integrated the kept B-rep to the same accuracy, that pass is reused.
//! `--reference` keeps solidcore's own per-query integrator for comparison.

mod admit;
mod fast;
use solidcore::mass::gauss;

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
    /// The finest tolerance (mm) a claim asks of the facts: a body whose edge or
    /// vertex tolerance was raised above it refuses its facts.
    claim_tolerance: Option<f64>,
    /// Report admission findings without refusing (measurement only).
    lenient: bool,
    path: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        open_only: std::env::var_os("PARITY_OPEN_ONLY").is_some_and(|v| v == "1"),
        reference: false,
        threads: 1,
        accuracy: 1e-7,
        claim_tolerance: None,
        lenient: false,
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
            "--lenient-admission" => args.lenient = true,
            "--claim-tolerance" => {
                args.claim_tolerance = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .filter(|a: &f64| a.is_finite() && *a > 0.0)
                        .ok_or("--claim-tolerance needs a positive number")?,
                );
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
    /// The kept topology build needed repair or was healed.
    repaired: bool,
    /// The largest edge or vertex tolerance of the built B-rep.
    max_tolerance: Option<f64>,
    /// Why this body's facts are refused ([`admit::facts`]).
    refusal: Option<String>,
    /// The file states no solid for it (a shell-based surface model or a loose
    /// face): a closed B-rep built from it is not a solid of the file.
    surface_model: bool,
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

/// The largest edge or vertex tolerance of `brep`.
fn max_tolerance(brep: &Brep) -> Option<f64> {
    use solidcore::topology::{edge, vertex};
    let edges = brep
        .edge_list()
        .iter()
        .filter_map(|&e| edge::tolerance(brep, e).ok());
    let vertices = brep
        .vertex_list()
        .iter()
        .filter_map(|&v| vertex::tolerance(brep, v).ok());
    edges.chain(vertices).reduce(f64::max)
}

/// Refuses a built body's facts ([`admit::facts`]) when they cannot describe a
/// solid, or when it has no extent; a refused body keeps no facts.
fn refuse_facts(r: &mut BodyResult, claim_tolerance: Option<f64>) {
    if !r.built {
        return;
    }
    // A surface model the build closed is not a solid the file states: it has an
    // area and a box, never a volume.
    if r.surface_model && r.manifold == Some(true) {
        r.manifold = Some(false);
        r.solids = 0;
        r.voids = 0;
        r.volume = None;
        r.volume_only = None;
        r.centroid = None;
    }
    let reason = if r.bounds.is_none() {
        Some(format!(
            "extent: none ({})",
            r.facts_error.as_deref().unwrap_or("no box")
        ))
    } else {
        admit::facts(
            &admit::Facts {
                manifold: r.manifold == Some(true),
                volume: r.volume,
                area: r.area,
                centroid: r.centroid,
                bounds: r.bounds,
                max_tolerance: r.max_tolerance,
            },
            claim_tolerance,
        )
    };
    if let Some(reason) = reason {
        r.refusal = Some(reason);
        r.volume = None;
        r.volume_only = None;
        r.area = None;
        r.centroid = None;
        r.bounds = None;
    }
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
    claim_tolerance: Option<f64>,
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
    out.repaired = built.repaired;
    out.surface_model = body.kind == BodyKind::ShellModel;
    out.max_tolerance = max_tolerance(&built.brep);
    out.face_failures = built.face_failures.len();
    out.faces = built.brep.face_list().len();
    out.edges = built.brep.edge_list().len();
    out.vertices = built.brep.vertex_list().len();
    if !open_only {
        let t = Instant::now();
        body_facts(k, &mut built.brep, accuracy, &mut out);
        out.facts_ms = ms(t);
        refuse_facts(&mut out, claim_tolerance);
    }
    out
}

fn run_bodies(bodies: &[Body], pcurves: &FilePcurves, args: &Args) -> Vec<BodyResult> {
    let threads = args.threads.min(bodies.len()).max(1);
    if threads == 1 {
        let k = Kernel::default();
        return bodies
            .iter()
            .map(|b| {
                run_body(
                    &k,
                    b,
                    pcurves,
                    args.open_only,
                    args.accuracy,
                    args.claim_tolerance,
                )
            })
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
                        .map(|(i, b)| {
                            let r = run_body(
                                &k,
                                b,
                                pcurves,
                                args.open_only,
                                args.accuracy,
                                args.claim_tolerance,
                            );
                            (i, r)
                        })
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
    let model = match step_import::import_threads(&bytes, &Limits::default(), args.threads) {
        Ok(m) => m,
        Err(e) => {
            return json!({
                "open": { "error": format!("part21: line {}: {}", e.line, e.message) },
                "phases": { "readMs": read_ms, "parseMs": ms(t) },
            })
        }
    };
    let parse_ms = ms(t);
    let refusal = admit::admission(bytes.len(), &model.admission);
    if let (Some(r), false) = (&refusal, args.lenient) {
        return json!({
            "open": {
                "error": format!("admission: {}", r.reasons.first().map_or("", String::as_str)),
                "refusal": { "class": r.class, "reasons": r.reasons, "count": r.count },
            },
            "phases": { "readMs": read_ms, "parseMs": parse_ms },
        });
    }
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
            "stitched": results.iter().filter(|r| r.built && !r.path_topology).count(),
            "repaired": results.iter().filter(|r| r.repaired).count(),
            "admission": refusal.as_ref().map(|r| json!({ "class": r.class, "reasons": r.reasons, "count": r.count })),
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
    if let Some(t) = results
        .iter()
        .filter(|r| r.built)
        .filter_map(|r| r.max_tolerance)
        .reduce(f64::max)
    {
        row.insert("maxTolerance".into(), json!(t));
    }
    // Closure (every built body a manifold solid) plus trim consistency (no face
    // left out or untrimmed, no tolerance raised above the zone tolerance).
    let built_bodies = || results.iter().filter(|r| r.built);
    let manifold_file = manifold > 0
        && manifold == built
        && built_bodies().all(|r| {
            r.face_failures == 0
                && r.refusal.is_none()
                && r.max_tolerance.is_some_and(|t| !admit::raised(t))
        });
    row.insert("manifold".into(), json!(manifold_file));
    row.insert(
        "oracle".into(),
        json!(manifold_file && built_bodies().all(|r| r.path_topology && !r.repaired)),
    );
    row.insert(
        "integrity".into(),
        json!(results
            .iter()
            .filter(|r| r.built)
            .all(|r| r.integrity == Some(true))),
    );
    let refused: Vec<String> = results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.refusal.as_ref().map(|e| format!("body {i}: {e}")))
        .collect();
    if !refused.is_empty() {
        row.insert(
            "refusal".into(),
            json!({
                "class": "facts",
                "reasons": refused.iter().take(admit::MAX_REASONS).collect::<Vec<_>>(),
                "count": refused.len(),
            }),
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
    // The stand-alone volume query must agree with the mass-properties volume.
    let stand_alone: Vec<f64> = results.iter().filter_map(|r| r.volume_only).collect();
    if !stand_alone.is_empty() {
        row.insert("volumeQuery".into(), json!(stand_alone.iter().sum::<f64>()));
    }
    Value::Object(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_cube() -> BodyResult {
        BodyResult {
            built: true,
            manifold: Some(true),
            solids: 1,
            volume: Some(1000.0),
            area: Some(600.0),
            centroid: Some([5.0, 5.0, 5.0]),
            bounds: Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
            max_tolerance: Some(1e-5),
            ..BodyResult::default()
        }
    }

    #[test]
    fn a_closed_solid_keeps_its_facts() {
        let mut r = closed_cube();
        refuse_facts(&mut r, None);
        assert_eq!(
            (r.refusal, r.volume, r.manifold),
            (None, Some(1000.0), Some(true))
        );
    }

    #[test]
    fn a_closed_surface_model_has_no_volume() {
        let mut r = closed_cube();
        r.surface_model = true;
        refuse_facts(&mut r, None);
        assert_eq!(
            (r.refusal, r.volume, r.manifold, r.solids),
            (None, None, Some(false), 0)
        );
        assert_eq!(r.area, Some(600.0));
    }

    #[test]
    fn a_body_without_extent_or_with_a_bad_fact_keeps_none() {
        let mut r = closed_cube();
        r.bounds = None;
        refuse_facts(&mut r, None);
        assert!(r.refusal.is_some_and(|e| e.starts_with("extent: none")));
        assert_eq!((r.volume, r.area), (None, None));
        let mut r = closed_cube();
        r.centroid = Some([5.0, 5.0, 50.0]);
        refuse_facts(&mut r, None);
        assert!(r.refusal.is_some_and(|e| e.starts_with("centroid:")));
        assert_eq!((r.volume, r.area, r.bounds), (None, None, None));
    }
}
