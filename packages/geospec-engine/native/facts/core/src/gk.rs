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
//! integrand smooth over the panel. The area element `|n|` is not smooth at
//! the scale of a panel where `n` passes close to zero (a steep net, reviews
//! A4-R2 F2 and A6-1): such a panel is split until the direction of `n`
//! turns little between its neighbouring nodes and next to its ends, and,
//! where the integrand states it, until the Bernstein hull of a polynomial
//! along `n` keeps it within one cone over the whole panel ([`adaptive`]).

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

/// A node's hull vector and its error bound, `[x, y, z, e]`: the value at the
/// node of a vector polynomial in the integration variable whose direction is
/// the area direction's ([`direction`]), where the integrand has one (see
/// [`Check::hull`]); zero elsewhere.
pub type H = [f64; 4];

/// The integrand at a batch of points (a panel's [`NODES`] nodes, or the
/// directions at panels' ends): `f(xs, ys, es, hs)` sets `ys[i]` to
/// the value at `xs[i]`, `es[i]` to that value's own error bound (zero for an
/// exact value) and `hs[i]` to its hull vector ([`H`]), or refuses.
pub trait Nodes: FnMut(&[f64], &mut [V], &mut [V], &mut [H]) -> Result<(), Refusal> {}
impl<F: FnMut(&[f64], &mut [V], &mut [V], &mut [H]) -> Result<(), Refusal>> Nodes for F {}

/// How [`adaptive`] checks that the area element is resolved on a panel.
#[derive(Clone, Copy)]
pub struct Check<'a> {
    /// Check the area direction's turns between nodes and at panels' ends.
    /// Off only where `|n|` is smooth over the face, so that it cannot have
    /// the softened kink the checks look for.
    pub turns: bool,
    /// With `turns`: for a panel `[a, b]`, the degree (at most [`NODES`]
    /// minus 1) of the vector polynomial whose values the nodes' hull vectors
    /// are, where they are one, for [`hull_resolved`].
    pub hull: Option<&'a dyn Fn(f64, f64) -> Option<usize>>,
    /// The hull applies to a panel only where every node has an area
    /// direction (the outer integrand, whose direction is an integral of `n`
    /// and can cancel where `|n|` does not).
    pub gate: bool,
    /// With `turns`: for a panel `[a, b]`, whether the area element is
    /// resolved over the whole region of the surface the panel integrates
    /// (an outer panel's two-dimensional cell), or the refusal where that
    /// cannot be decided. Asked only of a child that is otherwise resolved.
    pub cell: Option<&'a dyn Fn(f64, f64) -> Result<bool, Refusal>>,
}

#[cfg(test)]
impl Check<'static> {
    /// The turn checks without a hull.
    pub const TURNS: Self = Self {
        turns: true,
        hull: None,
        gate: false,
        cell: None,
    };
}

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
    /// How far the area direction turns along the nodes.
    turn: Turn,
    /// Where the hull applied ([`hull_resolved`]): whether it shows the
    /// area direction within [`HULL_COS`]'s cone over the whole panel.
    hull: Option<bool>,
}

/// The nodes in parameter order from a panel's `a` end to its `b` end (slot
/// `2 j` is `c - h XGK[j]`, slot `2 j + 1` is `c + h XGK[j]`, slot 14 the
/// centre).
const ORDER: [usize; NODES] = [0, 2, 4, 6, 8, 10, 12, 14, 13, 11, 9, 7, 5, 3, 1];

/// The cosine of the largest angle between the area directions
/// ([`direction`]) at neighbouring nodes of an accepted panel: about 0.505
/// radians, a chord of 0.5 between unit vectors (review A4-R2 F2). See
/// [`adaptive`].
const TURN_COS: f64 = 0.875;

/// The cosine of the largest angle, over a panel the hull applies to,
/// between its hull's coefficients and their axis ([`hull_resolved`]): 60
/// degrees, so the direction turns by at most 120 degrees over the panel.
const HULL_COS: f64 = 0.5;

/// Where a span's end directions are taken, as a fraction of its width from
/// each end: inside the span (its ends may be creases), far inside the gap
/// between an end and the nearest node (`0.0085` of a panel's half-width).
const END_INSET: f64 = 1.0 / (1u64 << 30) as f64;

/// An area direction, scaled to a largest component of 1.
type Dir = [f64; 3];

/// The area direction at a node with value `y` and error bound `e`: the
/// natural normal `(n_x, n_y, n_z)` (an inner node, whose area value `|n|`
/// is positive) or its integral over `v` (an outer node, where every
/// component carries the trim's `-u'`: the sign of the area value removes
/// it), scaled to a largest component of 1. `None` where it is zero or not
/// finite, where its length is under half the area value (the normal turns
/// along `v`, and its integral may cancel to round-off), or where its error
/// bound could turn it by more than about an eighth of a radian.
fn direction(y: &V, e: &V) -> Option<Dir> {
    let s = if y[AREA] > 0.0 {
        1.0
    } else if y[AREA] < 0.0 {
        -1.0
    } else {
        return None;
    };
    let sq = y[NX] * y[NX] + y[NY] * y[NY] + y[CLOSURE] * y[CLOSURE];
    let noise = 8.0 * (e[NX] + e[NY] + e[CLOSURE]);
    if !(sq >= 0.25 * y[AREA] * y[AREA] && sq > noise * noise) {
        return None;
    }
    let big = y[NX].abs().max(y[NY].abs()).max(y[CLOSURE].abs());
    let k = s / big;
    let d = [k * y[NX], k * y[NY], k * y[CLOSURE]];
    d.iter().all(|x| x.is_finite()).then_some(d)
}

/// Whether directions `p` and `q` are more than [`TURN_COS`]'s angle apart;
/// false if either is unknown.
fn turned(p: Option<Dir>, q: Option<Dir>) -> bool {
    let (Some(p), Some(q)) = (p, q) else {
        return false;
    };
    let dot = p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    let pp = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
    let qq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2];
    dot < 0.0 || dot * dot < TURN_COS * TURN_COS * pp * qq
}

/// How far the area direction turns along a panel: whether it turns by more
/// than [`TURN_COS`]'s angle between neighbouring nodes (and, once the panel
/// is placed, between its nodes and its sibling's or its own end directions
/// next to them), the directions at the nodes nearest each end, and whether
/// every node has one.
#[derive(Clone, Copy, Debug, Default)]
struct Turn {
    unresolved: bool,
    ends: [Option<Dir>; 2],
    all: bool,
}

impl Turn {
    fn of(ys: &[V; NODES], es: &[V; NODES]) -> Self {
        let mut t = Self {
            all: true,
            ..Self::default()
        };
        let at = |i: usize| ys.get(i).zip(es.get(i)).and_then(|(y, e)| direction(y, e));
        for i in ORDER {
            let Some(d) = at(i) else {
                t.all = false;
                continue;
            };
            if t.ends[1].is_some() {
                t.unresolved |= turned(t.ends[1], Some(d));
            } else {
                t.ends[0] = Some(d);
            }
            t.ends[1] = Some(d);
        }
        t
    }
}

/// The weighted least-squares fit of a polynomial of one degree `d` to
/// values at the [`NODES`] nodes (Kronrod weights): `rows[i]`, per node
/// slot, gives the `i`-th of its `d + 1` Bernstein coefficients over the
/// panel. It reproduces any polynomial of degree at most `d`, up to
/// `residual` (the largest row sum of `|rows B - I|`, with `B` the Bernstein
/// basis at the nodes) times its largest coefficient. `lambda` is the
/// largest row sum of `|rows|`, which bounds how far node errors move a
/// coefficient.
struct Fit {
    rows: Vec<[f64; NODES]>,
    lambda: f64,
    residual: f64,
}

/// Each node slot's abscissa on `[-1, 1]` and Kronrod weight, in [`gk15`]'s
/// order.
fn slots() -> [(f64, f64); NODES] {
    let mut out = [(0.0, 0.0); NODES];
    let mut it = out.iter_mut();
    for (&x, &w) in XGK.iter().zip(WGK.iter()) {
        let points: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
        for &s in points {
            if let Some(o) = it.next() {
                *o = (s * x, w);
            }
        }
    }
    out
}

/// The `d + 1` Bernstein basis values of degree `d` at `t` on `[-1, 1]`.
fn bernstein(d: usize, t: f64) -> Vec<f64> {
    let s = 0.5 * (1.0 + t);
    let mut b = vec![0.0; d + 1];
    if let Some(b0) = b.first_mut() {
        *b0 = 1.0;
    }
    // De Casteljau's triangle: degree k from degree k - 1.
    for k in 1..=d {
        let mut prev = 0.0;
        for x in b.iter_mut().take(k + 1) {
            let cur = *x;
            *x = (1.0 - s) * cur + s * prev;
            prev = cur;
        }
    }
    b
}

/// [`Fit`] of degree `d` (at most [`NODES`] minus 1), by modified
/// Gram-Schmidt, twice, on the weighted basis.
fn fit(d: usize) -> Fit {
    let m = d + 1;
    let slots = slots();
    let root: Vec<f64> = slots.iter().map(|&(_, w)| w.sqrt()).collect();
    // Columns: the weighted basis functions at the nodes.
    let basis: Vec<Vec<f64>> = slots.iter().map(|&(t, _)| bernstein(d, t)).collect();
    let column = |j: usize| -> [f64; NODES] {
        let mut c = [0.0; NODES];
        for ((ci, bk), r) in c.iter_mut().zip(&basis).zip(&root) {
            *ci = bk.get(j).copied().unwrap_or(0.0) * r;
        }
        c
    };
    let dot = |a: &[f64; NODES], b: &[f64; NODES]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut q: Vec<[f64; NODES]> = Vec::with_capacity(m);
    let mut r = vec![vec![0.0; m]; m];
    for j in 0..m {
        let mut v = column(j);
        for _ in 0..2 {
            for (k, qk) in q.iter().enumerate() {
                let c = dot(qk, &v);
                for (vi, qi) in v.iter_mut().zip(qk) {
                    *vi -= c * qi;
                }
                if let Some(x) = r.get_mut(k).and_then(|row| row.get_mut(j)) {
                    *x += c;
                }
            }
        }
        let n = dot(&v, &v).sqrt();
        for vi in &mut v {
            *vi /= n;
        }
        if let Some(x) = r.get_mut(j).and_then(|row| row.get_mut(j)) {
            *x = n;
        }
        q.push(v);
    }
    // rows = R^-1 Q^T diag(root), by back substitution.
    let mut rows = vec![[0.0; NODES]; m];
    for i in (0..m).rev() {
        let mut row = [0.0; NODES];
        if let Some(qi) = q.get(i) {
            for ((x, qv), rt) in row.iter_mut().zip(qi).zip(&root) {
                *x = qv * rt;
            }
        }
        let ri = r.get(i).cloned().unwrap_or_default();
        for (k, done) in rows.iter().enumerate().skip(i + 1) {
            let c = ri.get(k).copied().unwrap_or(0.0);
            for (x, y) in row.iter_mut().zip(done) {
                *x -= c * y;
            }
        }
        let diag = ri.get(i).copied().unwrap_or(f64::NAN);
        for x in &mut row {
            *x /= diag;
        }
        if let Some(out) = rows.get_mut(i) {
            *out = row;
        }
    }
    let lambda = rows
        .iter()
        .map(|row| row.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0_f64, f64::max);
    let mut residual = 0.0_f64;
    for (i, row) in rows.iter().enumerate() {
        let mut sum = 0.0;
        for j in 0..m {
            let mut x: f64 = row
                .iter()
                .zip(&basis)
                .map(|(rk, bk)| rk * bk.get(j).copied().unwrap_or(0.0))
                .sum();
            if i == j {
                x -= 1.0;
            }
            sum += x.abs();
        }
        residual = residual.max(sum);
    }
    if !(lambda.is_finite() && residual.is_finite()) {
        return Fit {
            rows: Vec::new(),
            lambda: f64::INFINITY,
            residual: f64::INFINITY,
        };
    }
    Fit {
        rows,
        lambda,
        residual,
    }
}

/// [`Fit`] per degree `1..NODES`, made once (the same bits on every run).
fn fits() -> &'static [Fit] {
    static FITS: std::sync::OnceLock<Vec<Fit>> = std::sync::OnceLock::new();
    FITS.get_or_init(|| (1..NODES).map(fit).collect())
}

/// Whether the hull vectors `hs` at a panel's nodes, the values of a vector
/// polynomial `p` of [`Fit`]'s degree, show `p` within [`HULL_COS`]'s cone
/// over the whole panel. The fit gives `p`'s Bernstein coefficients over the
/// panel, up to a radius from the nodes' errors, the fit's residual and its
/// own rounding. `p` is a convex combination of them, so it lies in any
/// convex cone that holds them all. The test is a cone about the sum of the
/// unit coefficients; a coefficient within its radius of zero (round-off of
/// a normal that vanishes, at a pole) is not held against it, and if all
/// are, `p` is round-off and passes.
fn hull_resolved(fit: &Fit, hs: &[H; NODES]) -> bool {
    // The largest error, and a bound on the largest value (its 1-norm).
    let (mut emax, mut ymax) = (0.0_f64, 0.0_f64);
    for h in hs {
        emax = emax.max(h[3]);
        ymax = ymax.max(h[0].abs() + h[1].abs() + h[2].abs());
    }
    let mut cs = [[0.0; 3]; NODES];
    for (c, row) in cs.iter_mut().zip(&fit.rows) {
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        for (w, h) in row.iter().zip(hs) {
            x += w * h[0];
            y += w * h[1];
            z += w * h[2];
        }
        *c = [x, y, z];
    }
    let used = cs.get(..fit.rows.len()).unwrap_or_default();
    let mut ls = [0.0; NODES];
    let mut cmax = 0.0_f64;
    for (l, c) in ls.iter_mut().zip(used) {
        *l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        cmax = cmax.max(*l);
    }
    let r = fit.lambda * (emax + 32.0 * f64::EPSILON * ymax) + 2.0 * fit.residual * cmax;
    if !(r.is_finite() && cmax.is_finite()) {
        return false;
    }
    let mut axis = [0.0; 3];
    for (c, &l) in used.iter().zip(&ls) {
        if l > r {
            for (a, x) in axis.iter_mut().zip(c) {
                *a += x / l;
            }
        }
    }
    let al = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if al == 0.0 {
        return cmax <= r;
    }
    used.iter().zip(&ls).all(|(c, &l)| {
        let along = (c[0] * axis[0] + c[1] * axis[1] + c[2] * axis[2]) / al;
        l <= r || along - r >= HULL_COS * (l + r)
    })
}

/// Fifteen-point Kronrod and seven-point Gauss estimates on `[a, b]`; the
/// nodes are evaluated as one batch, then summed in their order. With `turn`,
/// how far the area direction turns along them (not where `hull` applies
/// without `gate`); with `hull` (and, with `gate`, where every node has a
/// direction), [`hull_resolved`].
fn gk15<F: Nodes>(
    f: &mut F,
    a: f64,
    b: f64,
    turn: bool,
    hull: Option<&Fit>,
    gate: bool,
) -> Result<Estimate, Refusal> {
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
    let mut hs = [[0.0; 4]; NODES];
    f(&xs, &mut ys, &mut es, &mut hs)?;
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
    // Where the hull applies without a gate it replaces the comparisons at
    // points, which it implies over the closed panel.
    let turn = if turn && (hull.is_none() || gate) {
        Turn::of(&ys, &es)
    } else {
        Turn::default()
    };
    let hull = hull
        .filter(|_| !gate || turn.all)
        .map(|fit| hull_resolved(fit, &hs));
    Ok(Estimate {
        k,
        g,
        resabs,
        carried,
        turn,
        hull,
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
/// carried node errors and `|f|` integral, how its area direction turns,
/// and the area directions at its ends (`None` until they are evaluated,
/// then `Some` of the direction or of `None` where it is unknown).
struct Panel {
    a: f64,
    b: f64,
    k: V,
    err: V,
    carried: V,
    resabs: V,
    turn: Turn,
    hull: Option<bool>,
    ends: [Option<Option<Dir>>; 2],
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
            turn: e.turn,
            hull: e.hull,
            ends: [None, None],
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

/// The area directions at `xs`, each `None` where it is unknown. A refusal
/// here only leaves them unknown: the panels' own nodes decide whether the
/// integration refuses.
fn directions_at<F: Nodes>(f: &mut F, xs: &[f64]) -> Vec<Option<Dir>> {
    let mut ys = vec![[0.0; N]; xs.len()];
    let mut es = vec![[0.0; N]; xs.len()];
    let mut hs = vec![[0.0; 4]; xs.len()];
    if f(xs, &mut ys, &mut es, &mut hs).is_err() {
        return vec![None; xs.len()];
    }
    ys.iter().zip(&es).map(|(y, e)| direction(y, e)).collect()
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
///
/// Both estimates assume an integrand smooth at the panel's scale. The area
/// element `|n|` is not smooth where `n` passes close to zero: there it is
/// a softened kink, about as wide as the distance of `n` from zero over its
/// rate of change, across which the direction of `n` turns through half a
/// turn (review A4-R2 F2). Unless `exact` or `check` has no `turns`, a
/// child is unresolved where:
///
/// - its area direction ([`direction`]) turns by more than [`TURN_COS`]'s
///   angle (about half a radian) between neighbouring nodes, or across the
///   midpoint it shares with its sibling (between their nodes nearest it),
///   unless the hull applies to it without [`Check::gate`] (an inner
///   panel), where the hull's test implies these over the closed panel;
/// - where the hull does not apply to it, between either of its end nodes
///   and the direction at that end: at a bisection point the direction
///   there, at a span's end the direction [`END_INSET`] of the span inside
///   it. So a kink in the gap between any panel's end and its nearest node
///   shows at every level, not only at the first. The directions are taken
///   lazily, for a child only once it needs them;
/// - where the hull applies to it ([`Check::hull`]), [`hull_resolved`] does
///   not show the direction within 60 degrees of one axis over the whole
///   panel;
/// - where the caller checks the region of the surface the panel
///   integrates ([`Check::cell`], an outer panel on a NURBS surface), that
///   check fails. It is asked only of a child the rules above leave
///   resolved.
///
/// An unresolved child carries at least its `|f|` integral as its area
/// estimate. It is split while the piece's summed error exceeds the
/// tolerance, and accepted with that estimate once the sum fits, so an
/// unresolved panel widens the bound rather than refusing. The piece
/// refuses `no-convergence` at the panel cap only when the sum never fits.
///
/// What this covers, and what it does not:
///
/// - On an inner panel the hull bounds the normal's direction over the whole
///   closed panel, so it sees any kink within the panel, including two close
///   kinks whose turns cancel at the nodes. On an outer panel it bounds the
///   direction of `∫n dv`, not of `n` along `v`; there the cell check
///   ([`Check::cell`]) bounds `n` itself over the panel's region of the
///   surface. The hull applies only where the caller states that the hull
///   vectors are the values of a polynomial of known degree
///   ([`Check::hull`]). Its radius takes the nodes' errors as given: an
///   outer node's is the inner integral's estimate.
/// - Without either, the checks compare directions at points. Two close kinks
///   between neighbouring nodes are not seen. A kink closer to a span's end
///   than [`END_INSET`] of the span is not seen either; its error is at most
///   the slope's jump times that distance squared.
/// - A resolved panel's error is still an estimate, not a bound. For one
///   kink of the form `n = (a (t - t0), 0, 1)` within the panel, a turn of
///   at most 120 degrees over the panel keeps its branch points
///   `t0 ± i / a` at least 0.58 of the half-width from the panel (half a
///   radian between neighbouring nodes: about 0.4), where the Kronrod-Gauss
///   difference exceeds the Kronrod error, as on a smooth integrand. A kink
///   just outside the panel is resolved by the splitting of its own panel,
///   not of this one.
#[allow(clippy::too_many_lines)]
fn adaptive<F: Nodes>(
    f: &mut F,
    initial: Vec<Panel>,
    width: f64,
    tol: &V,
    max_panels: usize,
    exact: bool,
    check: &Check<'_>,
) -> Result<Piece, Refusal> {
    let (lo_end, hi_end) = initial
        .first()
        .zip(initial.last())
        .map_or((0.0, 0.0), |(p, q)| (p.a, q.b));
    let turns = !exact && check.turns;
    let hull_fit = |a: f64, b: f64| -> Option<&'static Fit> {
        let d = check.hull.filter(|_| turns).and_then(|h| h(a, b))?;
        fits().get(d.checked_sub(1)?)
    };
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
        let lo = gk15(f, p.a, m, turns, hull_fit(p.a, m), check.gate)?;
        let hi = gk15(f, m, p.b, turns, hull_fit(m, p.b), check.gate)?;
        let mut split = [0.0; N];
        for (((s, pk), l), h) in split.iter_mut().zip(&p.k).zip(&lo.k).zip(&hi.k) {
            *s = 0.5 * (pk - (l + h)).abs();
        }
        for (t, e) in total.iter_mut().zip(&p.err) {
            *t -= e;
        }
        count += 1;
        // The turn across the midpoint, between the nodes nearest it.
        let cross = turned(lo.turn.ends[1], hi.turn.ends[0]);
        let child_err = |e: &Estimate| {
            let mut err = err_of(&e.k, &e.g);
            for (ei, si) in err.iter_mut().zip(&split) {
                *ei = ei.max(*si);
            }
            err
        };
        let (lo_err, hi_err) = (child_err(&lo), child_err(&hi));
        let mut children = [
            Panel::new(p.a, m, lo, lo_err),
            Panel::new(m, p.b, hi, hi_err),
        ];
        // The end directions of the children that need them: not covered
        // by the hull and not already unresolved. At a bisection point the
        // direction there serves both children (it is no crease); at a
        // span's end it is taken just inside. They are taken lazily: a
        // span's own on its first bisection, a child's that were not needed
        // by its own children where they need them.
        let [pa, pb] = p.ends;
        let [c0, c1] = &mut children;
        c0.ends = [pa, None];
        c1.ends = [None, pb];
        let at = |x: f64, inside: f64| {
            if x == lo_end || x == hi_end {
                x + END_INSET * (inside - x)
            } else {
                x
            }
        };
        let places = children.each_ref().map(|c| [at(c.a, c.b), at(c.b, c.a)]);
        let mut wanted: Vec<f64> = Vec::new();
        for (c, xs) in children.iter().zip(&places) {
            if !turns || cross || c.hull.is_some() || c.turn.unresolved {
                continue;
            }
            for (end, &x) in c.ends.iter().zip(xs) {
                if end.is_none() && !wanted.contains(&x) {
                    wanted.push(x);
                }
            }
        }
        if !wanted.is_empty() {
            let dirs = directions_at(f, &wanted);
            for (c, xs) in children.iter_mut().zip(&places) {
                for (end, x) in c.ends.iter_mut().zip(xs) {
                    if end.is_none() {
                        if let Some(k) = wanted.iter().position(|w| w == x) {
                            *end = dirs.get(k).copied();
                        }
                    }
                }
            }
        }
        for mut child in children {
            let [first, last] = child.turn.ends;
            let [at_a, at_b] = child.ends;
            let ends = child.hull != Some(true)
                && (turned(at_a.flatten(), first) || turned(last, at_b.flatten()));
            child.turn.unresolved |= cross || ends || child.hull == Some(false);
            if let Some(cell) = check.cell.filter(|_| turns && !child.turn.unresolved) {
                child.turn.unresolved = !cell(child.a, child.b)?;
            }
            if turns && child.turn.unresolved {
                child.err[AREA] = child.err[AREA].max(child.resabs[AREA]);
            }
            add(&mut total, &child.err);
            // A child that cannot be bisected keeps its estimate from this
            // bisection and is not queued.
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
/// bisected. `check` says how the area element's resolution is checked
/// ([`adaptive`]).
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
    check: &Check<'_>,
) -> Result<Piece, Refusal> {
    let mut sum = Piece::default();
    for pair in breaks.windows(2) {
        let &[a, b] = pair else { continue };
        let p = integrate_piece(f, a, b, accuracy, size, floor, max_panels, exact, check)?;
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
    check: &Check<'_>,
) -> Result<Piece, Refusal> {
    if a == b {
        return Ok(Piece::default());
    }
    // The first panel is always bisected or kept on its `|f|` integral: its
    // turn is not needed.
    let first = gk15(f, a, b, false, None, false)?;
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
    adaptive(f, initial, (b - a).abs(), &tol, max_panels, exact, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn each(mut g: impl FnMut(f64) -> V) -> impl Nodes {
        move |xs: &[f64], ys: &mut [V], _: &mut [V], _: &mut [H]| {
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
        let p = integrate_piece(
            &mut f,
            0.0,
            2.0,
            1e-12,
            1.0,
            1e-15,
            200,
            false,
            &Check::TURNS,
        )
        .unwrap();
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

    /// The fits reproduce Bernstein coefficients of polynomials of their
    /// degree, with a small residual and a moderate gain on node errors.
    #[test]
    fn fits_reproduce_polynomials_of_their_degree() {
        let slots = slots();
        for (i, fit) in fits().iter().enumerate() {
            let d = i + 1;
            assert!(fit.residual < 1e-9 && fit.lambda < 1e5, "{d}");
            for j in 0..=d {
                let ys: Vec<f64> = slots.iter().map(|&(t, _)| bernstein(d, t)[j]).collect();
                for (k, row) in fit.rows.iter().enumerate() {
                    let c: f64 = row.iter().zip(&ys).map(|(r, y)| r * y).sum();
                    let want = if k == j { 1.0 } else { 0.0 };
                    assert!((c - want).abs() < 1e-9, "{d} {j} {k}: {c}");
                }
            }
        }
    }

    fn hull_of(fit: &Fit, n: impl Fn(f64) -> [f64; 3]) -> bool {
        let mut hs = [[0.0; 4]; NODES];
        for (h, (t, _)) in hs.iter_mut().zip(slots()) {
            let v = n(t);
            *h = [v[0], v[1], v[2], 0.0];
        }
        hull_resolved(fit, &hs)
    }

    /// The hull passes a direction that turns by 90 degrees over the panel,
    /// and fails two close sign changes of one component between
    /// neighbouring nodes, whose directions at every node agree.
    #[test]
    fn the_hull_sees_two_close_sign_changes_between_nodes() {
        let fit = &fits()[1];
        assert!(hull_of(fit, |t| [t, 0.0, 1.0]));
        let (r1, r2) = (0.1, 0.15);
        let kinks = |t: f64| [-1e4 * (t - r1) * (t - r2), 0.0, 1.0];
        let dirs: Vec<Option<Dir>> = slots()
            .iter()
            .map(|&(t, _)| {
                let n = kinks(t);
                let mut y = [0.0; N];
                y[AREA] = (n[0] * n[0] + 1.0).sqrt();
                y[NX] = n[0];
                y[CLOSURE] = n[2];
                direction(&y, &[0.0; N])
            })
            .collect();
        let mut ordered = ORDER.iter().map(|&i| dirs[i]);
        let mut prev = ordered.next().flatten();
        for d in ordered {
            assert!(!turned(prev, d));
            prev = d;
        }
        assert!(!hull_of(fit, kinks));
        // A normal that vanishes at an end, with one direction (a pole).
        assert!(hull_of(fit, |t| [0.0, 0.0, 1.0 + t]));
    }

    /// Review F5: a constant has `K == G` on the first panel; it is still
    /// bisected unless declared exact.
    #[test]
    fn a_first_panel_with_k_equal_to_g_is_still_bisected() {
        let mut f = each(|_| [1.0; N]);
        let p = integrate_piece(
            &mut f,
            0.0,
            1.0,
            1e-7,
            1.0,
            1e-15,
            200,
            false,
            &Check::TURNS,
        )
        .unwrap();
        assert!(p.panels >= 2, "{}", p.panels);
        let p =
            integrate_piece(&mut f, 0.0, 1.0, 1e-7, 1.0, 1e-15, 200, true, &Check::TURNS).unwrap();
        assert_eq!(p.panels, 1);
    }

    #[test]
    fn a_kink_refuses_without_convergence_at_a_tight_cap() {
        let mut f = each(|x| {
            let mut y = [0.0; N];
            y[0] = (x - 0.3).abs().sqrt();
            y
        });
        let r = integrate_piece(&mut f, 0.0, 1.0, 1e-14, 1.0, 1e-18, 8, false, &Check::TURNS);
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
        let p = integrate_breaks(&mut f, &at, 1e-9, 1.0, 1e-18, 200, false, &Check::TURNS).unwrap();
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
                let p =
                    integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false, &Check::TURNS)
                        .unwrap();
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
        let p =
            integrate_breaks(&mut f, &at, 1e-12, 1.0, 1e-18, 200, false, &Check::TURNS).unwrap();
        let err = (p.value[0] - (1.0 + truth)).abs();
        assert!(err <= p.estimate[0] + 1e-15, "{err} > {}", p.estimate[0]);
        assert!(p.estimate[0] <= 1e-11 * (1.0 + truth), "{}", p.estimate[0]);
        let q =
            integrate_breaks(&mut f, &at, 1e-12, 1.0, 1e-18, 200, false, &Check::TURNS).unwrap();
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
            let p = integrate_breaks(&mut f, &at, accuracy, 1.0, 1e-18, 200, false, &Check::TURNS)
                .unwrap();
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
        let p =
            integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false, &Check::TURNS).unwrap();
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
        let p =
            integrate_breaks(&mut f, &[a, e], 1e-10, 1.0, 1e-9, 200, false, &Check::TURNS).unwrap();
        let truth = (m - a) + 2.0 * (e - m);
        let err = (p.value[0] - truth).abs();
        assert!(err <= p.estimate[0], "{err:e} > {:e}", p.estimate[0]);
        assert_eq!(p.panels, 2);
        let r = integrate_breaks(
            &mut f,
            &[a, e],
            1e-10,
            1.0,
            1e-18,
            200,
            false,
            &Check::TURNS,
        )
        .unwrap_err();
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
        let r = integrate_breaks(&mut f, &[a, e], 1e-7, 1.0, 1e-18, 200, false, &Check::TURNS)
            .unwrap_err();
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
        let p =
            integrate_breaks(&mut f, &breaks, 1e-7, 1.0, 1e-18, 200, false, &Check::TURNS).unwrap();
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
            let p = integrate_breaks(
                &mut f,
                &[0.0, 0.5, 1.0],
                1e-7,
                1.0,
                1e-18,
                200,
                false,
                &Check::TURNS,
            )
            .unwrap();
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
        let p = integrate_breaks(
            &mut f,
            &breaks,
            1e-12,
            1.0,
            1e-18,
            200,
            false,
            &Check::TURNS,
        )
        .unwrap();
        assert_eq!(p.panels, 4);
        assert!((p.value[0] - (0.045 + 0.245)).abs() <= 1e-15);
        let p =
            integrate_breaks(&mut f, &breaks, 1e-12, 1.0, 1e-18, 200, true, &Check::TURNS).unwrap();
        assert_eq!(p.panels, 2);
    }
}
