//! Capacity and domain refusals keep distinct, typed outcomes (CAP-1).

use crate::{
    backend::{CapacityLimit, LimitUnit},
    budget::BudgetExceeded,
    codec::{encode, Json},
    registry::Capability,
    result::{finish, Diagnostic, Evaluation, Polarity},
    subject::{continuous_refusal, FIXED_CAPACITY_SUGGESTION},
};
use num_bigint::BigInt;
use num_rational::BigRational;

use super::tests::box_domain;
use super::*;
use crate::backend::brep::Bounds;

fn region() -> Bounds {
    Bounds {
        min: [0.0, 0.0, 0.0],
        max: [8.0, 4.0, 4.0],
    }
}

/// `count` disjoint unit-thick slabs along x, one occurrence each.
fn slabs(count: u32) -> Vec<crate::backend::brep::SelectedContinuousDomain> {
    (0..count)
        .map(|index| {
            let x = f64::from(index) * 0.25;
            box_domain(index + 1, [x, 1.0, 1.0], [x + 0.125, 3.0, 3.0])
        })
        .collect()
}

fn refusal(evaluation: Evaluation) -> Diagnostic {
    let Evaluation::Refused { mut diagnostics } = evaluation else {
        panic!("expected a refusal");
    };
    assert_eq!(diagnostics.len(), 1);
    diagnostics.remove(0)
}

fn details(diagnostic: &Diagnostic) -> String {
    String::from_utf8(encode(diagnostic.details.as_ref().expect("details")).unwrap()).unwrap()
}

fn status_and_code(evaluation: Evaluation) -> (String, String) {
    let Json::Object(fields) = finish(
        "outcome",
        Capability::ToHaveVoidContinuity,
        Polarity::Positive,
        evaluation,
    )
    .unwrap() else {
        panic!("canonical result object");
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .expect(name)
    };
    let Json::String(status) = field("status") else {
        panic!("status string");
    };
    let Json::Array(diagnostics) = field("diagnostics") else {
        panic!("diagnostics array");
    };
    let Some(Json::Object(first)) = diagnostics.first() else {
        panic!("one diagnostic");
    };
    let Some((_, Json::String(code))) = first.iter().find(|(key, _)| key == "code") else {
        panic!("diagnostic code");
    };
    (status, code.clone())
}

#[test]
fn a_seventeenth_distinct_material_is_a_capacity_limit_with_numbers() {
    let error = match GridPlan::new(&slabs(17), region()) {
        Err(error) => error,
        Ok(_) => panic!("17 distinct materials exceed the arrangement"),
    };
    assert_eq!(error.kind, ContinuousErrorKind::ResourceLimit);
    assert_eq!(
        error.limit,
        Some(CapacityLimit::analysis(
            LimitUnit::Count("distinct materials"),
            17,
            16
        ))
    );
    assert_eq!(error.condition, None);
    let diagnostic = refusal(continuous_refusal(error));
    assert_eq!(diagnostic.code, "GEOSPEC_ANALYSIS_LIMIT");
    assert_eq!(
        diagnostic.message,
        "The nominal continuous void arrangement needs 17 distinct materials; the analysis limit is 16 distinct materials."
    );
    assert_eq!(
        details(&diagnostic),
        r#"{"limit":16,"limitClass":"analysis","required":17,"unit":"distinct materials"}"#
    );
    assert_eq!(
        diagnostic.suggestion.as_deref(),
        Some(FIXED_CAPACITY_SUGGESTION)
    );

    // Green neighbours: sixteen distinct materials, also when repeated.
    assert!(GridPlan::new(&slabs(16), region()).is_ok());
    let mut repeated = slabs(16);
    repeated.push(repeated[0].clone());
    assert!(GridPlan::new(&repeated, region()).is_ok());
    // Repeats do not count toward what the claim needs.
    assert_eq!(
        material_limit(&[1, 2, 2, 3]).limit,
        Some(CapacityLimit::analysis(
            LimitUnit::Count("distinct materials"),
            3,
            16
        ))
    );
}

#[test]
fn the_exact_arithmetic_guard_reports_the_bits_it_needed() {
    let large = BigRational::from_integer(BigInt::from(1) << 8191usize);
    let two = exact::rational(2.0).unwrap();
    let error = exact::multiply(&large, &two).unwrap_err();
    assert_eq!(error.kind, ContinuousErrorKind::ArithmeticLimit);
    assert_eq!(
        error.limit,
        Some(CapacityLimit::analysis(
            LimitUnit::Count("bits"),
            8194,
            8192
        ))
    );
    let diagnostic = refusal(continuous_refusal(error));
    assert_eq!(diagnostic.code, "GEOSPEC_ANALYSIS_LIMIT");
    assert_eq!(
        details(&diagnostic),
        r#"{"limit":8192,"limitClass":"analysis","required":8194,"unit":"bits"}"#
    );
    // Green neighbour: a product inside the guard is exact.
    let small = BigRational::from_integer(BigInt::from(1) << 100usize);
    assert_eq!(
        exact::multiply(&small, &two).unwrap(),
        BigRational::from_integer(BigInt::from(1) << 101usize)
    );
}

#[test]
fn exhausted_bore_slab_refinement_is_a_capacity_limit_not_a_domain_gap() {
    let error = bore_slab_interference::refinement_limit();
    assert_eq!(error.kind, ContinuousErrorKind::ResourceLimit);
    assert_eq!(
        error.limit,
        Some(CapacityLimit::analysis(
            LimitUnit::Count("refinement panels"),
            16_384,
            8_192
        ))
    );
    let diagnostic = refusal(continuous_refusal(error));
    assert_eq!(diagnostic.code, "GEOSPEC_ANALYSIS_LIMIT");
    assert!(diagnostic.message.contains("within 8192 refinement panels"));
}

#[test]
fn a_domain_refusal_names_its_failed_condition_without_capacity_numbers() {
    let topology = GridPlan::new(&slabs(4), region()).unwrap().build().unwrap();
    let Outcome::Unsupported { reason, .. } = topology.evaluate_points(PointRequest {
        waypoints: &[[0.0625, 0.5, 0.5], [9.0, 0.5, 0.5]],
        isolated_from: &[],
    }) else {
        panic!("a waypoint outside the region is outside the domain");
    };
    assert_eq!(reason.kind, ContinuousErrorKind::UnsupportedDomain);
    assert_eq!(reason.condition, Some("waypoint-inside-region"));
    assert_eq!(reason.limit, None);
    let diagnostic = refusal(continuous_refusal(reason));
    assert_eq!(diagnostic.code, "GEOSPEC_EVIDENCE_UNSUPPORTED");
    assert_eq!(
        details(&diagnostic),
        r#"{"applicability":{"condition":"waypoint-inside-region"}}"#
    );
    assert_eq!(diagnostic.suggestion, None);
    // Green neighbour: both waypoints inside the region are decided.
    assert!(matches!(
        topology.evaluate_points(PointRequest {
            waypoints: &[[7.5, 0.5, 0.5], [7.5, 3.5, 3.5]],
            isolated_from: &[],
        }),
        Outcome::Decided { .. }
    ));
}

/// The five outcome classes of the capacity brief stay distinguishable by
/// status and code: a false claim fails, and each kind of non-verdict refuses
/// with a code no other class uses. Cancellation is a host outcome: the
/// engine never emits it for a claim.
#[test]
fn the_five_outcome_classes_stay_distinguishable_by_code() {
    // Geometrically false: a decided proof whose positive claim fails.
    let (mut materials, region) = (
        vec![
            box_domain(1, [0.0, 0.0, 0.0], [4.0, 1.0, 4.0]),
            box_domain(2, [0.0, 3.0, 0.0], [4.0, 4.0, 4.0]),
            box_domain(3, [0.0, 1.0, 0.0], [4.0, 3.0, 1.0]),
            box_domain(4, [0.0, 1.0, 3.0], [4.0, 3.0, 4.0]),
        ],
        Bounds {
            min: [0.0; 3],
            max: [4.0; 3],
        },
    );
    materials.push(box_domain(5, [1.5, 1.0, 1.0], [2.5, 3.0, 3.0]));
    let topology = GridPlan::new(&materials, region).unwrap().build().unwrap();
    let Outcome::Decided { positive, .. } = topology.evaluate_points(PointRequest {
        waypoints: &[[0.5, 2.0, 2.0], [3.5, 2.0, 2.0]],
        isolated_from: &[],
    }) else {
        panic!("a blocked channel is decided");
    };
    assert!(!positive);
    let false_claim = status_and_code(Evaluation::Geometric {
        positive_satisfied: positive,
        diagnostics: vec![Diagnostic::error(
            "GEOSPEC_VOID_CONTINUITY_MISMATCH",
            "The waypoints are disconnected.",
        )],
        evidence: Json::Object(Vec::new()),
        negated_diagnostic: None,
    });

    // Malformed input, an unsupported domain and two resource outcomes.
    let malformed = status_and_code(continuous_refusal(ContinuousError::invalid(
        "Continuous-domain coordinates and thresholds must be finite.",
    )));
    let unsupported = status_and_code(continuous_refusal(ContinuousError::outside(
        "complete-axis-aligned-box",
        "Continuous evidence requires a complete axis-aligned box.",
    )));
    let capacity = status_and_code(continuous_refusal(material_limit(
        &(1..=17).collect::<Vec<_>>(),
    )));
    let work = status_and_code(Evaluation::budget_exceeded(
        Capability::ToHaveVoidContinuity,
        BudgetExceeded {
            limit: 10,
            used: 11,
        },
    ));

    assert_eq!(
        false_claim,
        ("failed".into(), "GEOSPEC_VOID_CONTINUITY_MISMATCH".into())
    );
    assert_eq!(
        malformed,
        ("refused".into(), "GEOSPEC_INVALID_EVIDENCE".into())
    );
    assert_eq!(
        unsupported,
        ("refused".into(), "GEOSPEC_EVIDENCE_UNSUPPORTED".into())
    );
    assert_eq!(
        capacity,
        ("refused".into(), "GEOSPEC_ANALYSIS_LIMIT".into())
    );
    assert_eq!(work, ("refused".into(), "MATCHER_TIMEOUT".into()));

    // No class shares a code, and none uses the evidence-gap code or a host
    // interruption reason.
    let codes = [
        &false_claim.1,
        &malformed.1,
        &unsupported.1,
        &capacity.1,
        &work.1,
    ];
    for (index, code) in codes.iter().enumerate() {
        assert!(!codes[index + 1..].contains(code), "{code}");
        assert_ne!(code.as_str(), "GEOSPEC_UNSUPPORTED_EVIDENCE");
        for interruption in [
            "cancelled",
            "executor-exited",
            "deadline",
            "engine-error",
            "OPERATION_TIMEOUT",
        ] {
            assert_ne!(code.as_str(), interruption);
        }
    }
}
