//! Exact integer sign predicates over binary64 coordinates.
//!
//! A finite binary64 value is `m * 2^e` for an odd integer `m`, or zero.
//! Multiplying every coordinate a predicate reads by `2^-E`, where `E` is the
//! least of their exponents, makes each of them an integer, and subtracting
//! one of them as the origin keeps them integers. Each predicate below is the
//! sign of a polynomial that is homogeneous in coordinate differences, so the
//! positive scale and the translation leave its sign unchanged, and integer
//! arithmetic decides it exactly with no rationals and no rounding.
//!
//! Every predicate runs first in 128-bit integers with checked arithmetic.
//! An overflow reports `None`, and the same predicate then runs in
//! arbitrary-precision integers, which cannot overflow. No predicate here
//! approximates: each answer is the exact sign.

use std::{array::from_fn, cmp::Ordering};

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

/// One integer arithmetic for the predicates: fixed width with overflow
/// detection, or arbitrary precision.
pub(super) trait Ring: Clone {
    /// `mantissa * 2^shift`.
    fn scaled(mantissa: i64, shift: u32) -> Self;
    fn small(value: i64) -> Self;
    fn add(&self, other: &Self) -> Self;
    fn sub(&self, other: &Self) -> Self;
    fn mul(&self, other: &Self) -> Self;
    /// The exact sign, or `None` after a fixed-width overflow.
    fn sign(&self) -> Option<Ordering>;
}

/// A 128-bit integer that becomes `None` on overflow and stays there.
#[derive(Clone, Copy, Debug)]
pub(super) struct Wide(Option<i128>);

impl Ring for Wide {
    fn scaled(mantissa: i64, shift: u32) -> Self {
        let width = 64 - mantissa.unsigned_abs().leading_zeros();
        if width + shift > 126 {
            Self(None)
        } else {
            Self(Some(i128::from(mantissa) << shift))
        }
    }
    fn small(value: i64) -> Self {
        Self(Some(i128::from(value)))
    }
    fn add(&self, other: &Self) -> Self {
        Self(self.0.zip(other.0).and_then(|(a, b)| a.checked_add(b)))
    }
    fn sub(&self, other: &Self) -> Self {
        Self(self.0.zip(other.0).and_then(|(a, b)| a.checked_sub(b)))
    }
    fn mul(&self, other: &Self) -> Self {
        Self(self.0.zip(other.0).and_then(|(a, b)| a.checked_mul(b)))
    }
    fn sign(&self) -> Option<Ordering> {
        self.0.map(|value| value.cmp(&0))
    }
}

impl Ring for BigInt {
    fn scaled(mantissa: i64, shift: u32) -> Self {
        BigInt::from(mantissa) << shift
    }
    fn small(value: i64) -> Self {
        BigInt::from(value)
    }
    fn add(&self, other: &Self) -> Self {
        self + other
    }
    fn sub(&self, other: &Self) -> Self {
        self - other
    }
    fn mul(&self, other: &Self) -> Self {
        self * other
    }
    fn sign(&self) -> Option<Ordering> {
        Some(if self.is_zero() {
            Ordering::Equal
        } else if self.is_negative() {
            Ordering::Less
        } else {
            Ordering::Greater
        })
    }
}

/// `value = mantissa * 2^exponent` with an odd mantissa, or `None` for zero
/// of either sign. Non-finite values never reach here.
pub(super) fn decompose(value: f64) -> Option<(i64, i32)> {
    let bits = value.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if field == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), field - 1075)
    };
    if mantissa == 0 {
        return None;
    }
    let zeros = mantissa.trailing_zeros();
    let magnitude = (mantissa >> zeros) as i64;
    let signed = if bits >> 63 == 1 {
        -magnitude
    } else {
        magnitude
    };
    Some((signed, exponent + zeros as i32))
}

/// The least exponent among `values`, for a frame in which all are integers.
pub(super) fn least<'a>(values: impl IntoIterator<Item = &'a f64>) -> i32 {
    values
        .into_iter()
        .filter_map(|value| decompose(*value))
        .map(|(_, exponent)| exponent)
        .min()
        .unwrap_or(0)
}

/// `value * 2^-least` as an integer; `least` is at most its exponent.
fn integer<R: Ring>(value: f64, least: i32) -> R {
    match decompose(value) {
        None => R::small(0),
        Some((mantissa, exponent)) => R::scaled(mantissa, (exponent - least) as u32),
    }
}

pub(super) type Vector<R> = [R; 3];

/// `(point - origin) * 2^-least` as integers.
pub(super) fn relative<R: Ring>(point: &[f64; 3], origin: &[f64; 3], least: i32) -> Vector<R> {
    from_fn(|axis| integer::<R>(point[axis], least).sub(&integer::<R>(origin[axis], least)))
}

pub(super) fn sub<R: Ring>(a: &Vector<R>, b: &Vector<R>) -> Vector<R> {
    from_fn(|axis| a[axis].sub(&b[axis]))
}

pub(super) fn dot<R: Ring>(a: &Vector<R>, b: &Vector<R>) -> R {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

pub(super) fn cross<R: Ring>(a: &Vector<R>, b: &Vector<R>) -> Vector<R> {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

/// `Some(true)` when every component is zero.
fn is_zero<R: Ring>(a: &Vector<R>) -> Option<bool> {
    let mut zero = true;
    for value in a {
        zero &= value.sign()? == Ordering::Equal;
    }
    Some(zero)
}

fn compare<R: Ring>(a: &R, b: &R) -> Option<Ordering> {
    a.sub(b).sign()
}

/// Runs a predicate in 128-bit integers, then exactly on overflow.
macro_rules! exactly {
    ($predicate:ident ( $($argument:expr),* $(,)? )) => {
        $predicate::<Wide>($($argument),*)
            .or_else(|| $predicate::<BigInt>($($argument),*))
            .expect("arbitrary-precision integers do not overflow")
    };
}

/// The triangle's normal `(b - a) x (c - a)` in the frame of its vertices.
fn normal<R: Ring>(t: &[[f64; 3]; 3], origin: &[f64; 3], least: i32) -> Vector<R> {
    let a = relative::<R>(&t[0], origin, least);
    let b = relative::<R>(&t[1], origin, least);
    let c = relative::<R>(&t[2], origin, least);
    cross(&sub(&b, &a), &sub(&c, &a))
}

fn degenerate_in<R: Ring>(t: &[[f64; 3]; 3]) -> Option<bool> {
    let least = least(t.iter().flatten());
    is_zero(&normal::<R>(t, &t[0], least))
}

/// The exact `norm(normal(t)) == 0` of the rational path.
pub(super) fn degenerate(t: &[[f64; 3]; 3]) -> bool {
    exactly!(degenerate_in(t))
}

fn folded_in<R: Ring>(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> Option<bool> {
    let least = least(a.iter().chain(b).flatten());
    let na = normal::<R>(a, &a[0], least);
    let nb = normal::<R>(b, &a[0], least);
    if !is_zero(&cross(&na, &nb))? {
        return Some(false);
    }
    Some(dot(&na, &nb).sign()? != Ordering::Greater)
}

/// For two triangles sharing an edge: their normals are parallel and not
/// positively aligned, so they overlap in one plane. This is the exact
/// `norm(na x nb) == 0 && na . nb <= 0` of the rational path.
pub(super) fn folded(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> bool {
    exactly!(folded_in(a, b))
}

/// The candidate separating directions of two triangles: both normals, the
/// in-plane normals of every edge and every edge-edge cross product. These
/// include a strictly separating direction for every disjoint pair.
fn axis<R: Ring>(index: usize, normals: &[Vector<R>; 2], edges: &[[Vector<R>; 3]; 2]) -> Vector<R> {
    match index {
        0 | 1 => normals[index].clone(),
        2..=7 => {
            let side = (index - 2) / 3;
            cross(&normals[side], &edges[side][(index - 2) % 3])
        }
        _ => cross(&edges[0][(index - 8) / 3], &edges[1][(index - 8) % 3]),
    }
}

fn apart_in<R: Ring>(
    a: &[[f64; 3]; 3],
    b: &[[f64; 3]; 3],
    shared: Option<(usize, usize)>,
) -> Option<bool> {
    let origin = shared.map_or(a[0], |(index, _)| a[index]);
    let least = least(a.iter().chain(b).flatten());
    let pa: [Vector<R>; 3] = from_fn(|i| relative::<R>(&a[i], &origin, least));
    let pb: [Vector<R>; 3] = from_fn(|i| relative::<R>(&b[i], &origin, least));
    let edges = [
        from_fn(|i| sub(&pa[(i + 1) % 3], &pa[i])),
        from_fn(|i| sub(&pb[(i + 1) % 3], &pb[i])),
    ];
    let normals = [
        cross(&edges[0][0], &sub(&pa[2], &pa[0])),
        cross(&edges[1][0], &sub(&pb[2], &pb[0])),
    ];
    for index in 0..17 {
        let w = axis(index, &normals, &edges);
        if is_zero(&w)? {
            continue;
        }
        let sa: [R; 3] = from_fn(|i| dot(&w, &pa[i]));
        let sb: [R; 3] = from_fn(|i| dot(&w, &pb[i]));
        match shared {
            None => {
                // A strict gap along `w` separates the closed triangles.
                for (left, right) in [(&sa, &sb), (&sb, &sa)] {
                    let mut separated = true;
                    for l in left {
                        for r in right {
                            separated &= compare(l, r)? == Ordering::Less;
                        }
                    }
                    if separated {
                        return Some(true);
                    }
                }
            }
            Some((ia, ib)) => {
                // The shared vertex is the origin, so it projects to zero.
                let others = |s: &[R; 3], skip: usize| -> Option<[(usize, Ordering); 2]> {
                    let mut out = [(0, Ordering::Equal); 2];
                    let mut slot = 0;
                    for (i, value) in s.iter().enumerate() {
                        if i != skip {
                            out[slot] = (i, value.sign()?);
                            slot += 1;
                        }
                    }
                    Some(out)
                };
                let oa = others(&sa, ia)?;
                let ob = others(&sb, ib)?;
                for side in [Ordering::Less, Ordering::Greater] {
                    let weak = |o: &[(usize, Ordering); 2], toward: Ordering| {
                        o.iter().all(|&(_, s)| s == toward || s == Ordering::Equal)
                    };
                    if !weak(&oa, side) || !weak(&ob, side.reverse()) {
                        continue;
                    }
                    // Both lie in closed opposite half-spaces, so they meet
                    // only where each touches the plane: the shared vertex,
                    // or an edge from it.
                    let touch = |o: &[(usize, Ordering); 2]| -> Vec<usize> {
                        o.iter()
                            .filter(|&&(_, s)| s == Ordering::Equal)
                            .map(|&(i, _)| i)
                            .collect()
                    };
                    let (ta, tb) = (touch(&oa), touch(&ob));
                    if ta.is_empty() || tb.is_empty() {
                        return Some(true);
                    }
                    if ta.len() == 1 && tb.len() == 1 {
                        // Two segments from the shared vertex meet only there
                        // unless they lie along one ray.
                        let (u, v) = (&pa[ta[0]], &pb[tb[0]]);
                        if !is_zero(&cross(u, v))? || dot(u, v).sign()? == Ordering::Less {
                            return Some(true);
                        }
                    }
                }
            }
        }
    }
    Some(false)
}

/// A certificate that the closed triangles meet nowhere (no `shared`
/// vertex), or only at the shared vertex `a[i] == b[j]`. `false` means only
/// that no certificate was found; the exact rational test then decides.
pub(super) fn apart(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3], shared: Option<(usize, usize)>) -> bool {
    exactly!(apart_in(a, b, shared))
}

fn volume_in<R: Ring>(
    triangles: &mut dyn Iterator<Item = [[f64; 3]; 3]>,
    origin: &[f64; 3],
    least: i32,
) -> Option<Ordering> {
    let mut sum = R::small(0);
    for t in triangles {
        let p: [Vector<R>; 3] = from_fn(|i| relative::<R>(&t[i], origin, least));
        sum = sum.add(&dot(&p[0], &cross(&p[1], &p[2])));
        sum.sign()?;
    }
    sum.sign()
}

/// The sign of a closed shell's oriented volume. On a closed oriented
/// surface the sum of `t0 . (t1 x t2)` does not depend on the origin, since
/// the terms an origin shift adds cancel across each edge's two opposite
/// uses; so `origin` may be any point.
pub(super) fn volume<I>(triangles: impl Fn() -> I, origin: &[f64; 3], least: i32) -> Ordering
where
    I: Iterator<Item = [[f64; 3]; 3]>,
{
    volume_in::<Wide>(&mut triangles(), origin, least)
        .or_else(|| volume_in::<BigInt>(&mut triangles(), origin, least))
        .expect("arbitrary-precision integers do not overflow")
}

/// The positive integers `k <= limit` for which the ray direction
/// `(1, k, k^2)` is parallel to the plane of `t`: the integer roots of
/// `n0 + n1 k + n2 k^2`, for the triangle's normal `n`. The normal is
/// nonzero, so there are at most two.
pub(super) fn parallel_directions(t: &[[f64; 3]; 3], limit: u64) -> Vec<u64> {
    use num_integer::Integer;
    let least = least(t.iter().flatten());
    let [n0, n1, n2] = normal::<BigInt>(t, &t[0], least);
    let mut roots = Vec::new();
    let mut keep = |numerator: BigInt, denominator: &BigInt| {
        if denominator.is_zero() {
            return;
        }
        let (k, remainder) = numerator.div_rem(denominator);
        if remainder.is_zero() && k.is_positive() {
            if let Some(k) = num_traits::ToPrimitive::to_u64(&k) {
                if k <= limit {
                    roots.push(k);
                }
            }
        }
    };
    if n2.is_zero() {
        keep(-n0, &n1);
    } else {
        let discriminant = &n1 * &n1 - BigInt::from(4) * &n2 * &n0;
        if !discriminant.is_negative() {
            let root = discriminant.sqrt();
            if &root * &root == discriminant {
                let twice = BigInt::from(2) * &n2;
                keep(-&n1 + &root, &twice);
                keep(-&n1 - &root, &twice);
            }
        }
    }
    roots
}

fn misses_in<R: Ring>(p: &[f64; 3], low: &[f64; 3], high: &[f64; 3], k: i64) -> Option<bool> {
    let least = least(p.iter().chain(low).chain(high));
    let lo = relative::<R>(low, p, least);
    let hi = relative::<R>(high, p, least);
    let k = R::small(k);
    let scale = [k.mul(&k), k, R::small(1)];
    // The ray p + s (1, k, k^2), s > 0, meets the box at some s when the
    // slab ranges of T = k^2 s intersect at a positive T.
    let mut entry = scale[0].mul(&lo[0]);
    let mut exit = scale[0].mul(&hi[0]);
    for axis in 1..3 {
        let (l, h) = (scale[axis].mul(&lo[axis]), scale[axis].mul(&hi[axis]));
        if compare(&l, &entry)? == Ordering::Greater {
            entry = l;
        }
        if compare(&h, &exit)? == Ordering::Less {
            exit = h;
        }
    }
    Some(exit.sign()? != Ordering::Greater || compare(&entry, &exit)? == Ordering::Greater)
}

/// The ray from `p` along `(1, k, k^2)` misses the closed box `[low, high]`
/// at every positive parameter, so it meets nothing inside the box there.
pub(super) fn misses(p: &[f64; 3], low: &[f64; 3], high: &[f64; 3], k: i64) -> bool {
    exactly!(misses_in(p, low, high, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposition_is_exact_for_every_binary64_class() {
        for value in [
            1.0,
            -1.0,
            0.1,
            -3.75,
            f64::MIN_POSITIVE,
            5e-324,
            -5e-324,
            f64::MAX,
            f64::from(0.1f32),
        ] {
            let (mantissa, exponent) = decompose(value).unwrap();
            assert!(mantissa % 2 != 0);
            let rebuilt = num_rational::BigRational::from_integer(mantissa.into())
                * num_rational::BigRational::from_float(2f64)
                    .unwrap()
                    .pow(exponent);
            assert_eq!(
                rebuilt,
                num_rational::BigRational::from_float(value).unwrap()
            );
        }
        assert_eq!(decompose(0.0), None);
        assert_eq!(decompose(-0.0), None);
    }

    #[test]
    fn parallel_directions_are_the_exact_integer_roots() {
        // Normal (2, -3, 1): 2 - 3k + k^2 = (k - 1)(k - 2).
        let t = [[0.0; 3], [1.0, 1.0, 1.0], [0.5, 0.0, -1.0]];
        let normal = normal::<BigInt>(&t, &t[0], least(t.iter().flatten()));
        let roots = parallel_directions(&t, 100);
        for k in 1..=100i64 {
            let along = &normal[0] + &normal[1] * k + &normal[2] * k * k;
            assert_eq!(along.is_zero(), roots.contains(&(k as u64)), "k = {k}");
        }
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn wide_overflow_falls_back_to_the_same_exact_sign() {
        let a = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1e-30, 0.0]];
        let b = [[0.0; 3], [1e30, 0.0, 0.0], [0.0, 1.0, 1e-30]];
        let small = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.5, 0.0]];
        assert_eq!(degenerate_in::<Wide>(&small), Some(false));
        assert_eq!(
            folded_in::<Wide>(&a, &b),
            None,
            "the frame spans more than 128 bits"
        );
        assert_eq!(folded(&a, &b), folded_in::<BigInt>(&a, &b).unwrap());
    }
}
