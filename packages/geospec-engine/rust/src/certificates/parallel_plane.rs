//! Fixed source-derived supporting-plane proof and per-subject success cache.

use std::{cell::OnceCell, mem::size_of, rc::Rc};

use crate::{
    analysis::parallel_plane_distance::{self, F2Domain, F2Error, F2Evidence, F2Limit},
    backend::{BackendError, BackendErrorKind, LimitUnit},
    budget::Budget,
    codec::Json,
    protocol::{invalid_claim, object, require_fields, string_field},
    registry::Capability,
    result::{Diagnostic, Evaluation},
    subject::{backend_refusal, fixed_capacity_diagnostic, Subject},
    ProtocolError,
};

pub(crate) const CONTRACT: &str = "geospec.pmi.parallel-plane-distance/v1";
pub(crate) const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

pub(crate) struct SourceProof {
    source: Vec<u8>,
    evidence: OnceCell<Rc<F2Evidence>>,
    source_error: OnceCell<F2Error>,
}

impl SourceProof {
    pub(crate) fn source_bytes(&self) -> &[u8] {
        &self.source
    }

    pub(crate) fn new(source: Vec<u8>) -> Self {
        Self {
            source,
            evidence: OnceCell::new(),
            source_error: OnceCell::new(),
        }
    }

    pub(crate) fn owned_bytes(&self) -> u64 {
        let error_bytes = self
            .source_error
            .get()
            .map_or(0, |error| error.message().capacity());
        (size_of::<Self>() + self.source.capacity() + error_bytes) as u64
            + self.evidence.get().map_or(0, |value| {
                (value.owned_bytes() + 2 * size_of::<usize>()) as u64
            })
    }

    fn verified(&self, subject: &Subject) -> Result<Rc<F2Evidence>, Evaluation> {
        if let Some(value) = self.evidence.get() {
            return Ok(Rc::clone(value));
        }
        if let Some(error) = self.source_error.get() {
            return Err(refusal(error));
        }
        // Reserve the declared derivation retention envelope before cold work.
        // The reported parser peak is checked separately below; this reservation
        // is not a proof of transient allocator or kernel RSS bounds.
        subject
            .check_f2_pending(MAX_RETAINED_BYTES as u64)
            .map_err(backend_refusal)?;
        let admitted = match parallel_plane_distance::admit(&self.source, None) {
            Ok(value) => value,
            Err(error) => {
                let result = refusal(&error);
                // Only refusals determined by immutable source bytes are retained.
                // Resource and backend failures never poison the subject cell.
                if !matches!(error, F2Error::ResourceLimit(_)) {
                    let bytes = error.message().capacity() as u64;
                    subject.check_f2_pending(bytes).map_err(backend_refusal)?;
                    let _ = self.source_error.set(error);
                }
                return Err(result);
            }
        };
        subject
            .check_f2_pending(admitted.work_counts().projected_temporary_peak_bytes as u64)
            .map_err(backend_refusal)?;
        let backend = subject.brep.as_deref().ok_or_else(outside_domain)?;
        let keys = admitted.source_keys();
        let associations = [
            backend
                .resolve_source_face(&keys[0])
                .map_err(backend_refusal)?,
            backend
                .resolve_source_face(&keys[1])
                .map_err(backend_refusal)?,
        ];
        let value = admitted
            .evaluate(&associations)
            .map_err(|error| refusal(&error))?;
        let bytes = value.owned_bytes() + 2 * size_of::<usize>();
        if bytes > MAX_RETAINED_BYTES {
            return Err(refusal(&parallel_plane_distance::limit(
                "The F2 retained proof",
                LimitUnit::Bytes,
                bytes as u64,
                MAX_RETAINED_BYTES as u64,
            )));
        }
        // Both source derivation and projected evidence are live until publication.
        subject
            .check_f2_pending((admitted.owned_bytes() + bytes) as u64)
            .map_err(backend_refusal)?;
        let value = Rc::new(value);
        let _ = self.evidence.set(Rc::clone(&value));
        Ok(value)
    }

    pub(crate) fn evaluate(
        &self,
        subject: &Subject,
        plan_hash: &str,
        budget: &Budget,
    ) -> Evaluation {
        // Fixed new-entry logical schedule; no measurement of kernel time or RSS.
        let units = 1 + self.source.len() as u64 + 8192 + 2048 + 8192 + 2;
        if let Err(error) = budget.charge(units) {
            return Evaluation::budget_exceeded(Capability::ToSatisfyParallelPlaneDistance, error);
        }
        let value = match self.verified(subject) {
            Ok(value) => value,
            Err(error) => return error,
        };
        let mut witness = value.to_json();
        // Allocation accounting is target-specific execution evidence, not part
        // of the cross-target mathematical certificate. WorkCounts retains it.
        if let Json::Object(fields) = &mut witness {
            if let Some((_, Json::Object(work))) =
                fields.iter_mut().find(|(name, _)| name == "work")
            {
                work.retain(|(name, _)| name != "projectedTemporaryPeakBytes");
            }
        }
        let diagnostic = Diagnostic::error(
            "GEOSPEC_PARALLEL_PLANE_DISTANCE_MISMATCH",
            "The associated supporting-plane distance is outside its source-declared interval.",
        );
        Evaluation::Geometric {
            positive_satisfied: value.geometric_pass,
            diagnostics: vec![diagnostic],
            evidence: Json::object([
                ("profile", Json::string(CONTRACT)),
                ("subjectContentHash", Json::string(&subject.content_hash)),
                (
                    "subjectHash",
                    Json::string(
                        subject
                            .semantic_identity
                            .get()
                            .expect("admitted STEP identity")
                            .hash(),
                    ),
                ),
                ("planHash", Json::string(plan_hash)),
                (
                    "verifierSourceHash",
                    Json::string(super::parallel_plane_definition::VERIFIER_SOURCE_HASH),
                ),
                ("normalizedExpected", payload()),
                (
                    "measured",
                    Json::object([("satisfiesSourceInterval", Json::Bool(value.geometric_pass))]),
                ),
                ("witnesses", witness),
            ]),
            negated_diagnostic: None,
        }
    }
}

pub(crate) fn prepare(value: &Json) -> Result<(), ProtocolError> {
    let fields = object(value, "parallel-plane distance payload")?;
    require_fields(
        fields,
        &["contract"],
        &["contract"],
        "parallel-plane distance payload",
    )?;
    if string_field(fields, "contract")? != CONTRACT {
        return invalid_claim(
            "Parallel-plane distance requires geospec.pmi.parallel-plane-distance/v1.",
        );
    }
    Ok(())
}

pub(crate) fn payload() -> Json {
    Json::object([("contract", Json::string(CONTRACT))])
}

pub(crate) fn outside_domain() -> Evaluation {
    refusal(&F2Error::UnsupportedSource(
        "The subject has no retained STEP source within the exact 1 MiB profile.".into(),
    ))
}

pub(crate) fn outside_domain_request(budget: &Budget) -> Evaluation {
    match budget.charge(1) {
        Ok(()) => outside_domain(),
        Err(error) => {
            Evaluation::budget_exceeded(Capability::ToSatisfyParallelPlaneDistance, error)
        }
    }
}

/// A capacity limit refuses with its typed limit code and numbers, a domain
/// gap as unsupported evidence with its applicability condition, and any other
/// source failure as an unverified certificate.
fn refusal(error: &F2Error) -> Evaluation {
    let mut diagnostic = match error {
        F2Error::ResourceLimit(F2Limit { message, limit }) => {
            fixed_capacity_diagnostic(BackendError {
                kind: BackendErrorKind::LimitExceeded(*limit),
                message: message.clone(),
            })
        }
        F2Error::UnsupportedDomain(_) => {
            Diagnostic::error("GEOSPEC_EVIDENCE_UNSUPPORTED", error.to_string())
        }
        F2Error::InvalidInput(_) | F2Error::UnsupportedSource(_) => {
            Diagnostic::error("GEOSPEC_CERTIFICATE_UNVERIFIED", error.to_string())
        }
    };
    let mut fields = match diagnostic.details.take() {
        Some(Json::Object(fields)) => fields,
        _ => Vec::new(),
    };
    fields.push((
        "matcher".into(),
        Json::string("toSatisfyParallelPlaneDistance"),
    ));
    fields.push(("contract".into(), Json::string(CONTRACT)));
    fields.push(("reason".into(), Json::string(error.code())));
    if let F2Error::UnsupportedDomain(F2Domain { condition, .. }) = error {
        fields.push((
            "applicability".into(),
            Json::object([("condition", Json::string(condition))]),
        ));
    }
    diagnostic.details = Some(Json::Object(fields));
    Evaluation::Refused {
        diagnostics: vec![diagnostic],
    }
}

pub(crate) fn profile() -> Json {
    Json::object([
        ("id", Json::string(CONTRACT)),
        ("subjectFormat", Json::string("step")),
        (
            "representation",
            Json::string("source-derived-ideal-supporting-planes"),
        ),
        (
            "limits",
            Json::object([
                ("maxPrimaryBytes", Json::Number(MAX_SOURCE_BYTES as f64)),
                ("maxEntityRecords", Json::Number(8192.0)),
                ("maxRecordBytes", Json::Number(65536.0)),
                ("maxListItems", Json::Number(32.0)),
                ("maxSignificandDigits", Json::Number(64.0)),
                ("maxAbsoluteExponent", Json::Number(308.0)),
                ("maxOccurrenceRoute", Json::Number(32.0)),
                ("maxFaceEdges", Json::Number(8.0)),
                ("maxDecimals", Json::Number(2048.0)),
                ("maxRationalOperations", Json::Number(8192.0)),
                ("maxArithmeticIntegerBits", Json::Number(12416.0)),
                (
                    "maxRetainedOwnedBytes",
                    Json::Number(MAX_RETAINED_BYTES as f64),
                ),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::CapacityLimit;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../native/occt/rust/tests/fixtures/parallel-plane-distance-source.step"),
        )
        .expect("F2 AP242 fixture")
    }

    fn diagnostic(error: &F2Error) -> Diagnostic {
        let Evaluation::Refused { mut diagnostics } = refusal(error) else {
            panic!("F2 errors refuse");
        };
        diagnostics.remove(0)
    }

    fn details(diagnostic: &Diagnostic) -> String {
        String::from_utf8(crate::codec::encode(diagnostic.details.as_ref().unwrap()).unwrap())
            .unwrap()
    }

    #[test]
    fn nonparallel_source_planes_refuse_as_unsupported_with_their_condition() {
        let source = fixture().replace(
            "#627=DIRECTION('',(0.,0.,1.));",
            "#627=DIRECTION('',(1.,0.,0.));",
        );
        let error = parallel_plane_distance::admit(source.as_bytes(), None).unwrap_err();
        let F2Error::UnsupportedDomain(F2Domain { condition, .. }) = &error else {
            panic!("nonparallel planes are outside the domain: {error}");
        };
        assert_eq!(*condition, "parallel-source-planes");
        let diagnostic = diagnostic(&error);
        assert_eq!(diagnostic.code, "GEOSPEC_EVIDENCE_UNSUPPORTED");
        assert_eq!(
            details(&diagnostic),
            r#"{"applicability":{"condition":"parallel-source-planes"},"contract":"geospec.pmi.parallel-plane-distance/v1","matcher":"toSatisfyParallelPlaneDistance","reason":"unsupported-domain"}"#
        );
        // Green neighbour: the parallel source admits.
        assert!(parallel_plane_distance::admit(fixture().as_bytes(), None).is_ok());
    }

    #[test]
    fn an_oversized_step_list_is_a_capacity_limit_with_numbers() {
        let list = |count: usize| {
            let items = (0..count).map(|_| "1.").collect::<Vec<_>>().join(",");
            format!("ISO-10303-21;HEADER;ENDSEC;DATA;#1=X(({items}));ENDSEC;END-ISO-10303-21;")
        };
        let error = parallel_plane_distance::admit(list(33).as_bytes(), None).unwrap_err();
        let F2Error::ResourceLimit(F2Limit { limit, .. }) = &error else {
            panic!("33 list items exceed the profile: {error}");
        };
        assert_eq!(
            *limit,
            CapacityLimit::analysis(LimitUnit::Count("items"), 33, 32)
        );
        let diagnostic = diagnostic(&error);
        assert_eq!(diagnostic.code, "GEOSPEC_ANALYSIS_LIMIT");
        assert_eq!(
            diagnostic.message,
            "A STEP list needs 33 items; the analysis limit is 32 items."
        );
        assert_eq!(
            details(&diagnostic),
            r#"{"contract":"geospec.pmi.parallel-plane-distance/v1","limit":32,"limitClass":"analysis","matcher":"toSatisfyParallelPlaneDistance","reason":"resource-limit","required":33,"unit":"items"}"#
        );
        assert_eq!(
            diagnostic.suggestion.as_deref(),
            Some(crate::subject::FIXED_CAPACITY_SUGGESTION)
        );
        // Green neighbour: 32 items pass the list capacity and fail later on
        // the missing declaration, which is not a capacity refusal.
        let error = parallel_plane_distance::admit(list(32).as_bytes(), None).unwrap_err();
        assert!(!matches!(error, F2Error::ResourceLimit(_)), "{error}");
        assert_eq!(diagnostic_code(&error), "GEOSPEC_CERTIFICATE_UNVERIFIED");
    }

    fn diagnostic_code(error: &F2Error) -> String {
        diagnostic(error).code
    }
}
