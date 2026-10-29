use super::super::tests::{boxes, cube};
use super::*;

fn reference(left: &MeshAnalysisRecord, right: &MeshAnalysisRecord) -> Q {
    let budget = Budget::new(512_000_000);
    let limit = 256 * 1024 * 1024;
    let a = qualify(Rc::new(left.clone()), &budget, limit).unwrap();
    let b = qualify(Rc::new(right.clone()), &budget, limit).unwrap();
    let mut trace = ChargeTrace::disabled();
    let work = Work(&budget, RefCell::new(&mut trace));
    let mut result = Q::zero();
    for i in 0..left.triangles.len() {
        let p = coordinates(left, i).map(|v| v.map(|x| arithmetic::rational(x).unwrap()));
        let pd = determinant(&p, &work).unwrap();
        if pd.is_zero() {
            continue;
        }
        for j in 0..right.triangles.len() {
            let q = coordinates(right, j).map(|v| v.map(|x| arithmetic::rational(x).unwrap()));
            let qd = determinant(&q, &work).unwrap();
            if qd.is_zero() || disjoint(&p, &q, &work).unwrap() {
                continue;
            }
            let cell = convex_volume(&p, &q, &work, None, None).unwrap();
            if pd.is_negative() ^ a.orientation[i] ^ qd.is_negative() ^ b.orientation[j] {
                result = work.sub(&result, &cell).unwrap();
            } else {
                result = work.add(&result, &cell).unwrap();
            }
        }
    }
    result
}

#[test]
fn material_certificates_match_signed_scalar_in_both_orders() {
    let solid = cube([0.; 3], [4.; 3]);
    let hollow = boxes(&[([0.; 3], [4.; 3], false), ([1.; 3], [3.; 3], true)]);
    let disconnected = boxes(&[([0.; 3], [1.; 3], false), ([3.; 3], [4.; 3], false)]);
    for (a, b, expected) in [
        (solid.clone(), cube([5.; 3], [6.; 3]), 0),
        (solid.clone(), cube([1.; 3], [2.; 3]), 1),
        (solid.clone(), cube([3.; 3], [5.; 3]), 1),
        (solid.clone(), cube([4., 0., 0.], [5., 4., 4.]), 0),
        (solid.clone(), cube([4., 4., 0.], [5., 5., 4.]), 0),
        (solid.clone(), cube([4.; 3], [5.; 3]), 0),
        (hollow.clone(), cube([1.5; 3], [2.5; 3]), 0),
        (disconnected, cube([1.5; 3], [2.5; 3]), 0),
        (solid.clone(), solid, 64),
    ] {
        let scalar = Q::from_integer(expected.into());
        for (left, right) in [(&a, &b), (&b, &a)] {
            assert_eq!(reference(left, right), scalar);
            assert_eq!(
                intersection(left, right, &Budget::new(512_000_000), 256 * 1024 * 1024).unwrap(),
                scalar
            );
        }
    }
    let crossed = cube([0.5; 3], [2.; 3]);
    assert_eq!(
        intersection(
            &hollow,
            &crossed,
            &Budget::new(512_000_000),
            256 * 1024 * 1024
        )
        .unwrap(),
        reference(&hollow, &crossed)
    );
}

#[test]
fn certificate_capacity_is_optional_and_work_exhaustion_refuses() {
    let hollow = boxes(&[([0.; 3], [4.; 3], false), ([1.; 3], [3.; 3], true)]);
    let inner = cube([1.5; 3], [2.5; 3]);
    let budget = Budget::new(512_000_000);
    let a = qualify(Rc::new(hollow), &budget, 256 * 1024 * 1024).unwrap();
    let b = qualify(Rc::new(inner), &budget, 256 * 1024 * 1024).unwrap();
    let required = WORKSPACE_BYTES + a.allocated_bytes() + b.allocated_bytes();
    assert!(
        intersection_qualified(&a, &b, &budget, required, &mut ChargeTrace::disabled())
            .unwrap()
            .is_zero()
    );
    assert!(matches!(
        intersection_qualified(&a, &b, &budget, required - 1, &mut ChargeTrace::disabled())
            .unwrap_err()
            .kind,
        BackendErrorKind::LimitExceeded(_)
    ));
    let error = intersection_qualified(
        &a,
        &b,
        &Budget::new(1),
        256 * 1024 * 1024,
        &mut ChargeTrace::disabled(),
    )
    .unwrap_err();
    assert!(matches!(
        error.kind,
        BackendErrorKind::BudgetExceeded { .. }
    ));
    let mut trace = ChargeTrace::disabled();
    let tiny = Budget::new(1);
    let work = Work(&tiny, RefCell::new(&mut trace));
    assert!(matches!(
        zero_material(&a.record, &b.record, &work, 256 * 1024 * 1024)
            .unwrap_err()
            .kind,
        BackendErrorKind::BudgetExceeded { .. }
    ));
}

#[test]
fn combined_span_is_checked_and_extremes_do_not_select_filters() {
    assert!(material::scalar_certificate_eligible(
        [[0., -0., 1.], [2f64.powi(-32), 2f64.powi(30), 0.]].iter()
    ));
    assert!(!material::scalar_certificate_eligible(
        [[2f64.powi(-80), 0., 0.], [2f64.powi(80), 0., 0.]].iter()
    ));
    for x in [f64::MAX, f64::from_bits(1), f64::INFINITY, f64::NAN] {
        assert!(!material::scalar_certificate_eligible([[x, 0., 0.]].iter()));
    }
}

#[test]
fn exact_filters_agree_with_scalar_cells_including_large_integer_fallback() {
    let mut state = 0x81a274u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 32) % 17
    };
    let mut proved = 0;
    for scale in [1., 2f64.powi(45), 2f64.powi(-40)] {
        for _ in 0..64 {
            let a: [[f64; 3]; 4] =
                std::array::from_fn(|_| std::array::from_fn(|_| next() as f64 / 4. * scale));
            let b: [[f64; 3]; 4] =
                std::array::from_fn(|_| std::array::from_fn(|_| next() as f64 / 4. * scale));
            assert!(material::scalar_certificate_eligible(a.iter().chain(&b)));
            let budget = Budget::new(8_000_000);
            let mut trace = ChargeTrace::disabled();
            let work = Work(&budget, RefCell::new(&mut trace));
            let rational =
                |p: [[f64; 3]; 4]| p.map(|v| v.map(|x| arithmetic::rational(x).unwrap()));
            let (aq, bq) = (rational(a), rational(b));
            if determinant(&aq, &work).unwrap().is_zero()
                || determinant(&bq, &work).unwrap().is_zero()
            {
                continue;
            }
            if separated(&a, &b, &work).unwrap() {
                proved += 1;
                assert!(convex_volume(&aq, &bq, &work, None, None)
                    .unwrap()
                    .is_zero());
            }
        }
    }
    assert!(proved > 10);
}

#[test]
fn coplanar_faces_retain_scalar_path_and_thin_overlap_is_not_filtered() {
    let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let b = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., -1.]];
    let budget = Budget::new(8_000_000);
    let mut trace = ChargeTrace::disabled();
    let work = Work(&budget, RefCell::new(&mut trace));
    assert!(!separated(&a, &b, &work).unwrap());
    let shifted = a.map(|p| [p[0] + (1. - 2f64.powi(-40)), p[1], p[2]]);
    assert!(!separated(&a, &shifted, &work).unwrap());
    let far = a.map(|p| [p[0] + 3., p[1], p[2]]);
    assert!(separated(&a, &far, &work).unwrap());
}

/// Two independently closed shells under separate primitive labels: the
/// partition the combined certificate record must not collapse.
fn labelled_boxes(specs: &[([f64; 3], [f64; 3], bool)]) -> MeshAnalysisRecord {
    let mut result = boxes(specs);
    let per_shell = result.triangles.len() / specs.len();
    result.primitives = (0..specs.len())
        .map(|index| Primitive {
            name: format!("shell-{index}"),
            vertex_start: (index * 8) as u32,
            vertex_count: 8,
        })
        .collect();
    result.triangle_primitives = (0..result.triangles.len())
        .map(|triangle| (triangle / per_shell) as u32)
        .collect();
    result
}

#[test]
fn a_multi_primitive_operand_declines_the_zero_material_certificate() {
    // Relabelling this operand as one primitive would turn its inward shell
    // into a same-primitive cavity, make the combined clusters look pure and
    // certify zero for a genuine positive overlap.
    let left = labelled_boxes(&[([0.; 3], [8.; 3], false), ([2.; 3], [6.; 3], true)]);
    assert_eq!(left.primitives.len(), 2);
    let right = cube([3.; 3], [5.; 3]);
    let scalar = reference(&left, &right);
    assert!(!scalar.is_zero(), "the witness must be a positive overlap");
    for (a, b) in [(&left, &right), (&right, &left)] {
        assert_eq!(
            intersection(a, b, &Budget::new(512_000_000), 256 * 1024 * 1024).unwrap(),
            scalar
        );
    }
    let budget = Budget::new(512_000_000);
    let mut trace = ChargeTrace::default();
    let work = Work(&budget, RefCell::new(&mut trace));
    assert!(
        !zero_material(&left, &right, &work, 256 * 1024 * 1024).unwrap(),
        "a multi-primitive operand declines instead of relabelling"
    );
    assert_eq!(budget.used(), 0, "declining costs no work");
}

#[test]
fn point_and_edge_contact_filters_admit_fewer_cells_than_the_centroid_stage_needs() {
    let budget = Budget::new(8_000_000);
    let mut trace = ChargeTrace::disabled();
    let work = Work(&budget, RefCell::new(&mut trace));
    let rational = |p: [[f64; 3]; 4]| p.map(|v| v.map(|x| arithmetic::rational(x).unwrap()));
    let own = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    // One far vertex on the separating plane: a point contact.
    let point_contact = [[0., 0., 0.], [1., 0., -1.], [0., 1., -1.], [0., 0., -1.]];
    // Two far vertices on the separating plane: an edge contact.
    let edge_contact = [[0., 0., 0.], [1., 0., 0.], [0., 1., -1.], [0., 0., -1.]];
    for other in [point_contact, edge_contact] {
        assert!(separated(&own, &other, &work).unwrap());
        assert!(
            convex_volume(&rational(own), &rational(other), &work, None, None)
                .unwrap()
                .is_zero()
        );
    }
}

#[test]
fn a_certificate_miss_leaves_the_optional_trace_ceiling_where_it_found_it() {
    let left = cube([0.; 3], [4.; 3]);
    let right = cube([1.; 3], [2.; 3]);
    let budget = Budget::new(512_000_000);
    let mut trace = ChargeTrace::default();
    let before = trace.limit_bytes();
    let work = Work(&budget, RefCell::new(&mut trace));
    // A containment overlap: the clusters union, so the certificate misses.
    assert!(!zero_material(&left, &right, &work, 256 * 1024 * 1024).unwrap());
    assert_eq!(work.1.borrow().limit_bytes(), before);
    assert!(
        work.1.borrow().complete,
        "a miss does not discard the trace"
    );
}
