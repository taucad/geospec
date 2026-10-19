//! Positive-only source inventory on the existing ancillary operation path.

use crate::{
    analysis::parallel_plane_distance::{inventory, F2Error, F2Limit},
    backend::{
        pmi::{PmiField, PmiFieldStatus},
        BackendError, BackendErrorKind, CapacityLimit, LimitUnit,
    },
    codec::Json,
    protocol::{invalid_claim, object, require_fields},
    registry::Capability,
    result::{Diagnostic, Evaluation},
    subject::{backend_diagnostic, backend_refusal, EvaluationContext, Subject, SubjectFormat},
    ProtocolError,
};

/// The most records and output bytes one query may request.
const MAX_QUERY_RECORDS: usize = 1024;
const MAX_QUERY_OUTPUT_BYTES: usize = 1_048_576;

const CAPACITY_SUGGESTION: &str = "The inventory could not be returned within this capacity, and no truncated inventory is returned. Keep the claim unchanged; queryPmi accepts maxRecords up to 1024 and maxOutputBytes up to 1048576, and the other PMI capacities are fixed by the inventory profile.";

pub(crate) struct PreparedPmi {
    max_records: usize,
    max_output_bytes: usize,
}

fn refusal(code: &str, message: impl Into<String>) -> Evaluation {
    Evaluation::Refused {
        diagnostics: vec![Diagnostic::error(code, message)],
    }
}

/// A PMI capacity refusal: `GEOSPEC_ANALYSIS_LIMIT` with the limit and what
/// the query needed.
fn capacity_refusal(subject: &str, unit: LimitUnit, required: u64, limit: u64) -> Evaluation {
    let limit = CapacityLimit::analysis(unit, required, limit);
    limit_refusal(BackendError::limit(subject, limit).message, limit)
}

fn limit_refusal(message: String, limit: CapacityLimit) -> Evaluation {
    let mut diagnostic = backend_diagnostic(BackendError {
        kind: BackendErrorKind::LimitExceeded(limit),
        message,
    });
    diagnostic.suggestion = Some(CAPACITY_SUGGESTION.into());
    Evaluation::Refused {
        diagnostics: vec![diagnostic],
    }
}

fn extraction_refusal(error: F2Error) -> Evaluation {
    match error {
        F2Error::ResourceLimit(F2Limit { message, limit }) => {
            limit_refusal(format!("resource-limit: {message}"), limit)
        }
        error => refusal(
            "GEOSPEC_PMI_EXTRACTION_FAILED",
            format!("{}: {error}", error.code()),
        ),
    }
}

/// Why a subject has no retained PMI source: a STEP source over the source
/// capacity is a capacity refusal; any other subject is outside the query's
/// domain.
fn missing_source(subject: &Subject) -> Evaluation {
    let primary_bytes = subject
        .semantic_identity
        .get()
        .and_then(|identity| match identity.descriptor() {
            Json::Object(fields) => fields.iter().find(|(name, _)| name == "primary"),
            _ => None,
        })
        .and_then(|(_, primary)| match primary {
            Json::Object(fields) => fields.iter().find(|(name, _)| name == "byteLength"),
            _ => None,
        })
        .and_then(|(_, length)| match length {
            Json::Number(bytes) => Some(*bytes as u64),
            _ => None,
        });
    if let (SubjectFormat::Step, Some(_), Some(bytes)) =
        (subject.format, subject.brep.as_ref(), primary_bytes)
    {
        if bytes > inventory::MAX_SOURCE_BYTES as u64 {
            return extraction_refusal(inventory::source_limit(bytes as usize));
        }
        let retention = subject.retention_limits.max_mesh_bytes;
        if bytes > retention {
            // The host-selectable analysis retention limit, not a fixed one.
            return backend_refusal(BackendError::limit(
                "The retained PMI source",
                CapacityLimit::analysis(LimitUnit::Bytes, bytes, retention),
            ));
        }
    }
    let mut diagnostic = Diagnostic::error(
        "GEOSPEC_UNSUPPORTED_EVIDENCE",
        "queryPmi requires a retained STEP source.",
    );
    diagnostic.details = Some(Json::object([(
        "applicability",
        Json::object([("condition", Json::string("retained-step-source"))]),
    )]));
    Evaluation::Refused {
        diagnostics: vec![diagnostic],
    }
}

impl PreparedPmi {
    pub(crate) fn prepare(payload: &Json) -> Result<Self, ProtocolError> {
        let mut result = Self {
            max_records: MAX_QUERY_RECORDS,
            max_output_bytes: MAX_QUERY_OUTPUT_BYTES,
        };
        if *payload == Json::Null {
            return Ok(result);
        }
        let fields = object(payload, "queryPmi payload")?;
        require_fields(
            fields,
            &["maxRecords", "maxOutputBytes"],
            &[],
            "queryPmi payload",
        )?;
        for (key, value) in fields {
            let Json::Number(number) = value else {
                return invalid_claim("queryPmi limits must be positive integers.");
            };
            let maximum = if key == "maxRecords" {
                MAX_QUERY_RECORDS
            } else {
                MAX_QUERY_OUTPUT_BYTES
            };
            if !number.is_finite()
                || *number < 1.0
                || number.fract() != 0.0
                || *number > maximum as f64
            {
                return invalid_claim("queryPmi limits exceed supported positive integer bounds.");
            }
            if key == "maxRecords" {
                result.max_records = *number as usize;
            } else {
                result.max_output_bytes = *number as usize;
            }
        }
        Ok(result)
    }

    pub(crate) fn normalized_payload(&self) -> Json {
        Json::object([
            ("maxRecords", Json::Number(self.max_records as f64)),
            ("maxOutputBytes", Json::Number(self.max_output_bytes as f64)),
        ])
    }

    pub(crate) fn evaluate(&self, context: &mut EvaluationContext<'_>) -> Evaluation {
        let subject = context.subject();
        let source = subject.pmi_source_bytes();
        // Source bytes are immutable admission storage. Charge before parsing,
        // querying the retained transfer or checking any retention cell.
        if let Err(error) = context
            .budget()
            .charge(1 + source.map_or(0, |bytes| bytes.len() as u64))
        {
            return Evaluation::budget_exceeded(Capability::QueryPmi, error);
        }
        let Some(source) = source else {
            return missing_source(subject);
        };
        let Some(backend) = subject.brep.as_deref() else {
            return refusal(
                "GEOSPEC_UNSUPPORTED_EVIDENCE",
                "queryPmi requires a retained STEP transfer.",
            );
        };
        // Requested-payload reservation, not an allocator/RSS security bound:
        // lexer nodes/reverse edges + retained records + serde/encoding copy +
        // bounded adapter association transfer. No persistent inventory cache.
        let Some(reservation) = source
            .len()
            .checked_mul(128)
            .and_then(|bytes| bytes.checked_add(self.max_output_bytes * 3 + 32 * 1_048_576))
        else {
            // The reservation exceeds what `usize` counts; report it saturated.
            return backend_refusal(BackendError::limit(
                "The PMI inventory reservation",
                CapacityLimit::analysis(
                    LimitUnit::Bytes,
                    u64::MAX,
                    subject.retention_limits.max_mesh_bytes,
                ),
            ));
        };
        if let Err(error) = subject.check_f2_pending(reservation as u64) {
            return backend_refusal(error);
        }
        let mut inventory = match inventory::parse_with_output_limit(
            source,
            self.max_records,
            self.max_output_bytes,
        ) {
            Ok(value) => value,
            Err(error) => return extraction_refusal(error),
        };
        let mut association_bytes = 0usize;
        for record in &mut inventory.records {
            for reference in record.first.iter_mut().chain(&mut record.second) {
                if matches!(
                    reference.requested_route.status,
                    PmiFieldStatus::Invalid | PmiFieldStatus::Ambiguous
                ) {
                    reference.associations = PmiField::unavailable(
                        reference.requested_route.status,
                        "Source occurrence route is not uniquely valid.",
                    );
                    continue;
                }
                let mut associations = Vec::new();
                let mut unavailable = match reference.associations.status {
                    PmiFieldStatus::Invalid
                    | PmiFieldStatus::Ambiguous
                    | PmiFieldStatus::Unsupported => Some((
                        reference.associations.status,
                        reference.associations.reason.clone().unwrap_or_default(),
                    )),
                    _ => None,
                };
                for source_id in &reference.source_item_ids {
                    if let Err(error) = context.budget().charge(1) {
                        return Evaluation::budget_exceeded(Capability::QueryPmi, error);
                    }
                    let result = match backend.pmi_source_faces(*source_id) {
                        Ok(value) => value,
                        Err(error) => return backend_refusal(error),
                    };
                    if result.status != PmiFieldStatus::Supported && unavailable.is_none() {
                        unavailable =
                            Some((result.status, result.reason.clone().unwrap_or_default()));
                    }
                    if let Some(values) = result.value {
                        // The bridge returns at most4096 bounded routes per call.
                        // Cap aggregate caller retention before appending a group;
                        // repeated source references cannot multiply it unboundedly.
                        let bytes = match serde_json::to_vec(&values) {
                            Ok(value) => value.len(),
                            Err(error) => {
                                return refusal("GEOSPEC_PMI_EXTRACTION_FAILED", error.to_string())
                            }
                        };
                        let total = association_bytes.saturating_add(bytes);
                        if total > self.max_output_bytes {
                            return capacity_refusal(
                                "The PMI association content",
                                LimitUnit::Bytes,
                                total as u64,
                                self.max_output_bytes as u64,
                            );
                        }
                        association_bytes = total;
                        associations.extend(values.into_iter().filter(|value| {
                            reference
                                .requested_route
                                .value
                                .as_ref()
                                .is_none_or(|route| *route == value.occurrence_route)
                        }));
                    }
                }
                reference.associations = if let Some((status, reason)) = unavailable {
                    // Partial qualified identities remain visible alongside the
                    // explicit incomplete binding status.
                    PmiField {
                        status,
                        value: Some(associations),
                        reason: Some(reason),
                    }
                } else if associations.is_empty() {
                    PmiField::unavailable(
                        PmiFieldStatus::Missing,
                        "No matching forward-transferred source face.",
                    )
                } else {
                    PmiField::supported(associations)
                };
            }
        }
        let bytes = match serde_json::to_vec(&inventory) {
            Ok(value) => value,
            Err(error) => return refusal("GEOSPEC_PMI_EXTRACTION_FAILED", error.to_string()),
        };
        let value = match crate::codec::decode(&bytes) {
            Ok(value) => value,
            Err(error) => return refusal("GEOSPEC_PMI_EXTRACTION_FAILED", error.to_string()),
        };
        let Some(identity) = subject.semantic_identity.get() else {
            return refusal(
                "GEOSPEC_PMI_EXTRACTION_FAILED",
                "Inventory subject has no verified source/profile identity.",
            );
        };
        let provenance = identity.descriptor().clone();
        let value = Json::object([
            ("inventory", value),
            ("subjectHash", Json::string(identity.hash())),
            ("provenance", provenance),
        ]);
        let encoded = match crate::codec::encode(&value) {
            Ok(value) => value,
            Err(error) => return refusal("GEOSPEC_PMI_EXTRACTION_FAILED", error.to_string()),
        };
        if encoded.len() > self.max_output_bytes {
            return capacity_refusal(
                "The PMI inventory",
                LimitUnit::Bytes,
                encoded.len() as u64,
                self.max_output_bytes as u64,
            );
        }
        Evaluation::Ancillary {
            value,
            diagnostics: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmi_inventory_parser_resource_error_maps_to_query_refusal() {
        let items = (20..29)
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        let values = (20..29)
            .map(|id| format!("#{id}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#40);"))
            .collect::<String>();
        let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DIMENSIONAL_LOCATION('d','',$,$);#2=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#3);#3=SHAPE_DIMENSION_REPRESENTATION('',({items}),#99);{values}#40=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));ENDSEC;END-ISO-10303-21;");
        let error = inventory::parse(source.as_bytes(), 1024).unwrap_err();
        let Evaluation::Refused { diagnostics } = extraction_refusal(error) else {
            panic!("A hard parser limit must not return partial ancillary success");
        };
        assert_eq!(diagnostics.len(), 1);
        // A fixed parser capacity is an analysis limit with its numbers, not
        // an admission limit and not an extraction failure.
        assert_eq!(diagnostics[0].code, "GEOSPEC_ANALYSIS_LIMIT");
        assert_eq!(diagnostics[0].severity, crate::result::Severity::Error);
        assert_eq!(
            crate::codec::encode(diagnostics[0].details.as_ref().unwrap()).unwrap(),
            br#"{"limit":8,"limitClass":"analysis","required":9,"unit":"scalar inventory values"}"#
        );
        assert_eq!(
            diagnostics[0].message,
            "resource-limit: A dimension needs 9 scalar inventory values; the analysis limit is 8 scalar inventory values."
        );
        assert_eq!(
            diagnostics[0].suggestion.as_deref(),
            Some(CAPACITY_SUGGESTION)
        );
        // Green neighbour: eight values are inside the capacity.
        let items = (20..28)
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        let values = (20..28)
            .map(|id| format!("#{id}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#40);"))
            .collect::<String>();
        let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DIMENSIONAL_LOCATION('d','',$,$);#2=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#3);#3=SHAPE_DIMENSION_REPRESENTATION('',({items}),#99);{values}#40=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));ENDSEC;END-ISO-10303-21;");
        assert!(inventory::parse(source.as_bytes(), 1024).is_ok());
    }

    #[test]
    fn query_output_and_source_capacities_carry_their_numbers() {
        let Evaluation::Refused { diagnostics } =
            capacity_refusal("The PMI inventory", LimitUnit::Bytes, 2_000, 1_024)
        else {
            panic!("an output capacity refuses");
        };
        assert_eq!(diagnostics[0].code, "GEOSPEC_ANALYSIS_LIMIT");
        assert_eq!(
            crate::codec::encode(diagnostics[0].details.as_ref().unwrap()).unwrap(),
            br#"{"limit":1024,"limitClass":"analysis","required":2000,"unit":"bytes"}"#
        );
        // A STEP source over the fixed 8 MiB inventory source capacity.
        let Evaluation::Refused { diagnostics } =
            extraction_refusal(inventory::source_limit(8 * 1024 * 1024 + 1))
        else {
            panic!("an oversized source refuses");
        };
        assert_eq!(diagnostics[0].code, "GEOSPEC_ANALYSIS_LIMIT");
        assert_eq!(
            crate::codec::encode(diagnostics[0].details.as_ref().unwrap()).unwrap(),
            br#"{"limit":8388608,"limitClass":"analysis","required":8388609,"unit":"bytes"}"#
        );
        // A record limit requested by the query refuses with the request's
        // bound and the first record it could not return.
        let source = "ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DATUM('A','',$,.F.,'A');#2=DATUM('B','',$,.F.,'B');ENDSEC;END-ISO-10303-21;";
        let Err(F2Error::ResourceLimit(F2Limit { limit, .. })) =
            inventory::parse(source.as_bytes(), 1)
        else {
            panic!("two records exceed a one-record query");
        };
        assert_eq!(
            limit,
            CapacityLimit::analysis(LimitUnit::Count("records"), 2, 1)
        );
        assert!(inventory::parse(source.as_bytes(), 2).is_ok());
    }
}
