//! Owned data shared by native geometry adapters; no backend types cross this boundary.

use std::{error::Error, fmt};

pub mod brep;
pub mod csg;
pub mod csg_scope;
pub mod pmi;
pub mod resources;

/// Explicit retention limits, separate from control or binary input lengths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisRetentionLimits {
    pub max_mesh_bytes: u64,
    pub max_mesh_entries: u32,
    pub max_solid_entries: u32,
}

/// Indexed triangles with coordinates in the adapter's declared numeric profile.
#[derive(Clone, Debug, PartialEq)]
pub struct TriangleMesh {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Neutral backend failure categories, distinct from matcher results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendErrorKind {
    InvalidInput,
    Unsupported,
    ComputationFailed,
    BudgetExceeded {
        limit: u64,
        used: u64,
    },
    /// The claim needs more capacity than a declared limit allows. Not an
    /// evidence gap: the same claim on a smaller subject or under a larger
    /// limit can be decided.
    LimitExceeded(CapacityLimit),
}

/// Which declared limit a capacity refusal names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitClass {
    /// Retained or scratch analysis capacity (`AnalysisRetentionLimits` and
    /// the fixed per-evidence capacities of the profile).
    Analysis,
    /// Binary admission limits (`BinaryAdmissionLimits`) applied to derived
    /// evidence.
    Admission,
}

impl LimitClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Admission => "admission",
        }
    }
}

/// What a capacity limit counts: bytes, or a plural noun such as "entries".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitUnit {
    Bytes,
    Count(&'static str),
}

impl LimitUnit {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Count(noun) => noun,
        }
    }
}

/// The numbers behind a capacity refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityLimit {
    pub class: LimitClass,
    pub unit: LimitUnit,
    /// The declared limit.
    pub limit: u64,
    /// What the claim needs, counted the same way; saturates at `u64::MAX`.
    pub required: u64,
    /// For exact mesh scratch: the positions and triangles it was sized from.
    pub mesh: Option<(u64, u64)>,
}

impl CapacityLimit {
    pub fn analysis(unit: LimitUnit, required: u64, limit: u64) -> Self {
        Self {
            class: LimitClass::Analysis,
            unit,
            limit,
            required,
            mesh: None,
        }
    }

    pub fn admission(unit: LimitUnit, required: u64, limit: u64) -> Self {
        Self {
            class: LimitClass::Admission,
            ..Self::analysis(unit, required, limit)
        }
    }

    pub fn with_mesh(self, positions: u64, triangles: u64) -> Self {
        Self {
            mesh: Some((positions, triangles)),
            ..self
        }
    }
}

impl BackendError {
    /// A refusal naming what `subject` needs and the limit it exceeds, for
    /// example "Material exact scratch needs 942.0 MiB (987773920 bytes); the
    /// analysis limit is 256 MiB (268435456 bytes)."
    pub fn limit(subject: &str, limit: CapacityLimit) -> Self {
        let plural = subject.ends_with('s') && !subject.ends_with("ss");
        Self {
            kind: BackendErrorKind::LimitExceeded(limit),
            message: format!(
                "{subject} {} {}; the {} limit is {}.",
                if plural { "need" } else { "needs" },
                quantity(limit.unit, limit.required),
                limit.class.name(),
                quantity(limit.unit, limit.limit)
            ),
        }
    }
}

/// `count` in `unit`; a byte count above 1 KiB also names a binary-prefixed size.
pub fn quantity(unit: LimitUnit, count: u64) -> String {
    match unit {
        LimitUnit::Count(noun) => format!("{count} {noun}"),
        LimitUnit::Bytes if count == 1 => "1 byte".into(),
        LimitUnit::Bytes if count < 1024 => format!("{count} bytes"),
        LimitUnit::Bytes => {
            let prefixes = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
            let mut scale = 1024u64;
            let mut index = 0;
            while index + 1 < prefixes.len() && count / scale >= 1024 {
                scale *= 1024;
                index += 1;
            }
            let size = if count % scale == 0 {
                format!("{} {}", count / scale, prefixes[index])
            } else {
                format!("{:.1} {}", count as f64 / scale as f64, prefixes[index])
            };
            format!("{size} ({count} bytes)")
        }
    }
}

/// An owned backend error, without pointers or foreign exception types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendError {
    pub kind: BackendErrorKind,
    pub message: String,
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BackendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_refusals_state_what_was_needed_and_the_limit() {
        let scratch = BackendError::limit(
            "Material exact scratch",
            CapacityLimit::analysis(LimitUnit::Bytes, 987_773_920, 256 * 1024 * 1024)
                .with_mesh(161_505, 323_010),
        );
        assert_eq!(
            scratch.message,
            "Material exact scratch needs 942.0 MiB (987773920 bytes); the analysis limit is 256 MiB (268435456 bytes)."
        );
        assert_eq!(
            scratch.kind,
            BackendErrorKind::LimitExceeded(CapacityLimit {
                class: LimitClass::Analysis,
                unit: LimitUnit::Bytes,
                limit: 268_435_456,
                required: 987_773_920,
                mesh: Some((161_505, 323_010)),
            })
        );
        let occurrences = BackendError::limit(
            "The STEP subject",
            CapacityLimit::admission(LimitUnit::Count("occurrences"), 65_537, 65_536),
        );
        assert_eq!(
            occurrences.message,
            "The STEP subject needs 65537 occurrences; the admission limit is 65536 occurrences."
        );
        let plural = BackendError::limit(
            "Tessellations",
            CapacityLimit::analysis(LimitUnit::Bytes, 2, 1),
        );
        assert_eq!(
            plural.message,
            "Tessellations need 2 bytes; the analysis limit is 1 byte."
        );
        assert_eq!(quantity(LimitUnit::Bytes, 1536), "1.5 KiB (1536 bytes)");
        assert_eq!(
            quantity(LimitUnit::Bytes, u64::MAX),
            "16.0 EiB (18446744073709551615 bytes)"
        );
    }
}
