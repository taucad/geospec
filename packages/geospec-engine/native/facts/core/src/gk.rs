//! Adaptive Gauss-Kronrod 7-15 over a vector integrand, from the solidcore
//! evaluation's patch 0001 (`mass/gauss.rs`): QUADPACK `qk15`, global adaptive
//! bisection of the panel with the largest scaled error, panels summed left to
//! right. Added here: every node value may carry its own error (an inner
//! integral's), which the panel carries through its weights, and the result
//! keeps the panels' Kronrod-Gauss differences, so the caller can bound it.

use crate::refusal::{Refusal, RefusalCode};

/// Components: area, volume, the three volume first moments, and the closure
/// integral of the normal's z component.
pub const N: usize = 6;
/// One value per component.
pub type V = [f64; N];
/// Nodes of one panel.
pub const NODES: usize = 15;
/// The closure component, which never drives the adaptivity.
pub const CLOSURE: usize = 5;

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

/// One panel: Kronrod and Gauss estimates, the integral of `|area|`, and the
/// node errors carried through the Kronrod weights.
struct Estimate {
    k: V,
    g: V,
    area_abs: f64,
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
    let mut area_abs = 0.0;
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
            area_abs += w * y[0].abs();
            for ((ki, ci), (yi, ei)) in k.iter_mut().zip(carried.iter_mut()).zip(y.iter().zip(e)) {
                *ki += w * yi;
                *ci += w * ei;
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
    for v in &mut carried {
        *v *= h.abs();
    }
    Ok(Estimate {
        k,
        g,
        area_abs: area_abs * h.abs(),
        carried,
    })
}

/// An integrated piece: the value, the sum of its panels' Kronrod-Gauss
/// differences, the node errors carried through, and the panel count.
#[derive(Clone, Copy, Debug, Default)]
pub struct Piece {
    pub value: V,
    pub estimate: V,
    pub carried: V,
    pub panels: usize,
}

fn err_of(k: &V, g: &V) -> V {
    let mut e = [0.0; N];
    for ((ei, ki), gi) in e.iter_mut().zip(k.iter()).zip(g.iter()) {
        *ei = (ki - gi).abs();
    }
    e
}

/// Globally adaptive Gauss-Kronrod (QUADPACK `qag` style): split the panel
/// with the largest scaled error until every component's summed difference is
/// within `tol`, or refuse after `max_panels`. Panels at round-off width are
/// not split further. The result sums panels left to right.
fn adaptive<F: Nodes>(
    f: &mut F,
    a: f64,
    b: f64,
    first: Estimate,
    tol: &V,
    max_panels: usize,
) -> Result<Piece, Refusal> {
    struct Panel {
        a: f64,
        b: f64,
        k: V,
        err: V,
        carried: V,
    }
    let width = (b - a).abs();
    let mut panels = vec![Panel {
        a,
        b,
        err: err_of(&first.k, &first.g),
        k: first.k,
        carried: first.carried,
    }];
    loop {
        let mut total = [0.0; N];
        for p in &panels {
            add(&mut total, &p.err);
        }
        if !total.iter().all(|e| e.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("integrand not finite on [{a}, {b}]"),
            ));
        }
        if total.iter().zip(tol.iter()).all(|(e, t)| e <= t) {
            break;
        }
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
            // Only round-off-width panels remain; their differences stay in
            // the estimate.
            break;
        };
        if panels.len() >= max_panels {
            return Err(Refusal::new(
                RefusalCode::NoConvergence,
                format!(
                    "no convergence in {max_panels} panels on [{a}, {b}]: error {total:?} against {tol:?}"
                ),
            ));
        }
        let p = panels.swap_remove(i);
        let m = 0.5 * (p.a + p.b);
        for (lo, hi) in [(p.a, m), (m, p.b)] {
            let e = gk15(f, lo, hi)?;
            panels.push(Panel {
                a: lo,
                b: hi,
                err: err_of(&e.k, &e.g),
                k: e.k,
                carried: e.carried,
            });
        }
    }
    panels.sort_by(|x, y| x.a.total_cmp(&y.a));
    let mut piece = Piece {
        panels: panels.len(),
        ..Piece::default()
    };
    for p in &panels {
        add(&mut piece.value, &p.k);
        add(&mut piece.estimate, &p.err);
        add(&mut piece.carried, &p.carried);
    }
    Ok(piece)
}

/// Integrates `[a, b]` adaptively with tolerances relative to the piece's own
/// first estimate (area against itself, volume against area times size,
/// moments against area times size squared) and an absolute floor scaled the
/// same way. The closure component never drives the adaptivity.
pub fn integrate_piece<F: Nodes>(
    f: &mut F,
    a: f64,
    b: f64,
    accuracy: f64,
    size: f64,
    floor: f64,
    max_panels: usize,
) -> Result<Piece, Refusal> {
    if a == b {
        return Ok(Piece::default());
    }
    let first = gk15(f, a, b)?;
    let area = first.area_abs;
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
    adaptive(f, a, b, first, &tol, max_panels)
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
        let mut f = each(|x| [x.powi(5), 1.0, libm::exp(x), libm::sin(x), 0.0, 0.0]);
        let p = integrate_piece(&mut f, 0.0, 2.0, 1e-12, 1.0, 1e-15, 200).unwrap();
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

    #[test]
    fn a_kink_refuses_without_convergence_at_a_tight_cap() {
        let mut f = each(|x| [(x - 0.3).abs().sqrt(), 0.0, 0.0, 0.0, 0.0, 0.0]);
        let r = integrate_piece(&mut f, 0.0, 1.0, 1e-14, 1.0, 1e-18, 8);
        assert_eq!(r.unwrap_err().code, RefusalCode::NoConvergence);
    }
}
