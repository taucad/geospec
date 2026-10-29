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
//! arbitrary-precision integers, which cannot overflow. [`volume`] inserts one
//! further fixed-width stage, the stack-only 448-bit [`Stacked`], between the
//! two: its signs dominate the scalar-certificate filter, whose span
//! precondition bounds them below 448 bits while leaving them above the
//! 128-bit stage's reach, so without it that stage overflows on every
//! non-dyadic coordinate set and the whole determinant is recomputed in
//! arbitrary precision. `BigInt` remains the terminal fallback everywhere. No
//! predicate here approximates: each answer is the exact sign, and each stage
//! decides the sign of the same integer polynomial.

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

/// The limb count of [`Stacked`]: `7 * 64 = 448` magnitude bits.
///
/// 448 bits decides every sign on the scalar-certificate path, and the bound
/// is forced by a precondition the engine already enforces. A tetrahedron pair
/// reaches `certificates::separated` only when `certificates` holds
/// (`material_intersection.rs` tetrahedron loop), and that flag is
/// `material::scalar_certificate_eligible` over both operands' whole position
/// arrays, whose last line is `high - low <= 128`, with `low <= 0 <= high`.
/// Every coordinate `separated` reads is one of those positions, so for each
/// of them `decompose` yields `(mantissa, exponent)` with
/// `exponent >= low` and `exponent + bits(mantissa) <= high`, while the frame
/// `least` is a minimum over a subset of the same exponents and so satisfies
/// `least >= low`. Therefore:
///
/// - a scaled coordinate `mantissa << (exponent - least)` has magnitude below
///   `2^(exponent + bits(mantissa) - least) <= 2^(high - low) <= 2^128`;
/// - a `relative` difference of two of them is below `2^129`;
/// - a `cross` component `a*b - c*d` of such differences is below `2^259`;
/// - a `dot` of a difference with such a cross product sums three products
///   each below `2^388`, so it is below `2^390`.
///
/// `scalar_face_sign` sums exactly one triangle's term, so 390 magnitude bits
/// bound the whole accumulator and 448 leaves 58 bits of headroom. `BigInt`
/// stays the terminal fallback because `integer::volume` also serves
/// `material::staged`, which sums a whole shell under no span precondition.
const LIMBS: usize = 7;

/// A fixed 448-bit sign-magnitude integer that becomes `None` on overflow and
/// stays there. Stack-only: every value is one `bool` and `[u64; 7]`, and no
/// operation allocates.
///
/// Overflow detection is exact and never optimistic. `scaled` refuses a shift
/// that could reach the 448th magnitude bit, `add` refuses a carry out of the
/// top limb, and `mul` computes the full `2 * LIMBS` product and refuses
/// unless the upper half is entirely zero. A missed overflow would return a
/// wrong exact sign, so each check rejects on the bound rather than near it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stacked(Option<(bool, [u64; LIMBS])>);

/// `Ordering` of two magnitudes, most significant limb first.
fn magnitude_cmp(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> Ordering {
    for (left, right) in a.iter().rev().zip(b.iter().rev()) {
        match left.cmp(right) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
    }
    Ordering::Equal
}

/// `a + b`, or `None` when the sum carries out of the top limb.
fn magnitude_add(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> Option<[u64; LIMBS]> {
    let mut out = [0u64; LIMBS];
    let mut carry = 0u64;
    for (slot, (left, right)) in out.iter_mut().zip(a.iter().zip(b)) {
        let total = u128::from(*left) + u128::from(*right) + u128::from(carry);
        *slot = total as u64;
        carry = (total >> 64) as u64;
    }
    if carry == 0 {
        Some(out)
    } else {
        None
    }
}

/// `a - b` for `a >= b`, which never borrows out of the top limb.
fn magnitude_sub(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> [u64; LIMBS] {
    let mut out = [0u64; LIMBS];
    let mut borrow = 0u64;
    for (slot, (left, right)) in out.iter_mut().zip(a.iter().zip(b)) {
        let (value, under) = left.overflowing_sub(*right);
        let (value, again) = value.overflowing_sub(borrow);
        *slot = value;
        borrow = u64::from(under) + u64::from(again);
    }
    out
}

/// `a * b`, or `None` when the exact product needs more than `LIMBS` limbs.
fn magnitude_mul(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> Option<[u64; LIMBS]> {
    let mut wide = [0u64; 2 * LIMBS];
    for (i, &left) in a.iter().enumerate() {
        if left == 0 {
            continue;
        }
        let mut carry = 0u128;
        for (j, &right) in b.iter().enumerate() {
            let total = u128::from(left) * u128::from(right) + u128::from(wide[i + j]) + carry;
            wide[i + j] = total as u64;
            carry = total >> 64;
        }
        let mut at = i + LIMBS;
        while carry != 0 && at < 2 * LIMBS {
            let total = u128::from(wide[at]) + carry;
            wide[at] = total as u64;
            carry = total >> 64;
            at += 1;
        }
        // An `n`-limb by `n`-limb product fits in `2n` limbs, so the carry is
        // exhausted before the slice ends; refuse rather than wrap if not.
        if carry != 0 {
            return None;
        }
    }
    if wide[LIMBS..].iter().any(|limb| *limb != 0) {
        return None;
    }
    let mut out = [0u64; LIMBS];
    out.copy_from_slice(&wide[..LIMBS]);
    Some(out)
}

/// `negative` normalised to `false` for zero, so signs compare canonically.
fn signed(negative: bool, magnitude: [u64; LIMBS]) -> (bool, [u64; LIMBS]) {
    let zero = magnitude.iter().all(|limb| *limb == 0);
    (negative && !zero, magnitude)
}

impl Stacked {
    fn combine(&self, other: &Self, flip: bool) -> Self {
        let Some((mut a_negative, a)) = self.0 else {
            return Self(None);
        };
        let Some((b_negative, b)) = other.0 else {
            return Self(None);
        };
        let b_negative = b_negative != flip;
        if a_negative == b_negative {
            return Self(magnitude_add(&a, &b).map(|sum| signed(a_negative, sum)));
        }
        let (larger, smaller) = match magnitude_cmp(&a, &b) {
            Ordering::Less => {
                a_negative = b_negative;
                (b, a)
            }
            _ => (a, b),
        };
        Self(Some(signed(a_negative, magnitude_sub(&larger, &smaller))))
    }
}

impl Ring for Stacked {
    fn scaled(mantissa: i64, shift: u32) -> Self {
        let width = 64 - mantissa.unsigned_abs().leading_zeros();
        // Saturating, so an implausibly large shift refuses instead of
        // wrapping the comparison into the admitted range.
        if width.saturating_add(shift) > (LIMBS as u32) * 64 - 1 {
            return Self(None);
        }
        let mut limbs = [0u64; LIMBS];
        let magnitude = u128::from(mantissa.unsigned_abs()) << (shift % 64);
        let at = (shift / 64) as usize;
        let high = (magnitude >> 64) as u64;
        if at >= LIMBS || (high != 0 && at + 1 >= LIMBS) {
            // Unreachable under the width check above; refuse rather than
            // drop limbs if it ever becomes reachable.
            return Self(None);
        }
        limbs[at] = magnitude as u64;
        if at + 1 < LIMBS {
            limbs[at + 1] = high;
        }
        Self(Some(signed(mantissa < 0, limbs)))
    }
    fn small(value: i64) -> Self {
        let mut limbs = [0u64; LIMBS];
        limbs[0] = value.unsigned_abs();
        Self(Some(signed(value < 0, limbs)))
    }
    fn add(&self, other: &Self) -> Self {
        self.combine(other, false)
    }
    fn sub(&self, other: &Self) -> Self {
        self.combine(other, true)
    }
    fn mul(&self, other: &Self) -> Self {
        let (Some((a_negative, a)), Some((b_negative, b))) = (self.0, other.0) else {
            return Self(None);
        };
        Self(magnitude_mul(&a, &b).map(|product| signed(a_negative != b_negative, product)))
    }
    fn sign(&self) -> Option<Ordering> {
        self.0.map(|(negative, magnitude)| {
            if magnitude.iter().all(|limb| *limb == 0) {
                Ordering::Equal
            } else if negative {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
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
        .or_else(|| volume_in::<Stacked>(&mut triangles(), origin, least))
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

    /// `Stacked`'s magnitude as a `BigInt`, for checking it limb by limb.
    fn stacked_value(value: &Stacked) -> Option<BigInt> {
        let (negative, limbs) = value.0?;
        let mut magnitude = BigInt::zero();
        for limb in limbs.iter().rev() {
            magnitude = (magnitude << 64) + BigInt::from(*limb);
        }
        Some(if negative { -magnitude } else { magnitude })
    }

    #[test]
    fn stacked_arithmetic_matches_arbitrary_precision_until_it_refuses() {
        let mut state = 0x5eed_1234u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            state
        };
        let mut refused = 0;
        let mut agreed = 0;
        for _ in 0..4000 {
            let build = |mantissa: i64, shift: u32| {
                (
                    Stacked::scaled(mantissa, shift),
                    <BigInt as Ring>::scaled(mantissa, shift),
                )
            };
            // Shifts reach past 448 bits so both the agreeing and the
            // refusing side of every check are exercised.
            let ma = (next() as i64) | 1;
            let mb = (next() as i64) | 1;
            let (sa, sb) = ((next() % 420) as u32, (next() % 420) as u32);
            let (fa, ba) = build(ma, sa);
            let (fb, bb) = build(mb, sb);
            for (fixed, exact) in [
                (fa.add(&fb), <BigInt as Ring>::add(&ba, &bb)),
                (fa.sub(&fb), <BigInt as Ring>::sub(&ba, &bb)),
                (fa.mul(&fb), <BigInt as Ring>::mul(&ba, &bb)),
                (fa.mul(&fb).sub(&fb.mul(&fa)), BigInt::zero()),
            ] {
                match stacked_value(&fixed) {
                    None => {
                        assert_eq!(fixed.sign(), None, "a refusal has no sign");
                        refused += 1;
                    }
                    Some(value) => {
                        assert_eq!(value, exact, "fixed width disagrees with exact");
                        assert_eq!(fixed.sign(), Ring::sign(&exact));
                        agreed += 1;
                    }
                }
            }
        }
        assert!(agreed > 1000, "agreed on {agreed}");
        assert!(refused > 1000, "refused on {refused}");
    }

    #[test]
    fn stacked_overflow_is_detected_at_its_boundary() {
        // 447 magnitude bits are the widest `scaled` admits, and the 448th is
        // refused, so no value can reach the top limb's sign-free capacity.
        assert_eq!(
            stacked_value(&Stacked::scaled(1, 446)),
            Some(BigInt::from(1) << 446)
        );
        assert_eq!(Stacked::scaled(1, 447).sign(), None);
        assert_eq!(Stacked::scaled(3, 446).sign(), None, "width 2 plus 446");
        assert_eq!(
            stacked_value(&Stacked::scaled(3, 445)),
            Some(BigInt::from(3) << 445)
        );
        // An addition that carries out of the top limb refuses; one bit less
        // does not.
        let big = Stacked::scaled(1, 446);
        let under = big.add(&big).add(&big);
        assert_eq!(stacked_value(&under), Some(BigInt::from(3) << 446));
        assert_eq!(under.add(&big).sign(), None, "4 * 2^446 needs 449 bits");
        // The product boundary: 2^223 * 2^223 fits, one more bit does not.
        let half = Stacked::scaled(1, 223);
        assert_eq!(
            stacked_value(&half.mul(&half)),
            Some(BigInt::from(1) << 446)
        );
        assert_eq!(half.mul(&half).mul(&Stacked::small(4)).sign(), None);
        // A refusal is sticky, as `Wide`'s is.
        let dead = Stacked::scaled(1, 447);
        for alive in [Stacked::small(0), Stacked::small(1)] {
            assert_eq!(dead.add(&alive).sign(), None);
            assert_eq!(alive.sub(&dead).sign(), None);
            assert_eq!(alive.mul(&dead).sign(), None);
        }
        // Zero is signless in either direction and never negative.
        assert_eq!(Stacked::small(0).sign(), Some(Ordering::Equal));
        assert_eq!(
            Stacked::small(-5).add(&Stacked::small(5)).sign(),
            Some(Ordering::Equal)
        );
        assert_eq!(Stacked::small(i64::MIN).sign(), Some(Ordering::Less));
        assert_eq!(
            stacked_value(&Stacked::small(i64::MIN)),
            Some(BigInt::from(i64::MIN))
        );
    }

    #[test]
    fn the_three_volume_stages_agree_on_eligible_and_ineligible_spans() {
        let mut state = 0x9e37_79b9u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) % 17
        };
        let mut stacked_decided = 0;
        let mut stacked_refused = 0;
        let mut wide_decided = 0;
        // The first three scales are the three the certificate corpus already
        // uses, and all are inside the eligible span `high - low <= 128`. The
        // fourth mixes `2^-1000` into a `1e300` frame, which is one span far
        // outside it, where 448 bits must refuse and `BigInt` must decide.
        for (scale, eligible) in [
            (1., true),
            (2f64.powi(45), true),
            (2f64.powi(-40), true),
            (1e300, false),
        ] {
            for case in 0..96 {
                let mut face: [[f64; 3]; 3] =
                    std::array::from_fn(|_| std::array::from_fn(|_| next() as f64 / 7. * scale));
                let point: [f64; 3] = std::array::from_fn(|_| next() as f64 / 7. * scale);
                if !eligible {
                    face[0][0] = 2f64.powi(-1000);
                }
                assert_eq!(
                    super::super::scalar_certificate_eligible(face.iter().chain([&point])),
                    eligible,
                    "case {case} at {scale}"
                );
                let least = least(face.iter().flatten().chain(&point));
                let shell = || std::iter::once(face);
                let exact = volume_in::<BigInt>(&mut shell(), &point, least).unwrap();
                match volume_in::<Stacked>(&mut shell(), &point, least) {
                    Some(sign) => {
                        assert_eq!(sign, exact, "448-bit sign differs at {scale}");
                        stacked_decided += 1;
                    }
                    None => stacked_refused += 1,
                }
                if let Some(sign) = volume_in::<Wide>(&mut shell(), &point, least) {
                    assert_eq!(sign, exact, "128-bit sign differs at {scale}");
                    wide_decided += 1;
                }
                // `volume` is the shipped composition of all three stages.
                assert_eq!(volume(shell, &point, least), exact);
            }
        }
        // Every eligible span is decided in 448 bits, which is the bound this
        // stage exists to exploit; the ineligible span exercises `BigInt`.
        assert_eq!(stacked_decided, 96 * 3, "{stacked_decided} decided");
        assert_eq!(stacked_refused, 96, "{stacked_refused} refused");
        // 128 bits hold 1 of these 384 non-dyadic frames, which is the whole
        // reason the 448-bit stage earns its place: without it, 383 of 384
        // throw an `i128` determinant away and recompute it in `BigInt`.
        assert_eq!(wide_decided, 1, "128 bits decided {wide_decided}");
    }
}
