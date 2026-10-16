//! Capacity refusals raised outside a claim, such as during admission.
//!
//! Such a refusal is a `limit-exceeded` protocol error. Its message follows one
//! grammar, `<what> <verb> <required>; the configured <name> limit is <limit>.`,
//! where both quantities come from `backend::quantity`. The accessors below are
//! the exact inverse of that grammar, so every host reads the same limit class,
//! unit, limit and requirement that a claim refusal states in its details.
//! (`ProtocolError` holds only a kind and a message; its bytes are pinned by the
//! rational-plate verifier inventory, so the numbers travel in the message.)

use crate::{
    backend::{quantity, BackendErrorKind, CapacityLimit, LimitClass, LimitUnit},
    ErrorKind, ProtocolError,
};

/// Every limit a protocol capacity refusal can name: its message name, class and unit.
const LIMITS: [(&str, LimitClass, LimitUnit); 9] = [
    ("binary subject", LimitClass::Admission, LimitUnit::Bytes),
    ("binary resource", LimitClass::Admission, LimitUnit::Bytes),
    ("total binary", LimitClass::Admission, LimitUnit::Bytes),
    (
        "vertex",
        LimitClass::Admission,
        LimitUnit::Count("vertices"),
    ),
    (
        "triangle",
        LimitClass::Admission,
        LimitUnit::Count("triangles"),
    ),
    (
        "occurrence",
        LimitClass::Admission,
        LimitUnit::Count("occurrences"),
    ),
    (
        "retained subject",
        LimitClass::Admission,
        LimitUnit::Count("retained subjects"),
    ),
    ("analysis retention", LimitClass::Analysis, LimitUnit::Bytes),
    (
        "mesh entry",
        LimitClass::Analysis,
        LimitUnit::Count("mesh entries"),
    ),
];

const CONFIGURED: &str = "; the configured ";
const IS: &str = " limit is ";

/// A `limit-exceeded` refusal stating that `subject` `verb` `required` against
/// the configured limit `name`, for example "The STEP document has 70000
/// occurrences; the configured occurrence limit is 65536 occurrences."
///
/// # Panics
///
/// When `name` is not a declared limit; every caller passes a constant.
pub(crate) fn refusal(
    subject: &str,
    verb: &str,
    name: &str,
    required: u64,
    limit: u64,
) -> ProtocolError {
    let (_, _, unit) = LIMITS
        .iter()
        .find(|(known, ..)| *known == name)
        .copied()
        .expect("declared capacity limit name");
    ProtocolError::new(
        ErrorKind::LimitExceeded,
        format!(
            "{subject} {verb} {}{CONFIGURED}{name}{IS}{}.",
            quantity(unit, required),
            quantity(unit, limit)
        ),
    )
}

/// The integer `text` ends with when written as a `unit` quantity, and the text before it.
fn trailing_quantity(text: &str, unit: LimitUnit) -> Option<(u64, &str)> {
    let digits = match unit {
        LimitUnit::Bytes => match text.strip_suffix(" bytes)") {
            Some(head) => &head[head.rfind('(')? + 1..],
            None => {
                let head = text
                    .strip_suffix(" bytes")
                    .or_else(|| text.strip_suffix(" byte"))?;
                &head[head.rfind(' ').map_or(0, |space| space + 1)..]
            }
        },
        LimitUnit::Count(noun) => {
            let head = text.strip_suffix(noun)?.strip_suffix(' ')?;
            &head[head.rfind(' ').map_or(0, |space| space + 1)..]
        }
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<u64>().ok()?;
    let written = quantity(unit, value);
    text.strip_suffix(written.as_str())
        .map(|head| (value, head))
}

fn parse(message: &str) -> Option<CapacityLimit> {
    let (head, tail) = message.rsplit_once(CONFIGURED)?;
    let (name, limit) = tail.strip_suffix('.')?.split_once(IS)?;
    let (_, class, unit) = LIMITS.iter().find(|(known, ..)| *known == name).copied()?;
    let (limit, "") = trailing_quantity(limit, unit)? else {
        return None;
    };
    let (required, subject) = trailing_quantity(head, unit)?;
    // A subject and a verb precede the requirement.
    if subject.trim_end().split(' ').count() < 2 || !subject.ends_with(' ') {
        return None;
    }
    Some(CapacityLimit {
        class,
        unit,
        limit,
        required,
        mesh: None,
    })
}

impl ProtocolError {
    /// The limit class, unit, limit and requirement of a capacity refusal, or
    /// `None` for any other error, including a `limit-exceeded` error that is
    /// not a configured capacity limit (such as a length overflow).
    pub fn capacity_limit(&self) -> Option<CapacityLimit> {
        if self.kind != ErrorKind::LimitExceeded {
            return None;
        }
        parse(&self.message)
    }

    /// `GEOSPEC_RESOURCE_LIMIT` for an admission limit or `GEOSPEC_ANALYSIS_LIMIT`
    /// for an analysis limit: the code a claim refusal over the same limit carries.
    pub fn capacity_code(&self) -> Option<&'static str> {
        self.capacity_limit()
            .map(|limit| crate::subject::backend_code(BackendErrorKind::LimitExceeded(limit)))
    }

    /// Canonical JSON `{limit, limitClass, required, unit}` of a capacity
    /// refusal, the same details object a claim refusal carries.
    pub fn capacity_details(&self) -> Option<Vec<u8>> {
        let limit = self.capacity_limit()?;
        crate::codec::encode(&crate::subject::capacity_details(&limit)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUES: [u64; 9] = [
        0,
        1,
        2,
        1023,
        1024,
        1536,
        64 * 1024 * 1024,
        73_400_321,
        u64::MAX,
    ];

    #[test]
    fn every_capacity_refusal_reads_back_its_numbers() {
        for (name, class, unit) in LIMITS {
            for required in VALUES {
                for limit in VALUES {
                    let error = refusal("The decoded mesh", "has", name, required, limit);
                    assert_eq!(error.code(), "limit-exceeded");
                    assert_eq!(
                        error.capacity_limit(),
                        Some(CapacityLimit {
                            class,
                            unit,
                            limit,
                            required,
                            mesh: None,
                        }),
                        "{error}"
                    );
                }
            }
        }
    }

    #[test]
    fn capacity_refusals_carry_their_code_and_details() {
        let error = refusal("This admission", "needs", "retained subject", 33, 32);
        assert_eq!(
            error.to_string(),
            "This admission needs 33 retained subjects; the configured retained subject limit is 32 retained subjects."
        );
        assert_eq!(error.capacity_code(), Some("GEOSPEC_RESOURCE_LIMIT"));
        assert_eq!(
            error.capacity_details().unwrap(),
            br#"{"limit":32,"limitClass":"admission","required":33,"unit":"retained subjects"}"#
        );
        let error = refusal(
            "Subject diagnostics",
            "need",
            "analysis retention",
            3000,
            2048,
        );
        assert_eq!(
            error.to_string(),
            "Subject diagnostics need 2.9 KiB (3000 bytes); the configured analysis retention limit is 2 KiB (2048 bytes)."
        );
        assert_eq!(error.capacity_code(), Some("GEOSPEC_ANALYSIS_LIMIT"));
        assert_eq!(
            error.capacity_details().unwrap(),
            br#"{"limit":2048,"limitClass":"analysis","required":3000,"unit":"bytes"}"#
        );
    }

    #[test]
    fn other_errors_carry_no_capacity_numbers() {
        for error in [
            ProtocolError::new(ErrorKind::LimitExceeded, "Binary closure length overflow."),
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "AP242 occurrence identity table exceeds one MiB.",
            ),
            ProtocolError::new(
                ErrorKind::InvalidRequest,
                "Primary geometry is 17 bytes; the configured binary subject limit is 16 bytes.",
            ),
            // Unknown limit names, altered quantities and missing subjects do not parse.
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "Primary geometry is 17 bytes; the configured binary limit is 16 bytes.",
            ),
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "Primary geometry is 17 bytes; the configured binary subject limit is 16 byte.",
            ),
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "Primary geometry is 2.0 KiB (2048 bytes); the configured binary subject limit is 1 KiB (1023 bytes).",
            ),
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "17 bytes; the configured binary subject limit is 16 bytes.",
            ),
            ProtocolError::new(
                ErrorKind::LimitExceeded,
                "The mesh has -1 vertices; the configured vertex limit is 16 vertices.",
            ),
        ] {
            assert_eq!(error.capacity_limit(), None, "{error}");
            assert_eq!(error.capacity_code(), None);
            assert_eq!(error.capacity_details(), None);
        }
        let error = ProtocolError::new(
            ErrorKind::LimitExceeded,
            "Primary geometry is 17 bytes; the configured binary subject limit is 16 bytes.",
        );
        assert_eq!(error.capacity_limit().unwrap().required, 17);
    }
}
