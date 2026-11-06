//! `solidcore-facts`: GeoSpec's admission facts through solidcore.
//!
//! Usage: `geospec-solidcore-eval [--open-only] [--threads N] [--accuracy A]
//! [--claim-tolerance T] [--lenient-admission] [--lenient-closure] [--trim-floor-mm F]
//! [--trim-floor-rel R] [--origin-limit L] <file.step>`
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
//! not built or skipped). Each body is built from the file's topology alone, never
//! stitched, healed or integrated during the build, and its closure is tested
//! ([`closure`]): every edge used exactly twice, every trim on its 3D edge at the file's
//! tolerance, a volume that does not depend on the origin, no face of a closed solid
//! inverted. A body that fails refuses the file's facts (`not-closed`,
//! `closure-unchecked`, `face-inverted`, `trim-inconsistent`, `origin-dependent`); a
//! surface model stays a sheet, with an area and a box and no
//! volume. `manifold` is every body built, closed by that test and a manifold solid,
//! with no face left out or untrimmed and no tolerance raised above the zone
//! tolerance; `oracle` adds that every body kept the file's topology unrepaired (no
//! repaired build): only such files serve as a differential oracle.
//! `--lenient-admission` reports the admission findings without refusing, and
//! `--lenient-closure` the closure verdicts (in `closure.bodies`) without refusing:
//! both for measurement only.
//!
//! `--threads N` builds and measures bodies on N scoped worker threads, each with
//! its own `Kernel` (solidcore's kernel is single-threaded by design). Results are
//! gathered in body order, so the output is identical for every N.
//!
//! By default volume, area and centre of mass come from one shared pass of the
//! adaptive Gauss-Kronrod integrator (`solidcore::mass::gauss`, from `patches/`),
//! run per face across the same N threads once every body is built; the same pass
//! gives each face's vector area for the two-origin test. `--reference` keeps
//! solidcore's own per-query integrator for comparison.

mod admit;
mod closure;
mod fast;
mod filegeom;
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
    /// Report closure verdicts without refusing (measurement only).
    lenient_closure: bool,
    /// The closure test's thresholds.
    limits: closure::Limits,
    path: String,
}

/// The closure test's default thresholds (see the harness README for the evidence).
pub(crate) const DEFAULT_LIMITS: closure::Limits = closure::Limits {
    floor_mm: 1e-5,
    floor_rel: 0.0,
    origin: 1e-7,
};

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        open_only: std::env::var_os("PARITY_OPEN_ONLY").is_some_and(|v| v == "1"),
        reference: false,
        threads: 1,
        accuracy: 1e-7,
        claim_tolerance: None,
        lenient: false,
        lenient_closure: false,
        limits: DEFAULT_LIMITS,
        path: String::new(),
    };
    let positive = |v: Option<String>, what: &str| -> Result<f64, String> {
        v.and_then(|v| v.parse().ok())
            .filter(|a: &f64| a.is_finite() && *a >= 0.0)
            .ok_or_else(|| format!("{what} needs a non-negative number"))
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
            "--lenient-closure" => args.lenient_closure = true,
            "--trim-floor-mm" => args.limits.floor_mm = positive(it.next(), "--trim-floor-mm")?,
            "--trim-floor-rel" => args.limits.floor_rel = positive(it.next(), "--trim-floor-rel")?,
            "--origin-limit" => args.limits.origin = positive(it.next(), "--origin-limit")?,
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
    /// The closure measurements of the built B-rep ([`closure::check`]).
    closure: Option<closure::Closure>,
    /// The faces' signed vector area from the integration pass.
    vector_area: Option<Result<[f64; 3], String>>,
    /// Faces whose trims bound a negative area in the integration pass.
    faces_inverted: usize,
    /// The closure verdict: `closed`, `shell` or a refusal class.
    closure_class: Option<&'static str>,
    /// The closure refusal `(class, reason)`; the body keeps no facts.
    closure_refusal: Option<(&'static str, String)>,
    /// [`closure::origin_dependence`] and [`closure::world_origin_dependence`].
    origin_dependence: Option<f64>,
    world_origin_dependence: Option<f64>,
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

/// [`refuse_facts`], then the closure verdict ([`closure::verdict`]): a body that is
/// not closed refuses its facts and keeps none. A body whose facts are already refused
/// keeps that refusal; its closure measurements are still reported.
pub(crate) fn settle_facts(r: &mut BodyResult, args: &Args) {
    refuse_facts(r, args.claim_tolerance);
    if !r.built || r.refusal.is_some() {
        return;
    }
    let Some(c) = r.closure.as_ref() else {
        r.closure_class = Some("closure-unchecked");
        r.closure_refusal = Some(("closure-unchecked", "closure not measured".to_string()));
        clear_facts(r);
        return;
    };
    if let (Some(Ok(a)), Some(v), Some(b)) = (&r.vector_area, r.volume, r.bounds) {
        r.origin_dependence = Some(closure::origin_dependence(*a, v, &b));
        r.world_origin_dependence = Some(closure::world_origin_dependence(*a, v, &b));
    }
    let v = closure::verdict(
        &closure::Body3 {
            closure: c,
            manifold: r.manifold == Some(true),
            stated_solid: r.stated_solid,
            vector_area: r.vector_area.as_ref(),
            faces_inverted: r.faces_inverted,
            volume: r.volume,
            bounds: r.bounds,
        },
        &args.limits,
    );
    match v {
        closure::Verdict::Closed => r.closure_class = Some("closed"),
        closure::Verdict::Shell => r.closure_class = Some("shell"),
        closure::Verdict::Refused(class, _) if args.lenient_closure => {
            r.closure_class = Some(class);
        }
        closure::Verdict::Refused(class, why) => {
            r.closure_class = Some(class);
            r.closure_refusal = Some((class, why));
            clear_facts(r);
        }
    }
}

fn clear_facts(r: &mut BodyResult) {
    r.volume = None;
    r.volume_only = None;
    r.area = None;
    r.centroid = None;
    r.bounds = None;
}

/// Whether the file states a body of `kind` as a solid.
pub(crate) fn is_stated_solid(kind: BodyKind) -> bool {
    matches!(
        kind,
        BodyKind::ManifoldSolid | BodyKind::SolidWithVoids | BodyKind::Faceted
    )
}

/// The file-level verdicts over every body's result.
#[derive(Debug, PartialEq)]
struct RollUp {
    /// Every body built, none skipped, every built body a manifold solid that passed
    /// the closure test, plus trim consistency (no face left out or untrimmed, no body
    /// refused, no tolerance raised above the zone tolerance or unreadable).
    manifold: bool,
    /// `manifold`, and every body kept the file's topology unrepaired.
    oracle: bool,
    /// The file-level facts refusal, `(class, reasons)`: `facts` when a body's facts
    /// are refused; else the first closure class ([`CLOSURE_CLASSES`]) a body refuses
    /// on; else `incomplete` when a body was not built or was skipped, or a stated
    /// solid has no volume, so a file-level volume would leave it out.
    refusal: Option<(&'static str, Vec<String>)>,
}

/// The closure refusal classes, in the order a file reports them.
const CLOSURE_CLASSES: [&str; 5] = [
    "not-closed",
    "closure-unchecked",
    "face-inverted",
    "trim-inconsistent",
    "origin-dependent",
];

/// Rolls `results` (in body order) and `skipped` (bodies the importer skipped) up
/// to the file's verdicts.
fn roll_up(results: &[BodyResult], skipped: usize) -> RollUp {
    let all_built = skipped == 0 && results.iter().all(|r| r.built);
    let manifold = all_built
        && !results.is_empty()
        && results.iter().all(|r| {
            r.manifold == Some(true)
                && r.closure_class == Some("closed")
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
    let closure_class = CLOSURE_CLASSES.into_iter().find(|c| {
        results
            .iter()
            .any(|r| r.closure_refusal.as_ref().is_some_and(|(k, _)| k == c))
    });
    let refusal = if !refused.is_empty() {
        Some(("facts", refused))
    } else if let Some(class) = closure_class {
        let reasons = results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.closure_refusal
                    .as_ref()
                    .map(|(k, why)| format!("body {i}: {k}: {why}"))
            })
            .collect();
        Some((class, reasons))
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

fn run_body(k: &Kernel, body: &Body, pcurves: &FilePcurves, args: &Args) -> BodyResult {
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
    out.stated_solid = is_stated_solid(body.kind);
    out.max_tolerance = max_tolerance(&built.brep);
    out.marker_geometry = marker_geometry(&built.brep);
    out.face_failures = built.face_failures.len();
    out.faces = built.brep.face_list().len();
    out.edges = built.brep.edge_list().len();
    out.vertices = built.brep.vertex_list().len();
    if !args.open_only {
        let t = Instant::now();
        body_facts(k, &mut built.brep, args.accuracy, &mut out);
        if let Some(b) = out.bounds {
            let size = (b[3] - b[0])
                .max(b[4] - b[1])
                .max(b[5] - b[2])
                .max(f64::MIN_POSITIVE);
            let centre = Point3::new(
                0.5 * (b[0] + b[3]),
                0.5 * (b[1] + b[4]),
                0.5 * (b[2] + b[5]),
            );
            out.closure = Some(closure::check(
                k,
                body,
                &mut built.brep,
                size,
                out.stated_solid,
                &args.limits,
            ));
            // the reference path integrates with solidcore's own integrator; the
            // closure tests read the shared pass's per-face results, made here too
            let mut inverted = 0;
            out.vector_area = Some(
                gauss::prepare(k, &mut built.brep, 0, centre, size, args.accuracy).and_then(
                    |tasks| {
                        let mut total = [0.0; 3];
                        for t in &tasks {
                            if gauss::run(k, t).is_ok_and(|v| v[0] < 0.0) {
                                inverted += 1;
                            }
                            let a = gauss::vector_area(k, t)?;
                            for s in &t.volume_signs {
                                for (o, x) in total.iter_mut().zip(a) {
                                    *o += s * x;
                                }
                            }
                        }
                        Ok(total)
                    },
                ),
            );
            out.faces_inverted = inverted;
        }
        out.facts_ms = ms(t);
        settle_facts(&mut out, args);
    }
    out
}

fn run_bodies(bodies: &[Body], pcurves: &FilePcurves, args: &Args) -> Vec<BodyResult> {
    let threads = args.threads.min(bodies.len()).max(1);
    if threads == 1 {
        let k = Kernel::default();
        return bodies
            .iter()
            .map(|b| run_body(&k, b, pcurves, args))
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
                            let r = run_body(&k, b, pcurves, args);
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
            "closureMsSum": results
                .iter()
                .filter_map(|r| r.closure.as_ref().map(|c| c.ms))
                .sum::<f64>(),
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
    row.insert("closure".into(), closure_summary(&results, &args.limits));
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

/// The file's closure measurements: verdicts by body, sums of the counts, the
/// largest trim deviation and origin dependence, and the limits applied. Bodies with
/// a trim beyond twice its edge's gap list `[criticalMm, sizeMm]`, so any other floor
/// can be applied to the same run.
fn closure_summary(results: &[BodyResult], limits: &closure::Limits) -> Value {
    let built: Vec<&BodyResult> = results.iter().filter(|r| r.built).collect();
    let cs: Vec<&closure::Closure> = built.iter().filter_map(|r| r.closure.as_ref()).collect();
    let mut verdicts = Map::new();
    for r in &built {
        let v = r.closure_class.unwrap_or(if r.refusal.is_some() {
            "facts-refused"
        } else {
            "unmeasured"
        });
        let n = verdicts.get(v).and_then(Value::as_u64).unwrap_or(0);
        verdicts.insert(v.into(), json!(n + 1));
    }
    let fmax = |xs: &mut dyn Iterator<Item = f64>| {
        xs.fold(None, |a: Option<f64>, x| {
            Some(a.map_or(x, |a| nan_max(a, x)))
        })
    };
    json!({
        "bodies": verdicts,
        "edgeUsesBad": cs.iter().map(|c| c.edge_uses_bad).sum::<usize>(),
        "facesInverted": built.iter().map(|r| r.faces_inverted).sum::<usize>(),
        "facesUnchecked": cs.iter().map(|c| c.faces_unchecked).sum::<usize>(),
        "trims": cs.iter().map(|c| c.trims).sum::<usize>(),
        "trimsMade": cs.iter().map(|c| c.trims_made).sum::<usize>(),
        "trimsBad": cs.iter().map(|c| c.trims_bad).sum::<usize>(),
        "trimsSkippedBodies": cs.iter().filter(|c| c.trims_skipped).count(),
        "criticalMm": fmax(&mut cs.iter().map(|c| c.critical_mm)),
        "criticalPerSize": fmax(&mut cs.iter().map(|c| c.critical_mm / c.size.max(f64::MIN_POSITIVE))),
        "critical": cs.iter().filter(|c| c.critical_mm > 0.0).map(|c| [c.critical_mm, c.size]).collect::<Vec<_>>(),
        "gapMaxMm": fmax(&mut cs.iter().map(|c| c.gap_max_mm)),
        "originDependence": fmax(&mut built.iter().filter_map(|r| r.origin_dependence)),
        "worldOriginDependence": fmax(&mut built.iter().filter_map(|r| r.world_origin_dependence)),
        "limits": { "floorMm": limits.floor_mm, "floorRel": limits.floor_rel, "origin": limits.origin },
    })
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
            closure_class: Some("closed"),
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

    /// A body that fails the closure test refuses the file's facts with its class,
    /// before `incomplete`, after `facts`; the file is not manifold.
    #[test]
    fn a_closure_refusal_refuses_the_file_by_class() {
        let cube = closed_cube;
        let refused = |class: &'static str| BodyResult {
            closure_class: Some(class),
            closure_refusal: Some((class, "why".into())),
            volume: None,
            ..cube()
        };
        for class in CLOSURE_CLASSES {
            let roll = roll_up(&[cube(), refused(class)], 0);
            assert!(!roll.manifold);
            assert!(
                roll.refusal
                    .as_ref()
                    .is_some_and(|(c, r)| *c == class && r == &[format!("body 1: {class}: why")]),
                "{:?}",
                roll.refusal
            );
        }
        // the first class in CLOSURE_CLASSES order wins, whatever the body order
        let roll = roll_up(&[refused("origin-dependent"), refused("not-closed")], 0);
        assert!(roll
            .refusal
            .is_some_and(|(c, r)| c == "not-closed" && r.len() == 2));
        // a facts refusal comes first
        let facts = BodyResult {
            refusal: Some("extent: none".into()),
            ..BodyResult::default()
        };
        let roll = roll_up(&[facts, refused("trim-inconsistent")], 0);
        assert!(roll.refusal.is_some_and(|(c, _)| c == "facts"));
        // a shell is not closed, and not refused
        let shell = BodyResult {
            closure_class: Some("shell"),
            manifold: Some(false),
            stated_solid: false,
            volume: None,
            ..cube()
        };
        let roll = roll_up(&[cube(), shell], 0);
        assert_eq!((roll.manifold, roll.refusal), (false, None));
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
