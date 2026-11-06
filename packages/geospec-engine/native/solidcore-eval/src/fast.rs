//! The levers-applied path: bodies build in parallel, longest first, each with
//! an inner thread budget for its per-face stages; every face of every body
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
use step_import::model::{Body, Bound, FilePcurves, Surface};

use crate::gauss::{self, FaceTask};
use crate::{marker_geometry, max_tolerance, ms, refuse_facts, Args, BodyResult, PassStats};

/// One face's outcome with its face index and volume signs.
type FaceOutcome = (usize, Result<[f64; gauss::N], String>, Vec<f64>);

struct Built {
    result: BodyResult,
    brep: Option<Brep>,
    origin: Point3,
    tasks: Vec<FaceTask>,
    /// Mass properties the build already integrated (to the same accuracy).
    mass: Option<gauss::BodyMass>,
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
    accuracy: f64,
    inner_threads: usize,
) -> Built {
    let mut out = BodyResult::default();
    let t = Instant::now();
    let built = build_body_with_file_pcurves(
        body,
        pcurves,
        BuildOptions {
            threads: inner_threads,
            mass_accuracy: accuracy,
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
                mass: None,
            };
        }
    };
    out.built = true;
    out.path_topology = built.path == BuildPath::Topology;
    out.repaired = built.repaired;
    out.surface_model = body.kind == step_import::model::BodyKind::ShellModel;
    out.max_tolerance = max_tolerance(&built.brep);
    out.marker_geometry = marker_geometry(&built.brep);
    out.face_failures = built.face_failures.len();
    out.faces = built.brep.face_list().len();
    out.edges = built.brep.edge_list().len();
    out.vertices = built.brep.vertex_list().len();
    // The build's closed-solid check integrated the kept B-rep to this accuracy, with
    // its tight box: reuse both rather than compute them again.
    let mass = built.mass.filter(|m| m.accuracy == accuracy);
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
    let bounds = match &mass {
        Some(m) => Ok(m.bounds),
        None => brep_bounding_box(k, brep, true).or_else(|_| brep_bounding_box(k, brep, false)),
    };
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
    let tasks = if mass.is_some() {
        Vec::new()
    } else {
        match gauss::prepare(k, brep, index, origin, size, accuracy) {
            Ok(tasks) => tasks,
            Err(e) => {
                out.fallback = Some(format!("prepare: {e}"));
                Vec::new()
            }
        }
    };
    out.prepare_ms = ms(t);
    out.facts_ms = ms(facts);
    Built {
        result: out,
        brep: Some(built.brep),
        origin,
        tasks,
        mass,
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
        Some(b) => build_one(k, b, i, pcurves, args.accuracy, inner),
        None => Built {
            result: BodyResult::default(),
            brep: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            tasks: Vec::new(),
            mass: None,
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
                mass: None,
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
    built
        .into_iter()
        .zip(per_body)
        .map(|(b, faces)| settle(&k, b, faces, args))
        .collect()
}

/// A built body's facts from the build's own integration, solidcore's integrator
/// (where the shared pass failed for one of its faces) or the shared pass, then
/// [`refuse_facts`]: whichever of the three gave them, they are refused alike.
fn settle(k: &Kernel, b: Built, mut faces: Vec<FaceOutcome>, args: &Args) -> BodyResult {
    let mut r = b.result;
    if !r.built {
        return r;
    }
    if let Some(mass) = b.mass {
        r.area = Some(mass.area);
        if r.manifold == Some(true) {
            r.volume = Some(mass.volume);
            r.centroid = mass.centroid().map(|c| [c.x, c.y, c.z]);
        }
        refuse_facts(&mut r, args.claim_tolerance);
        return r;
    }
    faces.sort_by_key(|f| f.0);
    let failed = faces.iter().find_map(|f| f.1.as_ref().err().cloned());
    if let Some(e) = failed.or_else(|| r.fallback.clone()) {
        r.fallback = Some(e);
        if let Some(mut brep) = b.brep {
            let t = Instant::now();
            fallback(k, &mut brep, &mut r, args.accuracy);
            r.facts_ms += ms(t);
        }
        refuse_facts(&mut r, args.claim_tolerance);
        return r;
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
    refuse_facts(&mut r, args.claim_tolerance);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::extent3d::Extent3d;

    /// A built 10 mm cube's result before its facts, its tolerance raised to 1e-3 mm.
    fn cube() -> BodyResult {
        BodyResult {
            built: true,
            manifold: Some(true),
            solids: 1,
            bounds: Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
            max_tolerance: Some(1e-3),
            ..BodyResult::default()
        }
    }

    fn built(mass: Option<gauss::BodyMass>) -> Built {
        Built {
            result: cube(),
            brep: None,
            origin: Point3::new(5.0, 5.0, 5.0),
            tasks: Vec::new(),
            mass,
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
            path: String::new(),
        }
    }

    /// The cube as six faces of the shared pass: area and volume, no moment about
    /// its centre.
    fn faces() -> Vec<FaceOutcome> {
        (0..6)
            .map(|i| (i, Ok([100.0, 1000.0 / 6.0, 0.0, 0.0, 0.0]), vec![1.0]))
            .collect()
    }

    fn mass() -> gauss::BodyMass {
        gauss::BodyMass {
            area: 600.0,
            volume: 1000.0,
            moments: [0.0; 3],
            origin: Point3::new(5.0, 5.0, 5.0),
            bounds: Extent3d::from_bounds(0.0, 0.0, 0.0, 10.0, 10.0, 10.0),
            accuracy: 1e-7,
        }
    }

    /// Each of the three ways to a body's facts keeps them within the claim and
    /// refuses them, keeping none, for a claim finer than the body's tolerance.
    #[test]
    fn every_way_to_the_facts_refuses_alike() {
        let k = Kernel::default();
        let failing = vec![(0, Err("did not converge".to_string()), vec![1.0])];
        let cases: [(&str, Option<gauss::BodyMass>, Vec<FaceOutcome>); 3] = [
            ("build's integration", Some(mass()), Vec::new()),
            ("solidcore's integrator", None, failing),
            ("shared pass", None, faces()),
        ];
        for (name, m, f) in cases {
            let kept = settle(&k, built(m), f.clone(), &args(Some(1e-2)));
            assert_eq!(kept.refusal, None, "{name}");
            let refused = settle(&k, built(m), f, &args(Some(1e-4)));
            assert!(
                refused
                    .refusal
                    .as_deref()
                    .is_some_and(|e| e.starts_with("tolerance:")),
                "{name}: {:?}",
                refused.refusal
            );
            assert_eq!(
                (
                    refused.volume,
                    refused.area,
                    refused.centroid,
                    refused.bounds
                ),
                (None, None, None, None),
                "{name}"
            );
        }
        let kept = settle(&k, built(Some(mass())), Vec::new(), &args(None));
        assert_eq!((kept.volume, kept.area), (Some(1000.0), Some(600.0)));
        let kept = settle(&k, built(None), faces(), &args(None));
        assert_eq!(kept.area, Some(600.0));
        assert!(kept.volume.is_some_and(|v| (v - 1000.0).abs() < 1e-9));
        assert_eq!(kept.centroid, Some([5.0, 5.0, 5.0]));
    }
}
