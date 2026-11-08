//! The accuracy contract of a numeric fact: its `bound`, `achieved`
//! accuracy, provenance and the file's own tolerance, and the interval rule
//! that decides a claim once a fact carries a bound.
//!
//! Every fact the engine computes under `geospec-demand-v6` comes from a path with no error
//! bound (OCCT's fixed-order mass integration, `AddOptimal` boxes, mesh
//! sums), so it is reported as `legacy-fixed-order` with `bound = null`, and a
//! claim on it keeps the comparison it always had. The interval rule, the
//! file-tolerance refusal and the closure refusal below are implemented now
//! but act only on a fact that carries a bound or a file tolerance, which no
//! fact does until the bounded facts layer ships under `geospec-demand-v7`.
//! Until then this record changes no verdict.

use crate::{codec::Json, prepared::NumericExpectation, result::Diagnostic};

/// A claim's tolerance cannot separate pass from fail: the measured interval
/// `[m - bound, m + bound]` straddles the tolerance boundary.
pub(crate) const ACCURACY_INSUFFICIENT: &str = "GEOSPEC_ACCURACY_INSUFFICIENT";
/// The claim's tolerance is tighter than the file itself defines the
/// geometry (its maximum topology tolerance carried into the fact).
pub(crate) const FILE_TOLERANCE_EXCEEDS_CLAIM: &str = "GEOSPEC_FILE_TOLERANCE_EXCEEDS_CLAIM";
/// Volume and centre of mass are facts only of closed shells.
pub(crate) const SUBJECT_NOT_CLOSED: &str = "GEOSPEC_SUBJECT_NOT_CLOSED";

/// How a fact was computed, which decides what its `bound` means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Method {
    /// The `geospec-demand-v6` facts: fixed-order integration or optimisation with no error
    /// bound. `bound` is always null.
    LegacyFixedOrder,
    /// Exact up to round-off (analytic faces, polynomial integrands by degree).
    #[cfg_attr(not(test), allow(dead_code))]
    ExactByDegree,
    /// Adaptive integration with a checked, heuristic error bound.
    #[cfg_attr(not(test), allow(dead_code))]
    Adaptive,
    /// A bracket (the bounding box): an attained and a rigorous outer value
    /// per side, so `bound` (their largest difference) is a guarantee.
    #[cfg_attr(not(test), allow(dead_code))]
    Bracket,
}

impl Method {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyFixedOrder => "legacy-fixed-order",
            Self::ExactByDegree => "exact-by-degree",
            Self::Adaptive => "adaptive",
            Self::Bracket => "bracket",
        }
    }
}

/// Whether `achieved` is relative (extensive facts: volume, area, mass) or
/// absolute in millimetres (positions: centre of mass, bounding box).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scale {
    Extensive,
    Positional,
}

/// The accuracy record of one numeric fact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FactAccuracy {
    /// Absolute error bound in the fact's own unit; `None` is "unbounded".
    pub bound: Option<f64>,
    pub method: Method,
    pub scale: Scale,
    /// The file's own maximum topology tolerance in millimetres, when known.
    pub file_tolerance: Option<f64>,
    /// The file tolerance carried into this fact's unit (for example
    /// `tolerance x area / |volume|` for volume); a claim tolerance below it
    /// refuses with `GEOSPEC_FILE_TOLERANCE_EXCEEDS_CLAIM`.
    pub gap_bound: Option<f64>,
    /// Whether every shell of the subject is closed, when known. A bounded
    /// fact of an open subject refuses with `GEOSPEC_SUBJECT_NOT_CLOSED`.
    pub closed: Option<bool>,
}

impl FactAccuracy {
    /// A fact computed by the `geospec-demand-v6` paths: no bound, no file tolerance.
    pub(crate) const fn legacy(scale: Scale) -> Self {
        Self {
            bound: None,
            method: Method::LegacyFixedOrder,
            scale,
            file_tolerance: None,
            gap_bound: None,
            closed: None,
        }
    }

    /// `achieved`: the bound relative to the measurement for extensive facts,
    /// the bound itself (mm) for positions; null without a bound.
    fn achieved(&self, measured: f64) -> Option<f64> {
        let bound = self.bound?;
        match self.scale {
            Scale::Positional => Some(bound),
            Scale::Extensive if measured != 0.0 && measured.is_finite() => {
                Some(bound / measured.abs())
            }
            Scale::Extensive => None,
        }
    }

    /// Whether every number of the record is finite, so the record encodes
    /// and the interval rule can decide. A legacy fact always is; a bounded
    /// fact with a non-finite bound, or an `achieved` that overflows (a bound
    /// over a subnormal measurement), refuses instead.
    fn is_finite(&self, measured: f64) -> bool {
        self.bound.is_none_or(f64::is_finite)
            && self.achieved(measured).is_none_or(f64::is_finite)
            && self.file_tolerance.is_none_or(f64::is_finite)
    }

    /// The report record. `measured` is the scalar the bound applies to (the
    /// fact itself, or its largest-magnitude coordinate for positions). A
    /// non-finite number is reported as null, so the record always encodes;
    /// `refusal_before_verdict` refuses such a claim.
    pub(crate) fn to_json(self, measured: f64) -> Json {
        let number = |value: Option<f64>| {
            value
                .filter(|value| value.is_finite())
                .map_or(Json::Null, Json::Number)
        };
        Json::object([
            ("bound", number(self.bound)),
            ("achieved", number(self.achieved(measured))),
            (
                "provenance",
                Json::object([("method", Json::string(self.method.as_str()))]),
            ),
            ("fileTolerance", number(self.file_tolerance)),
            (
                "verdictRule",
                Json::string(if self.bound.is_some() {
                    "interval"
                } else {
                    "legacy"
                }),
            ),
        ])
    }

    /// The refusals that come before any comparison: an open subject for a
    /// bounded fact, a record with a non-finite number, and a claim
    /// tolerance tighter than the file's own gap. `tolerance` is `None` when
    /// the claim has no value compared within a tolerance (comparators
    /// only), so the gap refusal does not apply to it. None of these applies
    /// to a legacy fact, so under `geospec-demand-v6` this never refuses.
    pub(crate) fn refusal_before_verdict(
        &self,
        matcher: &str,
        measured: f64,
        tolerance: Option<f64>,
    ) -> Option<Diagnostic> {
        if self.bound.is_some() && self.closed == Some(false) {
            let mut diagnostic = Diagnostic::error(
                SUBJECT_NOT_CLOSED,
                format!("GeoSpec matcher '{matcher}' measures a fact of closed shells only, and the subject has an open shell."),
            );
            diagnostic.suggestion =
                Some("Close the shells in the model, or claim a fact open shells define.".into());
            diagnostic.details = Some(Json::object([("matcher", Json::string(matcher))]));
            return Some(diagnostic);
        }
        if !self.is_finite(measured) {
            return Some(insufficient(
                matcher,
                measured,
                self.bound,
                tolerance.unwrap_or(0.0),
            ));
        }
        let tolerance = tolerance?;
        let gap = self.gap_bound?;
        (tolerance < gap).then(|| {
            let mut diagnostic = Diagnostic::error(
                FILE_TOLERANCE_EXCEEDS_CLAIM,
                format!(
                    "GeoSpec matcher '{matcher}' was asked for tolerance {}, tighter than the file defines the geometry ({} at the file's tolerance).",
                    format_number(tolerance),
                    format_number(gap)
                ),
            );
            diagnostic.suggestion = Some(
                "Raise the claim's tolerance to at least the file's gap, or export the model at a tighter tolerance.".into(),
            );
            diagnostic.details = Some(Json::object([
                ("matcher", Json::string(matcher)),
                ("tolerance", encodable(tolerance)),
                ("gapBound", encodable(gap)),
                (
                    "fileTolerance",
                    self.file_tolerance.map_or(Json::Null, encodable),
                ),
            ]));
            diagnostic
        })
    }
}

/// One side of an interval verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    Pass,
    Fail,
    /// The interval straddles the boundary: refuse, in either polarity.
    Straddle,
}

impl Verdict {
    fn from_bool(holds: bool) -> Self {
        if holds {
            Self::Pass
        } else {
            Self::Fail
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Fail, _) | (_, Self::Fail) => Self::Fail,
            (Self::Straddle, _) | (_, Self::Straddle) => Self::Straddle,
            (Self::Pass, Self::Pass) => Self::Pass,
        }
    }
}

/// `|m - e| <= tol`, with the interval rule when a bound exists:
/// pass iff `|m - e| + bound <= tol`, fail iff `|m - e| - bound > tol`.
/// Without a bound this is exactly the legacy comparison.
pub(crate) fn within(measured: f64, expected: f64, tolerance: f64, bound: Option<f64>) -> Verdict {
    let deviation = (measured - expected).abs();
    let Some(bound) = bound else {
        // Legacy: NaN compares false, so a non-finite deviation fails.
        return Verdict::from_bool(deviation <= tolerance);
    };
    // A bounded fact that is not finite cannot place its interval: refuse,
    // so a negated claim never passes from it.
    if !deviation.is_finite() || !bound.is_finite() {
        return Verdict::Straddle;
    }
    if deviation + bound <= tolerance {
        Verdict::Pass
    } else if deviation - bound > tolerance {
        Verdict::Fail
    } else {
        Verdict::Straddle
    }
}

/// A one-sided comparator under the interval rule: `holds(m)` for every `m`
/// in `[measured - bound, measured + bound]` passes, for none fails.
fn comparator(measured: f64, bound: f64, holds: impl Fn(f64) -> bool) -> Verdict {
    match (holds(measured - bound), holds(measured + bound)) {
        (true, true) => Verdict::Pass,
        (false, false) => Verdict::Fail,
        _ => Verdict::Straddle,
    }
}

/// `NumericExpectation::holds` under the interval rule. Without a bound it
/// is `holds` itself, so a legacy fact keeps its verdict bit for bit.
pub(crate) fn numeric(
    expected: &NumericExpectation,
    measured: f64,
    tolerance: f64,
    bound: Option<f64>,
) -> Verdict {
    let Some(width) = bound else {
        return Verdict::from_bool(expected.holds(measured, tolerance));
    };
    if !measured.is_finite() || !width.is_finite() {
        return Verdict::Straddle;
    }
    match expected {
        NumericExpectation::Equal(value) => within(measured, *value, tolerance, bound),
        NumericExpectation::Conditions {
            value,
            greater_than,
            greater_than_or_equal,
            less_than,
            less_than_or_equal,
        } => [
            value.map(|value| within(measured, value, tolerance, bound)),
            greater_than.map(|limit| comparator(measured, width, |m| m > limit)),
            greater_than_or_equal.map(|limit| comparator(measured, width, |m| m >= limit)),
            less_than.map(|limit| comparator(measured, width, |m| m < limit)),
            less_than_or_equal.map(|limit| comparator(measured, width, |m| m <= limit)),
        ]
        .into_iter()
        .flatten()
        .fold(Verdict::Pass, Verdict::and),
    }
}

/// The typed refusal diagnostic for a straddle. The claim refuses, and a
/// refusal carries no polarity, so a negated claim never passes from a
/// straddle either.
pub(crate) fn insufficient(
    matcher: &str,
    measured: f64,
    bound: Option<f64>,
    tolerance: f64,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        ACCURACY_INSUFFICIENT,
        format!(
            "GeoSpec matcher '{matcher}' measured {} within ±{}, which straddles the claim's tolerance {}; the claim can neither pass nor fail.",
            format_number(measured),
            bound.map_or_else(|| "unbounded".into(), format_number),
            format_number(tolerance)
        ),
    );
    diagnostic.suggestion =
        Some("Widen the claim's tolerance beyond the fact's bound, or correct the model so the measurement clears the boundary.".into());
    diagnostic.details = Some(Json::object([
        ("matcher", Json::string(matcher)),
        ("measured", encodable(measured)),
        ("bound", bound.map_or(Json::Null, encodable)),
        ("tolerance", encodable(tolerance)),
    ]));
    diagnostic
}

/// A diagnostic detail number. Canonical JSON admits finite numbers only, so
/// a non-finite value is reported as its string (`"inf"`, `"-inf"`, `"NaN"`),
/// as the mesh matchers report a non-finite measurement.
fn encodable(value: f64) -> Json {
    if value.is_finite() {
        Json::Number(value)
    } else {
        Json::string(&value.to_string())
    }
}

fn format_number(value: f64) -> String {
    if value.is_finite() {
        ryu_js::Buffer::new().format_finite(value).to_owned()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::encode;

    fn conditions(
        value: Option<f64>,
        greater_than: Option<f64>,
        less_than_or_equal: Option<f64>,
    ) -> NumericExpectation {
        NumericExpectation::Conditions {
            value,
            greater_than,
            greater_than_or_equal: None,
            less_than: None,
            less_than_or_equal,
        }
    }

    #[test]
    fn a_legacy_fact_reports_a_null_bound_and_the_legacy_rule() {
        let record = FactAccuracy::legacy(Scale::Extensive).to_json(1000.0);
        assert_eq!(
            String::from_utf8(encode(&record).unwrap()).unwrap(),
            r#"{"achieved":null,"bound":null,"fileTolerance":null,"provenance":{"method":"legacy-fixed-order"},"verdictRule":"legacy"}"#
        );
    }

    #[test]
    fn without_a_bound_every_comparison_is_the_legacy_one() {
        let samples = [-1.0, 0.0, 0.019_999, 0.02, 0.020_001, 5.0, f64::NAN];
        let expectations = [
            NumericExpectation::Equal(0.0),
            conditions(Some(0.0), None, None),
            conditions(None, Some(0.0), Some(5.0)),
            NumericExpectation::Conditions {
                value: Some(0.01),
                greater_than: Some(-1.0),
                greater_than_or_equal: Some(0.0),
                less_than: Some(5.0),
                less_than_or_equal: Some(5.0),
            },
        ];
        for expected in &expectations {
            for measured in samples {
                assert_eq!(
                    numeric(expected, measured, 0.02, None),
                    Verdict::from_bool(expected.holds(measured, 0.02)),
                    "{expected:?} at {measured}"
                );
            }
        }
        for (actual, expected) in [(0.0, 0.02), (0.0, 0.020_001), (f64::NAN, 0.0)] {
            assert_eq!(
                within(actual, expected, 0.02, None) == Verdict::Fail,
                (actual - expected)
                    .abs()
                    .partial_cmp(&0.02)
                    .is_none_or(std::cmp::Ordering::is_gt)
            );
        }
    }

    #[test]
    fn the_interval_rule_passes_fails_and_refuses_a_straddle() {
        // |m - e| + bound <= tol passes; |m - e| - bound > tol fails.
        assert_eq!(within(10.0, 10.0, 0.5, Some(0.25)), Verdict::Pass);
        assert_eq!(within(10.25, 10.0, 0.5, Some(0.25)), Verdict::Pass);
        assert_eq!(within(10.3, 10.0, 0.5, Some(0.25)), Verdict::Straddle);
        assert_eq!(within(10.75, 10.0, 0.5, Some(0.25)), Verdict::Straddle);
        assert_eq!(within(10.8, 10.0, 0.5, Some(0.25)), Verdict::Fail);
        // A zero bound is the legacy comparison on finite values.
        assert_eq!(within(10.5, 10.0, 0.5, Some(0.0)), Verdict::Pass);
        assert_eq!(within(10.6, 10.0, 0.5, Some(0.0)), Verdict::Fail);
    }

    #[test]
    fn comparators_straddle_when_the_interval_crosses_the_limit() {
        let greater = conditions(None, Some(100.0), None);
        assert_eq!(numeric(&greater, 101.0, 0.0, Some(0.5)), Verdict::Pass);
        assert_eq!(numeric(&greater, 100.2, 0.0, Some(0.5)), Verdict::Straddle);
        assert_eq!(numeric(&greater, 99.0, 0.0, Some(0.5)), Verdict::Fail);
        // Any failing part fails the claim; otherwise any straddle refuses.
        let both = conditions(Some(100.0), Some(50.0), Some(100.1));
        assert_eq!(numeric(&both, 100.0, 1.0, Some(0.05)), Verdict::Pass);
        assert_eq!(numeric(&both, 100.0, 1.0, Some(0.2)), Verdict::Straddle);
        assert_eq!(numeric(&both, 102.0, 1.0, Some(0.2)), Verdict::Fail);
    }

    #[test]
    fn a_straddle_refuses_in_both_polarities() {
        use crate::{
            registry::Capability,
            result::{Evaluation, Polarity},
        };
        for polarity in [Polarity::Positive, Polarity::Negative] {
            let result = crate::result::finish(
                "c",
                Capability::ToHaveVolume,
                polarity,
                Evaluation::Refused {
                    diagnostics: vec![insufficient("toHaveVolume", 10.3, Some(0.25), 0.5)],
                },
            )
            .unwrap();
            let text = String::from_utf8(encode(&result).unwrap()).unwrap();
            assert!(text.contains(r#""status":"refused""#), "{text}");
            assert!(text.contains(ACCURACY_INSUFFICIENT), "{text}");
        }
    }

    #[test]
    fn bounded_facts_report_achieved_accuracy_and_the_interval_rule() {
        let volume = FactAccuracy {
            bound: Some(0.5),
            method: Method::Adaptive,
            ..FactAccuracy::legacy(Scale::Extensive)
        };
        assert_eq!(
            String::from_utf8(encode(&volume.to_json(-1000.0)).unwrap()).unwrap(),
            r#"{"achieved":0.0005,"bound":0.5,"fileTolerance":null,"provenance":{"method":"adaptive"},"verdictRule":"interval"}"#
        );
        let centre = FactAccuracy {
            bound: Some(1e-9),
            method: Method::ExactByDegree,
            file_tolerance: Some(1e-7),
            ..FactAccuracy::legacy(Scale::Positional)
        };
        assert_eq!(
            String::from_utf8(encode(&centre.to_json(42.0)).unwrap()).unwrap(),
            r#"{"achieved":1e-9,"bound":1e-9,"fileTolerance":1e-7,"provenance":{"method":"exact-by-degree"},"verdictRule":"interval"}"#
        );
        let side = FactAccuracy {
            bound: Some(2e-6),
            method: Method::Bracket,
            ..FactAccuracy::legacy(Scale::Positional)
        };
        assert_eq!(
            String::from_utf8(encode(&side.to_json(10.0)).unwrap()).unwrap(),
            r#"{"achieved":0.000002,"bound":0.000002,"fileTolerance":null,"provenance":{"method":"bracket"},"verdictRule":"interval"}"#
        );
    }

    #[test]
    fn closure_and_file_gap_refuse_only_when_known() {
        let legacy = FactAccuracy::legacy(Scale::Extensive);
        assert!(legacy
            .refusal_before_verdict("toHaveVolume", 1000.0, Some(0.0))
            .is_none());
        let open = FactAccuracy {
            bound: Some(1e-6),
            closed: Some(false),
            ..legacy
        };
        let code = |diagnostic: Option<Diagnostic>| diagnostic.map(|value| value.code);
        assert_eq!(
            code(open.refusal_before_verdict("toHaveVolume", 1000.0, Some(1.0))).as_deref(),
            Some(SUBJECT_NOT_CLOSED)
        );
        // An open subject without a bound keeps the legacy behaviour.
        let open_legacy = FactAccuracy {
            closed: Some(false),
            ..legacy
        };
        assert!(open_legacy
            .refusal_before_verdict("toHaveVolume", 1000.0, Some(1.0))
            .is_none());
        let gap = FactAccuracy {
            file_tolerance: Some(1e-3),
            gap_bound: Some(0.4),
            ..legacy
        };
        assert_eq!(
            code(gap.refusal_before_verdict("toHaveVolume", 1000.0, Some(0.02))).as_deref(),
            Some(FILE_TOLERANCE_EXCEEDS_CLAIM)
        );
        assert!(gap
            .refusal_before_verdict("toHaveVolume", 1000.0, Some(0.4))
            .is_none());
        // A comparator-only claim compares no value within a tolerance, so
        // the file's gap does not refuse it.
        assert!(gap
            .refusal_before_verdict("toHaveVolume", 1000.0, None)
            .is_none());
    }

    #[test]
    fn a_bounded_fact_that_is_not_finite_refuses_in_both_polarities() {
        let greater = conditions(None, Some(100.0), None);
        for (measured, bound) in [
            (f64::NAN, 0.25),
            (f64::INFINITY, 0.25),
            (10.0, f64::INFINITY),
            (10.0, f64::NAN),
        ] {
            assert_eq!(
                within(measured, 10.0, 0.5, Some(bound)),
                Verdict::Straddle,
                "{measured} ± {bound}"
            );
            assert_eq!(
                numeric(&greater, measured, 0.5, Some(bound)),
                Verdict::Straddle,
                "{measured} ± {bound}"
            );
            assert_eq!(
                numeric(&NumericExpectation::Equal(10.0), measured, 0.5, Some(bound)),
                Verdict::Straddle,
                "{measured} ± {bound}"
            );
        }
        // Without a bound a non-finite fact keeps the legacy comparison.
        assert_eq!(within(f64::NAN, 10.0, 0.5, None), Verdict::Fail);
    }

    #[test]
    fn a_record_that_would_not_encode_reports_null_and_refuses() {
        // A bound over a subnormal measurement overflows `achieved`.
        let overflow = FactAccuracy {
            bound: Some(1.0),
            method: Method::Adaptive,
            ..FactAccuracy::legacy(Scale::Extensive)
        };
        assert_eq!(
            String::from_utf8(encode(&overflow.to_json(1e-320)).unwrap()).unwrap(),
            r#"{"achieved":null,"bound":1,"fileTolerance":null,"provenance":{"method":"adaptive"},"verdictRule":"interval"}"#
        );
        let infinite = FactAccuracy {
            bound: Some(f64::INFINITY),
            file_tolerance: Some(f64::NAN),
            ..FactAccuracy::legacy(Scale::Positional)
        };
        assert_eq!(
            String::from_utf8(encode(&infinite.to_json(0.0)).unwrap()).unwrap(),
            r#"{"achieved":null,"bound":null,"fileTolerance":null,"provenance":{"method":"legacy-fixed-order"},"verdictRule":"interval"}"#
        );
        let code = |diagnostic: Option<Diagnostic>| diagnostic.map(|value| value.code);
        // The whole refused result, details included, encodes canonically in
        // either polarity, whichever of measurement, bound or tolerance is
        // not finite.
        let nan_measured = FactAccuracy {
            bound: Some(0.5),
            ..FactAccuracy::legacy(Scale::Extensive)
        };
        for (fact, measured, tolerance) in [
            (overflow, 1e-320, Some(1.0)),
            (infinite, 0.0, Some(1.0)),
            (nan_measured, f64::NAN, Some(1.0)),
            (nan_measured, f64::NEG_INFINITY, None),
        ] {
            let refusal = fact
                .refusal_before_verdict("toHaveVolume", measured, tolerance)
                .unwrap_or_else(|| {
                    // A non-finite measurement with a finite record refuses
                    // at the verdict instead.
                    assert_eq!(within(measured, 10.0, 1.0, fact.bound), Verdict::Straddle);
                    insufficient("toHaveVolume", measured, fact.bound, f64::INFINITY)
                });
            for polarity in [
                crate::result::Polarity::Positive,
                crate::result::Polarity::Negative,
            ] {
                let result = crate::result::finish(
                    "c",
                    crate::registry::Capability::ToHaveVolume,
                    polarity,
                    crate::result::Evaluation::Refused {
                        diagnostics: vec![refusal.clone()],
                    },
                )
                .unwrap();
                let text = String::from_utf8(encode(&result).unwrap()).unwrap();
                assert!(text.contains(r#""status":"refused""#), "{text}");
                assert!(text.contains(ACCURACY_INSUFFICIENT), "{text}");
            }
        }
        let gap = FactAccuracy {
            gap_bound: Some(f64::INFINITY),
            ..FactAccuracy::legacy(Scale::Extensive)
        };
        // A non-finite file gap still refuses as a file gap, and encodes.
        let refusal = gap
            .refusal_before_verdict("toHaveVolume", 1000.0, Some(1.0))
            .unwrap();
        assert_eq!(refusal.code, FILE_TOLERANCE_EXCEEDS_CLAIM);
        let result = crate::result::finish(
            "c",
            crate::registry::Capability::ToHaveVolume,
            crate::result::Polarity::Negative,
            crate::result::Evaluation::Refused {
                diagnostics: vec![refusal],
            },
        )
        .unwrap();
        let text = String::from_utf8(encode(&result).unwrap()).unwrap();
        assert!(text.contains(r#""gapBound":"inf""#), "{text}");
        for (fact, measured) in [(overflow, 1e-320), (infinite, 0.0)] {
            assert_eq!(
                code(fact.refusal_before_verdict("toHaveVolume", measured, Some(1.0))).as_deref(),
                Some(ACCURACY_INSUFFICIENT)
            );
            assert_eq!(
                code(fact.refusal_before_verdict("toHaveVolume", measured, None)).as_deref(),
                Some(ACCURACY_INSUFFICIENT)
            );
        }
        // A finite bounded record does not refuse here.
        assert!(overflow
            .refusal_before_verdict("toHaveVolume", 1000.0, Some(1.0))
            .is_none());
    }
}
