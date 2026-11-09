//! Adaptive Gauss-Kronrod 7-15 over a vector integrand, from the solidcore
//! evaluation's patch 0001 (`mass/gauss.rs`): QUADPACK `qk15`, global adaptive
//! bisection of the panel with the largest scaled error, panels summed left to
//! right. Added here: every node value may carry its own error (an inner
//! integral's), which the panel carries through its weights; the result keeps
//! each panel's error estimate (the larger of its Kronrod-Gauss difference and
//! its share of the difference between its parent's Kronrod value and the sum
//! of its own and its sibling's) and the integral of `|f|` (QUADPACK's
//! `resabs`), so the caller can bound it. Every piece that is not exact by
//! degree is bisected at least once, so the second estimate always exists.
//! Both are estimates, not guaranteed bounds. A piece over several spans is
//! integrated span by span, each against its own tolerance (review A4-R4),
//! so every panel lies within one span, since both estimates assume an
//! integrand smooth over the panel.

use crate::refusal::{Refusal, RefusalCode};

/// Components, each an integral over the face of a function times the area
/// element or the natural normal `n = S_u × S_v`, about the body centre:
/// [`AREA`] `|n|`, [`VOL`] `z n_z`, the first moments `x z n_z`, `y z n_z`,
/// `z^2 n_z / 2`, [`CLOSURE`] `n_z`, the other two volume forms [`VOL_X`]
/// `x n_x` and [`VOL_Y`] `y n_y`, the closure components [`NX`] `n_x` and
/// [`NY`] `n_y`, and the moments' closure terms [`XNZ`] `x n_z` and [`YNZ`]
/// `y n_z`. Over a closed body the closure terms are zero and the three volume
/// forms agree.
pub const N: usize = 12;
/// One value per component.
pub type V = [f64; N];
/// Nodes of one panel.
pub const NODES: usize = 15;
/// The area component.
pub const AREA: usize = 0;
/// The volume as the integral of `z n_z`.
pub const VOL: usize = 1;
/// The closure component `n_z`.
pub const CLOSURE: usize = 5;
/// The volume as the integral of `x n_x`.
pub const VOL_X: usize = 6;
/// The volume as the integral of `y n_y`.
pub const VOL_Y: usize = 7;
/// The closure component `n_x`.
pub const NX: usize = 8;
/// The closure component `n_y`.
pub const NY: usize = 9;
/// The x moment's closure term `x n_z`.
pub const XNZ: usize = 10;
/// The y moment's closure term `y n_z`.
pub const YNZ: usize = 11;
/// The components that drive the adaptivity: area, volume and the moments.
const DRIVING: usize = 5;

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

/// The integrand at a panel's nodes: `f(xs, ys, es)` sets `ys[i]` to the value
/// at `xs[i]` and `es[i]` to that value's own error bound (zero for an exact
/// value), or refuses.
pub trait Nodes:
    FnMut(&[f64; NODES], &mut [V; NODES], &mut [V; NODES]) -> Result<(), Refusal>
{
}
impl<F: FnMut(&[f64; NODES], &mut [V; NODES], &mut [V; NODES]) -> Result<(), Refusal>> Nodes for F {}

fn add(a: &mut V, b: &V) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y;
    }
}

/// One panel: Kronrod and Gauss estimates, the Kronrod integral of `|f|` per
/// component, and the node errors carried through the Kronrod weights.
struct Estimate {
    k: V,
    g: V,
    resabs: V,
    carried: V,
}

/// Fifteen-point Kronrod and seven-point Gauss estimates on `[a, b]`; the
/// nodes are evaluated as one batch, then summed in their order.
fn gk15<F: Nodes>(f: &mut F, a: f64, b: f64) -> Result<Estimate, Refusal> {
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let mut k = [0.0; N];
    let mut g = [0.0; N];
    let mut carried = [0.0; N];
    let mut resabs = [0.0; N];
    let mut xs = [0.0; NODES];
    let mut slots = xs.iter_mut();
    for &x in &XGK {
        let points: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
        for &s in points {
            if let Some(slot) = slots.next() {
                *slot = c + s * h * x;
            }
        }
    }
    let mut ys = [[0.0; N]; NODES];
    let mut es = [[0.0; N]; NODES];
    f(&xs, &mut ys, &mut es)?;
    let mut values = ys.iter().zip(es.iter());
    for (j, (&x, &w)) in XGK.iter().zip(WGK.iter()).enumerate() {
        let points: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
        for _ in points {
            let Some((y, e)) = values.next() else {
                return Err(Refusal::new(RefusalCode::NonFinite, "gk15 nodes"));
            };
            for (((ki, ci), ai), (yi, ei)) in k
                .iter_mut()
                .zip(carried.iter_mut())
                .zip(resabs.iter_mut())
                .zip(y.iter().zip(e))
            {
                *ki += w * yi;
                *ci += w * ei;
                *ai += w * yi.abs();
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
    for v in carried.iter_mut().chain(resabs.iter_mut()) {
        *v *= h.abs();
    }
    Ok(Estimate {
        k,
        g,
        resabs,
        carried,
    })
}

/// A scalar `f` on `[a, b]`: the Kronrod value, `|Kronrod - Gauss|` and the
/// Kronrod integral of `|f|`.
pub fn gk15_scalar(mut f: impl FnMut(f64) -> f64, a: f64, b: f64) -> [f64; 3] {
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let (mut k, mut g, mut abs) = (0.0, 0.0, 0.0);
    for (j, (&x, &w)) in XGK.iter().zip(WGK.iter()).enumerate() {
        let points: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
        for &s in points {
            let y = f(c + s * h * x);
            k += w * y;
            abs += w * y.abs();
            if j % 2 == 1 {
                g += WG.get(j / 2).copied().unwrap_or(0.0) * y;
            }
        }
    }
    [k * h, ((k - g) * h).abs(), abs * h.abs()]
}

/// An integrated piece: the value, the sum of its panels' error estimates,
/// the node errors carried through, the integral of `|f|`, and the panel
/// count.
#[derive(Clone, Copy, Debug, Default)]
pub struct Piece {
    pub value: V,
    pub estimate: V,
    pub carried: V,
    pub resabs: V,
    pub panels: usize,
}

fn err_of(k: &V, g: &V) -> V {
    let mut e = [0.0; N];
    for ((ei, ki), gi) in e.iter_mut().zip(k.iter()).zip(g.iter()) {
        *ei = (ki - gi).abs();
    }
    e
}

/// One panel of [`adaptive`]: its ends, its Kronrod value, error estimate,
/// carried node errors and `|f|` integral.
struct Panel {
    a: f64,
    b: f64,
    k: V,
    err: V,
    carried: V,
    resabs: V,
}

impl Panel {
    fn new(a: f64, b: f64, e: Estimate, err: V) -> Self {
        Self {
            a,
            b,
            k: e.k,
            err,
            carried: e.carried,
            resabs: e.resabs,
        }
    }
}

/// A panel waiting to be split, ordered by its scaled error, then by its
/// sequence number (the earlier first): a total order, so the order panels
/// are split in is the same on every run.
struct Queued {
    scaled: f64,
    seq: usize,
    panel: Panel,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for Queued {}
impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.scaled
            .total_cmp(&other.scaled)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

/// Globally adaptive Gauss-Kronrod (QUADPACK `qag` style) over initial
/// panels within one span: bisect the panel with the largest scaled error
/// until every driving component's summed estimate is within `tol`, or
/// refuse after `max_panels`. A bisected panel's children each carry the
/// larger of their own Kronrod-Gauss difference and half the difference
/// between the parent's Kronrod value and the sum of the two children's.
/// Unless `exact`, every initial panel is bisected once, so every accepted
/// panel has that second estimate from a parent within its span (review
/// A4-R3). A
/// panel whose midpoint is not representable between its ends (a width of
/// an ulp or two) is never queued: as a child it keeps its estimate, as an
/// initial panel it is kept with an estimate of at least its `|f|`
/// integral. Panels at round-off width (relative to `width`) are not
/// refined further. The result sums panels in parameter order.
fn adaptive<F: Nodes>(
    f: &mut F,
    initial: Vec<Panel>,
    width: f64,
    tol: &V,
    max_panels: usize,
    exact: bool,
) -> Result<Piece, Refusal> {
    let (lo_end, hi_end) = initial
        .first()
        .zip(initial.last())
        .map_or((0.0, 0.0), |(p, q)| (p.a, q.b));
    let scaled = |p: &Panel| {
        p.err
            .iter()
            .zip(tol.iter())
            .take(DRIVING)
            .map(|(e, t)| e / t)
            .fold(0.0_f64, f64::max)
    };
    let splittable = |p: &Panel| (p.b - p.a).abs() > 1e-12 * width;
    // Whether the midpoint lies strictly between the ends.
    let bisectable = |p: &Panel| {
        let m = 0.5 * (p.a + p.b);
        p.a.min(p.b) < m && m < p.a.max(p.b)
    };
    // An initial panel that cannot be bisected (a width of an ulp or two,
    // far from zero) has no second estimate: it is kept with an estimate of
    // at least its `|f|` integral. Queued, it would be split at an end
    // without end. That is an estimate, not a strict bound: where `f`
    // changes sign within the panel its error can exceed the Kronrod `|f|`
    // integral, by about the panel's width times `|f|` (an ulp times `|f|`;
    // review A4-R4). Only an inner span next to `base_v` or the node's `v`
    // can be this narrow; outer spans narrower than round-off are merged.
    let unsplit = |mut p: Panel| {
        for (e, r) in p.err.iter_mut().zip(&p.resabs) {
            *e = e.max(*r);
        }
        p
    };
    let within = |total: &V| {
        total
            .iter()
            .zip(tol.iter())
            .take(DRIVING)
            .all(|(e, t)| e <= t)
    };
    let not_finite = |total: &V| {
        Refusal::new(
            RefusalCode::NonFinite,
            format!("integrand not finite on [{lo_end:?}, {hi_end:?}]: {total:?}"),
        )
    };
    // Whether a panel to be refined was kept for want of a midpoint (where
    // main split it at its end into a copy of itself until the panel cap):
    // a child, or an initial panel whose retained estimate (at least its
    // `|f|` integral) exceeds the tolerance. If the estimate is not within
    // the tolerance once the queue is empty, the piece refuses: no span is
    // accepted carrying more than its tolerance.
    let mut stuck = false;
    let mut count = initial.len();
    let mut seq = 0usize;
    let mut fixed: Vec<Panel> = Vec::new();
    let mut queue = std::collections::BinaryHeap::new();
    let mut forced: Vec<Panel> = Vec::new();
    for p in initial {
        if !exact && bisectable(&p) {
            forced.push(p);
        } else if !exact || (splittable(&p) && !bisectable(&p)) {
            let p = unsplit(p);
            stuck |= splittable(&p) && !within(&p.err);
            fixed.push(p);
        } else if splittable(&p) {
            queue.push(Queued {
                scaled: scaled(&p),
                seq,
                panel: p,
            });
            seq += 1;
        } else {
            fixed.push(p);
        }
    }
    let mut total = [0.0; N];
    for p in queue.iter().map(|q| &q.panel).chain(&fixed).chain(&forced) {
        add(&mut total, &p.err);
    }
    // The forced bisections first, in order, then the worst panel each time.
    forced.reverse();
    loop {
        if !total.iter().all(|e| e.is_finite()) {
            return Err(not_finite(&total));
        }
        let next = match forced.pop() {
            Some(p) => p,
            None => {
                if within(&total) {
                    // Confirm with a fresh sum (the running one is updated
                    // by differences).
                    let mut fresh = [0.0; N];
                    for p in queue.iter().map(|q| &q.panel).chain(&fixed) {
                        add(&mut fresh, &p.err);
                    }
                    total = fresh;
                    if !total.iter().all(|e| e.is_finite()) {
                        return Err(not_finite(&total));
                    }
                    if within(&total) {
                        break;
                    }
                }
                let Some(q) = queue.pop() else {
                    if stuck && !within(&total) {
                        return Err(Refusal::new(
                            RefusalCode::NoConvergence,
                            format!(
                                "no convergence on [{lo_end:?}, {hi_end:?}]: a panel without a midpoint leaves error {total:?} against {tol:?}"
                            ),
                        ));
                    }
                    // Only round-off-width panels remain; their differences
                    // stay in the estimate.
                    break;
                };
                q.panel
            }
        };
        if count >= max_panels {
            return Err(Refusal::new(
                RefusalCode::NoConvergence,
                format!(
                    "no convergence in {max_panels} panels on [{lo_end:?}, {hi_end:?}]: error {total:?} against {tol:?}"
                ),
            ));
        }
        let p = next;
        let m = 0.5 * (p.a + p.b);
        let lo = gk15(f, p.a, m)?;
        let hi = gk15(f, m, p.b)?;
        let mut split = [0.0; N];
        for (((s, pk), l), h) in split.iter_mut().zip(&p.k).zip(&lo.k).zip(&hi.k) {
            *s = 0.5 * (pk - (l + h)).abs();
        }
        for (t, e) in total.iter_mut().zip(&p.err) {
            *t -= e;
        }
        count += 1;
        for (x0, x1, e) in [(p.a, m, lo), (m, p.b, hi)] {
            let mut err = err_of(&e.k, &e.g);
            for (ei, si) in err.iter_mut().zip(&split) {
                *ei = ei.max(*si);
            }
            // A child that cannot be bisected keeps its estimate from this
            // bisection and is not queued.
            let child = Panel::new(x0, x1, e, err);
            add(&mut total, &child.err);
            stuck |= splittable(&child) && !bisectable(&child);
            if splittable(&child) && bisectable(&child) {
                queue.push(Queued {
                    scaled: scaled(&child),
                    seq,
                    panel: child,
                });
                seq += 1;
            } else {
                fixed.push(child);
            }
        }
    }
    let mut panels: Vec<Panel> = queue.into_iter().map(|q| q.panel).collect();
    panels.append(&mut fixed);
    panels.sort_by(|x, y| x.a.min(x.b).total_cmp(&y.a.min(y.b)));
    let mut piece = Piece {
        panels: panels.len(),
        ..Piece::default()
    };
    for p in &panels {
        add(&mut piece.value, &p.k);
        add(&mut piece.estimate, &p.err);
        add(&mut piece.carried, &p.carried);
        add(&mut piece.resabs, &p.resabs);
    }
    Ok(piece)
}

/// Integrates over the spans between `breaks` (monotone, ends included),
/// each span on its own as [`integrate_piece`] (main's per-span rule), and
/// sums them in order. `max_panels` is per span. With `exact` (a piecewise
/// polynomial within the rule's degree on every span), the spans are not
/// bisected.
///
/// # Errors
///
/// The first span's refusal, in order: [`RefusalCode::NoConvergence`] past
/// the panel cap, a non-finite integrand, or the integrand's own refusal.
#[allow(clippy::too_many_arguments)]
pub fn integrate_breaks<F: Nodes>(
    f: &mut F,
    breaks: &[f64],
    accuracy: f64,
    size: f64,
    floor: f64,
    max_panels: usize,
    exact: bool,
) -> Result<Piece, Refusal> {
    let mut sum = Piece::default();
    for pair in breaks.windows(2) {
        let &[a, b] = pair else { continue };
        let p = integrate_piece(f, a, b, accuracy, size, floor, max_panels, exact)?;
        add(&mut sum.value, &p.value);
        add(&mut sum.estimate, &p.estimate);
        add(&mut sum.carried, &p.carried);
        add(&mut sum.resabs, &p.resabs);
        sum.panels += p.panels;
    }
    Ok(sum)
}

/// Integrates over `[a, b]` adaptively, with tolerances relative to its
/// first estimate (area against itself, volume against area times size,
/// moments against area times size squared) and an absolute floor scaled
/// the same way. Only those components drive the adaptivity; the others
/// keep their estimates.
#[allow(clippy::too_many_arguments)]
fn integrate_piece<F: Nodes>(
    f: &mut F,
    a: f64,
    b: f64,
    accuracy: f64,
    size: f64,
    floor: f64,
    max_panels: usize,
    exact: bool,
) -> Result<Piece, Refusal> {
    if a == b {
        return Ok(Piece::default());
    }
    let first = gk15(f, a, b)?;
    let area = first.resabs[AREA];
    let s2 = size * size;
    let reference = [area, area * size, area * s2, area * s2, area * s2];
    let floors = [floor, floor * size, floor * s2, floor * s2, floor * s2];
    let mut tol = [f64::INFINITY; N];
    for (((t, r), v), fl) in tol
        .iter_mut()
        .zip(reference.iter())
        .zip(first.k.iter())
        .zip(floors.iter())
    {
        *t = (accuracy * r.max(v.abs())).max(*fl).max(f64::MIN_POSITIVE);
    }
    let err = err_of(&first.k, &first.g);
    let initial = vec![Panel::new(a, b, first, err)];
    adaptive(f, initial, (b - a).abs(), &tol, max_panels, exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn each(mut g: impl FnMut(f64) -> V) -> impl Nodes {
        move |xs: &[f64; NODES], ys: &mut [V; NODES], _: &mut [V; NODES]| {
            for (x, y) in xs.iter().zip(ys.iter_mut()) {
                *y = g(*x);
            }
            Ok(())
        }
    }

    #[test]
    fn polynomials_are_exact_and_smooth_functions_converge() {
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[..4].copy_from_slice(&[x * x * x * x * x, 1.0, libm::exp(x), libm::sin(x)]);
            y
        });
        let p = integrate_piece(&mut f, 0.0, 2.0, 1e-12, 1.0, 1e-15, 200, false).unwrap();
        assert!((p.value[0] - 64.0 / 6.0).abs() < 1e-13);
        assert!((p.value[1] - 2.0).abs() < 1e-15);
        assert!((p.value[2] - (libm::exp(2.0) - 1.0)).abs() < 1e-13);
        assert!((p.value[3] - (1.0 - libm::cos(2.0))).abs() < 1e-14);
        for (e, v) in
            p.estimate
                .iter()
                .zip([64.0 / 6.0, 2.0, libm::exp(2.0) - 1.0, 1.0 - libm::cos(2.0)])
        {
            assert!(*e <= 1e-10 * v.abs().max(1.0));
        }
    }

    /// Review F5: a constant has `K == G` on the first panel; it is still
    /// bisected unless declared exact.
    #[test]
    fn a_first_panel_with_k_equal_to_g_is_still_bisected() {
        let mut f = each(|_| [1.0; N]);
        let p = integrate_piece(&mut f, 0.0, 1.0, 1e-7, 1.0, 1e-15, 200, false).unwrap();
        assert!(p.panels >= 2, "{}", p.panels);
        let p = integrate_piece(&mut f, 0.0, 1.0, 1e-7, 1.0, 1e-15, 200, true).unwrap();
        assert_eq!(p.panels, 1);
    }

    #[test]
    fn a_kink_refuses_without_convergence_at_a_tight_cap() {
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = (x - 0.3).abs().sqrt();
            y
        });
        let r = integrate_piece(&mut f, 0.0, 1.0, 1e-14, 1.0, 1e-18, 8, false);
        assert_eq!(r.unwrap_err().code, RefusalCode::NoConvergence);
    }

    /// Many spans over a smooth integrand: every span is bisected at least
    /// once, and the estimate covers the error.
    #[test]
    fn many_spans_over_a_smooth_integrand_are_each_bisected() {
        let at: Vec<f64> = (0..=400).map(|i| f64::from(i) / 40.0).collect();
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = 2.0 + libm::sin(x);
            y[1] = libm::exp(-x);
            y
        });
        let p = integrate_breaks(&mut f, &at, 1e-9, 1.0, 1e-18, 200, false).unwrap();
        assert!(p.panels >= 2 * 400, "{}", p.panels);
        let truth = [20.0 + 1.0 - libm::cos(10.0), 1.0 - libm::exp(-10.0)];
        for (k, t) in truth.iter().enumerate() {
            assert!(
                (p.value[k] - t).abs() <= p.estimate[k] + 1e-14,
                "{k}: {} vs {t} within {}",
                p.value[k],
                p.estimate[k]
            );
        }
    }

    /// Uniform cubic B-spline basis on `[0, 4]`.
    fn bump(s: f64) -> f64 {
        if !(0.0..4.0).contains(&s) {
            0.0
        } else if s < 1.0 {
            s * s * s / 6.0
        } else if s < 2.0 {
            (((-3.0 * s + 12.0) * s - 12.0) * s + 4.0) / 6.0
        } else if s < 3.0 {
            (((3.0 * s - 24.0) * s + 60.0) * s - 44.0) / 6.0
        } else {
            (4.0 - s) * (4.0 - s) * (4.0 - s) / 6.0
        }
    }

    /// Review A4-R1 P1: a constant plus one C2 cubic B-spline bump four
    /// spans wide on `n` uniform breaks, at every position: the estimate
    /// covers the error at every position (panels grouped over several
    /// spans returned exactly 1 with an estimate of 0).
    #[test]
    fn a_bump_between_the_nodes_of_a_soft_spanning_panel_is_not_missed() {
        for n in [128u32, 256, 512] {
            let d = 1.0 / f64::from(n);
            let breaks: Vec<f64> = (0..=n).map(|i| f64::from(i) * d).collect();
            // Every position on the coarsest grid, every eighth on the
            // others (at the grouped build, 26 of 124, 18 of 32 and 52 of 64
            // of these positions were outside their estimates).
            let step = if n == 128 { 1 } else { 8 };
            for j in (0..n - 4).step_by(step) {
                let c = f64::from(j) * d;
                let mut f = each(|x| {
                    let mut y = [0.0; N];
                    y[0] = 1.0 + bump((x - c) / d);
                    y
                });
                let p = integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false).unwrap();
                let err = (p.value[0] - (1.0 + d)).abs();
                assert!(
                    err <= p.estimate[0] + 1e-15,
                    "n {n}, bump at {j}: error {err:e} > estimate {:e} ({} panels)",
                    p.estimate[0],
                    p.panels
                );
            }
        }
    }

    /// A C1 integrand (second derivative jumping at every soft break) is
    /// split at its breaks where its error estimate requires, and the
    /// estimate covers the error; the result is the same on every run.
    #[test]
    fn soft_breaks_with_jumps_in_the_second_derivative_are_split_as_needed() {
        let knots: Vec<f64> = (1..20).map(|i| f64::from(i) / 20.0).collect();
        let mut at = vec![0.0];
        at.extend(&knots);
        at.push(1.0);
        let g = |x: f64| -> f64 {
            knots
                .iter()
                .map(|&k| if x > k { (x - k) * (x - k) } else { 0.0 })
                .sum()
        };
        let truth: f64 = knots
            .iter()
            .map(|&k| (1.0 - k) * (1.0 - k) * (1.0 - k) / 3.0)
            .sum();
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = 1.0 + g(x);
            y
        });
        let p = integrate_breaks(&mut f, &at, 1e-12, 1.0, 1e-18, 200, false).unwrap();
        let err = (p.value[0] - (1.0 + truth)).abs();
        assert!(err <= p.estimate[0] + 1e-15, "{err} > {}", p.estimate[0]);
        assert!(p.estimate[0] <= 1e-11 * (1.0 + truth), "{}", p.estimate[0]);
        let q = integrate_breaks(&mut f, &at, 1e-12, 1.0, 1e-18, 200, false).unwrap();
        assert_eq!(p.value[0].to_bits(), q.value[0].to_bits());
        assert_eq!(p.estimate[0].to_bits(), q.estimate[0].to_bits());
    }

    /// A C2 integrand (third derivative jumping at every soft break, as on a
    /// cubic B-spline trim) over irregular knots: at every tolerance the
    /// estimate covers the error.
    #[test]
    fn soft_breaks_with_jumps_in_the_third_derivative_keep_the_estimate_a_bound() {
        let knots: Vec<f64> = (1..24)
            .map(|i| {
                let x = f64::from(i) / 24.0;
                x + 0.3 * (x * (1.0 - x)) * libm::sin(7.0 * x)
            })
            .collect();
        let mut at = vec![0.0];
        at.extend(&knots);
        at.push(1.0);
        let jump = |i: usize| {
            if i % 3 == 0 {
                -5.0
            } else {
                3.0 + i as f64 / 7.0
            }
        };
        let g = |x: f64| -> f64 {
            knots
                .iter()
                .enumerate()
                .map(|(i, &k)| {
                    if x > k {
                        jump(i) * (x - k) * (x - k) * (x - k)
                    } else {
                        0.0
                    }
                })
                .sum::<f64>()
                + libm::sin(3.0 * x)
        };
        let truth: f64 = knots
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                let d = 1.0 - k;
                jump(i) * d * d * d * d / 4.0
            })
            .sum::<f64>()
            + (1.0 - libm::cos(3.0)) / 3.0;
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = g(x);
            y
        });
        for accuracy in [1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10, 1e-11] {
            let p = integrate_breaks(&mut f, &at, accuracy, 1.0, 1e-18, 200, false).unwrap();
            let err = (p.value[0] - truth).abs();
            assert!(
                err <= p.estimate[0] + 1e-14,
                "accuracy {accuracy}: {err} > {}",
                p.estimate[0]
            );
        }
    }

    /// An initial panel narrower than round-off relative to the whole range
    /// is still bisected once (it had only its first estimate), whenever
    /// its midpoint lies strictly between its ends.
    #[test]
    fn a_narrow_initial_panel_is_still_bisected() {
        let mut f = each(|_| [1.0; N]);
        let breaks = [0.0, 1e-13, 1.0];
        let p = integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false).unwrap();
        assert_eq!(p.panels, 4);
    }

    /// CodeRabbit on f16acd3 and bc751ba: a span two ulps wide far from
    /// zero bisects into children an ulp wide, wider than round-off but
    /// without a representable midpoint. Queued, such a child was split at
    /// an end into a copy of itself until the panel cap. It is kept with its
    /// estimate instead: within the tolerance the span passes, as on main;
    /// beyond it the span refuses NoConvergence at once, as main did at its
    /// cap, never passing with an estimate over the tolerance.
    #[test]
    fn a_child_without_a_midpoint_is_kept_not_split_at_its_end() {
        let a = 1.0e6;
        let up = |x: f64| f64::from_bits(x.to_bits() + 1);
        let (m, e) = (up(a), up(up(a)));
        // A step of 1 inside the span: the children's nodes straddle it.
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = if x < m { 1.0 } else { 2.0 };
            y
        });
        let p = integrate_breaks(&mut f, &[a, e], 1e-10, 1.0, 1e-9, 200, false).unwrap();
        let truth = (m - a) + 2.0 * (e - m);
        let err = (p.value[0] - truth).abs();
        assert!(err <= p.estimate[0], "{err:e} > {:e}", p.estimate[0]);
        assert_eq!(p.panels, 2);
        let r = integrate_breaks(&mut f, &[a, e], 1e-10, 1.0, 1e-18, 200, false).unwrap_err();
        assert_eq!(r.code, RefusalCode::NoConvergence);
        assert!(r.detail.contains("without a midpoint"), "{}", r.detail);
    }

    /// CodeRabbit on d44f4b4: a span one ulp wide far from zero has no
    /// midpoint and no second estimate. With a constant integrand its
    /// Kronrod-Gauss difference is 0, but the estimate it carries is its
    /// `|f|` integral, far above its tolerance: it refuses rather than pass
    /// carrying more than its tolerance.
    #[test]
    fn a_span_an_ulp_wide_over_its_tolerance_refuses() {
        let a = 1.0e6_f64;
        let e = f64::from_bits(a.to_bits() + 1);
        let mut f = each(|_| {
            let mut y = [0.0; N];
            y[0] = 1.0e6;
            y
        });
        let r = integrate_breaks(&mut f, &[a, e], 1e-7, 1.0, 1e-18, 200, false).unwrap_err();
        assert_eq!(r.code, RefusalCode::NoConvergence);
    }

    /// `1 + 1/(1 + ((x - c)/w)^2)` over `[0, 1]` with `breaks` inside, at
    /// accuracy 1e-7: the error and the estimate.
    fn lorentzian(inner: &[f64], c: f64, w: f64) -> (f64, f64, usize) {
        let mut breaks = vec![0.0];
        breaks.extend(inner);
        breaks.push(1.0);
        let mut f = each(|x| {
            let mut y = [0.0; N];
            let t = (x - c) / w;
            y[0] = 1.0 + 1.0 / (1.0 + t * t);
            y
        });
        let p = integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false).unwrap();
        let truth = 1.0 + w * (libm::atan((1.0 - c) / w) - libm::atan(-c / w));
        ((p.value[0] - truth).abs(), p.estimate[0], p.panels)
    }

    /// Reviews A4-R2 F1 and A4-R3: a narrow peak in a span whose panel came
    /// from splitting a panel over several spans was accepted on that
    /// parent's estimate (a break at 0.001: error 9.3e-4 against 1.0e-8;
    /// breaks at 0.2501 and 0.4376: 9.25e-4 against 8.0e-8). Every span is
    /// now its own initial panel and is bisected within itself: the
    /// estimate covers the error at every position the reviews scanned.
    #[test]
    fn a_peak_is_estimated_from_a_bisection_within_its_span() {
        let nested = [0.2501, 0.2501 + 0.7499 * 0.2501];
        for (inner, c, w) in [
            (&[0.001][..], 0.654_595_75, 3e-4),
            (&[0.01][..], 0.835_412_5, 1e-4),
            (&nested[..], 0.578_625, 3e-4),
            (&[0.2501][..], 0.739_875, 1e-4),
            (&[0.26][..], 0.668_875, 1e-4),
            (&[0.5][..], 0.582_875, 1e-4),
        ] {
            let (err, est, panels) = lorentzian(inner, c, w);
            assert!(
                err <= est + 1e-15,
                "breaks {inner:?}, peak at {c} width {w}: error {err:e} > estimate {est:e} ({panels} panels)"
            );
        }
        for (inner, w, n) in [
            (&[0.001][..], 1e-4, 400u32),
            (&[0.2501][..], 1e-4, 4000),
            (&nested[..], 3e-4, 4000),
            (&nested[..], 1e-4, 4000),
            (&[0.5][..], 1e-4, 4000),
        ] {
            for i in 0..n {
                let c = (f64::from(i) + 0.5) / f64::from(n);
                let (err, est, panels) = lorentzian(inner, c, w);
                assert!(
                    err <= est + 1e-15,
                    "breaks {inner:?}, peak at {c} width {w}: error {err:e} > estimate {est:e} ({panels} panels)"
                );
            }
        }
    }

    /// Review A4-R4 F1: a large smooth span next to a small one with a
    /// narrow peak. With one tolerance for both (relative to their summed
    /// first estimates) the small span stopped after its first bisection,
    /// where main refined it against its own tolerance (big 1, frequency
    /// 10, width 3e-4, peak at 0.708625: error 8.97e-6 against 2.16e-8).
    /// Each span now has its own tolerance: the estimate covers the error
    /// at every position the review scanned.
    #[test]
    fn a_small_span_beside_a_large_one_has_its_own_tolerance() {
        let run = |big: f64, freq: f64, c: f64, w: f64| {
            let mut f = each(|x| {
                let mut y = [0.0; N];
                let t = (x - c) / w;
                y[0] = if x < 0.5 {
                    big * (2.0 + libm::sin(3.0 * x))
                } else {
                    0.01 * (1.5 + libm::sin(freq * x)) + 0.01 / (1.0 + t * t)
                };
                y
            });
            let p =
                integrate_breaks(&mut f, &[0.0, 0.5, 1.0], 1e-7, 1.0, 1e-18, 200, false).unwrap();
            let truth = big * (1.0 + (1.0 - libm::cos(1.5)) / 3.0)
                + 0.01 * (0.75 + (libm::cos(0.5 * freq) - libm::cos(freq)) / freq)
                + 0.01 * w * (libm::atan((1.0 - c) / w) - libm::atan((0.5 - c) / w));
            let err = (p.value[0] - truth).abs();
            assert!(
                err <= p.estimate[0] + 1e-15 * truth,
                "big {big}, frequency {freq}, peak at {c} width {w}: error {err:e} > estimate {:e} ({} panels)",
                p.estimate[0],
                p.panels
            );
        };
        run(1.0, 10.0, 0.708_625, 3e-4);
        for (big, freq, w) in [
            (1.0, 10.0, 3e-4),
            (1.0, 10.0, 1e-4),
            (1.0, 60.0, 1e-3),
            (100.0, 10.0, 1e-3),
            (1e4, 60.0, 3e-4),
        ] {
            for k in 0..2000u32 {
                run(big, freq, 0.5 + 0.5 * (f64::from(k) + 0.5) / 2000.0, w);
            }
        }
    }

    /// Every initial panel (one per span) is bisected once unless the piece
    /// is exact, and a break at a kink ends a panel there.
    #[test]
    fn hard_breaks_end_panels_and_each_is_bisected_once() {
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = (x - 0.3).abs();
            y
        });
        let breaks = [0.0, 0.3, 1.0];
        let p = integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, false).unwrap();
        assert_eq!(p.panels, 4);
        assert!((p.value[0] - (0.045 + 0.245)).abs() <= 1e-15);
        let p = integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, true).unwrap();
        assert_eq!(p.panels, 2);
    }
}
