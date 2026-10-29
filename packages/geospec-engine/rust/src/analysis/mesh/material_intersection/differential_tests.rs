//! Differential cover for the bounded exact fast paths.
//!
//! Each `reference` function here is the general guarded rational arithmetic
//! the shipped code used before the bounded integer paths were added. The
//! tests require the shipped code to return the identical value or the
//! identical typed refusal, to consume the identical work, and to record the
//! identical charge trace, at coefficient widths on both sides of every bound
//! and at work limits on both sides of every charge.

use super::*;
use num_bigint::BigInt;

fn compare_trace(left: &ChargeTrace, right: &ChargeTrace) {
    assert_eq!(left.steps, right.steps);
    assert_eq!(left.complete, right.complete);
    assert_eq!(left.stage_calls, right.stage_calls);
    assert_eq!(left.stage_units, right.stage_units);
}

/// [`super::convex_volume`] with the original recomputing plane preparation
/// and the original rational vertex and plane-side arithmetic.
fn reference(a: &[Point; 4], b: &[Point; 4], work: &Work<'_>) -> Result<Q> {
    let mut constraints = planes(a, work)?;
    for plane in planes(b, work)? {
        work.charge(constraints.len() as u64)?;
        if !constraints.contains(&plane) {
            constraints.push(plane);
        }
    }
    let mut points = Vec::<Point>::with_capacity(56);
    for i in 0..constraints.len() {
        for j in i + 1..constraints.len() {
            for k in j + 1..constraints.len() {
                work.charge(1)?;
                let Some(point) =
                    reference_vertex(&constraints[i], &constraints[j], &constraints[k], work)?
                else {
                    continue;
                };
                let mut inside = true;
                for plane in &constraints {
                    if work.compare(&work.dot(&plane.normal, &point)?, &plane.offset)?
                        == Ordering::Greater
                    {
                        inside = false;
                        break;
                    }
                }
                if inside {
                    work.charge(points.len() as u64 * 3)?;
                    if !points.contains(&point) {
                        points.push(point);
                    }
                }
            }
        }
    }
    if points.len() < 4 {
        return Ok(Q::zero());
    }
    let mut center: Point = std::array::from_fn(|_| Q::zero());
    for p in &points {
        for axis in 0..3 {
            center[axis] = work.add(&center[axis], &p[axis])?;
        }
    }
    let count = Q::from_integer(points.len().into());
    for c in &mut center {
        *c = work.div(c, &count)?;
    }
    let mut volume = Q::zero();
    for plane in constraints {
        let mut face = Vec::<usize>::with_capacity(points.len());
        for (index, p) in points.iter().enumerate() {
            if work.compare(&work.dot(&plane.normal, p)?, &plane.offset)? == Ordering::Equal {
                face.push(index);
            }
        }
        if face.len() < 3 {
            continue;
        }
        let mut face_center: Point = std::array::from_fn(|_| Q::zero());
        for &index in &face {
            for (axis, coordinate) in face_center.iter_mut().enumerate() {
                *coordinate = work.add(coordinate, &points[index][axis])?;
            }
        }
        let count = Q::from_integer(face.len().into());
        for c in &mut face_center {
            *c = work.div(c, &count)?;
        }
        let drop = plane
            .normal
            .iter()
            .position(|n| !n.is_zero())
            .expect("qualified nonzero plane");
        let axes: Vec<_> = (0..3).filter(|&axis| axis != drop).collect();
        // Bounded insertion sort propagates exact-arithmetic/budget refusal;
        // atan2 and an infallible comparator would hide those failure paths.
        let compare = |left: usize, right: usize| -> Result<Ordering> {
            let a = work.difference(&points[left], &face_center)?;
            let b = work.difference(&points[right], &face_center)?;
            let half = |p: &Point| {
                p[axes[1]].is_negative() || (p[axes[1]].is_zero() && p[axes[0]].is_negative())
            };
            if half(&a) != half(&b) {
                return Ok(half(&a).cmp(&half(&b)));
            }
            let cross = work.sub(
                &work.mul(&a[axes[0]], &b[axes[1]])?,
                &work.mul(&a[axes[1]], &b[axes[0]])?,
            )?;
            Ok(if cross.is_positive() {
                Ordering::Less
            } else if cross.is_negative() {
                Ordering::Greater
            } else {
                Ordering::Equal
            })
        };
        for i in 1..face.len() {
            let mut j = i;
            while j > 0 && compare(face[j], face[j - 1])? == Ordering::Less {
                face.swap(j, j - 1);
                j -= 1;
            }
        }
        for i in 1..face.len() - 1 {
            let p = work.difference(&points[face[0]], &center)?;
            let q = work.difference(&points[face[i]], &center)?;
            let r = work.difference(&points[face[i + 1]], &center)?;
            // Interior center makes every face pyramid nonnegative. Unique
            // constraints prevent duplicate coincident face integration.
            volume = work.add(
                &volume,
                &work.div(
                    &work.dot(&p, &work.cross(&q, &r)?)?.abs(),
                    &Q::from_integer(6.into()),
                )?,
            )?;
        }
    }
    Ok(volume)
}

#[test]
fn convex_cells_preserve_exact_results_and_budget_failures() {
    let mut state = 0x819a27u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 32) % 17) as f64 / 4.
    };
    for scale in [1., 2f64.powi(45), 2f64.powi(-40)] {
        for _ in 0..32 {
            let a: [[f64; 3]; 4] = std::array::from_fn(|_| std::array::from_fn(|_| next() * scale));
            let b: [[f64; 3]; 4] = std::array::from_fn(|_| std::array::from_fn(|_| next() * scale));
            let a = a.map(|p| p.map(|x| arithmetic::rational(x).unwrap()));
            let b = b.map(|p| p.map(|x| arithmetic::rational(x).unwrap()));
            let qualification = Budget::new(1000);
            let mut trace = ChargeTrace::disabled();
            let work = Work(&qualification, RefCell::new(&mut trace));
            if determinant(&a, &work).unwrap().is_zero()
                || determinant(&b, &work).unwrap().is_zero()
            {
                continue;
            }
            for limit in [0, 1, 100, 1000, 5_000_000] {
                let shipped_budget = Budget::new(limit);
                let plain_budget = Budget::new(limit);
                let mut shipped_trace = ChargeTrace::default();
                let mut plain_trace = ChargeTrace::default();
                let shipped_work = Work(&shipped_budget, RefCell::new(&mut shipped_trace));
                let plain_work = Work(&plain_budget, RefCell::new(&mut plain_trace));
                assert_eq!(
                    format!("{:?}", convex_volume(&a, &b, &shipped_work, None, None)),
                    format!("{:?}", reference(&a, &b, &plain_work)),
                    "scale={scale} limit={limit}"
                );
                assert_eq!(shipped_budget.used(), plain_budget.used());
                compare_trace(&shipped_trace, &plain_trace);
            }
        }
    }
}

/// [`super::vertex`]'s original guarded rational Cramer solve.
fn reference_vertex(a: &Plane, b: &Plane, c: &Plane, work: &Work<'_>) -> Result<Option<Point>> {
    let bc = work.cross(&b.normal, &c.normal)?;
    let determinant = work.dot(&a.normal, &bc)?;
    if determinant.is_zero() {
        return Ok(None);
    }
    let ca = work.cross(&c.normal, &a.normal)?;
    let ab = work.cross(&a.normal, &b.normal)?;
    let mut point = std::array::from_fn(|_| Q::zero());
    for axis in 0..3 {
        point[axis] = work.div(
            &work.add(
                &work.add(
                    &work.mul(&bc[axis], &a.offset)?,
                    &work.mul(&ca[axis], &b.offset)?,
                )?,
                &work.mul(&ab[axis], &c.offset)?,
            )?,
            &determinant,
        )?;
    }
    Ok(Some(point))
}

#[test]
fn integer_vertices_preserve_numeric_and_budget_boundaries() {
    // One work limit on each side of every charge in the solve.
    let limits = [
        0, 1, 8, 9, 13, 14, 22, 23, 31, 32, 33, 37, 38, 43, 44, 49, 50, 5000,
    ];
    // Widths on both sides of INTEGER_VERTEX_BITS and of the measured wider
    // domain, up to the 8192-bit arithmetic guard.
    for bits in [
        1usize, 31, 63, 64, 65, 127, 128, 255, 256, 257, 511, 4095, 8190, 8191,
    ] {
        let magnitude = (BigInt::one() << (bits - 1)) + BigInt::one();
        let ratio = Q::new(magnitude.clone(), &magnitude + BigInt::one());
        for variant in 0..4 {
            let a = Plane {
                normal: [Q::one(), ratio.clone(), Q::zero()],
                offset: ratio.clone(),
            };
            let b = Plane {
                normal: [Q::zero(), Q::one(), -ratio.clone()],
                offset: Q::one(),
            };
            let c = match variant {
                0 => Plane {
                    normal: [ratio.clone(), Q::zero(), Q::one()],
                    offset: -ratio.clone(),
                },
                1 => a.clone(),
                2 => Plane {
                    normal: [ratio.clone(), ratio.clone(), ratio.clone()],
                    offset: Q::from_integer(magnitude.clone()),
                },
                _ => Plane {
                    normal: [Q::from_integer(magnitude.clone()), Q::one(), ratio.clone()],
                    offset: Q::new(BigInt::one(), magnitude.clone()),
                },
            };
            for limit in limits {
                let shipped_budget = Budget::new(limit);
                let plain_budget = Budget::new(limit);
                let mut shipped_trace = ChargeTrace::default();
                let mut plain_trace = ChargeTrace::default();
                let shipped_work = Work(&shipped_budget, RefCell::new(&mut shipped_trace));
                let plain_work = Work(&plain_budget, RefCell::new(&mut plain_trace));
                let shipped = vertex(&a, &b, &c, &shipped_work);
                let expected = reference_vertex(&a, &b, &c, &plain_work);
                assert_eq!(
                    format!("{shipped:?}"),
                    format!("{expected:?}"),
                    "bits={bits} variant={variant} limit={limit}"
                );
                assert_eq!(shipped_budget.used(), plain_budget.used());
                compare_trace(&shipped_trace, &plain_trace);
            }
        }
    }
}

#[test]
fn plane_sides_preserve_numeric_and_budget_boundaries() {
    // Widths on both sides of PLANE_SIDE_BITS, up to the arithmetic guard.
    for bits in [1usize, 31, 64, 128, 511, 512, 513, 1024, 4095, 8191] {
        let magnitude = (BigInt::one() << (bits - 1)) + BigInt::one();
        let ratio = Q::new(magnitude.clone(), &magnitude + BigInt::one());
        for variant in 0..4 {
            let mut plane = Plane {
                normal: [ratio.clone(), Q::one(), -ratio.clone()],
                offset: ratio.clone(),
            };
            let mut point = [ratio.clone(), -ratio.clone(), Q::one()];
            match variant {
                1 => plane.normal[1] = Q::zero(),
                2 => point[2] = Q::zero(),
                3 => plane.offset = Q::from_integer(magnitude.clone()),
                _ => {}
            }
            for limit in [0, 4, 5, 6, 100] {
                let shipped_budget = Budget::new(limit);
                let plain_budget = Budget::new(limit);
                let mut shipped_trace = ChargeTrace::default();
                let mut plain_trace = ChargeTrace::default();
                let shipped_work = Work(&shipped_budget, RefCell::new(&mut shipped_trace));
                let plain_work = Work(&plain_budget, RefCell::new(&mut plain_trace));
                let shipped = plane_side(&plane, &point, &shipped_work);
                let expected = (|| {
                    plain_work.compare(&plain_work.dot(&plane.normal, &point)?, &plane.offset)
                })();
                assert_eq!(
                    format!("{shipped:?}"),
                    format!("{expected:?}"),
                    "bits={bits} variant={variant} limit={limit}"
                );
                assert_eq!(shipped_budget.used(), plain_budget.used());
                compare_trace(&shipped_trace, &plain_trace);
            }
        }
    }
}

/// The fixtures both scaled-face differentials run, including every degenerate
/// shape the transform could plausibly disturb: zero outcomes (separated,
/// touching and cavity), a `2^-40` sliver, identical operands
/// (self-intersection), coplanar shared faces, a non-dyadic operand, and
/// non-finite and out-of-domain coordinates.
fn scaled_face_cases() -> Vec<(&'static str, MeshAnalysisRecord, MeshAnalysisRecord)> {
    use super::tests::{boxes, cube};
    let solid = cube([0.; 3], [4.; 3]);
    let hollow = boxes(&[([0.; 3], [4.; 3], false), ([1.; 3], [3.; 3], true)]);
    let sliver = 2f64.powi(-40);
    let huge = 2f64.powi(900);
    vec![
        ("contained", solid.clone(), cube([1.; 3], [2.; 3])),
        ("partial", solid.clone(), cube([3.; 3], [5.; 3])),
        ("separated-zero", solid.clone(), cube([4.5; 3], [5.5; 3])),
        (
            "touching-face-zero",
            solid.clone(),
            cube([4., 0., 0.], [5., 4., 4.]),
        ),
        ("cavity-zero", hollow.clone(), cube([1.5; 3], [2.5; 3])),
        ("crossing-wall", hollow, cube([0.5; 3], [2.; 3])),
        ("self-intersection", solid.clone(), solid.clone()),
        ("coplanar-shared-face", solid.clone(), solid.clone()),
        (
            "sliver",
            solid.clone(),
            cube([4. - sliver, 0., 0.], [5., 4., 4.]),
        ),
        (
            "non-dyadic",
            cube([0.; 3], [4.; 3]),
            cube([1. / 3., 1. / 7., 1. / 11.], [2. / 3., 2. / 7., 2. / 11.]),
        ),
        (
            "out-of-domain-wide",
            solid.clone(),
            cube([-huge; 3], [huge; 3]),
        ),
        (
            "non-finite",
            solid.clone(),
            cube([0.5; 3], [f64::INFINITY, 2., 2.]),
        ),
        ("not-a-number", solid, cube([0.5; 3], [f64::NAN, 2., 2.])),
    ]
}

/// Evaluate one case at one work limit on both arms of [`super::scaled_cell`]
/// and require them to be indistinguishable: the same exact value or the same
/// typed refusal byte for byte, and the same work consumed.
fn scaled_face_arms(
    name: &str,
    a: &MeshAnalysisRecord,
    b: &MeshAnalysisRecord,
    limit: u64,
) -> (String, bool, u64) {
    scaled_cells_taken();
    let scaled_budget = Budget::new(limit);
    let scaled = format!(
        "{:?}",
        intersection(a, b, &scaled_budget, 256 * 1024 * 1024)
    );
    let cells = scaled_cells_taken();
    let plain_budget = Budget::new(limit);
    let plain = without_scaled_cells(|| {
        format!("{:?}", intersection(a, b, &plain_budget, 256 * 1024 * 1024))
    });
    assert_eq!(scaled_cells_taken(), 0, "{name} declined every cell");
    assert_eq!(scaled, plain, "{name} limit={limit} outcome differs");
    // Folding `L^3` into the fan loop's existing charged division adds no
    // charge, so this is an equality and not a documented delta. If it ever
    // moves, the variant has lost the only property that justifies it.
    assert_eq!(
        scaled_budget.used(),
        plain_budget.used(),
        "{name} limit={limit} charge moved"
    );
    (scaled, cells > 0, plain_budget.used())
}

/// A whole-intersection differential over the scaled face integration.
///
/// Every fixture is evaluated twice in one process on identical inputs, once
/// with [`super::scaled_cell`] engaged and once with it declined on every
/// cell, and the two must agree on the exact `BigRational` or on the typed
/// refusal, byte for byte, and on the work consumed, at every work limit.
#[test]
fn scaled_face_integration_preserves_every_exact_result_and_refusal() {
    let cases = scaled_face_cases();
    let mut compared = 0;
    let mut scaled_any = 0;
    for (name, a, b) in &cases {
        for limit in [0u64, 1, 2, 100, 10_000, 512_000_000] {
            let (_, scaled, _) = scaled_face_arms(name, a, b, limit);
            scaled_any += usize::from(scaled);
            compared += 1;
        }
    }
    assert_eq!(compared, cases.len() * 6);
    // The corpus must actually exercise the scaled path, or it proves nothing.
    assert!(scaled_any > 0, "no cell was scaled at any limit");
    println!(
        "scaled-face-differential {{\"cases\":{},\"limits\":6,\"comparisons\":{compared},\"scaledArms\":{scaled_any}}}",
        cases.len()
    );
}

/// The budget refusal boundary is identical on both arms, swept across it.
///
/// This is the probe that found the defect in the earlier design, where the
/// scaling added one charge per cell and so opened a window of declared
/// budgets in which the rational path passed and the scaled path refused. For
/// each fixture it takes the rational path's exact total and sweeps the
/// declared budget through it from below and from above, asserting at every
/// step that both arms pass together or refuse together, with the identical
/// outcome. A single differing step is a pass turned into a refusal or the
/// reverse, which nothing licenses.
#[test]
fn scaled_face_integration_preserves_the_budget_refusal_boundary() {
    let cases = scaled_face_cases();
    let mut swept = 0;
    let mut boundaries = 0;
    for (name, a, b) in &cases {
        let total = {
            let budget = Budget::new(512_000_000);
            let outcome = without_scaled_cells(|| intersection(a, b, &budget, 256 * 1024 * 1024));
            if outcome.is_err() {
                // A case that refuses for a reason other than the budget has
                // no exhaustion point to sweep; the differential above still
                // covers it at every limit.
                continue;
            }
            budget.used()
        };
        // Below the total every limit must refuse on both arms, at the total
        // and above every limit must pass on both, and the step from one to
        // the other must happen at the same limit on both. Swept upwards and
        // then downwards so the order cannot hide a stateful difference.
        let span: Vec<u64> = (0..=64u64)
            .filter_map(|k| total.checked_sub(32).map(|low| low + k))
            .collect();
        for direction in [false, true] {
            let mut limits = span.clone();
            if direction {
                limits.reverse();
            }
            let mut last: Option<(u64, bool)> = None;
            for limit in limits {
                // `scaled_face_arms` has already asserted that both arms
                // return the identical outcome and consume the identical
                // work at this limit, so one verdict describes both.
                let (outcome, _, _) = scaled_face_arms(name, a, b, limit);
                let passes = outcome.starts_with("Ok");
                assert_eq!(
                    passes,
                    limit >= total,
                    "{name} limit={limit} total={total}: the refusal boundary \
                     is not the rational path's own total"
                );
                if let Some((previous, was)) = last {
                    if was != passes {
                        // The step is between adjacent limits, and the first
                        // passing one is exactly the rational total.
                        boundaries += 1;
                        assert_eq!(
                            previous.max(limit),
                            total,
                            "{name} crossed at {previous}..{limit}, not at {total}"
                        );
                    }
                }
                last = Some((limit, passes));
                swept += 1;
            }
        }
    }
    assert!(swept > 0, "nothing was swept");
    // Every sweepable case must have shown the boundary once in each
    // direction, or the sweep did not actually cross it.
    assert!(boundaries >= 2, "the sweep never crossed a boundary");
    println!("scaled-face-budget-sweep {{\"steps\":{swept},\"boundaryCrossings\":{boundaries}}}");
}

/// A cell whose widths are exactly `numerator_bits` and `denominator_bits`.
///
/// [`super::scaled_cell`] reads only the vertex and plane coefficient widths,
/// so a synthetic cell pins its preflight at an exact bit. One coordinate
/// carries the whole common denominator and another the widest numerator,
/// which makes `D = denominator_bits` and `W = numerator_bits + D` exactly,
/// and the single unit plane keeps `N = 1` so the two plane conditions stay
/// far from their bounds and cannot be what decides.
fn synthetic_cell(numerator_bits: u64, denominator_bits: u64) -> (Vec<Point>, Vec<Plane>) {
    assert!(numerator_bits >= 1 && denominator_bits >= 1);
    // Odd, so `Q::new` cannot reduce the denominator away.
    let denominator = (BigInt::from(1) << (denominator_bits - 1)) + BigInt::from(1);
    assert_eq!(denominator.bits(), denominator_bits);
    let wide = Q::new(BigInt::from(1), denominator);
    let tall = Q::from_integer(BigInt::from(1) << (numerator_bits - 1));
    assert_eq!(tall.numer().bits(), numerator_bits);
    let one = Q::from_integer(BigInt::from(1));
    let points = vec![
        [Q::zero(), Q::zero(), Q::zero()],
        [tall, Q::zero(), Q::zero()],
        [Q::zero(), wide, Q::zero()],
        [one.clone(), one.clone(), one.clone()],
    ];
    let planes = vec![Plane {
        normal: [one.clone(), one.clone(), one.clone()],
        offset: one,
    }];
    (points, planes)
}

/// Run [`super::scaled_cell`]'s preflight on a synthetic cell of those widths.
///
/// Returns whether the cell was admitted, and asserts the two outcomes the
/// fallback depends on: an admitted cell leaves every coordinate an integer
/// over the returned `L`, and a declined cell leaves `points` byte-identical,
/// which is what lets the unchanged rational integration run on them.
fn synthetic_preflight(numerator_bits: u64, denominator_bits: u64) -> bool {
    let (mut points, planes) = synthetic_cell(numerator_bits, denominator_bits);
    let before = points.clone();
    let borrowed: Vec<&Plane> = planes.iter().collect();
    match scaled_cell(&mut points, &borrowed) {
        Some((lcm, scaled)) => {
            assert_eq!(lcm.bits(), denominator_bits, "scaled by the wrong L");
            for coordinate in points.iter().flatten() {
                assert_eq!(
                    *coordinate.denom(),
                    BigInt::from(1),
                    "an admitted cell must be integral over L"
                );
            }
            assert_eq!(scaled.len(), planes.len());
            true
        }
        None => {
            assert_eq!(points, before, "a declined cell must be left untouched");
            false
        }
    }
}

/// The width preflight admits at its bound and declines one bit past it.
///
/// `span` is the room below the 8192-bit arithmetic guard, and the condition
/// that decides here is `3W + 3D <= span` with `W = w + D`, so the bound is
/// `3w + 6D <= span`. At `D = 1000` that is `w <= 688`, and this holds the
/// preflight to that bit: 688 is admitted with `3w + 6D` exactly `span`, and
/// 689 is declined. The other three conditions are held far from their bounds
/// by the construction, so neither verdict can come from one of them.
#[test]
fn the_scaled_cell_width_preflight_declines_one_bit_past_its_bound() {
    let span = MAX_INTEGER_BITS - super::SCALED_CELL_HEADROOM_BITS;
    let denominator = 1000;
    let admitted = 688;
    assert_eq!(3 * (admitted + denominator) + 3 * denominator, span);
    assert!(
        synthetic_preflight(admitted, denominator),
        "a cell exactly at the width bound must be admitted"
    );
    assert!(
        !synthetic_preflight(admitted + 1, denominator),
        "one bit past the width bound must decline"
    );
    // The bound is the sum, not either term, so the same step in `D` decides
    // the same way.
    assert!(synthetic_preflight(682, denominator + 1));
    assert!(!synthetic_preflight(688, denominator + 1));
    println!(
        "scaled-cell-width-boundary {{\"span\":{span},\"denominatorBits\":{denominator},\"admittedNumeratorBits\":{admitted}}}"
    );
}

/// The `6D` condition is real but can never be the condition that decides.
///
/// `widest` is `bits(numer) + bits(L)`, so `W >= D + 1` for any cell with a
/// nonzero coordinate, and `3W + 3D = 3 bits(numer) + 6D > 6D`. The width
/// condition therefore trips strictly before the denominator condition can,
/// and no cell exists that the denominator condition declines on its own.
/// This test exercises the real preflight rather than a copy of its
/// arithmetic: at the narrowest possible numerator it finds the deciding bit
/// in `D` and shows it is below `span / 6`, where `6D > span` is still false.
/// Kept because the condition still bounds the accumulator's denominator, and
/// if `widest` ever stops including `D` this test fails and says so.
#[test]
fn the_scaled_cell_denominator_condition_cannot_decide_alone() {
    let span = MAX_INTEGER_BITS - super::SCALED_CELL_HEADROOM_BITS;
    let alone = span / 6;
    // With `bits(numer) = 1` the width condition is `3 + 6D <= span`.
    assert!(synthetic_preflight(1, alone - 1), "D = span/6 - 1 admits");
    assert!(
        !synthetic_preflight(1, alone),
        "D = span/6 is already declined by the width condition"
    );
    // ... and `6D > span` only begins one bit further on, so every cell the
    // denominator condition would decline has already been declined.
    assert!(6 * alone <= span && 6 * (alone + 1) > span);
    assert!(!synthetic_preflight(1, alone + 1));
    // Any wider cell in the denominator condition's own region declines too.
    for extra in [1u64, 2, 64, 512] {
        assert!(!synthetic_preflight(1, alone + extra));
    }
    println!(
        "scaled-cell-denominator-dominated {{\"span\":{span},\"sixthOfSpan\":{alone},\"decidingDenominatorBits\":{alone}}}"
    );
}

/// A constructed cell too wide for the preflight keeps the rational result.
///
/// The operands are a fixed tetrahedron and a copy displaced by `1/3^k`, so
/// `k` moves the solved vertices' common denominator in steps of under two
/// bits and crosses the preflight's width bound without changing the shape of
/// the cell. At `k = 565` the cell is admitted; at `k = 566` the preflight
/// declines it, and the claim must still return the unchanged rational path's
/// exact volume, consume its exact charge and record its exact trace. `k =
/// 900` is past the arithmetic guard itself, where both arms must refuse
/// identically: a declined cell may not refuse where the base path completes,
/// and it may not complete where the base path refuses either.
#[test]
fn a_cell_too_wide_for_the_preflight_returns_the_rational_result() {
    let unit: [[i32; 3]; 4] = [[0, 0, 0], [4, 0, 0], [0, 4, 0], [0, 0, 4]];
    let a: [Point; 4] = unit.map(|p| p.map(|x| Q::from_integer(BigInt::from(x))));
    let half = Q::new(BigInt::from(1), BigInt::from(2));
    let mut seen = Vec::new();
    for (k, expect_admitted, expect_ok) in [
        (565u32, true, true),
        (566, false, true),
        (900, false, false),
    ] {
        let step = Q::new(BigInt::from(1), BigInt::from(3u32).pow(k));
        let b: [Point; 4] = unit.map(|p| {
            let mut axis = 0;
            p.map(|x| {
                axis += 1;
                Q::from_integer(BigInt::from(x)) + &step * Q::from_integer(BigInt::from(axis))
                    - half.clone()
            })
        });
        scaled_cells_taken();
        let budget = Budget::new(512_000_000);
        let mut trace = ChargeTrace::default();
        let (outcome, positive) = {
            let work = Work(&budget, RefCell::new(&mut trace));
            let value = convex_volume(&a, &b, &work, None, None);
            let positive = value.as_ref().is_ok_and(Q::is_positive);
            (format!("{value:?}"), positive)
        };
        let cells = scaled_cells_taken();
        let plain_budget = Budget::new(512_000_000);
        let mut plain_trace = ChargeTrace::default();
        let plain = without_scaled_cells(|| {
            let work = Work(&plain_budget, RefCell::new(&mut plain_trace));
            format!("{:?}", convex_volume(&a, &b, &work, None, None))
        });
        assert_eq!(
            scaled_cells_taken(),
            0,
            "k={k} the declined arm scaled a cell"
        );
        assert_eq!(
            cells > 0,
            expect_admitted,
            "k={k}: the preflight did not select the arm this case is for"
        );
        assert_eq!(
            outcome.starts_with("Ok"),
            expect_ok,
            "k={k} unexpected outcome"
        );
        assert_eq!(
            outcome, plain,
            "k={k} outcome differs from the rational path"
        );
        assert_eq!(
            budget.used(),
            plain_budget.used(),
            "k={k} charge differs from the rational path"
        );
        compare_trace(&trace, &plain_trace);
        // A strictly positive volume, so neither verdict is being compared on
        // a shape that integrates to nothing either way.
        assert_eq!(positive, expect_ok, "k={k} the cell integrates to nothing");
        seen.push((k, cells, budget.used()));
    }
    println!("scaled-cell-wide-fallback {{\"rows\":{seen:?}}}");
}
