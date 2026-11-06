//! `geospec-facts-solidcore`: one STEP file through solidcore's importer and
//! B-rep builder, then the facts layer over the solidcore adapter.
//!
//! Usage: `geospec-facts-solidcore [--threads N] [--accuracy A] [--reference]
//! <file.step>`
//!
//! Prints one JSON line: per body the facts with their bounds (or the typed
//! refusal), and per face the raw integrals and bounds (`--faces`). Bodies are
//! built and exported on N scoped threads (one kernel each) and every face of
//! every body integrates on one pool of N threads; results are in body and
//! face order, so the output is identical for every N except the timings.
//! `--reference` adds patch 0001's integrator (`solidcore::mass::gauss`) on
//! the same B-rep, for the differential.

use std::time::Instant;

use geospec_facts::{bodies_facts, BodyDefinition, BodyFacts, Options, Refusal};
use serde_json::{json, Value};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::Kernel;
use step_brep::{build_body_with_file_pcurves, BuildOptions};
use step_import::model::{Body, FilePcurves};
use step_import::Limits;

struct Args {
    threads: usize,
    accuracy: f64,
    reference: bool,
    faces: bool,
    path: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        threads: 1,
        accuracy: 1e-7,
        reference: false,
        faces: false,
        path: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--reference" => args.reference = true,
            "--faces" => args.faces = true,
            "--threads" => {
                args.threads = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--threads needs a positive integer")?;
            }
            "--accuracy" => {
                args.accuracy = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|a: &f64| *a > 0.0)
                    .ok_or("--accuracy needs a positive number")?;
            }
            _ if args.path.is_empty() => args.path = arg,
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    if args.path.is_empty() {
        return Err("usage: geospec-facts-solidcore [--threads N] [--accuracy A] [--reference] [--faces] <file.step>".into());
    }
    Ok(args)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// One body after build and export.
struct Built {
    error: Option<String>,
    manifold: Option<bool>,
    definition: Option<Result<BodyDefinition, Refusal>>,
    reference: Option<Value>,
    build_ms: f64,
    export_ms: f64,
}

fn build(k: &Kernel, body: &Body, pcurves: &FilePcurves, args: &Args) -> Built {
    let t = Instant::now();
    let built = build_body_with_file_pcurves(body, pcurves, BuildOptions::default());
    let build_ms = ms(t);
    let mut built = match built {
        Ok(b) => b,
        Err(e) => {
            return Built {
                error: Some(e.message),
                manifold: None,
                definition: None,
                reference: None,
                build_ms,
                export_ms: 0.0,
            }
        }
    };
    let manifold = brep_is_manifold_solid(&built.brep).ok();
    let t = Instant::now();
    let definition = geospec_facts_solidcore::body_definition(k, &mut built.brep);
    let export_ms = ms(t);
    let reference = args.reference.then(|| {
        match solidcore::mass::gauss::brep_mass(k, &mut built.brep, args.accuracy, 1) {
            Ok(m) => {
                let c = m.centroid();
                json!({
                    "volume": m.volume,
                    "area": m.area,
                    "centroid": c.map(|c| [c.x, c.y, c.z]),
                })
            }
            Err(e) => json!({ "error": e }),
        }
    });
    Built {
        error: None,
        manifold,
        definition: Some(definition),
        reference,
        build_ms,
        export_ms,
    }
}

fn build_all(bodies: &[Body], pcurves: &FilePcurves, args: &Args) -> Vec<Built> {
    let threads = args.threads.min(bodies.len()).max(1);
    let mut slots: Vec<Option<Built>> = bodies.iter().map(|_| None).collect();
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
                        .map(|(i, b)| (i, build(&k, b, pcurves, args)))
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
            s.unwrap_or_else(|| Built {
                error: Some("worker thread panicked".into()),
                manifold: None,
                definition: None,
                reference: None,
                build_ms: 0.0,
                export_ms: 0.0,
            })
        })
        .collect()
}

fn set(v: &mut Value, key: &str, x: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_string(), x);
    }
}

fn refusal_json(r: &Refusal) -> Value {
    json!({ "code": r.code.as_str(), "face": r.face, "detail": r.detail })
}

fn facts_json(f: &BodyFacts, faces: bool) -> Value {
    let mut v = json!({
        "volume": f.volume.value,
        "volumeBound": f.volume.accuracy.bound,
        "area": f.area.value,
        "areaBound": f.area.accuracy.bound,
        "method": f.volume.accuracy.method.as_str(),
        "closure": f.closure,
        "centroid": f.centroid.map(|c| [c.value.x, c.value.y, c.value.z]),
        "centroidBound": f.centroid.map(|c| [c.axis_bounds.x, c.axis_bounds.y, c.axis_bounds.z]),
    });
    if faces {
        let list: Vec<Value> = f
            .faces
            .iter()
            .map(
                |m| json!({ "raw": m.raw, "bound": m.bound, "exact": m.exact, "panels": m.panels }),
            )
            .collect();
        set(&mut v, "faces", Value::Array(list));
    }
    v
}

fn run(args: &Args) -> Value {
    let total = Instant::now();
    let bytes = match std::fs::read(&args.path) {
        Ok(b) => b,
        Err(e) => return json!({ "open": { "error": format!("read: {e}") } }),
    };
    let model = match step_import::import_threads(&bytes, &Limits::default(), args.threads) {
        Ok(m) => m,
        Err(e) => {
            return json!({ "open": { "error": format!("part21: line {}: {}", e.line, e.message) } })
        }
    };
    let t = Instant::now();
    let mut built = build_all(&model.bodies, &model.file_pcurves, args);
    let build_phase_ms = ms(t);
    // Every face of every exported body on one pool.
    let mut index = Vec::new();
    let mut definitions = Vec::new();
    for (i, b) in built.iter_mut().enumerate() {
        match b.definition.take() {
            Some(Ok(d)) => {
                index.push(i);
                definitions.push(d);
            }
            other => b.definition = other,
        }
    }
    let t = Instant::now();
    let options = Options {
        accuracy: args.accuracy,
        threads: args.threads,
    };
    let mut facts: Vec<Option<Result<BodyFacts, Refusal>>> = built.iter().map(|_| None).collect();
    for (i, r) in index.iter().zip(bodies_facts(&definitions, options)) {
        if let Some(slot) = facts.get_mut(*i) {
            *slot = Some(r);
        }
    }
    let integrate_ms = ms(t);
    let bodies: Vec<Value> = built
        .iter()
        .zip(&facts)
        .map(|(b, f)| {
            let mut v = json!({
                "manifold": b.manifold,
                "buildMs": b.build_ms,
                "exportMs": b.export_ms,
            });
            if let Some(e) = &b.error {
                set(&mut v, "buildError", json!(e));
            }
            if let Some(Err(r)) = &b.definition {
                set(&mut v, "refusal", refusal_json(r));
                set(&mut v, "stage", json!("export"));
            }
            match f {
                Some(Ok(f)) => set(&mut v, "facts", facts_json(f, args.faces)),
                Some(Err(r)) => {
                    set(&mut v, "refusal", refusal_json(r));
                    set(&mut v, "stage", json!("integrate"));
                }
                None => {}
            }
            if let Some(r) = &b.reference {
                set(&mut v, "reference", r.clone());
            }
            v
        })
        .collect();
    json!({
        "threads": args.threads,
        "accuracy": args.accuracy,
        "bodies": bodies,
        "phases": {
            "buildPhaseMs": build_phase_ms,
            "integrateMs": integrate_ms,
            "totalMs": ms(total),
        },
    })
}

fn main() {
    let row = match parse_args() {
        Ok(args) => run(&args),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    println!("{row}");
}
