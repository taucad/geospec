//! `geospec-facts-occt`: one STEP file through the bridge's admission, then
//! the facts layer over the OCCT adapter.
//!
//! Usage: `geospec-facts-occt [--threads N] [--accuracy A] [--faces]
//! [--eps-reference E] <file.step>`
//!
//! Prints one JSON line in the shape of the solidcore adapter's runner: per
//! body (solid) the facts with their bounds or the typed refusal, `manifold`
//! (every shell closed), and as `reference` the bridge's `BRepGProp`
//! measurement of the solid (no `Eps`); per file the bridge's whole-shape
//! facts. Bodies are exported on the calling thread (an OCCT document is
//! thread-confined) and every face of every exported body integrates on one
//! pool of N threads; results are in body and face order, so the output is
//! identical for every N except the timings. `--eps-reference E` adds per
//! body `BRepGProp` at relative accuracy E (M1's `volumeRef` method) as
//! `epsReference`.

use std::time::Instant;

use geospec_facts::{bodies_facts, BodyFacts, Options, Refusal};
use geospec_facts_occt::{BodyExport, EpsReference, ExportStats, Subject};
use serde_json::{json, Value};

struct Args {
    threads: usize,
    accuracy: f64,
    faces: bool,
    eps_reference: Option<f64>,
    path: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        threads: 1,
        accuracy: 1e-7,
        faces: false,
        eps_reference: None,
        path: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--faces" => args.faces = true,
            "--reference" => {}
            "--threads" => {
                args.threads = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--threads needs a positive integer")?;
            }
            "--eps-reference" => {
                args.eps_reference = Some(
                    it.next()
                        .and_then(|s| s.parse().ok())
                        .filter(|a: &f64| *a > 0.0)
                        .ok_or("--eps-reference needs a positive number")?,
                );
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
        return Err(
            "usage: geospec-facts-occt [--threads N] [--accuracy A] [--faces] [--eps-reference E] <file.step>".into(),
        );
    }
    Ok(args)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn set(v: &mut Value, key: &str, x: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_string(), x);
    }
}

fn refusal_json(r: &Refusal) -> Value {
    json!({ "code": r.code.as_str(), "face": r.face, "detail": r.detail })
}

fn finite(x: f64) -> Value {
    if x.is_finite() {
        json!(x)
    } else {
        Value::Null
    }
}

fn facts_json(f: &BodyFacts, faces: bool) -> Value {
    let mut v = json!({
        "volume": f.volume.value,
        "volumeBound": f.volume.accuracy.bound,
        "area": f.area.value,
        "areaBound": f.area.accuracy.bound,
        "method": f.volume.accuracy.method.as_str(),
        "closure": f.closure,
        "closedWithinBounds": f.closed_within_bounds,
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

fn stats_json(s: &ExportStats) -> Value {
    json!({
        "surfaces": s.surfaces,
        "curves": s.curves,
        "reflectedFaces": s.reflected_faces,
        "movedOriginFaces": s.moved_origin_faces,
        "shiftedFaces": s.shifted_faces,
        "seams": s.seams,
        "degenerate": s.degenerate,
        "gapLines": s.gap_lines,
        "gapFaces": s.gap_faces,
        "pastTwoPi": s.past_two_pi,
        "reversedFaces": s.reversed_faces,
    })
}

fn run(args: &Args) -> Value {
    let total = Instant::now();
    let bytes = match std::fs::read(&args.path) {
        Ok(b) => b,
        Err(e) => return json!({ "open": { "error": format!("read: {e}") } }),
    };
    let t = Instant::now();
    let mut subject = match Subject::open_step(&bytes) {
        Ok(s) => s,
        Err(e) => return json!({ "open": { "error": e } }),
    };
    let open_ms = ms(t);
    let shape = subject.shape_facts();
    let t = Instant::now();
    let mut exports: Vec<Result<BodyExport, Refusal>> = Vec::new();
    let mut export_ms = Vec::new();
    for i in 0..subject.body_count() {
        let t = Instant::now();
        exports.push(subject.export_body(i));
        export_ms.push(ms(t));
    }
    let export_phase_ms = ms(t);
    let eps: Vec<Option<Result<EpsReference, String>>> = (0..exports.len())
        .map(|i| args.eps_reference.map(|e| subject.eps_reference(i, e)))
        .collect();
    let mut index = Vec::new();
    let mut definitions = Vec::new();
    for (i, e) in exports.iter_mut().enumerate() {
        if let Ok(e) = e {
            if let Ok(d) = &e.definition {
                index.push(i);
                definitions.push(d.clone());
            }
        }
    }
    let t = Instant::now();
    let options = Options {
        accuracy: args.accuracy,
        threads: args.threads,
    };
    let mut facts: Vec<Option<Result<BodyFacts, Refusal>>> = exports.iter().map(|_| None).collect();
    for (i, r) in index.iter().zip(bodies_facts(&definitions, options)) {
        if let Some(slot) = facts.get_mut(*i) {
            *slot = Some(r);
        }
    }
    let integrate_ms = ms(t);
    let bodies: Vec<Value> = exports
        .iter()
        .zip(&facts)
        .zip(&export_ms)
        .zip(&eps)
        .map(|(((e, f), export_ms), eps)| {
            let mut v = json!({ "exportMs": export_ms });
            match eps {
                Some(Ok(r)) => set(
                    &mut v,
                    "epsReference",
                    json!({
                        "eps": args.eps_reference,
                        "volume": finite(r.volume),
                        "area": finite(r.area),
                        "centroid": r.centre.map(finite),
                        "volumeError": finite(r.volume_error),
                        "areaError": finite(r.area_error),
                        "ms": r.ms,
                    }),
                ),
                Some(Err(m)) => set(&mut v, "epsReference", json!({ "error": m })),
                None => {}
            }
            match e {
                Err(r) => {
                    set(&mut v, "manifold", Value::Null);
                    set(&mut v, "refusal", refusal_json(r));
                    set(&mut v, "stage", json!("export"));
                }
                Ok(e) => {
                    let r = &e.record;
                    set(&mut v, "manifold", json!(r.closed));
                    set(&mut v, "shells", json!(r.shells));
                    set(&mut v, "placement", json!(r.placement));
                    set(&mut v, "maxTolerance", json!(r.max_tolerance));
                    set(
                        &mut v,
                        "reference",
                        json!({
                            "volume": finite(r.gprop.volume),
                            "area": finite(r.gprop.area),
                            "centroid": r.gprop.centre.map(finite),
                            "ms": r.gprop.ms,
                        }),
                    );
                    set(&mut v, "export", stats_json(&e.stats));
                    if let Err(r) = &e.definition {
                        set(&mut v, "refusal", refusal_json(r));
                        set(&mut v, "stage", json!("export"));
                    }
                }
            }
            match f {
                Some(Ok(f)) => set(&mut v, "facts", facts_json(f, args.faces)),
                Some(Err(r)) => {
                    set(&mut v, "refusal", refusal_json(r));
                    set(&mut v, "stage", json!("integrate"));
                }
                None => {}
            }
            v
        })
        .collect();
    let shape = match shape {
        Ok(s) => json!({
            "volume": finite(s.volume),
            "area": finite(s.area),
            "centroid": s.centre.map(finite),
            "ms": s.ms,
        }),
        Err(e) => json!({ "error": e }),
    };
    json!({
        "threads": args.threads,
        "accuracy": args.accuracy,
        "freeFaces": subject.free_faces(),
        "shape": shape,
        "bodies": bodies,
        "phases": {
            "openMs": open_ms,
            "exportPhaseMs": export_phase_ms,
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
