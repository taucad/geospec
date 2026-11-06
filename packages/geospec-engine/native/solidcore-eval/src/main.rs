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
//! `open.refusal` and no facts. A body whose facts cannot describe a solid (geometry
//! or an extent at the infinity marker or not finite, a volume not finite and
//! positive or larger than its box, a centre of mass outside its box, a tolerance
//! raised above `--claim-tolerance` or unreadable) refuses the file's facts
//! (`refusal`, class `facts`); so does a body left out of them (class `incomplete`:
//! not built, skipped, or a stated solid that did not close). `manifold` is closure
//! (every body built and a manifold solid) plus trim consistency (no face left out or
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
    /// The largest edge or vertex tolerance of the built B-rep (NaN when one
    /// cannot be read).
    max_tolerance: Option<f64>,
    /// Faces and edges at solidcore's infinity marker in parameter space
    /// ([`marker_geometry`]).
    marker_geometry: usize,
    /// The file states the body as a solid (`MANIFOLD_SOLID_BREP`,
    /// `BREP_WITH_VOIDS` or `FACETED_BREP`): the file's volume needs its own.
    stated_solid: bool,
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

/// The largest edge or vertex tolerance of `brep`; NaN when a tolerance is NaN or
/// cannot be read, so that a failed lookup counts as raised ([`admit::raised`]) and
/// refuses every claim, never as the zone tolerance. `None` for a B-rep without
/// edges or vertices.
pub(crate) fn max_tolerance(brep: &Brep) -> Option<f64> {
    use solidcore::topology::{edge, vertex};
    let edges = brep
        .edge_list()
        .iter()
        .map(|&e| edge::tolerance(brep, e).unwrap_or(f64::NAN));
    let vertices = brep
        .vertex_list()
        .iter()
        .map(|&v| vertex::tolerance(brep, v).unwrap_or(f64::NAN));
    edges.chain(vertices).reduce(nan_max)
}

/// The larger of `a` and `b`, NaN when either is (`f64::max` drops a NaN).
fn nan_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// How many faces and edges of `brep` reach solidcore's infinity marker in their
/// parameter space: a face whose UV domain is unbounded and whose UV trims do not
/// bound it (they are unbounded too, or cannot be made), or an edge whose curve
/// interval is unbounded. Such a face is untrimmed geometry at the marker; once a
/// placement or unit scales or rotates it, its 3D box is no longer at the marker
/// itself, so the box test of [`admit::facts`] alone misses it.
pub(crate) fn marker_geometry(brep: &Brep) -> usize {
    use solidcore::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;
    use solidcore::topology::{edge, face};
    let k = Kernel::default();
    let mut copy: Option<Brep> = None;
    let faces = brep
        .face_list()
        .iter()
        .filter(|&&f| {
            if face::uv_domain(brep, f).is_ok_and(|d| d.is_bounded()) {
                return false;
            }
            let copy = copy.get_or_insert_with(|| brep.clone());
            !calculate_uv_domain_from_uv_trim_curves(&k, copy, f, true, false, false)
                .is_ok_and(|t| t.domain.is_bounded())
        })
        .count();
    let edges = brep
        .edge_list()
        .iter()
        .filter(|&&e| !edge::interval(brep, e).is_ok_and(|d| d.is_bounded()))
        .count();
    faces + edges
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
                marker_geometry: r.marker_geometry,
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

/// Whether the file states a body of `kind` as a solid.
fn is_stated_solid(kind: BodyKind) -> bool {
    matches!(
        kind,
        BodyKind::ManifoldSolid | BodyKind::SolidWithVoids | BodyKind::Faceted
    )
}

/// The file-level verdicts over every body's result.
#[derive(Debug, PartialEq)]
struct RollUp {
    /// Closure (every body built, none skipped, every built body a manifold solid)
    /// plus trim consistency (no face left out or untrimmed, no body refused, no
    /// tolerance raised above the zone tolerance or unreadable).
    manifold: bool,
    /// `manifold`, and every body kept the file's topology unrepaired.
    oracle: bool,
    /// The file-level facts refusal, `(class, reasons)`: `facts` when a body's facts
    /// are refused, else `incomplete` when a body was not built or was skipped, or a
    /// stated solid has no volume, so a file-level volume would leave it out.
    refusal: Option<(&'static str, Vec<String>)>,
}

/// Rolls `results` (in body order) and `skipped` (bodies the importer skipped) up
/// to the file's verdicts.
fn roll_up(results: &[BodyResult], skipped: usize) -> RollUp {
    let all_built = skipped == 0 && results.iter().all(|r| r.built);
    let manifold = all_built
        && !results.is_empty()
        && results.iter().all(|r| {
            r.manifold == Some(true)
                && r.face_failures == 0
                && r.refusal.is_none()
                && r.max_tolerance.is_some_and(|t| !admit::raised(t))
        });
    let oracle = manifold && results.iter().all(|r| r.path_topology && !r.repaired);
    let refused: Vec<String> = results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.refusal.as_ref().map(|e| format!("body {i}: {e}")))
        .collect();
    let refusal = if !refused.is_empty() {
        Some(("facts", refused))
    } else {
        let mut reasons: Vec<String> = Vec::new();
        if skipped > 0 {
            reasons.push(format!("import: {skipped} body(ies) skipped"));
        }
        for (i, r) in results.iter().enumerate() {
            if !r.built {
                reasons.push(format!(
                    "body {i}: not built ({})",
                    r.error.as_deref().unwrap_or("no build")
                ));
            } else if r.stated_solid && (r.manifold != Some(true) || r.volume.is_none()) {
                reasons.push(format!(
                    "body {i}: a stated solid without a volume ({})",
                    if r.manifold == Some(true) {
                        r.facts_error.as_deref().unwrap_or("not measured")
                    } else {
                        "not closed"
                    }
                ));
            }
        }
        (!reasons.is_empty()).then_some(("incomplete", reasons))
    };
    RollUp {
        manifold,
        oracle,
        refusal,
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
    out.marker_geometry = marker_geometry(&built.brep);
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
    // The size limit refuses from the file's length, before the file is read.
    if let Some(r) = std::fs::metadata(&args.path)
        .ok()
        .and_then(|m| admit::size(m.len()))
    {
        if !args.lenient {
            return json!({
                "open": {
                    "error": format!("admission: {}", r.reasons.first().map_or("", String::as_str)),
                    "refusal": { "class": r.class, "reasons": r.reasons, "count": r.count },
                },
            });
        }
    }
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
    let mut results = results;
    for (r, b) in results.iter_mut().zip(&model.bodies) {
        r.stated_solid = is_stated_solid(b.kind);
    }

    let solid_bodies = model
        .bodies
        .iter()
        .filter(|b| is_stated_solid(b.kind))
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
        .reduce(nan_max)
    {
        // NaN (a tolerance that could not be read) prints as null
        row.insert("maxTolerance".into(), json!(t));
    }
    let roll = roll_up(&results, model.skipped.len());
    row.insert("manifold".into(), json!(roll.manifold));
    row.insert("oracle".into(), json!(roll.oracle));
    row.insert(
        "integrity".into(),
        json!(results
            .iter()
            .filter(|r| r.built)
            .all(|r| r.integrity == Some(true))),
    );
    if let Some((class, reasons)) = roll.refusal {
        row.insert(
            "refusal".into(),
            json!({
                "class": class,
                "reasons": reasons.iter().take(admit::MAX_REASONS).collect::<Vec<_>>(),
                "count": reasons.len(),
            }),
        );
        return Value::Object(row);
    }
    // Volume, area and centroid over the manifold bodies: every body was built and
    // every stated solid measured ([`roll_up`]), so no stated solid is left out.
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
            path_topology: true,
            stated_solid: true,
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

    #[test]
    fn the_file_roll_up_needs_every_body_built_closed_and_unrepaired() {
        let cube = closed_cube;
        let clean = roll_up(&[cube(), cube()], 0);
        assert_eq!(
            (clean.manifold, clean.oracle, clean.refusal),
            (true, true, None)
        );
        // a stitched or a repaired body is manifold, never an oracle
        let stitched = BodyResult {
            path_topology: false,
            ..cube()
        };
        let repaired = BodyResult {
            repaired: true,
            ..cube()
        };
        for r in [stitched, repaired] {
            let roll = roll_up(&[cube(), r], 0);
            assert_eq!((roll.manifold, roll.oracle), (true, false));
        }
        // a raised or unreadable tolerance, or a face left out, is not trim consistent
        for r in [
            BodyResult {
                max_tolerance: Some(2e-5),
                ..cube()
            },
            BodyResult {
                max_tolerance: Some(f64::NAN),
                ..cube()
            },
            BodyResult {
                max_tolerance: None,
                ..cube()
            },
            BodyResult {
                face_failures: 1,
                ..cube()
            },
        ] {
            let roll = roll_up(&[cube(), r], 0);
            assert_eq!(
                (roll.manifold, roll.oracle, roll.refusal),
                (false, false, None)
            );
        }
    }

    #[test]
    fn a_body_left_out_refuses_the_file_facts() {
        let cube = closed_cube;
        // a body that failed to build
        let failed = BodyResult {
            error: Some("no shell".into()),
            ..BodyResult::default()
        };
        let roll = roll_up(&[cube(), failed], 0);
        assert!(!roll.manifold);
        assert!(roll
            .refusal
            .is_some_and(|(c, r)| c == "incomplete" && r == ["body 1: not built (no shell)"]));
        // a body the importer skipped
        let roll = roll_up(&[cube()], 1);
        assert!(!roll.manifold);
        assert!(roll.refusal.is_some_and(|(c, _)| c == "incomplete"));
        // a stated solid that did not close has no volume to add
        let open = BodyResult {
            manifold: Some(false),
            volume: None,
            centroid: None,
            ..cube()
        };
        let roll = roll_up(&[cube(), open], 0);
        assert!(roll.refusal.is_some_and(|(c, r)| c == "incomplete"
            && r.first()
                .is_some_and(|x| x.starts_with("body 1: a stated solid"))));
        // a sheet the file does not state as a solid is not left out
        let sheet = BodyResult {
            manifold: Some(false),
            stated_solid: false,
            volume: None,
            ..cube()
        };
        let roll = roll_up(&[cube(), sheet], 0);
        assert_eq!((roll.manifold, roll.refusal), (false, None));
        // a refused body's facts refuse the file first
        let refused = BodyResult {
            refusal: Some("extent: none".into()),
            ..BodyResult::default()
        };
        let roll = roll_up(&[refused, BodyResult::default()], 0);
        assert!(roll.refusal.is_some_and(|(c, _)| c == "facts"));
    }

    #[test]
    fn an_unreadable_tolerance_is_the_largest() {
        assert!(nan_max(1.0, f64::NAN).is_nan());
        assert!(nan_max(f64::NAN, 1.0).is_nan());
        assert_eq!(nan_max(1.0, 2.0), 2.0);
    }

    #[test]
    fn geometry_at_the_marker_refuses_the_body() {
        let mut r = closed_cube();
        r.marker_geometry = 1;
        refuse_facts(&mut r, None);
        assert!(r.refusal.is_some_and(|e| e.starts_with("extent: 1 face")));
        assert_eq!((r.volume, r.area, r.bounds), (None, None, None));
    }
}
