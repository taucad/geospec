//! Exact nominal orthogonal insertion and void analysis.
//!
//! This module accepts only connector-certified, actual located complete boxes.
//! It does not infer exact geometry from reporting bounds or tessellation.

pub(crate) mod bore_slab_interference;
mod box_interference;
mod clearance;
mod cylindrical_band;
mod domain;
pub(crate) mod exact;
mod finite_contact;
pub(crate) use finite_contact::{
    finite_contact, FiniteContactRequest, FINITE_CONTACT_OUTPUT_BYTES,
    FINITE_CONTACT_RESERVATION_BYTES, FINITE_CONTACT_UNITS,
};
mod insertion;
mod nominal_analytic;
mod topology;

use crate::{
    backend::{BackendError, BackendErrorKind, CapacityLimit, LimitUnit},
    codec::Json,
};
#[cfg(test)]
pub(crate) use box_interference::nominal_analytic_box_tests::box_domain as nominal_analytic_box_control;
pub(crate) use box_interference::{
    box_interference, BoxInterferenceEvidence, BoxInterferenceRequest,
    BOX_INTERFERENCE_RESERVATION_BYTES,
};

pub(crate) use nominal_analytic::{
    nominal_analytic, AnalyticKind, NominalAnalyticRequest, NominalSupport, SupportKind,
    NOMINAL_ANALYTIC_OUTPUT_BYTES, NOMINAL_ANALYTIC_RESERVATION_BYTES, NOMINAL_ANALYTIC_UNITS,
};

pub(crate) use clearance::{clearance, ClearanceEvidence, ClearanceRequest, CLEARANCE_PAIR_UNITS};
pub(crate) use cylindrical_band::{
    cylindrical_band_clearance, CylindricalBandClearanceRequest,
    CYLINDRICAL_BAND_ENCODING_RESERVATION_BYTES, CYLINDRICAL_BAND_EVIDENCE_RESERVATION_BYTES,
    CYLINDRICAL_BAND_INPUT_PAIR_BYTES, CYLINDRICAL_BAND_PREDICATE_RESERVATION_BYTES,
};
pub(crate) use domain::DomainEvidence;
pub(crate) use insertion::{
    band_engagement, insertion, insertion_units, BandEngagementRequest, InsertionRequest,
};
pub(crate) use topology::{
    material_limit, GridPlan, PointEvidence, PointRequest, SectionRequest, Topology,
};

pub(crate) const PROFILE: &str = "geospec-nominal-orthogonal-continuous-v1";
pub(crate) const REPRESENTATION: &str = "selected-continuous-domain-axis-aligned-box";
pub(crate) const MAX_INTEGER_BITS: u64 = 8192;
pub(crate) const MAX_MATERIALS: usize = 16;
pub(crate) const MAX_PLANES_PER_AXIS: usize = 34;
pub(crate) const MAX_CELLS: usize = 35_937;
pub(crate) const MAX_OWNED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContinuousErrorKind {
    InvalidInput,
    UnsupportedDomain,
    ArithmeticLimit,
    ResourceLimit,
}

/// A continuous-domain refusal. A capacity refusal (`ArithmeticLimit` or
/// `ResourceLimit`) carries the limit and what the claim needed; an
/// unsupported-domain refusal may name the applicability condition that failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContinuousError {
    pub kind: ContinuousErrorKind,
    pub message: String,
    pub limit: Option<CapacityLimit>,
    pub condition: Option<&'static str>,
}

impl ContinuousError {
    pub(crate) fn invalid(message: &str) -> Self {
        Self::new(ContinuousErrorKind::InvalidInput, message)
    }

    /// An unsupported domain with no named condition; callers in this module
    /// use [`ContinuousError::outside`].
    pub(crate) fn unsupported(message: &str) -> Self {
        Self::new(ContinuousErrorKind::UnsupportedDomain, message)
    }

    /// The geometry is outside the proof's domain: `condition` names the
    /// applicability condition that failed, as a stable kebab-case id.
    pub(crate) fn outside(condition: &'static str, message: &str) -> Self {
        Self {
            condition: Some(condition),
            ..Self::unsupported(message)
        }
    }

    /// An exact-arithmetic envelope, such as an integer bit guard, that the
    /// claim needs more of; `subject` names what needed it.
    pub(crate) fn arithmetic(subject: &str, limit: CapacityLimit) -> Self {
        Self::capacity(ContinuousErrorKind::ArithmeticLimit, subject, limit)
    }

    /// A byte, count or cell capacity that the claim needs more of.
    pub(crate) fn resource(subject: &str, limit: CapacityLimit) -> Self {
        Self::capacity(ContinuousErrorKind::ResourceLimit, subject, limit)
    }

    /// Checked accounting overflowed: the requirement exceeds what `u64`
    /// counts, so it is reported saturated at the `u64` limit.
    pub(crate) fn overflow(message: &str, unit: LimitUnit) -> Self {
        Self {
            limit: Some(CapacityLimit::analysis(unit, u64::MAX, u64::MAX)),
            ..Self::new(ContinuousErrorKind::ArithmeticLimit, message)
        }
    }

    fn capacity(kind: ContinuousErrorKind, subject: &str, limit: CapacityLimit) -> Self {
        Self {
            limit: Some(limit),
            ..Self::new(kind, &BackendError::limit(subject, limit).message)
        }
    }

    fn new(kind: ContinuousErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.into(),
            limit: None,
            condition: None,
        }
    }

    /// `{"applicability": {"condition": ...}}` for a named domain condition.
    pub(crate) fn applicability(&self) -> Option<Json> {
        self.condition.map(|condition| {
            Json::object([(
                "applicability",
                Json::object([("condition", Json::string(condition))]),
            )])
        })
    }
}

/// The backend error of a continuous refusal: a capacity limit keeps its
/// numbers, malformed input stays invalid and anything else is unsupported.
impl From<ContinuousError> for BackendError {
    fn from(error: ContinuousError) -> Self {
        Self {
            kind: match (error.kind, error.limit) {
                (_, Some(limit)) => BackendErrorKind::LimitExceeded(limit),
                (ContinuousErrorKind::InvalidInput, None) => BackendErrorKind::InvalidInput,
                _ => BackendErrorKind::Unsupported,
            },
            message: error.message,
        }
    }
}

/// Integer bits of the larger of a rational's numerator and denominator.
pub(crate) fn rational_bits(value: &num_rational::BigRational) -> u64 {
    value.numer().bits().max(value.denom().bits())
}

/// A retained-slot or arithmetic bit guard: `required` bits against `limit`.
pub(crate) fn bits(required: u64, limit: u64) -> CapacityLimit {
    CapacityLimit::analysis(LimitUnit::Count("bits"), required, limit)
}

/// Owned evidence bytes against a fixed profile capacity.
pub(crate) fn bytes(required: usize, limit: usize) -> CapacityLimit {
    CapacityLimit::analysis(LimitUnit::Bytes, required as u64, limit as u64)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Outcome<T> {
    Decided {
        positive: bool,
        evidence: T,
    },
    Unsupported {
        reason: ContinuousError,
        evidence: Option<T>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExactScalar {
    pub numerator: String,
    pub denominator: String,
    pub display: f64,
}

impl ExactScalar {
    pub(crate) fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.numerator.capacity())
            .saturating_add(self.denominator.capacity())
    }

    pub(crate) fn to_json(&self) -> Json {
        Json::object([
            ("denominator", Json::string(&self.denominator)),
            ("display", Json::Number(self.display)),
            ("numerator", Json::string(&self.numerator)),
        ])
    }
}

pub(super) fn point_json(point: &[ExactScalar; 3]) -> Json {
    Json::Array(point.iter().map(ExactScalar::to_json).collect())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod clearance_tests;

#[cfg(test)]
mod exact_reference_tests;

#[cfg(test)]
mod outcome_tests;
