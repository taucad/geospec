//! Cover for the bounded, call-local tetrahedron and plane reuse.
//!
//! Reuse must be invisible: the same exact volume, the same typed refusal, the
//! same work charges in the same order and the same charge trace as the
//! original recomputing path, whatever the byte limit, the work limit, the slot
//! bound or the operand magnitude. These tests compare against actual uncached
//! operations rather than against an expected total, and they bound the heap
//! the reuse slots hold against their declared reservation.

use super::*;
use crate::analysis::test_alloc::{peak_bytes, retained_bytes};
use num_bigint::BigInt;
use std::collections::BTreeSet;

fn cube(min: [f64; 3], max: [f64; 3]) -> MeshAnalysisRecord {
    super::tests::cube(min, max)
}

fn origin() -> Point {
    std::array::from_fn(|_| Q::zero())
}

/// An anchor off every face plane of a cube, so no fan tetrahedron of it is
/// degenerate and its planes are all well defined.
fn outside_anchor() -> Point {
    std::array::from_fn(|_| Q::from_integer((-1).into()))
}

fn compare_trace(left: &ChargeTrace, right: &ChargeTrace) {
    assert_eq!(left.steps, right.steps);
    assert_eq!(left.complete, right.complete);
    assert_eq!(left.stage_calls, right.stage_calls);
    assert_eq!(left.stage_units, right.stage_units);
}

/// The uncached tetrahedron and determinant for one triangle.
fn uncached_tetra(
    record: &MeshAnalysisRecord,
    index: usize,
    anchor: &Point,
    work: &Work<'_>,
) -> Result<([Point; 4], Q)> {
    let tetra = super::tetra(record, index, anchor, work)?;
    let determinant = determinant(&tetra, work)?;
    Ok((tetra, determinant))
}

#[test]
fn reuse_atomic_charges_and_exhaustion_match() {
    let record = cube([0.; 3], [1.; 3]);
    let anchor = origin();
    let budget = Budget::new(100_000);
    let mut trace = ChargeTrace::default();
    let work = Work(&budget, RefCell::new(&mut trace));
    let mut tetra_memo = None;
    let tetra: [Point; 4] = {
        let (value, _) = reuse_tetra(&record, 6, &anchor, &work, Some(&mut tetra_memo)).unwrap();
        std::array::from_fn(|corner| value[corner].clone())
    };
    assert!(tetra_memo.is_some());
    let mut plane_memo = None;
    reuse_planes(&tetra, &work, Some(&mut plane_memo)).unwrap();
    assert!(plane_memo.is_some());
    // Every work limit through the whole cached charge schedule, so a hit
    // cannot exhaust the budget at a different step than a rebuild.
    for limit in 0..=160 {
        for planes_only in [false, true] {
            let cached = Budget::new(limit);
            let plain = Budget::new(limit);
            let mut cached_trace = ChargeTrace::default();
            let mut plain_trace = ChargeTrace::default();
            let cached_work = Work(&cached, RefCell::new(&mut cached_trace));
            let plain_work = Work(&plain, RefCell::new(&mut plain_trace));
            if planes_only {
                let hit = reuse_planes(&tetra, &cached_work, Some(&mut plane_memo));
                let rebuilt = planes(&tetra, &plain_work);
                assert_eq!(
                    format!("{:?}", hit.as_deref()),
                    format!("{:?}", rebuilt.as_deref()),
                    "planes at work limit {limit}"
                );
            } else {
                let hit = reuse_tetra(&record, 6, &anchor, &cached_work, Some(&mut tetra_memo));
                let rebuilt = uncached_tetra(&record, 6, &anchor, &plain_work);
                assert_eq!(
                    format!("{:?}", hit.map(|(t, d)| (t.into_owned(), d.into_owned()))),
                    format!("{rebuilt:?}"),
                    "tetrahedron at work limit {limit}"
                );
            }
            assert_eq!(cached.used(), plain.used());
            compare_trace(&cached_trace, &plain_trace);
        }
    }
}

#[test]
fn reuse_declines_noncompact_values_and_does_not_cache_errors() {
    assert!(empty_cache::<TetraMemo>(usize::MAX).is_empty());
    for scale in [1., 2f64.powi(-600), 2f64.powi(900)] {
        let record = cube([0.; 3], [scale; 3]);
        let anchor = origin();
        let mut memo = None;
        for _ in 0..2 {
            let cached = Budget::new(10_000);
            let plain = Budget::new(10_000);
            let mut cached_trace = ChargeTrace::default();
            let mut plain_trace = ChargeTrace::default();
            let cached_work = Work(&cached, RefCell::new(&mut cached_trace));
            let plain_work = Work(&plain, RefCell::new(&mut plain_trace));
            let hit = reuse_tetra(&record, 6, &anchor, &cached_work, Some(&mut memo));
            let rebuilt = uncached_tetra(&record, 6, &anchor, &plain_work);
            assert_eq!(
                format!("{:?}", hit.map(|(t, d)| (t.into_owned(), d.into_owned()))),
                format!("{rebuilt:?}")
            );
            assert_eq!(cached.used(), plain.used());
            compare_trace(&cached_trace, &plain_trace);
        }
        assert_eq!(memo.is_some(), scale == 1.);
    }
    let mut record = cube([0.; 3], [1.; 3]);
    let corner = record.triangles[6][0] as usize;
    record.positions[corner][0] = f64::NAN;
    let anchor = origin();
    let mut memo = None;
    for _ in 0..2 {
        let budget = Budget::new(1000);
        let mut trace = ChargeTrace::default();
        let work = Work(&budget, RefCell::new(&mut trace));
        assert!(reuse_tetra(&record, 6, &anchor, &work, Some(&mut memo)).is_err());
        assert!(memo.is_none(), "a refusal is never retained");
    }
    for huge in [false, true] {
        let mut tetra: [Point; 4] = std::array::from_fn(|_| std::array::from_fn(|_| Q::zero()));
        if huge {
            for (corner, point) in tetra.iter_mut().enumerate().skip(1) {
                point[corner - 1] = Q::from_integer(BigInt::one() << 8191usize);
            }
        }
        let mut memo = None;
        for _ in 0..2 {
            let cached = Budget::new(10_000);
            let plain = Budget::new(10_000);
            let mut cached_trace = ChargeTrace::default();
            let mut plain_trace = ChargeTrace::default();
            let cached_work = Work(&cached, RefCell::new(&mut cached_trace));
            let plain_work = Work(&plain, RefCell::new(&mut plain_trace));
            let hit = reuse_planes(&tetra, &cached_work, Some(&mut memo));
            let rebuilt = planes(&tetra, &plain_work);
            assert!(hit.is_err());
            assert_eq!(
                format!("{:?}", hit.as_deref()),
                format!("{:?}", rebuilt.as_deref())
            );
            assert!(memo.is_none());
            assert_eq!(cached.used(), plain.used());
            compare_trace(&cached_trace, &plain_trace);
        }
    }
}

/// A rational whose numerator and denominator are exactly `bits` wide.
fn wide(bits: u64) -> Q {
    let numerator = (BigInt::one() << (bits - 1)) + BigInt::one();
    let denominator = (BigInt::one() << (bits - 1)) + BigInt::from(3);
    Q::new_raw(numerator, denominator)
}

#[test]
fn reuse_declines_operands_wider_than_the_compact_bound() {
    assert!(compact(&wide(REUSE_COMPACT_BITS)));
    assert!(!compact(&wide(REUSE_COMPACT_BITS + 1)));
    assert!(!compact(&Q::new_raw(
        BigInt::one(),
        (BigInt::one() << REUSE_COMPACT_BITS) + BigInt::one()
    )));
    assert!(!compact(&Q::new_raw(
        (BigInt::one() << REUSE_COMPACT_BITS) + BigInt::one(),
        BigInt::one()
    )));
    // One wide coordinate is enough to keep a whole tetrahedron out of a slot,
    // and the planes of a tetrahedron with one wide coordinate stay out too.
    let anchor = origin();
    let record = cube([0.; 3], [1.; 3]);
    let budget = Budget::new(100_000);
    let mut trace = ChargeTrace::default();
    let work = Work(&budget, RefCell::new(&mut trace));
    let (compact_tetra, _) = uncached_tetra(&record, 6, &anchor, &work).unwrap();
    let mut memo = None;
    reuse_planes(&compact_tetra, &work, Some(&mut memo)).unwrap();
    assert!(
        memo.is_some(),
        "a compact tetrahedron's planes are retained"
    );
    let mut wide_tetra = compact_tetra.clone();
    wide_tetra[3][2] = wide(REUSE_COMPACT_BITS + 1);
    let mut memo = None;
    reuse_planes(&wide_tetra, &work, Some(&mut memo)).unwrap();
    assert!(memo.is_none(), "wide planes decline the compact slot");
}

#[test]
fn reuse_slot_payload_stays_inside_the_slot_reservation() {
    // The widest values a slot may hold: 13 tetrahedron and determinant
    // rationals plus one four-plane set of 16 rationals, each numerator and
    // denominator at the compact bound.
    let widest = wide(REUSE_COMPACT_BITS);
    let (slot, retained) = retained_bytes(|| {
        let point: Point = std::array::from_fn(|_| widest.clone());
        let memo = TetraMemo {
            tetra: std::array::from_fn(|_| point.clone()),
            determinant: widest.clone(),
        };
        let faces: Vec<Plane> = (0..4)
            .map(|_| Plane {
                normal: point.clone(),
                offset: widest.clone(),
            })
            .collect();
        (Some(memo), Some(faces))
    });
    let inline = (size_of::<Option<TetraMemo>>() + size_of::<Option<Vec<Plane>>>()) as u64;
    let payload = retained + inline;
    println!(
        "reuse-slot {{\"measuredPayloadBytes\":{payload},\"heapBytes\":{retained},\"inlineBytes\":{inline},\"reservedBytes\":{REUSE_SLOT_BYTES}}}"
    );
    assert!(
        payload <= REUSE_SLOT_BYTES,
        "a slot holds {payload} bytes but reserves {REUSE_SLOT_BYTES}"
    );
    assert!(slot.0.is_some() && slot.1.is_some());
    // The whole optional reservation is the right slots plus the one left set.
    assert_eq!(
        (MAX_REUSE_SLOTS as u64 + 1) * REUSE_SLOT_BYTES,
        67_239_936,
        "the declared optional ceiling is 64.125 MiB"
    );
}

fn operands() -> (QualifiedMaterial, QualifiedMaterial) {
    let budget = Budget::new(1_000_000);
    let left = qualify(Rc::new(cube([0.; 3], [4.; 3])), &budget, 256 * 1024 * 1024).unwrap();
    let right = qualify(Rc::new(cube([3.; 3], [5.; 3])), &budget, 256 * 1024 * 1024).unwrap();
    (left, right)
}

fn required_bytes(left: &QualifiedMaterial, right: &QualifiedMaterial) -> u64 {
    WORKSPACE_BYTES + left.allocated_bytes() + right.allocated_bytes()
}

#[test]
fn reuse_peak_heap_stays_inside_the_declared_slot_reservation() {
    let (left, right) = operands();
    let slots = right.record.triangles.len();
    let bytes =
        required_bytes(&left, &right) + WORKSPACE_BYTES + (slots as u64 + 1) * REUSE_SLOT_BYTES;
    let measure = |max_slots: usize| {
        let budget = Budget::new(512_000_000);
        let mut trace = ChargeTrace::default();
        let (value, peak) = peak_bytes(|| {
            intersection_bounded(
                &left, None, &right, None, &budget, bytes, &mut trace, max_slots,
            )
        });
        (format!("{value:?}"), budget.used(), peak)
    };
    let plain = measure(0);
    let cached = measure(MAX_REUSE_SLOTS);
    let ceiling = (slots as u64 + 1) * REUSE_SLOT_BYTES;
    let extra = cached.2.saturating_sub(plain.2);
    println!(
        "reuse-heap {{\"slots\":{slots},\"plainPeakBytes\":{},\"cachedPeakBytes\":{},\"extraBytes\":{extra},\"reservedBytes\":{ceiling}}}",
        plain.2, cached.2
    );
    assert_eq!(plain.0, cached.0);
    assert_eq!(plain.1, cached.1);
    assert!(
        extra <= ceiling,
        "reuse held {extra} bytes above the uncached peak but reserved {ceiling}"
    );
}

fn evaluated(
    left: &QualifiedMaterial,
    right: &QualifiedMaterial,
    selected: u8,
    limit: u64,
    bytes: u64,
    max_slots: usize,
) -> (String, u64, ChargeTrace) {
    let budget = Budget::new(limit);
    let mut trace = ChargeTrace::default();
    let members: Vec<usize> = match selected {
        2 => vec![6, 8, 10],
        3 => vec![6, 6, 8, 10],
        _ => (0..12).rev().collect(),
    };
    let members = (selected > 0).then_some(members.as_slice());
    let value = intersection_bounded(
        left, members, right, members, &budget, bytes, &mut trace, max_slots,
    );
    (format!("{value:?}"), budget.used(), trace)
}

#[test]
fn reuse_whole_intersections_preserve_values_refusals_and_trace_capacity() {
    for (min, max) in [
        ([3.; 3], [5.; 3]),
        ([1.; 3], [2.; 3]),
        ([3., 0., 0.], [5., 4., 4.]),
    ] {
        let qualification = Budget::new(1_000_000);
        let left = qualify(
            Rc::new(cube([0.; 3], [4.; 3])),
            &qualification,
            256 * 1024 * 1024,
        )
        .unwrap();
        let right = qualify(Rc::new(cube(min, max)), &qualification, 256 * 1024 * 1024).unwrap();
        let required = required_bytes(&left, &right);
        for selected in [0, 1, 2, 3] {
            for bytes in [
                required - 1,
                required,
                required + WORKSPACE_BYTES,
                required + WORKSPACE_BYTES + 3 * REUSE_SLOT_BYTES,
                256 * 1024 * 1024,
            ] {
                let completed = evaluated(&left, &right, selected, 512_000_000, bytes, 0);
                for limit in [
                    0,
                    6,
                    100,
                    completed.1.saturating_sub(1),
                    completed.1,
                    512_000_000,
                ] {
                    let original = evaluated(&left, &right, selected, limit, bytes, 0);
                    for max_slots in [1, 8, MAX_REUSE_SLOTS] {
                        let cached = evaluated(&left, &right, selected, limit, bytes, max_slots);
                        assert_eq!(
                            original.0, cached.0,
                            "selected={selected} bytes={bytes} slots={max_slots} limit={limit}"
                        );
                        assert_eq!(original.1, cached.1);
                        compare_trace(&original.2, &cached.2);
                    }
                }
            }
        }
        // The same qualified object on both sides shares one allocation debit.
        let bytes = 256 * 1024 * 1024;
        let alias = evaluated(&left, &left, 0, 512_000_000, bytes, 0);
        let alias_cached = evaluated(&left, &left, 0, 512_000_000, bytes, MAX_REUSE_SLOTS);
        assert_eq!(alias.0, alias_cached.0);
        assert_eq!(alias.1, alias_cached.1);
        compare_trace(&alias.2, &alias_cached.2);
    }
}

#[test]
fn reuse_preserves_results_for_operands_outside_the_compact_bound() {
    // Coordinates whose exact rationals need more than the compact bound, so
    // every slot declines and every pair takes the original guarded path.
    for scale in [2f64.powi(-600), 2f64.powi(600)] {
        let qualification = Budget::new(1_000_000);
        let left = qualify(
            Rc::new(cube([0.; 3], [4. * scale; 3])),
            &qualification,
            256 * 1024 * 1024,
        )
        .unwrap();
        let right = qualify(
            Rc::new(cube([3. * scale; 3], [5. * scale; 3])),
            &qualification,
            256 * 1024 * 1024,
        )
        .unwrap();
        let bytes = 256 * 1024 * 1024;
        let plain = evaluated(&left, &right, 0, 512_000_000, bytes, 0);
        let cached = evaluated(&left, &right, 0, 512_000_000, bytes, MAX_REUSE_SLOTS);
        assert_eq!(plain.0, cached.0, "scale={scale}");
        assert_eq!(plain.1, cached.1);
        compare_trace(&plain.2, &cached.2);
        // The exact unit-cube overlap, scaled, is still decided exactly.
        let expected = arithmetic::multiply(
            &arithmetic::rational(scale).unwrap(),
            &arithmetic::multiply(
                &arithmetic::rational(scale).unwrap(),
                &arithmetic::rational(scale).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(cached.0, format!("{:?}", Ok::<&Q, ()>(&expected)));
    }
}

#[test]
fn a_declined_slot_reservation_keeps_the_identical_outcome_charge_and_trace() {
    let (left, right) = operands();
    let bytes = 256 * 1024 * 1024;
    let disabled = evaluated(&left, &right, 0, 512_000_000, bytes, 0);
    let cached = evaluated(&left, &right, 0, 512_000_000, bytes, MAX_REUSE_SLOTS);
    assert_eq!(disabled.0, cached.0);
    REUSE_RESERVATION_FAILS.set(true);
    let declined = evaluated(&left, &right, 0, 512_000_000, bytes, MAX_REUSE_SLOTS);
    let declined_tight = evaluated(&left, &right, 0, 400, bytes, MAX_REUSE_SLOTS);
    REUSE_RESERVATION_FAILS.set(false);
    let tight = evaluated(&left, &right, 0, 400, bytes, 0);
    // A declined reservation is a fallback, never a new refusal.
    assert_eq!(declined.0, disabled.0, "the exact value is unchanged");
    assert_eq!(declined.1, disabled.1, "the work charge is unchanged");
    compare_trace(&declined.2, &disabled.2);
    assert_eq!(declined_tight.0, tight.0, "a work refusal is unchanged");
    assert_eq!(declined_tight.1, tight.1);
    compare_trace(&declined_tight.2, &tight.2);
    assert!(
        tight.0.contains("BudgetExceeded"),
        "the tight limit must still refuse, not pass: {}",
        tight.0
    );
}

#[test]
fn reuse_slots_never_return_another_operands_tetrahedron_or_planes() {
    let record = cube([0.; 3], [4.; 3]);
    let anchor = outside_anchor();
    let count = record.triangles.len();
    let mut expected = Vec::new();
    for index in 0..count {
        let budget = Budget::new(1_000_000);
        let mut trace = ChargeTrace::default();
        let work = Work(&budget, RefCell::new(&mut trace));
        let (tetra, determinant) = uncached_tetra(&record, index, &anchor, &work).unwrap();
        let faces = planes(&tetra, &work).unwrap();
        expected.push((tetra, determinant, faces));
    }
    // Distinct triangles must prepare distinct geometry, so a slot answering
    // for the wrong operand could not pass the comparisons below.
    let tetrahedra: BTreeSet<String> = expected.iter().map(|e| format!("{:?}", e.0)).collect();
    let faces: BTreeSet<String> = expected.iter().map(|e| format!("{:?}", e.2)).collect();
    assert_eq!(tetrahedra.len(), count);
    assert_eq!(faces.len(), count);
    let mut tetra_slots: Vec<Option<TetraMemo>> = empty_cache(count);
    let mut plane_slots: Vec<Option<Vec<Plane>>> = empty_cache(count);
    assert_eq!(tetra_slots.len(), count);
    // The first pass fills every slot; the second can only be served by hits.
    for pass in 0..2 {
        for (index, prepared) in expected.iter().enumerate() {
            let budget = Budget::new(1_000_000);
            let mut trace = ChargeTrace::default();
            let work = Work(&budget, RefCell::new(&mut trace));
            let (tetra, determinant) =
                reuse_tetra(&record, index, &anchor, &work, tetra_slots.get_mut(index)).unwrap();
            assert_eq!(*tetra, prepared.0, "pass {pass} triangle {index}");
            assert_eq!(*determinant, prepared.1, "pass {pass} triangle {index}");
            let faces = reuse_planes(&tetra, &work, plane_slots.get_mut(index)).unwrap();
            assert_eq!(
                faces.as_ref(),
                prepared.2.as_slice(),
                "pass {pass} triangle {index}"
            );
        }
        assert!(tetra_slots.iter().all(Option::is_some), "pass {pass}");
        assert!(plane_slots.iter().all(Option::is_some), "pass {pass}");
    }
    // The left plane slot is reset for each outer triangle, so a cell built
    // with it equals the same cell built with no reuse at all.
    let right = expected[6].0.clone();
    for (index, prepared) in expected.iter().enumerate() {
        let plain = Budget::new(1_000_000);
        let reused = Budget::new(1_000_000);
        let mut plain_trace = ChargeTrace::default();
        let mut reused_trace = ChargeTrace::default();
        let plain_work = Work(&plain, RefCell::new(&mut plain_trace));
        let reused_work = Work(&reused, RefCell::new(&mut reused_trace));
        let mut left_slot = None;
        let expected_cell = convex_volume(&prepared.0, &right, &plain_work, None, None);
        let reused_cell = convex_volume(
            &prepared.0,
            &right,
            &reused_work,
            Some(&mut left_slot),
            None,
        );
        assert_eq!(
            format!("{expected_cell:?}"),
            format!("{reused_cell:?}"),
            "triangle {index}"
        );
        assert_eq!(plain.used(), reused.used());
        compare_trace(&plain_trace, &reused_trace);
    }
}
