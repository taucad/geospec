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
//! Both are estimates, not guaranteed bounds. A piece may start with soft
//! breaks (where the integrand stays C1) inside its panels: such a panel is
//! split at the soft break nearest its midpoint, and never accepted while it
//! spans one, since both estimates assume an integrand smooth over the panel
//! (a feature between all of its nodes would otherwise go unseen).

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
/// carried node errors and `|f|` integral, and the soft breaks strictly
/// inside it (an index range into the piece's breaks).
struct Panel {
    a: f64,
    b: f64,
    k: V,
    err: V,
    carried: V,
    resabs: V,
    inner: std::ops::Range<usize>,
}

impl Panel {
    fn new(a: f64, b: f64, e: Estimate, err: V, inner: std::ops::Range<usize>) -> Self {
        Self {
            a,
            b,
            k: e.k,
            err,
            carried: e.carried,
            resabs: e.resabs,
            inner,
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

/// Where `[a, b]` with soft breaks `breaks[inner]` strictly inside it is
/// split: at the soft break nearest its midpoint (the first of two equally
/// near), so a panel never straddles a break once it is split down to one
/// span; without soft breaks, at the midpoint.
fn split_point(
    breaks: &[f64],
    a: f64,
    b: f64,
    inner: &std::ops::Range<usize>,
) -> (f64, std::ops::Range<usize>, std::ops::Range<usize>) {
    let m = 0.5 * (a + b);
    let mut best: Option<(usize, f64)> = None;
    for i in inner.clone() {
        let Some(&x) = breaks.get(i) else { continue };
        let d = (x - m).abs();
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    match best.and_then(|(i, _)| breaks.get(i).map(|&x| (i, x))) {
        Some((i, x)) => (x, inner.start..i, i + 1..inner.end),
        None => (m, inner.start..inner.start, inner.end..inner.end),
    }
}

/// Globally adaptive Gauss-Kronrod (QUADPACK `qag` style) over initial panels
/// that may span soft breaks: split the panel with the largest scaled error
/// (at its soft break nearest the midpoint while it has one, else at the
/// midpoint) until every driving component's summed estimate is within
/// `tol`, or refuse after `max_panels`. A split panel's children each carry
/// the larger of their own Kronrod-Gauss difference and half the difference
/// between the parent's Kronrod value and the sum of the two children's.
/// A panel that spans a soft break is never accepted: it is split at its
/// soft breaks first, so every accepted panel lies within one span and has
/// that second estimate (its parent's may span several). Panels at round-off
/// width (relative to `width`) without soft breaks are not split further.
/// The result sums panels in parameter order.
#[allow(clippy::too_many_arguments)]
fn adaptive<F: Nodes>(
    f: &mut F,
    breaks: &[f64],
    initial: Vec<Panel>,
    width: f64,
    tol: &V,
    max_panels: usize,
    exact: bool,
) -> Result<Piece, Refusal> {
    let scaled = |p: &Panel| {
        p.err
            .iter()
            .zip(tol.iter())
            .take(DRIVING)
            .map(|(e, t)| e / t)
            .fold(0.0_f64, f64::max)
    };
    let splittable = |p: &Panel| !p.inner.is_empty() || (p.b - p.a).abs() > 1e-12 * width;
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
            format!(
                "integrand not finite on [{:?}, {:?}]: {total:?}",
                breaks.first(),
                breaks.last()
            ),
        )
    };
    let mut count = initial.len();
    let mut seq = 0usize;
    let mut fixed: Vec<Panel> = Vec::new();
    let mut queue = std::collections::BinaryHeap::new();
    let mut forced: Vec<Panel> = Vec::new();
    for p in initial {
        if !exact && splittable(&p) {
            forced.push(p);
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
    // The forced splits first (in order, then their children that still
    // span a soft break, depth first), then the worst panel each time. A
    // panel spanning a soft break is never accepted: its estimates assume
    // an integrand smooth over the panel (review A4-R1).
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
                    "no convergence in {max_panels} panels on [{:?}, {:?}]: error {total:?} against {tol:?}",
                    breaks.first(),
                    breaks.last()
                ),
            ));
        }
        let p = next;
        let (m, left, right) = split_point(breaks, p.a, p.b, &p.inner);
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
        let mut spanning: Vec<Panel> = Vec::new();
        for (x0, x1, e, inner) in [(p.a, m, lo, left), (m, p.b, hi, right)] {
            let mut err = err_of(&e.k, &e.g);
            for (ei, si) in err.iter_mut().zip(&split) {
                *ei = ei.max(*si);
            }
            add(&mut total, &err);
            let child = Panel::new(x0, x1, e, err, inner);
            if !child.inner.is_empty() {
                spanning.push(child);
            } else if splittable(&child) {
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
        forced.extend(spanning.into_iter().rev());
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

/// Integrates over `breaks` (monotone, ends included, each with whether it is
/// hard) adaptively, with tolerances relative to the first estimates of the
/// initial panels together (area against itself, volume against area times
/// size, moments against area times size squared) and an absolute floor
/// scaled the same way. Only those components drive the adaptivity; the
/// others keep their estimates. The initial
/// panels run between hard breaks; a soft break (where the integrand stays
/// at least once continuously differentiable) may lie inside an initial
/// panel, which is split at its soft breaks before any panel is accepted, so
/// every accepted panel lies within one span. `max_panels`
/// is per interval between consecutive breaks. With `exact`, every break is
/// taken as hard (a piecewise polynomial is exact only per piece).
///
/// # Errors
///
/// [`RefusalCode::NoConvergence`] past the panel cap, a non-finite
/// integrand, or the integrand's own refusal.
#[allow(clippy::too_many_arguments)]
pub fn integrate_breaks<F: Nodes>(
    f: &mut F,
    breaks: &[(f64, bool)],
    accuracy: f64,
    size: f64,
    floor: f64,
    max_panels: usize,
    exact: bool,
) -> Result<Piece, Refusal> {
    let (Some(&(a, _)), Some(&(b, _))) = (breaks.first(), breaks.last()) else {
        return Ok(Piece::default());
    };
    if a == b {
        return Ok(Piece::default());
    }
    let at: Vec<f64> = breaks.iter().map(|x| x.0).collect();
    let last = at.len() - 1;
    let mut initial = Vec::new();
    let mut first = Estimate {
        k: [0.0; N],
        g: [0.0; N],
        resabs: [0.0; N],
        carried: [0.0; N],
    };
    let mut start = 0usize;
    for (i, &(x, hard)) in breaks.iter().enumerate().skip(1) {
        if !(hard || exact || i == last) {
            continue;
        }
        let Some(&x0) = at.get(start) else { break };
        if x != x0 {
            let e = gk15(f, x0, x)?;
            add(&mut first.k, &e.k);
            add(&mut first.resabs, &e.resabs);
            let err = err_of(&e.k, &e.g);
            initial.push(Panel::new(x0, x, e, err, start + 1..i));
        }
        start = i;
    }
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
    let cap = max_panels.saturating_mul(last.max(1));
    adaptive(f, &at, initial, (b - a).abs(), &tol, cap, exact)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        integrate_breaks(
            f,
            &[(a, true), (b, true)],
            accuracy,
            size,
            floor,
            max_panels,
            exact,
        )
    }

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

    fn soft(at: &[f64]) -> Vec<(f64, bool)> {
        let last = at.len() - 1;
        at.iter()
            .enumerate()
            .map(|(i, &x)| (x, i == 0 || i == last))
            .collect()
    }

    /// Many soft breaks over a smooth integrand: no accepted panel spans a
    /// soft break (so at least one panel per span), the grouped splits double
    /// as the leaves' second estimates (fewer GK15 batches than taking every
    /// break as hard), and the estimate covers the error.
    #[test]
    fn soft_breaks_over_a_smooth_integrand_split_to_spans_in_fewer_batches() {
        let at: Vec<f64> = (0..=400).map(|i| f64::from(i) / 40.0).collect();
        let g = |x: f64| {
            let mut y = [0.0; N];
            y[0] = 2.0 + libm::sin(x);
            y[1] = libm::exp(-x);
            y
        };
        let batches = std::cell::Cell::new(0usize);
        let mut f = |xs: &[f64; NODES], ys: &mut [V; NODES], _: &mut [V; NODES]| {
            batches.set(batches.get() + 1);
            for (x, y) in xs.iter().zip(ys.iter_mut()) {
                *y = g(*x);
            }
            Ok(())
        };
        let p = integrate_breaks(&mut f, &soft(&at), 1e-9, 1.0, 1e-18, 200, false).unwrap();
        let grouped = batches.replace(0);
        assert!(p.panels >= 400, "{}", p.panels);
        let truth = [20.0 + 1.0 - libm::cos(10.0), 1.0 - libm::exp(-10.0)];
        for (k, t) in truth.iter().enumerate() {
            assert!(
                (p.value[k] - t).abs() <= p.estimate[k] + 1e-14,
                "{k}: {} vs {t} within {}",
                p.value[k],
                p.estimate[k]
            );
        }
        // The same breaks taken as hard: two panels at least per interval,
        // and more batches.
        let hard: Vec<(f64, bool)> = at.iter().map(|&x| (x, true)).collect();
        let q = integrate_breaks(&mut f, &hard, 1e-9, 1.0, 1e-18, 200, false).unwrap();
        assert!(q.panels >= 2 * 400, "{}", q.panels);
        let batches = batches.get();
        assert!(
            grouped < batches,
            "{grouped} batches grouped, {batches} hard"
        );
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
    /// spans wide on `n` uniform soft breaks, at every position. A panel
    /// spanning soft breaks whose nodes all miss the bump must not be
    /// accepted: the estimate covers the error at every position (grouped
    /// panels returned exactly 1 with an estimate of 0).
    #[test]
    fn a_bump_between_the_nodes_of_a_soft_spanning_panel_is_not_missed() {
        for n in [128u32, 256, 512] {
            let d = 1.0 / f64::from(n);
            let breaks: Vec<(f64, bool)> = (0..=n)
                .map(|i| (f64::from(i) * d, i == 0 || i == n))
                .collect();
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
        let p = integrate_breaks(&mut f, &soft(&at), 1e-12, 1.0, 1e-18, 200, false).unwrap();
        let err = (p.value[0] - (1.0 + truth)).abs();
        assert!(err <= p.estimate[0] + 1e-15, "{err} > {}", p.estimate[0]);
        assert!(p.estimate[0] <= 1e-11 * (1.0 + truth), "{}", p.estimate[0]);
        let q = integrate_breaks(&mut f, &soft(&at), 1e-12, 1.0, 1e-18, 200, false).unwrap();
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
            let p = integrate_breaks(&mut f, &soft(&at), accuracy, 1.0, 1e-18, 200, false).unwrap();
            let err = (p.value[0] - truth).abs();
            assert!(
                err <= p.estimate[0] + 1e-14,
                "accuracy {accuracy}: {err} > {}",
                p.estimate[0]
            );
        }
    }

    /// Every initial panel (between hard breaks) is bisected once unless the
    /// piece is exact, and a hard break at a kink ends a panel there.
    #[test]
    fn hard_breaks_end_panels_and_each_is_bisected_once() {
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = (x - 0.3).abs();
            y
        });
        let breaks = [(0.0, true), (0.3, true), (1.0, true)];
        let p = integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, false).unwrap();
        assert_eq!(p.panels, 4);
        assert!((p.value[0] - (0.045 + 0.245)).abs() <= 1e-15);
        let p = integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, true).unwrap();
        assert_eq!(p.panels, 2);
        // A soft break at the kink is where the first split falls.
        let breaks = [(0.0, true), (0.3, false), (1.0, true)];
        let p = integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, false).unwrap();
        assert!((p.value[0] - (0.045 + 0.245)).abs() <= 1e-15);
        assert!(p.panels <= 6, "{}", p.panels);
    }
}
