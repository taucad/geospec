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
