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
//!
//! `--box` adds per body the box with its bracket (P5, `box`: inner and outer
//! boxes, the outer box grown by the body's largest tolerance
//! (`outerTolerant`, the one that holds OCCT's own geometry), `bound`,
//! `epsilon`, the accuracy record, the faces bounded over their trims'
//! parameter box, those whose refinement reached its cap and those whose
//! trims leave their domain, or `boxRefusal`) and
//! the bridge's per-face `AddOptimal` fold of the solid (`addOptimal`), and
//! per file `freeCounts` and the phases' `boxMs` and `addOptimalMs`.
//! `--box-accuracy A` sets the bracket's target width relative to each
//! body's size (1e-7); `--no-facts` skips the integration. `--repeat N`
//! runs the box and the `AddOptimal` fold N times, alternating, and adds
//! each one's fastest run as `boxBestMs` and `addOptimalBestMs` (the first
//! run stays `boxMs` and `addOptimalMs`).

use std::time::Instant;

use geospec_facts::{
    bodies_boxes, bodies_facts, BodyBox, BodyFacts, BoxOptions, Options, Refusal, Vec3,
};
use geospec_facts_occt::{AddOptimalBox, BodyExport, EpsReference, ExportStats, Subject};
use serde_json::{json, Value};

struct Args {
    threads: usize,
    accuracy: f64,
    faces: bool,
    eps_reference: Option<f64>,
    bracket: bool,
    box_accuracy: f64,
    no_facts: bool,
    repeat: usize,
    path: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        threads: 1,
        accuracy: 1e-7,
        faces: false,
        eps_reference: None,
        bracket: false,
        box_accuracy: 1e-7,
        no_facts: false,
        repeat: 1,
        path: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--faces" => args.faces = true,
            "--box" => args.bracket = true,
            "--no-facts" => args.no_facts = true,
            "--box-accuracy" => {
                args.box_accuracy = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|a: &f64| *a > 0.0)
                    .ok_or("--box-accuracy needs a positive number")?;
            }
            "--reference" => {}
            "--repeat" => {
                args.repeat = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--repeat needs a positive integer")?;
            }
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
            "usage: geospec-facts-occt [--threads N] [--accuracy A] [--faces] [--eps-reference E] [--box [--box-accuracy A] [--repeat N]] [--no-facts] <file.step>".into(),
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

fn v3(v: Vec3) -> Value {
    json!([finite(v.x), finite(v.y), finite(v.z)])
}

fn box_json(b: &BodyBox, tolerance: Option<f64>) -> Value {
    let [ilo, ihi] = b.inner();
    let [olo, ohi] = b.outer();
    let tolerant = tolerance.map(|t| {
        let [lo, hi] = b.tolerant_outer(t);
        [v3(lo), v3(hi)]
    });
    json!({
        "inner": [v3(ilo), v3(ihi)],
        "outer": [v3(olo), v3(ohi)],
        "outerTolerant": tolerant,
        "accuracy": {
            "bound": b.accuracy.bound.map(finite),
            "method": b.accuracy.method.as_str(),
            "fileTolerance": b.accuracy.file_tolerance,
        },
        "bound": finite(b.bound),
        "epsilon": b.epsilon,
        "fallbackFaces": b.fallback_faces,
        "cappedFaces": b.capped_faces,
        "extrapolatedFaces": b.extrapolated_faces,
        "work": b.work,
    })
}

fn addoptimal_json(a: &AddOptimalBox) -> Value {
    json!({
        "bounds": a.bounds.map(|[lo, hi]| [lo.map(finite), hi.map(finite)]),
        "ms": a.ms,
        "voidFaces": a.void_faces,
    })
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
    if !args.no_facts {
        for (i, r) in index.iter().zip(bodies_facts(&definitions, options)) {
            if let Some(slot) = facts.get_mut(*i) {
                *slot = Some(r);
            }
        }
    }
    let integrate_ms = ms(t);
    let mut boxes: Vec<Option<Result<BodyBox, Refusal>>> = exports.iter().map(|_| None).collect();
    let mut addoptimal: Vec<Option<Result<AddOptimalBox, String>>> =
        exports.iter().map(|_| None).collect();
    let (mut box_ms, mut addoptimal_ms) = (0.0, 0.0);
    let (mut box_best, mut addoptimal_best) = (0.0, 0.0);
    if args.bracket {
        let t = Instant::now();
        let box_options = BoxOptions {
            accuracy: args.box_accuracy,
            threads: args.threads,
        };
        for (i, r) in index.iter().zip(bodies_boxes(&definitions, box_options)) {
            if let Some(slot) = boxes.get_mut(*i) {
                *slot = Some(r);
            }
        }
        box_ms = ms(t);
        for (i, slot) in addoptimal.iter_mut().enumerate() {
            let r = subject.addoptimal_box(i);
            if let Ok(a) = &r {
                addoptimal_ms += a.ms;
            }
            *slot = Some(r);
        }
        (box_best, addoptimal_best) = (box_ms, addoptimal_ms);
        for _ in 1..args.repeat {
            let t = Instant::now();
            std::hint::black_box(bodies_boxes(&definitions, box_options));
            box_best = box_best.min(ms(t));
            let mut a_ms = 0.0;
            for i in 0..addoptimal.len() {
                if let Ok(a) = subject.addoptimal_box(i) {
                    a_ms += a.ms;
                }
            }
            addoptimal_best = addoptimal_best.min(a_ms);
        }
    }
    let bodies: Vec<Value> = exports
        .iter()
        .zip(&facts)
        .zip(&export_ms)
        .zip(&eps)
        .zip(boxes.iter().zip(&addoptimal))
        .map(|((((e, f), export_ms), eps), (bx, ao))| {
            let mut v = json!({ "exportMs": export_ms });
            match bx {
                Some(Ok(b)) => set(
                    &mut v,
                    "box",
                    box_json(b, e.as_ref().ok().map(|e| e.record.max_tolerance)),
                ),
                Some(Err(r)) => set(&mut v, "boxRefusal", refusal_json(r)),
                None => {}
            }
            match ao {
                Some(Ok(a)) => set(&mut v, "addOptimal", addoptimal_json(a)),
                Some(Err(m)) => set(&mut v, "addOptimal", json!({ "error": m })),
                None => {}
            }
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
    let free = if args.bracket {
        match subject.free_counts() {
            Ok(c) => json!({ "faces": c.faces, "edges": c.edges, "vertices": c.vertices }),
            Err(e) => json!({ "error": e }),
        }
    } else {
        Value::Null
    };
    json!({
        "threads": args.threads,
        "accuracy": args.accuracy,
        "boxAccuracy": args.bracket.then_some(args.box_accuracy),
        "freeFaces": subject.free_faces(),
        "freeCounts": free,
        "shape": shape,
        "bodies": bodies,
        "phases": {
            "openMs": open_ms,
            "exportPhaseMs": export_phase_ms,
            "integrateMs": integrate_ms,
            "boxMs": box_ms,
            "addOptimalMs": addoptimal_ms,
            "boxBestMs": (args.repeat > 1).then_some(box_best),
            "addOptimalBestMs": (args.repeat > 1).then_some(addoptimal_best),
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
