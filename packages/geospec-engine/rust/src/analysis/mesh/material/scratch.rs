//! Upper bounds on the heap bytes material classification allocates, computed
//! from the record before the containers they bound exist.
//!
//! Every bound is a sum of `size_of` of the real element types times a slot
//! count that the construction in `classify` cannot exceed. Three growth rules
//! cover all of them:
//! - an exact-size collect or `with_capacity` holds exactly `len` slots;
//! - a vector grown by `push` or `extend` to `len` holds at most `3 * len + 4`
//!   slots at any instant: at most `2 * len` after its last doubling (four is
//!   the first nonzero capacity) plus the `len` it moved out of during that
//!   doubling, counted as if the allocator could never grow in place;
//! - a `BTreeMap` that is only inserted into keeps every non-root node at five
//!   or more of its eleven slots (std splits a full node 5|6 or 6|5), so `n`
//!   entries use at most `n / 5 + 1` nodes, at most a sixth of them internal.
//!
//! The counting allocator test in `material.rs` measures the peak live heap of
//! whole classifications against these bounds.

use std::mem::size_of;

use super::super::{Aabb, BoxNode, MeshAnalysisRecord};
use super::{
    staged::{Pair, Shell as StagedShell},
    Point, Shell,
};

/// One `num-bigint` limb in this accounting: `u64` on 64-bit targets. On
/// 32-bit targets (`wasm32`) limbs are `u32` and the same value needs at most
/// `2 * k + 1` of them where it needs `k` 64-bit limbs; each bound below that
/// is written in 64-bit limbs also bounds that 32-bit layout.
const LIMB: u64 = 8;

/// Transient predicate working set, unchanged from the earlier reservation:
/// exact scalars have an 8192-bit arithmetic guard, so the bounded feature
/// predicates, their triangle clones, comparison and recursion temporaries and
/// the broad-phase traversal stack fit in this fixed allowance.
pub(super) const PREDICATE_BYTES: u64 = 640 * 1024;

/// Heap limbs one coordinate holds: as `BigRational::from_float` leaves it,
/// and as an exact clone of that reduced value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CoordinateLimbs {
    pub(super) built: u64,
    pub(super) clone: u64,
}

/// `from_float` in `num-rational` 0.4.2 over `num-bigint` 0.4.6 (both pinned)
/// decodes `value` as `m * 2^e` with `num_traits::float::integer_decode`.
/// - The numerator starts as `BigUint::from(m)`: one push into an empty
///   vector, so four limbs, or none for zero. For `e >= 0` it is shifted by
///   `e`; a shift of whole limbs `d > 0` reallocates `d + 2` limbs.
/// - For `e < 0` the denominator is `1 << -e`: one limb for `-e < 64`, else
///   `d + 2` limbs for `d = -e / 64`. For `e >= 0` it is `One`, one limb.
/// - Reduction divides by a power of two, in place or into a vector no longer
///   than the dividend, and zero keeps its denominator vector via `set_one`.
///
/// So the built capacities are at most the values below. A clone allocates
/// exactly the normalized length of the reduced value. With 32-bit limbs a
/// built numerator is at most four limbs, and shifted or power-of-two
/// integers of `d` whole 64-bit limbs take at most `2 * d + 3` 32-bit limbs,
/// which fit in `d + 2` 64-bit limbs of bytes.
pub(super) fn coordinate_limbs(value: f64) -> CoordinateLimbs {
    let bits = value.to_bits();
    let field = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1 << 52) - 1);
    let mantissa = if field == 0 {
        fraction << 1
    } else {
        fraction | (1 << 52)
    };
    let exponent = field as i64 - 1075;
    let whole = |shift: i64| -> u64 {
        let digits = (shift / 64) as u64;
        if digits == 0 {
            0
        } else {
            digits + 2
        }
    };
    let (numerator, denominator) = if exponent >= 0 {
        (4.max(whole(exponent)), 1)
    } else {
        (if mantissa == 0 { 0 } else { 4 }, 1.max(whole(-exponent)))
    };
    let limbs = |significant: u64| significant.div_ceil(64);
    let clone = if mantissa == 0 {
        1
    } else {
        let odd = mantissa >> mantissa.trailing_zeros();
        let reduced = exponent + i64::from(mantissa.trailing_zeros());
        let width = u64::from(64 - odd.leading_zeros());
        if reduced >= 0 {
            limbs(width + reduced as u64) + 1
        } else {
            limbs(width) + limbs(reduced.unsigned_abs() + 1)
        }
    };
    CoordinateLimbs {
        built: numerator + denominator,
        clone,
    }
}

/// Bytes of a `BTreeMap<K, V>` holding at most `entries` entries. A node is
/// its parent link, index and length (at most two words), eleven key and
/// value slots and at most one word of alignment padding; internal nodes add
/// twelve child links.
pub(super) fn btree_bytes(entries: u64, key: usize, value: usize) -> u64 {
    let word = size_of::<usize>() as u64;
    let leaf = 3 * word + 11 * (key as u64 + value as u64);
    let nodes = entries / 5 + 1;
    let internal = nodes / 6 + 1;
    nodes
        .saturating_mul(leaf)
        .saturating_add(internal.saturating_mul(12 * word))
}

/// Slots a pushed vector of final length `len` can hold at once.
fn grown(len: u64) -> u64 {
    len.saturating_mul(3).saturating_add(4)
}

/// Nodes `BoxTree::new` builds over `triangles` boxes: leaves hold at most
/// eight items, and a split range of nine or more halves into ranges of four
/// or more, so there are at most `triangles / 4` leaves and fewer than twice
/// as many nodes.
fn box_tree_nodes(triangles: u64) -> u64 {
    triangles / 2 + 1
}

/// One exact edge use: `(low vertex, high vertex, primitive, triangle, direction)`.
pub(super) type EdgeUse = (usize, usize, u32, usize, bool);

/// The scratch `classify` holds before shells are formed, with the inputs it
/// was derived from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Admission {
    pub(super) bytes: u64,
    /// Largest exact clone of one coordinate, for the per-shell stage.
    pub(super) clone_bytes: u64,
}

/// Points with their built limbs, the canonical-order permutation and vertex
/// map, union-find parents, triangle boxes, the broad-phase tree, the sorted
/// edge uses, the sorted shell memberships and the predicate working set.
/// Everything here stays allocated until `classify` returns.
pub(super) fn admission(record: &MeshAnalysisRecord) -> Admission {
    let positions = record.positions.len() as u64;
    let triangles = record.triangles.len() as u64;
    let word = size_of::<usize>() as u64;
    let mut limbs = 0u64;
    let mut clone = 0u64;
    for coordinate in record.positions.iter().flatten() {
        let value = coordinate_limbs(*coordinate);
        limbs = limbs.saturating_add(value.built);
        clone = clone.max(value.clone);
    }
    let bytes = positions
        .saturating_mul(size_of::<Point>() as u64)
        .saturating_add(limbs.saturating_mul(LIMB))
        // Canonical order permutation and the vertex map.
        .saturating_add(positions.saturating_mul(2 * word))
        // Union-find parents and triangle boxes.
        .saturating_add(triangles.saturating_mul(word + size_of::<Aabb>() as u64))
        // Tree items, then nodes pushed one by one.
        .saturating_add(triangles.saturating_mul(word))
        .saturating_add(
            grown(box_tree_nodes(triangles)).saturating_mul(size_of::<BoxNode>() as u64),
        )
        // Three edge uses per triangle, with capacity reserved up front.
        .saturating_add(triangles.saturating_mul(3 * size_of::<EdgeUse>() as u64))
        // One (root, triangle) membership per triangle.
        .saturating_add(triangles.saturating_mul(2 * word))
        .saturating_add(PREDICATE_BYTES);
    Admission {
        bytes,
        clone_bytes: clone.saturating_mul(LIMB),
    }
}

/// The scratch `classify` adds once it knows `shells` shells, the largest
/// with `largest` triangles, out of `primitives` primitives:
/// - the shell table and its six retained exact bound clones per shell, and
///   every shell's triangle list (each triangle in exactly one);
/// - the larger of one shell's vertex-link uses and connectivity forest, and
///   one triangle's broad-phase candidate list (the links are dropped first);
/// - nesting, union-find and per-child container lists and maps;
/// - the orientation, body and group outputs, each body and group list
///   extended shell by shell and each map holding at most one entry a shell.
///
/// The `shells * shells` pair matrices are checked on their own after the
/// shells are qualified.
pub(super) fn shells(
    admission: &Admission,
    triangles: u64,
    shells: u64,
    largest: u64,
    primitives: u64,
) -> u64 {
    let word = size_of::<usize>() as u64;
    let list = size_of::<Vec<usize>>();
    let table = shells
        .saturating_mul(size_of::<Shell>() as u64)
        .saturating_add(
            shells
                .saturating_mul(6)
                .saturating_mul(admission.clone_bytes),
        )
        .saturating_add(triangles.saturating_mul(word));
    let links = largest
        .saturating_mul(3 * size_of::<(usize, usize)>() as u64)
        .saturating_add(largest.saturating_mul(word));
    let candidates = grown(triangles).saturating_mul(word);
    let nesting = shells
        .saturating_mul(size_of::<Option<usize>>() as u64 + word)
        .saturating_add(2 * grown(shells).saturating_mul(word))
        .saturating_add(btree_bytes(
            shells.min(primitives),
            size_of::<u32>(),
            size_of::<(usize, usize)>(),
        ));
    // Each of bodies and groups: map nodes, lists extended to at most
    // `2 * len + 4` slots each plus one in-flight move of at most `triangles`,
    // and the collected outer vector.
    let output = btree_bytes(shells, size_of::<usize>(), list)
        .saturating_add(
            triangles
                .saturating_mul(3)
                .saturating_add(shells.saturating_mul(4))
                .saturating_mul(word),
        )
        .saturating_add(shells.saturating_mul(list as u64));
    admission
        .bytes
        .saturating_add(table)
        .saturating_add(links.max(candidates))
        .saturating_add(nesting)
        .saturating_add(triangles)
        .saturating_add(output.saturating_mul(2))
}

/// The three `shells * shells` boolean matrices and their row headers.
pub(super) fn pairs(shells: u64) -> u64 {
    let rows = shells.saturating_mul(size_of::<Vec<bool>>() as u64);
    shells
        .saturating_pow(2)
        .saturating_add(rows)
        .saturating_add(size_of::<Vec<Vec<bool>>>() as u64)
        .saturating_mul(3)
}

/// The scratch the staged path holds before shells are formed: the vertex
/// order (dropped once canonical vertices exist) and vertex map, union-find
/// parents, triangle boxes, the broad-phase tree, the sorted edge uses, the
/// sorted shell memberships and the predicate working set. It builds no
/// retained coordinate: each rational triangle a decisive predicate needs is
/// built for that predicate within the working set, as are the
/// arbitrary-precision integers of an overflowed integer predicate, whose
/// operands the coordinate span bounds.
pub(super) fn staged_admission(record: &MeshAnalysisRecord) -> u64 {
    let positions = record.positions.len() as u64;
    let triangles = record.triangles.len() as u64;
    let word = size_of::<usize>() as u64;
    positions
        .saturating_mul(2 * word)
        .saturating_add(triangles.saturating_mul(word + size_of::<Aabb>() as u64))
        .saturating_add(triangles.saturating_mul(word))
        .saturating_add(
            grown(box_tree_nodes(triangles)).saturating_mul(size_of::<BoxNode>() as u64),
        )
        .saturating_add(triangles.saturating_mul(3 * size_of::<EdgeUse>() as u64))
        .saturating_add(triangles.saturating_mul(2 * word))
        .saturating_add(PREDICATE_BYTES)
}

/// The scratch the staged path adds once it knows `shells` shells, the
/// largest with `largest` triangles, out of `primitives` primitives:
/// - the shell table and every shell's triangle list;
/// - the larger of one shell's vertex-link uses with one vertex's far-vertex
///   uses and link forest (a triangle is incident to a vertex once, so a
///   vertex has at most `largest` uses), and one broad-phase candidate list;
/// - nesting, union-find and per-child container lists and maps;
/// - the orientation, body and group outputs, as in the rational path.
///
/// Shell pairs are checked on their own once the sweep has counted them.
pub(super) fn staged_shells(
    admission: u64,
    triangles: u64,
    shells: u64,
    largest: u64,
    primitives: u64,
) -> u64 {
    let word = size_of::<usize>() as u64;
    let list = size_of::<Vec<usize>>();
    let table = shells
        .saturating_mul(size_of::<StagedShell>() as u64)
        .saturating_add(triangles.saturating_mul(word));
    let use_bytes = size_of::<(usize, usize)>() as u64;
    let links = largest
        .saturating_mul(3 * use_bytes)
        .saturating_add(largest.saturating_mul(2 * use_bytes + word));
    let candidates = grown(triangles).saturating_mul(word);
    let nesting = shells
        .saturating_mul(size_of::<Option<usize>>() as u64 + word)
        .saturating_add(2 * grown(shells).saturating_mul(word))
        .saturating_add(btree_bytes(
            shells.min(primitives),
            size_of::<u32>(),
            size_of::<(usize, usize)>(),
        ));
    let output = btree_bytes(shells, size_of::<usize>(), list)
        .saturating_add(
            triangles
                .saturating_mul(3)
                .saturating_add(shells.saturating_mul(4))
                .saturating_mul(word),
        )
        .saturating_add(shells.saturating_mul(list as u64));
    admission
        .saturating_add(table)
        .saturating_add(links.max(candidates))
        .saturating_add(nesting)
        .saturating_add(triangles)
        .saturating_add(output.saturating_mul(2))
}

/// The sweep order over `shells` shells, the `pairs` candidate shell pairs
/// and their containment list of at most two entries a pair, and the
/// parallel ray directions of each shell: at most two a triangle, out of
/// `triangles`.
pub(super) fn staged_pairs(shells: u64, pairs: u64, triangles: u64) -> u64 {
    let word = size_of::<usize>() as u64;
    shells
        .saturating_mul(word + size_of::<Option<Vec<u64>>>() as u64)
        .saturating_add(pairs.saturating_mul(size_of::<Pair>() as u64))
        .saturating_add(pairs.saturating_mul(2 * size_of::<(usize, usize)>() as u64))
        .saturating_add(triangles.saturating_mul(2 * size_of::<u64>() as u64))
}

/// Bytes of one exact coordinate as `classify` builds it, for tests.
#[cfg(test)]
pub(super) fn built_coordinate_bytes(value: f64) -> u64 {
    size_of::<num_rational::BigRational>() as u64 + coordinate_limbs(value).built * LIMB
}

/// Bytes of an exact clone of one coordinate, for tests.
#[cfg(test)]
pub(super) fn cloned_coordinate_bytes(value: f64) -> u64 {
    size_of::<num_rational::BigRational>() as u64 + coordinate_limbs(value).clone * LIMB
}
