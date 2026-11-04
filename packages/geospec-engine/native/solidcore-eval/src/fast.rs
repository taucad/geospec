//! The levers-applied path: bodies build in parallel; every face of every body
//! then integrates once (area, volume and first moments together) on a shared
//! pool, longest first; results reduce in body and face order, so they do not
//! depend on the thread count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use solidcore::mass::integrator::properties::{brep_area, brep_mass_properties_default};
use solidcore::query::api_queries::{
    brep_bounding_box, brep_is_manifold_solid, brep_material_census,
};
use solidcore::topology::integrity;
use solidcore::{Brep, Kernel, Point3};
use step_brep::{build_body_with_file_pcurves, BuildOptions, BuildPath};
use step_import::model::{Body, FilePcurves};

use crate::gauss::{self, FaceTask};
use crate::{ms, Args, BodyResult, PassStats};

/// One face's outcome with its face index and volume signs.
type FaceOutcome = (usize, Result<[f64; gauss::N], String>, Vec<f64>);

struct Built {
    result: BodyResult,
    brep: Option<Brep>,
    origin: Point3,
    tasks: Vec<FaceTask>,
}

/// Runs `job(i)` for `i in 0..n` on `threads` scoped workers, each with its own
/// kernel, taking indices dynamically; results come back in index order.
fn pool<T: Send, F>(n: usize, threads: usize, job: F) -> Vec<Option<T>>
where
    F: Fn(&Kernel, usize) -> T + Sync,
{
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let threads = threads.min(n).max(1);
    let work = || {
        let k = Kernel::default();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            let r = job(&k, i);
            if let Some(slot) = slots.get(i) {
                if let Ok(mut g) = slot.lock() {
                    *g = Some(r);
                }
            }
        }
    };
    if threads == 1 {
        work();
    } else {
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(work);
            }
        });
    }
    slots
        .into_iter()
        .map(|m| m.into_inner().ok().flatten())
        .collect()
}

fn build_one(k: &Kernel, body: &Body, index: usize, pcurves: &FilePcurves, accuracy: f64) -> Built {
    let mut out = BodyResult::default();
    let t = Instant::now();
    let built = build_body_with_file_pcurves(body, pcurves, BuildOptions::default());
    out.build_ms = ms(t);
    let mut built = match built {
        Ok(b) => b,
        Err(e) => {
            out.error = Some(e.message);
            return Built {
                result: out,
                brep: None,
                origin: Point3::new(0.0, 0.0, 0.0),
                tasks: Vec::new(),
            };
        }
    };
    out.built = true;
    out.path_topology = built.path == BuildPath::Topology;
    out.face_failures = built.face_failures.len();
    out.faces = built.brep.face_list().len();
    out.edges = built.brep.edge_list().len();
    out.vertices = built.brep.vertex_list().len();
    let brep = &mut built.brep;
    let facts = Instant::now();
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
    // Validity before trim construction, on the B-rep as built.
    let t = Instant::now();
    match integrity::check(brep) {
        Ok(()) => out.integrity = Some(true),
        Err(e) => {
            out.integrity = Some(false);
            out.integrity_error = Some(e.to_string());
        }
    }
    out.check_ms[5] = ms(t);
    let mut origin = Point3::new(0.0, 0.0, 0.0);
    let mut size = 1.0;
    match bounds {
        Ok(b) if b.is_bounded() => {
            let (m, s) = (b.mid(), b.size());
            origin = m;
            size = s.x.max(s.y).max(s.z).max(f64::MIN_POSITIVE);
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
        Err(e) => out.facts_error = Some(format!("bounds: {e}")),
    }
    let t = Instant::now();
    let tasks = match gauss::prepare(k, brep, index, origin, size, accuracy) {
        Ok(tasks) => tasks,
        Err(e) => {
            out.fallback = Some(format!("prepare: {e}"));
            Vec::new()
        }
    };
    out.prepare_ms = ms(t);
    out.facts_ms = ms(facts);
    Built {
        result: out,
        brep: Some(built.brep),
        origin,
        tasks,
    }
}

/// solidcore's own integrator for a body whose shared pass failed.
fn fallback(k: &Kernel, brep: &mut Brep, out: &mut BodyResult, accuracy: f64) {
    match brep_area(k, brep, accuracy) {
        Ok(a) => out.area = Some(a),
        Err(e) => out.facts_error = Some(format!("area: {e}")),
    }
    if out.manifold == Some(true) {
        match brep_mass_properties_default(k, brep, accuracy, 1.0, Point3::new(0.0, 0.0, 0.0)) {
            Ok(p) => {
                out.volume = Some(p.properties.volume);
                out.centroid = Some([p.centroid.x, p.centroid.y, p.centroid.z]);
            }
            Err(e) => out.facts_error = Some(format!("mass: {e}")),
        }
    }
}

pub fn run_bodies(
    bodies: &[Body],
    pcurves: &FilePcurves,
    args: &Args,
    pass: &mut PassStats,
) -> Vec<BodyResult> {
    let t = Instant::now();
    let built = pool(bodies.len(), args.threads, |k, i| match bodies.get(i) {
        Some(b) => build_one(k, b, i, pcurves, args.accuracy),
        None => Built {
            result: BodyResult::default(),
            brep: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            tasks: Vec::new(),
        },
    });
    pass.build_phase_ms = ms(t);
    let mut built: Vec<Built> = built
        .into_iter()
        .map(|b| {
            b.unwrap_or_else(|| Built {
                result: BodyResult {
                    error: Some("worker thread panicked".into()),
                    ..BodyResult::default()
                },
                brep: None,
                origin: Point3::new(0.0, 0.0, 0.0),
                tasks: Vec::new(),
            })
        })
        .collect();

    // Every face of every body, longest first.
    let mut tasks: Vec<FaceTask> = built
        .iter_mut()
        .flat_map(|b| std::mem::take(&mut b.tasks))
        .collect();
    tasks.sort_by(|a, b| {
        b.cost
            .cmp(&a.cost)
            .then(a.body.cmp(&b.body))
            .then(a.face.cmp(&b.face))
    });
    pass.tasks = tasks.len();
    let keys: Vec<(usize, usize, Vec<f64>)> = tasks
        .iter()
        .map(|t| (t.body, t.face, t.volume_signs.clone()))
        .collect();
    let cells: Vec<Mutex<Option<FaceTask>>> =
        tasks.into_iter().map(|t| Mutex::new(Some(t))).collect();
    let t = Instant::now();
    let results = pool(cells.len(), args.threads, |k, i| {
        let task = cells
            .get(i)
            .and_then(|c| c.lock().ok().and_then(|mut g| g.take()));
        match task {
            Some(task) => gauss::run(k, &task),
            None => Err("missing task".to_string()),
        }
    });
    pass.integrate_phase_ms = ms(t);

    // Reduce per body in face order.
    let mut per_body: Vec<Vec<FaceOutcome>> = built.iter().map(|_| Vec::new()).collect();
    for ((body, face, signs), r) in keys.into_iter().zip(results) {
        if let Some(v) = per_body.get_mut(body) {
            v.push((
                face,
                r.unwrap_or_else(|| Err("worker thread panicked".into())),
                signs,
            ));
        }
    }
    let k = Kernel::default();
    let mut out = Vec::with_capacity(built.len());
    for (b, mut faces) in built.into_iter().zip(per_body) {
        let mut r = b.result;
        if !r.built {
            out.push(r);
            continue;
        }
        faces.sort_by_key(|f| f.0);
        let failed = faces.iter().find_map(|f| f.1.as_ref().err().cloned());
        if let Some(e) = failed.or_else(|| r.fallback.clone()) {
            r.fallback = Some(e);
            if let Some(mut brep) = b.brep {
                let t = Instant::now();
                fallback(&k, &mut brep, &mut r, args.accuracy);
                r.facts_ms += ms(t);
            }
            out.push(r);
            continue;
        }
        let (mut area, mut volume, mut m) = (0.0, 0.0, [0.0; 3]);
        for (_, v, signs) in &faces {
            let Ok(v) = v else { continue };
            let [a, vol, mx, my, mz] = *v;
            area += a.abs();
            for s in signs {
                volume += s * vol;
                m[0] += s * mx;
                m[1] += s * my;
                m[2] += s * mz;
            }
        }
        r.area = Some(area);
        if r.manifold == Some(true) {
            r.volume = Some(volume);
            if volume != 0.0 {
                r.centroid = Some([
                    b.origin.x + m[0] / volume,
                    b.origin.y + m[1] / volume,
                    b.origin.z + m[2] / volume,
                ]);
            }
        }
        out.push(r);
    }
    out
}
