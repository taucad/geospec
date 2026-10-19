//! The single owner of effective polarity and canonical result envelopes.

use crate::{budget::BudgetExceeded, codec::Json, registry::Capability, ProtocolError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Polarity {
    Positive,
    Negative,
}

impl Polarity {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub suggestion: Option<String>,
    pub details: Option<Json>,
    pub spatial: Option<Json>,
}

impl Diagnostic {
    pub(crate) fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            suggestion: None,
            details: None,
            spatial: None,
        }
    }

    pub(crate) fn info(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            ..Self::error(code, message)
        }
    }

    pub(crate) fn to_json(&self) -> Json {
        let mut fields = vec![
            ("code".into(), Json::string(&self.code)),
            (
                "severity".into(),
                Json::string(match self.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "info",
                }),
            ),
            ("message".into(), Json::string(&self.message)),
        ];
        if let Some(value) = &self.suggestion {
            fields.push(("suggestion".into(), Json::string(value)));
        }
        if let Some(value) = &self.details {
            fields.push(("details".into(), value.clone()));
        }
        if let Some(value) = &self.spatial {
            fields.push(("spatial".into(), value.clone()));
        }
        Json::Object(fields)
    }
}

pub(crate) enum Evaluation {
    Geometric {
        positive_satisfied: bool,
        diagnostics: Vec<Diagnostic>,
        evidence: Json,
        /// A source-defined negative diagnostic, e.g. the unchanged bbox row.
        negated_diagnostic: Option<Box<Diagnostic>>,
    },
    Refused {
        diagnostics: Vec<Diagnostic>,
    },
    Ancillary {
        success: bool,
        value: Json,
        diagnostics: Vec<Diagnostic>,
    },
}

impl Evaluation {
    /// Appends subject notes to any outcome. `finish` keeps every
    /// non-error diagnostic of every status, so an info note never changes
    /// a claim's status.
    pub(crate) fn with_notes(mut self, notes: Vec<Diagnostic>) -> Self {
        debug_assert!(notes.iter().all(|note| note.severity == Severity::Info));
        match &mut self {
            Self::Geometric { diagnostics, .. }
            | Self::Refused { diagnostics }
            | Self::Ancillary { diagnostics, .. } => diagnostics.extend(notes),
        }
        self
    }

    pub(crate) fn budget_exceeded(capability: Capability, exceeded: BudgetExceeded) -> Self {
        let mut diagnostic = Diagnostic::error("MATCHER_TIMEOUT", format!(
            "GeoSpec matcher '{}' exhausted its {} work-unit execution budget and was abandoned (oversized or non-terminating proof).", capability.name(), exceeded.limit
        ));
        diagnostic.suggestion = Some("Narrow the claim, adjust evidence precision where the matcher permits it, or report a reproducible corpus that proves the canonical budget needs recalibration.".into());
        diagnostic.details = Some(Json::object([
            ("matcher", Json::string(capability.name())),
            ("budget", Json::Number(exceeded.limit as f64)),
            ("unitsUsed", Json::Number(exceeded.used as f64)),
            ("unit", Json::string("work-units")),
        ]));
        Self::Refused {
            diagnostics: vec![diagnostic],
        }
    }
}

/// Common full-family evidence. Family owners supply complete named witness
/// records; effective polarity is added once by finish().
pub(crate) fn family_evidence(
    content_hash: &str,
    normalized_expected: Json,
    measured: Json,
    witnesses: Json,
) -> Json {
    Json::object([
        ("profile", Json::string("geospec-original24-v1")),
        ("subjectContentHash", Json::string(content_hash)),
        ("normalizedExpected", normalized_expected),
        ("measured", measured),
        ("witnesses", witnesses),
    ])
}

pub(crate) fn finish(
    claim_id: &str,
    capability: Capability,
    polarity: Polarity,
    evaluation: Evaluation,
) -> Result<Json, ProtocolError> {
    crate::prepared::validate_query_polarity(capability, polarity)?;
    let (status, diagnostics, evidence) = match evaluation {
        Evaluation::Refused { diagnostics } => ("refused", diagnostics, None),
        Evaluation::Ancillary {
            success,
            value,
            diagnostics,
        } => (
            if success
                && !diagnostics
                    .iter()
                    .any(|item| item.severity == Severity::Error)
            {
                "passed"
            } else {
                "failed"
            },
            diagnostics,
            Some(value),
        ),
        Evaluation::Geometric {
            positive_satisfied,
            diagnostics,
            mut evidence,
            negated_diagnostic,
        } => {
            // Polarity is engine-owned even when the family supplies its measured fields.
            if let Json::Object(fields) = &mut evidence {
                fields.retain(|(key, _)| key != "polarity" && key != "positiveSatisfied");
                fields.push(("polarity".into(), Json::string(polarity.as_str())));
                fields.push(("positiveSatisfied".into(), Json::Bool(positive_satisfied)));
            }
            let passed = positive_satisfied == (polarity == Polarity::Positive);
            let selected = if !passed && polarity == Polarity::Positive {
                diagnostics
            } else {
                // H9: the informational diagnostics are moved, not cloned.
                let informational = diagnostics
                    .into_iter()
                    .filter(|diagnostic| diagnostic.severity != Severity::Error);
                if passed {
                    informational.collect()
                } else {
                    let mut selected = vec![negated_diagnostic.map(|value| *value).unwrap_or_else(|| {
                        let mut diagnostic = Diagnostic::error("GEOSPEC_NEGATED_MATCH", format!("GeoSpec matcher '{}' satisfies the declared requirement, but the claim requires it not to.", capability.name()));
                        diagnostic.suggestion = Some("Correct the model, or revise the negated requirement.".into());
                        diagnostic.details = Some(Json::object([("matcher", Json::string(capability.name())), ("polarity", Json::string("negative"))]));
                        diagnostic
                    })];
                    selected.extend(informational);
                    selected
                }
            };
            (
                if passed { "passed" } else { "failed" },
                selected,
                Some(evidence),
            )
        }
    };
    let mut result = vec![
        ("claimId".into(), Json::string(claim_id)),
        ("status".into(), Json::string(status)),
        (
            "diagnostics".into(),
            Json::Array(diagnostics.iter().map(Diagnostic::to_json).collect()),
        ),
    ];
    if let Some(value) = evidence {
        result.push(("evidence".into(), value));
    }
    Ok(Json::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::encode;

    fn diagnostic(severity: Severity, code: &str) -> Diagnostic {
        Diagnostic {
            code: code.into(),
            severity,
            message: code.into(),
            suggestion: None,
            details: None,
            spatial: None,
        }
    }

    #[test]
    fn successful_negation_keeps_informational_diagnostics() {
        let result = finish(
            "c",
            Capability::ToHaveVolume,
            Polarity::Negative,
            Evaluation::Geometric {
                positive_satisfied: false,
                diagnostics: vec![
                    diagnostic(Severity::Warning, "W"),
                    diagnostic(Severity::Error, "PREDICATE"),
                    diagnostic(Severity::Info, "I"),
                ],
                evidence: Json::object([("measured", Json::Number(2.0))]),
                negated_diagnostic: None,
            },
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(encode(&result).unwrap()).unwrap(),
            r#"{"claimId":"c","diagnostics":[{"code":"W","message":"W","severity":"warning"},{"code":"I","message":"I","severity":"info"}],"evidence":{"measured":2,"polarity":"negative","positiveSatisfied":false},"status":"passed"}"#
        );
    }

    #[test]
    fn info_notes_never_change_a_status() {
        let note = || vec![Diagnostic::info("GEOSPEC_STEP_SCHEMA_GAP", "gap")];
        let geometric = |positive_satisfied| Evaluation::Geometric {
            positive_satisfied,
            diagnostics: Vec::new(),
            evidence: Json::object([]),
            negated_diagnostic: None,
        };
        for (polarity, evaluation, status) in [
            (Polarity::Positive, geometric(true), "passed"),
            (Polarity::Positive, geometric(false), "failed"),
            (Polarity::Negative, geometric(false), "passed"),
            (Polarity::Negative, geometric(true), "failed"),
            (
                Polarity::Positive,
                Evaluation::Ancillary {
                    success: true,
                    value: Json::Null,
                    diagnostics: Vec::new(),
                },
                "passed",
            ),
            (
                Polarity::Positive,
                Evaluation::Refused {
                    diagnostics: Vec::new(),
                },
                "refused",
            ),
        ] {
            let capability = match evaluation {
                Evaluation::Ancillary { .. } => Capability::AnalyzeMesh,
                _ => Capability::ToHaveVolume,
            };
            let Json::Object(fields) =
                finish("c", capability, polarity, evaluation.with_notes(note())).unwrap()
            else {
                panic!("result object");
            };
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
                    .unwrap()
            };
            assert_eq!(field("status"), Json::string(status));
            let Json::Array(diagnostics) = field("diagnostics") else {
                panic!("diagnostics array");
            };
            assert_eq!(diagnostics.last(), Some(&note()[0].to_json()), "{status}");
        }
    }

    #[test]
    fn ancillary_observation_cannot_silently_ignore_negative_polarity() {
        let error = finish(
            "a",
            Capability::AnalyzeMesh,
            Polarity::Negative,
            Evaluation::Ancillary {
                success: true,
                value: Json::Null,
                diagnostics: Vec::new(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid-claim");
        assert_eq!(
            error.to_string(),
            "Ancillary capability 'analyzeMesh' requires positive polarity."
        );
    }

    #[test]
    fn ancillary_failure_stays_failed_with_its_owned_evidence() {
        let result = finish(
            "a",
            Capability::AnalyzeMeshOverlap,
            Polarity::Positive,
            Evaluation::Ancillary {
                success: false,
                value: Json::object([("checkedPairs", Json::Number(3.0))]),
                diagnostics: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(encode(&result).unwrap()).unwrap(),
            r#"{"claimId":"a","diagnostics":[],"evidence":{"checkedPairs":3},"status":"failed"}"#
        );
    }
}
