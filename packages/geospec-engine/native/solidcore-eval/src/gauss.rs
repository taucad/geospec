//! One shared, fixed-rule mass-properties pass over solidcore's public API.
//!
//! solidcore integrates each face with a nested adaptive Cash-Karp ODE solve
//! (`mass::integrator::properties::boundary_properties`), once per query. This
//! module keeps solidcore's geometry and its integrand (`OdeIsoCurveLength`,
//! the same Green's-theorem reduction to a boundary integral of iso-u
//! integrals) and replaces only the quadrature:
//!
//! - one pass per face yields area, volume and the volume first moments, so
//!   volume, area and centre of mass share it (lever 1);
//! - each level is adaptive Gauss-Kronrod 7-15 on the same smooth pieces
//!   solidcore splits at (trim-curve knots, surface knot-line crossings and
//!   surface v knots), so a smooth piece usually converges in one panel
//!   (lever 2);
//! - faces are independent tasks, so the pass runs on any number of threads,
//!   one kernel per thread; results are reduced in face order, so they are
//!   identical for every thread count (lever 3).
//!
//! A face that does not converge is reported, never dropped: the caller falls
//! back to solidcore's own integrator for that body.

use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, EFF_ZERO};
use solidcore::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2::{
    surface_knots_inside, trim_curve_breaks, OdeIsoCurveLength, PrecisePropertyLayout, PPF_AREA,
    PPF_VOLUME, PPF_VOLUME_FIRST_MOMENTS,
};
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel};

/// Area, volume and the three volume first moments.
pub const N: usize = 5;
type V = [f64; N];

/// Gauss-Kronrod 7-15 abscissae (descending, last is the centre) and weights
/// (QUADPACK `qk15`).
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];
/// Gauss 7 weights for `XGK[1]`, `XGK[3]`, `XGK[5]` and the centre.
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

/// The integration work of one face, owned so it can move to any thread.
pub struct FaceTask {
    pub body: usize,
    pub face: usize,
    surface: Surface,
    trims: Vec<Curve>,
    base_v: f64,
    /// Width of the trims in u: converts the body's absolute error floor to
    /// one per unit u for the inner integrals.
    u_width: f64,
    origin: Point3,
    split_at_surface_knots: bool,
    /// Signs applied to this face's raw volume and moments, one per counted
    /// face use (`brep_properties` with the default behaviour).
    pub volume_signs: Vec<f64>,
    /// Relative accuracy.
    accuracy: f64,
    /// Body size, the length scale of the absolute floors.
    size: f64,
    /// A rough cost for longest-first scheduling.
    pub cost: usize,
}

/// One face's result: `[area, volume, Mx, My, Mz]` before signs.
pub type FaceResult = Result<V, String>;

fn region_void(b: &Brep, fu: solidcore::topology::FaceuseId) -> Result<bool, String> {
    let shell = b
        .faceuse(fu)
        .map_err(|e| e.to_string())?
        .shell
        .ok_or("faceuse without shell")?;
    let region = b
        .shell(shell)
        .map_err(|e| e.to_string())?
        .region
        .ok_or("shell without region")?;
    Ok(b.region(region).map_err(|e| e.to_string())?.is_void != 0)
}

fn region_of(
    b: &Brep,
    fu: solidcore::topology::FaceuseId,
) -> Result<solidcore::topology::RegionId, String> {
    let shell = b
        .faceuse(fu)
        .map_err(|e| e.to_string())?
        .shell
        .ok_or("faceuse without shell")?;
    b.shell(shell)
        .map_err(|e| e.to_string())?
        .region
        .ok_or_else(|| "shell without region".to_string())
}

/// Builds the oriented UV trims of every face (this needs `&mut Brep` and the
/// kernel, so it runs on the body's own thread) and returns one owned task per
/// face. `origin` is the body's bounding-box centre, as solidcore integrates
/// about; `size` its largest extent.
pub fn prepare(
    k: &Kernel,
    b: &mut Brep,
    body: usize,
    origin: Point3,
    size: f64,
    accuracy: f64,
) -> Result<Vec<FaceTask>, String> {
    let faces: Vec<FaceId> =
        solidcore::topology::brep::p1::faces(b, None).map_err(|e| e.to_string())?;
    let mut tasks = Vec::with_capacity(faces.len());
    for (index, f) in faces.into_iter().enumerate() {
        // Which face uses count towards volume, as `brep_properties` decides
        // for the default behaviour (density 1, no sheet thickness): a
        // separating face's use on a non-void side.
        let up = b
            .face(f)
            .map_err(|e| e.to_string())?
            .faceuse
            .ok_or("face without faceuse")?;
        let mate = b
            .faceuse(up)
            .map_err(|e| e.to_string())?
            .mate
            .ok_or("faceuse without mate")?;
        let separating = region_of(b, up)? != region_of(b, mate)?;
        let mut volume_signs = Vec::new();
        if separating {
            for fu in [up, mate] {
                if !region_void(b, fu)? {
                    let orientation = b.faceuse(fu).map_err(|e| e.to_string())?.orientation;
                    volume_signs.push(if orientation == OrientType::Same {
                        -1.0
                    } else {
                        1.0
                    });
                }
            }
        }
        let key_of = |e: EntityRef| solidcore::geom::object::ObjectRef {
            type_code: 0,
            key: solidcore::geom::attribute::ObjectKey(match e {
                EntityRef::Edgeuse(eu) => u64::from(eu.index()),
                _ => 0,
            }),
        };
        let trims = solidcore::topology::face::p1::create_oriented_trim_boundary(
            k, b, f, true, true, None, &key_of,
        )
        .map_err(|e| format!("trims: {e}"))?
        .curves;
        let face = b.face(f).map_err(|e| e.to_string())?;
        let surface = face.surface.clone().ok_or("face without surface")?;
        let domain_min_v = face.uv_domain.min().y;
        let (trims_v, u_width) = trims_extent(k, &trims)?;
        let base_v = if k.fixes.precise_properties_trim_base_v
            && !trims.is_empty()
            && trims_v >= domain_min_v
        {
            trims_v
        } else {
            domain_min_v
        };
        let cost = trims.len() * 16;
        tasks.push(FaceTask {
            body,
            face: index,
            surface,
            trims,
            base_v,
            u_width,
            origin,
            split_at_surface_knots: k.fixes.precise_properties_split_at_surface_knots,
            volume_signs,
            accuracy,
            size,
            cost,
        });
    }
    Ok(tasks)
}

/// The trims' lowest v and their width in u.
fn trims_extent(k: &Kernel, trims: &[Curve]) -> Result<(f64, f64), String> {
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let (mut v0, mut u0, mut u1) = (f64::MAX, f64::MAX, f64::MIN);
    for trim in trims {
        let bbox = trim
            .calculate_bounding_box(k, &trim.natural_interval(), req, true)
            .map_err(|e| e.to_string())?
            .normal
            .ok_or("trim without bounding box")?;
        v0 = v0.min(bbox.min().y);
        u0 = u0.min(bbox.min().x);
        u1 = u1.max(bbox.max().x);
    }
    Ok((v0, (u1 - u0).max(0.0)))
}

#[allow(dead_code)]
fn trims_base_v(k: &Kernel, trims: &[Curve], min_v: f64) -> Result<f64, String> {
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let mut base = f64::MAX;
    for trim in trims {
        let bbox = trim
            .calculate_bounding_box(k, &trim.natural_interval(), req, true)
            .map_err(|e| e.to_string())?
            .normal
            .ok_or("trim without bounding box")?;
        base = base.min(bbox.min().y);
    }
    Ok(if trims.is_empty() || base < min_v {
        min_v
    } else {
        base
    })
}

fn add(a: &mut V, b: &V) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y;
    }
}

/// Fifteen-point Kronrod and seven-point Gauss estimates on `[a, b]`.
fn gk15<F>(f: &mut F, a: f64, b: f64) -> Result<(V, V, f64), String>
where
    F: FnMut(f64) -> Result<V, String>,
{
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let mut k = [0.0; N];
    let mut g = [0.0; N];
    let mut area_abs = 0.0;
    for (j, (&x, &w)) in XGK.iter().zip(WGK.iter()).enumerate() {
        let points: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
        for &s in points {
            let y = f(c + s * h * x)?;
            area_abs += w * y.first().copied().unwrap_or(0.0).abs();
            for i in 0..N {
                let (ki, yi) = (k.get_mut(i), y.get(i));
                if let (Some(ki), Some(yi)) = (ki, yi) {
                    *ki += w * yi;
                }
            }
            if j % 2 == 1 {
                let wg = WG.get(j / 2).copied().unwrap_or(0.0);
                for (gi, yi) in g.iter_mut().zip(y.iter()) {
                    *gi += wg * yi;
                }
            }
        }
    }
    for v in k.iter_mut().chain(g.iter_mut()) {
        *v *= h;
    }
    Ok((k, g, area_abs * h.abs()))
}

/// Globally adaptive Gauss-Kronrod (QUADPACK `qag` style): keep a list of
/// panels, split the one with the largest scaled error until every
/// component's summed Kronrod-Gauss difference is within `tol`, or fail after
/// `max_panels`. Panels at round-off width are not split further. The result
/// sums panels in left-to-right order, so it is deterministic.
fn adaptive<F>(
    f: &mut F,
    a: f64,
    b: f64,
    first: (V, V, f64),
    tol: &V,
    max_panels: usize,
) -> Result<V, String>
where
    F: FnMut(f64) -> Result<V, String>,
{
    struct Panel {
        a: f64,
        b: f64,
        k: V,
        err: V,
    }
    let err_of = |k: &V, g: &V| {
        let mut e = [0.0; N];
        for ((ei, ki), gi) in e.iter_mut().zip(k.iter()).zip(g.iter()) {
            *ei = (ki - gi).abs();
        }
        e
    };
    let width = (b - a).abs();
    let mut panels = vec![Panel {
        a,
        b,
        k: first.0,
        err: err_of(&first.0, &first.1),
    }];
    loop {
        let mut total_err = [0.0; N];
        for p in &panels {
            add(&mut total_err, &p.err);
        }
        if total_err.iter().zip(tol.iter()).all(|(e, t)| e <= t) {
            break;
        }
        // The splittable panel with the largest scaled error.
        let scaled = |p: &Panel| {
            p.err
                .iter()
                .zip(tol.iter())
                .map(|(e, t)| e / t)
                .fold(0.0_f64, f64::max)
        };
        let worst = panels
            .iter()
            .enumerate()
            .filter(|(_, p)| (p.b - p.a).abs() > 1e-12 * width)
            .max_by(|x, y| scaled(x.1).total_cmp(&scaled(y.1)))
            .map(|(i, _)| i);
        let Some(i) = worst else {
            // Only round-off-width panels remain: what is left is round-off.
            break;
        };
        if panels.len() >= max_panels {
            return Err(format!(
                "no convergence in {max_panels} panels on [{a}, {b}]: error {total_err:?} against {tol:?}"
            ));
        }
        let p = panels.swap_remove(i);
        let m = 0.5 * (p.a + p.b);
        for (lo, hi) in [(p.a, m), (m, p.b)] {
            let (k, g, _) = gk15(f, lo, hi)?;
            panels.push(Panel {
                a: lo,
                b: hi,
                err: err_of(&k, &g),
                k,
            });
        }
    }
    panels.sort_by(|x, y| x.a.total_cmp(&y.a));
    let mut sum = [0.0; N];
    for p in &panels {
        add(&mut sum, &p.k);
    }
    Ok(sum)
}

/// Integrates one piece adaptively with tolerances relative to the piece's own
/// first estimate: area against itself, volume against area times size, first
/// moments against area times size squared.
fn integrate_piece<F>(
    f: &mut F,
    a: f64,
    b: f64,
    accuracy: f64,
    size: f64,
    floor: f64,
    max_panels: usize,
) -> Result<V, String>
where
    F: FnMut(f64) -> Result<V, String>,
{
    if a == b {
        return Ok([0.0; N]);
    }
    let first = gk15(f, a, b)?;
    let k = first.0;
    // The integral of |area integrand|: a magnitude that does not cancel when
    // a trim turns back in u.
    let area = first.2;
    let reference = [
        area,
        area * size,
        area * size * size,
        area * size * size,
        area * size * size,
    ];
    let mut tol = [0.0; N];
    // The absolute floor: `floor` times the body's size to each component's
    // power, so a piece whose integral is (near) zero converges at round-off.
    let floors = [
        floor,
        floor * size,
        floor * size * size,
        floor * size * size,
        floor * size * size,
    ];
    for (((t, r), v), fl) in tol
        .iter_mut()
        .zip(reference.iter())
        .zip(k.iter())
        .zip(floors.iter())
    {
        *t = (accuracy * r.max(v.abs())).max(*fl).max(f64::MIN_POSITIVE);
    }
    adaptive(f, a, b, first, &tol, max_panels)
}

/// Runs one face's integration on the calling thread's kernel.
pub fn run(k: &Kernel, t: &FaceTask) -> FaceResult {
    let layout = PrecisePropertyLayout::new(PPF_AREA | PPF_VOLUME | PPF_VOLUME_FIRST_MOMENTS);
    let slots = [
        layout.area,
        layout.volume,
        layout.moments[4],
        layout.moments[4] + 1,
        layout.moments[4] + 2,
    ];
    let mut scratch = vec![0.0; layout.num_components + 1];
    let mut total = [0.0; N];
    let inner_accuracy = t.accuracy / 10.0;
    // A face's error floor: 1e-3 of the accuracy on the body's squared size,
    // spread per unit u for the inner (per-du) integrals.
    let outer_floor = 1e-3 * t.accuracy * t.size * t.size;
    let inner_floor = outer_floor / t.u_width.max(f64::MIN_POSITIVE);
    for trim in &t.trims {
        let breaks = if t.split_at_surface_knots {
            trim_curve_breaks(k, trim, &t.surface).map_err(|e| e.to_string())?
        } else {
            trim.knots(k, Some(&trim.natural_interval()))
                .map_err(|e| e.to_string())?
                .0
        };
        for pair in breaks.windows(2) {
            let &[a, b] = pair else { continue };
            if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let mut outer = |s: f64| -> Result<V, String> {
                let pv = trim
                    .evaluate(k, s, 1, true, true)
                    .map_err(|e| e.to_string())?;
                let (p0, p1) = match (pv.first(), pv.get(1)) {
                    (Some(p0), Some(p1)) => (*p0, *p1),
                    _ => return Err("trim evaluation".into()),
                };
                if p1.x.abs() <= EFF_ZERO {
                    return Ok([0.0; N]);
                }
                let iso = OdeIsoCurveLength::new(k, &t.surface, p0.x, t.origin, 0.0, &layout);
                let mut inner = |v: f64| -> Result<V, String> {
                    iso.evaluate(v, &mut scratch).map_err(|e| e.to_string())?;
                    let mut out = [0.0; N];
                    for (o, &slot) in out.iter_mut().zip(slots.iter()) {
                        *o = scratch.get(slot).copied().unwrap_or(0.0);
                    }
                    Ok(out)
                };
                let mut ends =
                    surface_knots_inside(k, &t.surface, SurfParamType::V, t.base_v, p0.y)
                        .map_err(|e| e.to_string())?;
                ends.insert(0, t.base_v);
                ends.push(p0.y);
                let mut sum = [0.0; N];
                for w in ends.windows(2) {
                    let &[lo, hi] = w else { continue };
                    let piece = integrate_piece(
                        &mut inner,
                        lo,
                        hi,
                        inner_accuracy,
                        t.size,
                        inner_floor,
                        200,
                    )?;
                    add(&mut sum, &piece);
                }
                for v in &mut sum {
                    *v = -(*v * p1.x);
                }
                Ok(sum)
            };
            let piece = integrate_piece(&mut outer, a, b, t.accuracy, t.size, outer_floor, 400)?;
            add(&mut total, &piece);
        }
    }
    Ok(total)
}
