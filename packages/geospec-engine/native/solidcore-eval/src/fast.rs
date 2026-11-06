//! The levers-applied path: bodies build in parallel, longest first, each with
//! an inner thread budget for its per-face stages, and their closure is measured
//! ([`crate::closure::check`]); every face of every body then integrates once (area,
//! volume and first moments together, and the vector area the two-origin test reads)
//! on a shared pool, longest first; results reduce in body and face order, so they do
//! not depend on the thread count. Nothing is integrated during the build.

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
use step_import::model::{Body, Bound, FilePcurves, Surface};

use crate::gauss::{self, FaceTask};
use crate::{
    closure, marker_geometry, max_tolerance, ms, settle_facts, Args, BodyResult, PassStats,
};

/// One face's integration: its values and its vector area.
type FaceRun = (Result<[f64; gauss::N], String>, Result<[f64; 3], String>);

/// One face's outcome with its face index and volume signs.
type FaceOutcome = (usize, FaceRun, Vec<f64>);

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

/// A body's expected build cost, known before the build: one per face plus one
/// per edge use of its bounds, a B-spline face counting four times (they
/// dominate integration and trimming). Only the order of bodies and their inner
/// thread budgets depend on it, never a result.
fn body_cost(b: &Body) -> usize {
    b.faces
        .iter()
        .map(|f| {
            let uses: usize = f
                .loops
                .iter()
                .map(|l| match &l.bound {
                    Bound::Edges(e) => e.len(),
                    Bound::Vertex(_) => 1,
                })
                .sum();
            let weight = if matches!(f.surface, Surface::BSpline(_)) {
                4
            } else {
                1
            };
            weight * (1 + uses)
        })
        .sum()
}

/// The schedule's view of the pass: bodies running, the threads they hold, and
/// the cost of the bodies not yet started.
struct Load {
    threads: usize,
    workers: usize,
    running: usize,
    busy: usize,
    left: usize,
}

impl Load {
    /// The inner thread budget of a body of `cost` that starts with `waiting`
    /// bodies queued behind it, and books it. A body holding at least a
    /// `threads`-th of the cost still to start (it bounds the pass) gets every
    /// thread; any other body shares the threads not held by running bodies
    /// with the workers about to start a waiting body, taking the larger part
    /// (it is the longer one), and always at least one.
    fn start(&mut self, cost: usize, waiting: usize) -> usize {
        let inner = if cost.saturating_mul(self.threads) >= self.left {
            self.threads
        } else {
            let reserve = waiting.min(self.workers.saturating_sub(self.running + 1));
            let free = self.threads.saturating_sub(self.busy + reserve);
            let share = reserve + 1;
            (free / share + usize::from(!free.is_multiple_of(share))).max(1)
        };
        self.running += 1;
        self.busy += inner;
        self.left = self.left.saturating_sub(cost);
        inner
    }

    fn finish(&mut self, inner: usize) {
        self.running = self.running.saturating_sub(1);
        self.busy = self.busy.saturating_sub(inner);
    }
}

/// Builds every body on `threads` threads, longest first by `costs`, and returns
/// the results in body order. `job(k, i, inner)` builds body `i` with `inner`
/// threads for its per-face stages, fixed when it starts ([`Load::start`]), so
/// the threads that are idle when a body starts are lent to it and the body that
/// bounds the pass gets them all (the operating system shares the cores while
/// other bodies still run). Every result is the same whatever its budget (the
/// per-face stages are thread count invariant), so only timings depend on the
/// schedule.
fn schedule_bodies<T: Send, F>(costs: &[usize], threads: usize, job: F) -> Vec<Option<T>>
where
    F: Fn(&Kernel, usize, usize) -> T + Sync,
{
    let n = costs.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| costs.get(b).cmp(&costs.get(a)).then(a.cmp(&b)));
    let threads = threads.max(1);
    let workers = threads.min(n).max(1);
    let next = AtomicUsize::new(0);
    let load = Mutex::new(Load {
        threads,
        workers,
        running: 0,
        busy: 0,
        left: costs.iter().sum(),
    });
    let lock = || match load.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let work = || {
        let k = Kernel::default();
        loop {
            let pos = next.fetch_add(1, Ordering::Relaxed);
            let Some(&i) = order.get(pos) else { break };
            let cost = costs.get(i).copied().unwrap_or(0);
            let inner = lock().start(cost, n - pos - 1);
            let r = job(&k, i, inner);
            lock().finish(inner);
            if let Some(slot) = slots.get(i) {
                if let Ok(mut g) = slot.lock() {
                    *g = Some(r);
                }
            }
        }
    };
    if workers == 1 {
        work();
    } else {
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(work);
            }
        });
    }
    slots
        .into_iter()
        .map(|m| m.into_inner().ok().flatten())
        .collect()
}

fn build_one(
    k: &Kernel,
    body: &Body,
    index: usize,
    pcurves: &FilePcurves,
    args: &Args,
    inner_threads: usize,
) -> Built {
    let mut out = BodyResult::default();
    let t = Instant::now();
    let accuracy = args.accuracy;
    let built = build_body_with_file_pcurves(
        body,
        pcurves,
        BuildOptions {
            threads: inner_threads,
            ..BuildOptions::default()
        },
    );
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
    out.repaired = built.repaired;
    out.surface_model = body.kind == step_import::model::BodyKind::ShellModel;
    out.stated_solid = crate::is_stated_solid(body.kind);
    out.max_tolerance = max_tolerance(&built.brep);
    out.marker_geometry = marker_geometry(&built.brep);
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
    // Closure on the B-rep as built: edge uses and trims against the file's edges.
    if out.bounds.is_some() {
        out.closure = Some(closure::check(
            k,
            body,
            brep,
            size,
            out.stated_solid,
            &args.limits,
        ));
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
    let costs: Vec<usize> = bodies.iter().map(body_cost).collect();
    let built = schedule_bodies(&costs, args.threads, |k, i, inner| match bodies.get(i) {
        Some(b) => build_one(k, b, i, pcurves, args, inner),
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
            Some(task) => (gauss::run(k, &task), gauss::vector_area(k, &task)),
            None => (
                Err("missing task".to_string()),
                Err("missing task".to_string()),
            ),
        }
    });
    pass.integrate_phase_ms = ms(t);

    // Reduce per body in face order.
    let mut per_body: Vec<Vec<FaceOutcome>> = built.iter().map(|_| Vec::new()).collect();
    for ((body, face, signs), r) in keys.into_iter().zip(results) {
        if let Some(v) = per_body.get_mut(body) {
            v.push((
                face,
                r.unwrap_or_else(|| {
                    (
                        Err("worker thread panicked".into()),
                        Err("worker thread panicked".into()),
                    )
                }),
                signs,
            ));
        }
    }
    let k = Kernel::default();
    built
        .into_iter()
        .zip(per_body)
        .map(|(b, faces)| settle(&k, b, faces, args))
        .collect()
}

/// A built body's facts from the shared pass, or solidcore's integrator where the
/// shared pass failed for one of its faces, then [`settle_facts`]: whichever gave
/// them, they are refused alike. The faces' vector areas, summed with their volume
/// signs, feed the two-origin test.
fn settle(k: &Kernel, b: Built, mut faces: Vec<FaceOutcome>, args: &Args) -> BodyResult {
    let mut r = b.result;
    if !r.built {
        return r;
    }
    faces.sort_by_key(|f| f.0);
    r.vector_area = signed_vector_area(&faces);
    r.faces_inverted = faces
        .iter()
        .filter(|f| f.1 .0.as_ref().is_ok_and(|v| v[0] < 0.0))
        .count();
    r.faces_unintegrated = faces.iter().filter(|f| f.1 .0.is_err()).count();
    let failed = faces.iter().find_map(|f| f.1 .0.as_ref().err().cloned());
    if let Some(e) = failed.or_else(|| r.fallback.clone()) {
        r.fallback = Some(e);
        if let Some(mut brep) = b.brep {
            let t = Instant::now();
            fallback(k, &mut brep, &mut r, args.accuracy);
            r.facts_ms += ms(t);
        }
        settle_facts(&mut r, args);
        return r;
    }
    let (mut area, mut volume, mut m) = (0.0, 0.0, [0.0; 3]);
    for (_, (v, _), signs) in &faces {
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
    settle_facts(&mut r, args);
    r
}

/// The body's vector area: each face's, times its volume signs, summed in face order;
/// `None` when the body had no faces to integrate, the first face's error when one
/// failed.
fn signed_vector_area(faces: &[FaceOutcome]) -> Option<Result<[f64; 3], String>> {
    if faces.is_empty() {
        return None;
    }
    let mut total = [0.0; 3];
    for (_, (_, area), signs) in faces {
        let a = match area {
            Ok(a) => a,
            Err(e) => return Some(Err(e.clone())),
        };
        for s in signs {
            for (t, x) in total.iter_mut().zip(a) {
                *t += s * x;
            }
        }
    }
    Some(Ok(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A built 10 mm cube's result before its facts, its tolerance raised to 1e-3 mm,
    /// closed by edge uses and trims.
    fn cube() -> BodyResult {
        BodyResult {
            built: true,
            manifold: Some(true),
            stated_solid: true,
            solids: 1,
            bounds: Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
            max_tolerance: Some(1e-3),
            closure: Some(closure::Closure {
                edges: 12,
                trims: 24,
                size: 10.0,
                floor_mm: 1e-5,
                ..closure::Closure::default()
            }),
            ..BodyResult::default()
        }
    }

    fn built() -> Built {
        Built {
            result: cube(),
            brep: None,
            origin: Point3::new(5.0, 5.0, 5.0),
            tasks: Vec::new(),
        }
    }

    fn args(claim_tolerance: Option<f64>) -> Args {
        Args {
            open_only: false,
            reference: false,
            threads: 1,
            accuracy: 1e-7,
            claim_tolerance,
            lenient: false,
            lenient_closure: false,
            limits: crate::DEFAULT_LIMITS,
            path: String::new(),
        }
    }

    /// The cube as six faces of the shared pass: area and volume, no moment about
    /// its centre, opposite vector areas.
    fn faces() -> Vec<FaceOutcome> {
        (0..6)
            .map(|i| {
                let mut a = [0.0; 3];
                if let Some(x) = a.get_mut(i / 2) {
                    *x = if i % 2 == 0 { 100.0 } else { -100.0 };
                }
                (
                    i,
                    (Ok([100.0, 1000.0 / 6.0, 0.0, 0.0, 0.0]), Ok(a)),
                    vec![1.0],
                )
            })
            .collect()
    }

    /// A 10 mm cube as a faceted B-rep in millimetres and radians.
    const CUBE: &str = "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;\
        #1=FACETED_BREP('',#2);\
        #2=CLOSED_SHELL('',(#10,#11,#12,#13,#14,#15));\
        #10=FACE('',(#20));#20=FACE_OUTER_BOUND('',#30,.T.);#30=POLY_LOOP('',(#100,#103,#102,#101));\
        #11=FACE('',(#21));#21=FACE_OUTER_BOUND('',#31,.T.);#31=POLY_LOOP('',(#104,#105,#106,#107));\
        #12=FACE('',(#22));#22=FACE_OUTER_BOUND('',#32,.T.);#32=POLY_LOOP('',(#100,#101,#105,#104));\
        #13=FACE('',(#23));#23=FACE_OUTER_BOUND('',#33,.T.);#33=POLY_LOOP('',(#101,#102,#106,#105));\
        #14=FACE('',(#24));#24=FACE_OUTER_BOUND('',#34,.T.);#34=POLY_LOOP('',(#102,#103,#107,#106));\
        #15=FACE('',(#25));#25=FACE_OUTER_BOUND('',#35,.T.);#35=POLY_LOOP('',(#103,#100,#104,#107));\
        #100=CARTESIAN_POINT('',(0.,0.,0.));#101=CARTESIAN_POINT('',(10.,0.,0.));\
        #102=CARTESIAN_POINT('',(10.,10.,0.));#103=CARTESIAN_POINT('',(0.,10.,0.));\
        #104=CARTESIAN_POINT('',(0.,0.,10.));#105=CARTESIAN_POINT('',(10.,0.,10.));\
        #106=CARTESIAN_POINT('',(10.,10.,10.));#107=CARTESIAN_POINT('',(0.,10.,10.));\
        #200=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
        #201=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\
        #202=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#200,#201)) REPRESENTATION_CONTEXT('',''));\
        #203=FACETED_BREP_SHAPE_REPRESENTATION('',(#1),#202);\
        ENDSEC;END-ISO-10303-21;";

    fn model(text: &str) -> Result<step_import::model::Model, String> {
        let model = step_import::import(text.as_bytes(), &step_import::Limits::default())
            .map_err(|e| e.message)?;
        if !model.admission.is_empty() {
            return Err(format!("admission: {:?}", model.admission));
        }
        Ok(model)
    }

    /// The cube built for real and measured by the shared pass.
    fn run_cube(text: &str) -> Result<Vec<BodyResult>, String> {
        run_with(text, &args(None))
    }

    fn run_with(text: &str, a: &Args) -> Result<Vec<BodyResult>, String> {
        let model = model(text)?;
        let mut pass = PassStats::default();
        Ok(run_bodies(&model.bodies, &model.file_pcurves, a, &mut pass))
    }

    /// The cube built for real, its shared pass failed, so that [`settle`] runs
    /// solidcore's integrator ([`fallback`]) on its B-rep.
    fn built_cube(k: &Kernel) -> Result<Built, String> {
        let model = model(CUBE)?;
        let body = model.bodies.first().ok_or("no body")?;
        let mut b = build_one(k, body, 0, &model.file_pcurves, &args(None), 1);
        if !b.result.built || b.brep.is_none() || b.result.manifold != Some(true) {
            return Err(format!("not a closed build: {:?}", b.result.error));
        }
        b.result.fallback = None;
        Ok(b)
    }

    /// The faces of a built cube as the shared pass would give them: each face's
    /// integration failed, its vector area measured.
    fn failing(b: &mut Built) -> Vec<FaceOutcome> {
        let k = Kernel::default();
        std::mem::take(&mut b.tasks)
            .iter()
            .map(|t| {
                (
                    t.face,
                    (
                        Err("did not converge".to_string()),
                        gauss::vector_area(&k, t),
                    ),
                    t.volume_signs.clone(),
                )
            })
            .collect()
    }

    /// solidcore's integrator measures the cube where the shared pass failed for a
    /// face, and its facts are refused for a claim finer than the cube's tolerance.
    #[test]
    fn the_integrator_measures_and_refuses_like_the_others() -> Result<(), String> {
        let k = Kernel::default();
        let t = built_cube(&k)?.result.max_tolerance.ok_or("no tolerance")?;
        // the faces the shared pass failed have no orientation: the body refuses
        // `closure-unchecked` and keeps no facts
        let mut b = built_cube(&k)?;
        let f = failing(&mut b);
        let strict = settle(&k, b, f, &args(Some(t)));
        assert_eq!(strict.faces_unintegrated, 6);
        assert_eq!(
            (
                strict.closure_class,
                strict.closure_refusal.as_ref().map(|r| r.0),
                strict.volume
            ),
            (Some("closure-unchecked"), Some("closure-unchecked"), None)
        );
        // measured with the closure verdict reported only, solidcore's integrator
        // gives the facts
        let mut b = built_cube(&k)?;
        let f = failing(&mut b);
        let kept = settle(
            &k,
            b,
            f,
            &Args {
                lenient_closure: true,
                ..args(Some(t))
            },
        );
        assert_eq!((kept.refusal, kept.closure_refusal), (None, None));
        assert_eq!(kept.fallback.as_deref(), Some("did not converge"));
        assert!(
            kept.area.is_some_and(|a| (a - 600.0).abs() < 1e-6),
            "{:?}",
            kept.area
        );
        assert!(
            kept.volume.is_some_and(|v| (v - 1000.0).abs() < 1e-6),
            "{:?}",
            kept.volume
        );
        assert!(kept
            .centroid
            .is_some_and(|c| c.iter().all(|x| (x - 5.0).abs() < 1e-9)));
        let mut b = built_cube(&k)?;
        let f = failing(&mut b);
        let refused = settle(&k, b, f, &args(Some(t / 2.0)));
        assert!(
            refused
                .refusal
                .as_deref()
                .is_some_and(|e| e.starts_with("tolerance:")),
            "{:?}",
            refused.refusal
        );
        assert_eq!(
            (
                refused.volume,
                refused.area,
                refused.centroid,
                refused.bounds
            ),
            (None, None, None, None)
        );
        Ok(())
    }

    /// The shared pass keeps a body's facts within the claim and refuses them,
    /// keeping none, for a claim finer than its tolerance (solidcore's integrator:
    /// [`the_integrator_measures_and_refuses_like_the_others`]).
    #[test]
    fn the_shared_pass_refuses_a_finer_claim() {
        let k = Kernel::default();
        let kept = settle(&k, built(), faces(), &args(Some(1e-2)));
        assert_eq!((kept.refusal, kept.closure_refusal), (None, None));
        let refused = settle(&k, built(), faces(), &args(Some(1e-4)));
        assert!(
            refused
                .refusal
                .as_deref()
                .is_some_and(|e| e.starts_with("tolerance:")),
            "{:?}",
            refused.refusal
        );
        assert_eq!(
            (
                refused.volume,
                refused.area,
                refused.centroid,
                refused.bounds
            ),
            (None, None, None, None)
        );
        let kept = settle(&k, built(), faces(), &args(None));
        assert_eq!(kept.area, Some(600.0));
        assert!(kept.volume.is_some_and(|v| (v - 1000.0).abs() < 1e-9));
        assert_eq!(kept.centroid, Some([5.0, 5.0, 5.0]));
        assert_eq!(kept.closure_class, Some("closed"));
        assert_eq!(kept.origin_dependence, Some(0.0));
    }

    /// A face whose bound the file reverses runs its trims against its surface: the
    /// shared pass reports it inverted (its area negative before the volume signs).
    /// Here the cube's volume is then implausible, so the facts refusal comes first; the
    /// `face-inverted` class is for inverted faces whose volume still looks plausible
    /// (`closure::tests::each_refusal_class_has_its_cause`).
    #[test]
    fn a_reversed_bound_counts_an_inverted_face() -> Result<(), String> {
        assert_eq!(run_cube(CUBE)?.first().map(|r| r.faces_inverted), Some(0));
        let text = CUBE.replace(
            "#20=FACE_OUTER_BOUND('',#30,.T.);",
            "#20=FACE_OUTER_BOUND('',#30,.F.);",
        );
        assert_ne!(text, CUBE);
        let r = run_cube(&text)?;
        let r = r.first().ok_or("no body")?;
        assert_eq!(r.faces_inverted, 1);
        assert!(r.volume.is_none() && r.refusal.is_some(), "{:?}", r.refusal);
        Ok(())
    }

    /// A real cube closes: every edge used twice, every trim on its edge, a signed
    /// vector area of zero; its facts are kept.
    #[test]
    fn a_closed_cube_passes_the_closure_test() -> Result<(), String> {
        let r = run_cube(CUBE)?;
        let r = r.first().ok_or("no body")?;
        let c = r.closure.as_ref().ok_or("no closure")?;
        assert_eq!(
            (
                c.edge_uses_bad,
                c.faces_unchecked,
                c.trims,
                c.trims_bad,
                c.trims_made
            ),
            (0, 0, 24, 0, 0)
        );
        assert!(c.critical_mm < 1e-9, "{}", c.critical_mm);
        assert_eq!(
            (
                r.closure_class,
                r.closure_refusal.as_ref(),
                r.refusal.as_ref()
            ),
            (Some("closed"), None, None)
        );
        assert!(
            r.origin_dependence.is_some_and(|d| d < 1e-12),
            "{:?}",
            r.origin_dependence
        );
        assert!(r.volume.is_some_and(|v| (v - 1000.0).abs() < 1e-9));
        Ok(())
    }

    const PLATE_GOOD: &str = include_str!("../fixtures/plate-good.step");
    const PLATE_TOP: &str = include_str!("../fixtures/plate-top.step");
    const PLATE_BOTH: &str = include_str!("../fixtures/plate-both.step");
    const ADVCUBE: &str = include_str!("../fixtures/advcube.step");
    const ADVCUBE_FLIP: &str = include_str!("../fixtures/advcube-flip.step");
    const ADVCUBE_TILT: &str = include_str!("../fixtures/advcube-tilt.step");
    const PLATE_SMALL_GOOD: &str = include_str!("../fixtures/plate-small-good.step");
    const PLATE_SMALL_BOTH: &str = include_str!("../fixtures/plate-small-both.step");
    const COUPLING: &str = include_str!(
        "../../occt/rust/tests/fixtures/kernel-skill-acceptance/build123d-coupling.step"
    );

    /// The first body's closure class, volume and misoriented edge count.
    fn verdict_of(text: &str) -> Result<(Option<&'static str>, Option<f64>, usize), String> {
        let r = run_cube(text)?;
        let r = r.first().ok_or("no body")?;
        let c = r.closure.as_ref().ok_or("no closure")?;
        Ok((r.closure_class, r.volume, c.edges_misoriented))
    }

    /// The L-plate with a square through-hole closes at 252 mm³. With the hole's bound
    /// reversed on the top face, or on both faces (the attempt-1 review's repro, which
    /// the three other tests admitted at 268 mm³), every edge still has two uses, but
    /// the integrated boundaries run the hole's edges twice in one direction:
    /// `misoriented`.
    #[test]
    fn a_reversed_hole_bound_refuses_as_misoriented() -> Result<(), String> {
        let (class, volume, bad) = verdict_of(PLATE_GOOD)?;
        assert_eq!((class, bad), (Some("closed"), 0));
        assert!(
            volume.is_some_and(|v| (v - 252.0).abs() < 1e-9),
            "{volume:?}"
        );
        assert_eq!(verdict_of(PLATE_TOP)?, (Some("misoriented"), None, 4));
        assert_eq!(verdict_of(PLATE_BOTH)?, (Some("misoriented"), None, 8));
        Ok(())
    }

    /// The holed plate with a 0.3 mm hole, reversed on both faces (the attempt-2
    /// re-review's repro). At the default floor every edge is probed and it refuses
    /// `misoriented`. At a 1e-2 mm floor the hole's edges are shorter than the probe's
    /// reach, so its two loops are not probed; their area bound (length squared, by the
    /// half-diagonal, over the volume) exceeds the origin limit, so the plate refuses
    /// `closure-unchecked`, as does the correct plate: neither can be told at that floor.
    #[test]
    fn a_loop_too_short_to_probe_refuses_unless_negligible() -> Result<(), String> {
        let (class, volume, bad) = verdict_of(PLATE_SMALL_GOOD)?;
        assert_eq!((class, bad), (Some("closed"), 0));
        assert!(
            volume.is_some_and(|v| (v - 259.82).abs() < 1e-9),
            "{volume:?}"
        );
        assert_eq!(
            verdict_of(PLATE_SMALL_BOTH)?,
            (Some("misoriented"), None, 8)
        );
        let coarse = Args {
            limits: crate::closure::Limits {
                floor_mm: 1e-2,
                ..crate::DEFAULT_LIMITS
            },
            ..args(None)
        };
        for text in [PLATE_SMALL_GOOD, PLATE_SMALL_BOTH] {
            let r = run_with(text, &coarse)?;
            let r = r.first().ok_or("no body")?;
            let c = r.closure.as_ref().ok_or("no closure")?;
            assert_eq!(
                (r.closure_class, r.volume, c.loops_unprobed, c.edges_short),
                (Some("closure-unchecked"), None, 2, 8)
            );
        }
        Ok(())
    }

    /// The coupling's fillet tori list two of their edges in the same direction as
    /// the neighbouring faces do, but the boundary the shared pass integrates (the trims
    /// and the UV lines closing the gaps at the seam) runs each edge once each way, and
    /// the volume is right: the test reads the integrated boundary, not the uses' flags.
    #[test]
    fn a_seam_face_whose_integrated_boundary_closes_is_admitted() -> Result<(), String> {
        let (class, volume, bad) = verdict_of(COUPLING)?;
        assert_eq!((class, bad), (Some("closed"), 0));
        // every edge, the tori's included, was probed on both its faces
        let r = run_cube(COUPLING)?;
        let c = r
            .first()
            .and_then(|r| r.closure.as_ref())
            .ok_or("no closure")?;
        assert_eq!(
            (
                c.edges_probed,
                c.edges_short,
                c.edges_unsided,
                c.loops_unprobed
            ),
            (c.edges, 0, 0, 0)
        );
        assert!(
            volume.is_some_and(|v| (v - 28_665.272_165).abs() < 1e-5),
            "{volume:?}"
        );
        Ok(())
    }

    /// Each closure class through the whole pass (build, closure check, shared
    /// integration, settle): a cube of advanced faces closes; with one face's
    /// `same_sense` reversed its trims run against its surface (`face-inverted`, the
    /// volume still plausible); with its top plane tilted by 4e-6 every trim stays
    /// within the floor of its edge but the faces no longer meet, so the volume moves
    /// with the origin (`origin-dependent`). `not-closed`, `misoriented` and
    /// `closure-unchecked` are reached by the tests beside this one.
    #[test]
    fn each_closure_class_is_reached_end_to_end() -> Result<(), String> {
        let (class, volume, _) = verdict_of(ADVCUBE)?;
        assert_eq!(class, Some("closed"));
        assert!(
            volume.is_some_and(|v| (v - 1000.0).abs() < 1e-9),
            "{volume:?}"
        );
        let r = run_cube(ADVCUBE_FLIP)?;
        let r = r.first().ok_or("no body")?;
        assert_eq!(
            (r.closure_class, r.faces_inverted, r.volume),
            (Some("face-inverted"), 1, None)
        );
        assert!(r.refusal.is_none());
        let r = run_cube(ADVCUBE_TILT)?;
        let r = r.first().ok_or("no body")?;
        let c = r.closure.as_ref().ok_or("no closure")?;
        assert_eq!(
            (r.closure_class, c.trims_bad, r.volume),
            (Some("origin-dependent"), 0, None)
        );
        assert!(
            r.origin_dependence.is_some_and(|d| d > 1e-6),
            "{:?}",
            r.origin_dependence
        );
        Ok(())
    }

    /// A trim whose distance to its edge is not finite at one sample is not measured
    /// (the face refuses `closure-unchecked`); it never hides behind the other samples.
    #[test]
    fn a_distance_that_is_not_finite_leaves_the_trim_unmeasured() -> Result<(), String> {
        let k = Kernel::default();
        let model = model(CUBE)?;
        let body = model.bodies.first().ok_or("no body")?;
        let mut built =
            build_body_with_file_pcurves(body, &model.file_pcurves, BuildOptions::default())
                .map_err(|e| e.message)?;
        let (trim, surface) = closure::first_trim(&k, &mut built.brep).ok_or("no trim")?;
        assert_eq!(
            closure::trim_deviation(&k, &trim, &surface, &|_| 0.0),
            Some(0.0)
        );
        for bad in [f64::NAN, f64::INFINITY] {
            let calls = std::cell::Cell::new(0);
            let d = |_: crate::filegeom::V3| {
                calls.set(calls.get() + 1);
                if calls.get() == 10 {
                    bad
                } else {
                    0.0
                }
            };
            assert_eq!(closure::trim_deviation(&k, &trim, &surface, &d), None);
        }
        Ok(())
    }

    /// A stated solid whose file topology leaves edges with one use (the cube less its
    /// top face) is built open, never stitched or healed into a solid, and refuses
    /// as `not-closed` with no facts.
    #[test]
    fn an_unpaired_edge_refuses_as_not_closed() -> Result<(), String> {
        let open = CUBE
            .replace("(#10,#11,#12,#13,#14,#15)", "(#10,#12,#13,#14,#15)")
            .replace(
                "#11=FACE('',(#21));#21=FACE_OUTER_BOUND('',#31,.T.);#31=POLY_LOOP('',(#104,#105,#106,#107));",
                "",
            );
        let r = run_cube(&open)?;
        let r = r.first().ok_or("no body")?;
        assert!(r.path_topology);
        assert_eq!(
            r.closure
                .as_ref()
                .map(|c| (c.edge_uses_bad, c.trims_skipped)),
            Some((4, true))
        );
        assert_eq!(r.closure_refusal.as_ref().map(|c| c.0), Some("not-closed"));
        assert_eq!((r.volume, r.area, r.bounds), (None, None, None));
        Ok(())
    }

    /// A closed surface model stays a sheet: closed by the test, an area and a box,
    /// never a volume.
    #[test]
    fn a_shell_stays_a_shell() -> Result<(), String> {
        let shell = CUBE
            .replace(
                "#1=FACETED_BREP('',#2);",
                "#1=SHELL_BASED_SURFACE_MODEL('',(#2));",
            )
            .replace(
                "FACETED_BREP_SHAPE_REPRESENTATION",
                "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION",
            );
        let r = run_cube(&shell)?;
        let r = r.first().ok_or("no body")?;
        assert!(r.surface_model, "{:?}", r.error);
        assert_eq!(r.closure_class, Some("shell"), "{:?}", r.closure_refusal);
        assert_eq!((r.volume, r.solids), (None, 0));
        assert!(r.area.is_some_and(|a| (a - 600.0).abs() < 1e-9));
        Ok(())
    }

    /// A trim off its file edge (the file's edge ends 0.5 mm short of where the B-rep's
    /// edge runs, with no gap from its faces) refuses as `trim-inconsistent`.
    #[test]
    fn a_trim_off_its_edge_refuses_as_trim_inconsistent() -> Result<(), String> {
        let k = Kernel::default();
        let model = model(CUBE)?;
        let body = model.bodies.first().ok_or("no body")?;
        let mut built =
            build_body_with_file_pcurves(body, &model.file_pcurves, BuildOptions::default())
                .map_err(|e| e.message)?;
        let mut file = body.clone();
        let edge = file.edges.first_mut().ok_or("no edge")?;
        edge.t_end -= 0.5 * (edge.t_end - edge.t_start) / 10.0;
        let limits = crate::DEFAULT_LIMITS;
        let c = closure::check(&k, &file, &mut built.brep, 10.0, true, &limits);
        assert_eq!((c.edge_uses_bad, c.faces_unchecked, c.trims_bad), (0, 0, 2));
        assert!((c.critical_mm - 0.25).abs() < 1e-6, "{}", c.critical_mm);
        let v = closure::verdict(
            &closure::Body3 {
                closure: &c,
                manifold: true,
                stated_solid: true,
                vector_area: Some(&Ok([0.0; 3])),
                faces_inverted: 0,
                faces_unintegrated: 0,
                volume: Some(1000.0),
                bounds: Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
            },
            &limits,
        );
        assert!(
            matches!(v, closure::Verdict::Refused("trim-inconsistent", _)),
            "{v:?}"
        );
        Ok(())
    }

    /// The vector areas the two-origin test reads: each face of the cube 100 mm² along
    /// its normal, summing to zero with their volume signs; without one face the sum is
    /// that face's, and the volume would move by 87% of itself within the box.
    #[test]
    fn a_missing_face_moves_the_volume_with_the_origin() -> Result<(), String> {
        let k = Kernel::default();
        let model = model(CUBE)?;
        let body = model.bodies.first().ok_or("no body")?;
        let mut built =
            build_body_with_file_pcurves(body, &model.file_pcurves, BuildOptions::default())
                .map_err(|e| e.message)?;
        let tasks = gauss::prepare(
            &k,
            &mut built.brep,
            0,
            Point3::new(5.0, 5.0, 5.0),
            10.0,
            1e-7,
        )?;
        let areas: Vec<[f64; 3]> = tasks
            .iter()
            .map(|t| {
                let a = gauss::vector_area(&k, t)?;
                let s: f64 = t.volume_signs.iter().sum();
                Ok(a.map(|x| s * x))
            })
            .collect::<Result<_, String>>()?;
        for a in &areas {
            let n = crate::filegeom::norm(*a);
            assert!((n - 100.0).abs() < 1e-9, "{a:?}");
        }
        let sum = |xs: &[[f64; 3]]| {
            xs.iter()
                .fold([0.0; 3], |s, a| [s[0] + a[0], s[1] + a[1], s[2] + a[2]])
        };
        let b = [0.0, 0.0, 0.0, 10.0, 10.0, 10.0];
        assert!(closure::origin_dependence(sum(&areas), 1000.0, &b) < 1e-12);
        let leak = closure::origin_dependence(sum(areas.get(1..).unwrap_or_default()), 1000.0, &b);
        assert!((leak - 0.3 * 3f64.sqrt() / 0.6).abs() < 1e-9, "{leak}");
        Ok(())
    }
}
