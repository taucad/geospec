//! Typed refusals: a face or body the facts layer cannot decide.

use std::fmt;

/// Why a fact is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RefusalCode {
    /// The definition is malformed: a non-finite number, a knot vector that does
    /// not fit its net, a non-positive weight or radius, an empty trim range.
    InvalidDefinition,
    /// The adapter met a surface kind it cannot express as a definition.
    UnsupportedSurface,
    /// The adapter met a trim curve kind it cannot express as a definition.
    UnsupportedTrim,
    /// The adapter's exported definition does not evaluate to the kernel's own
    /// geometry.
    ExportMismatch,
    /// A trim leaves its surface's parameter domain: the surface would be
    /// evaluated where it is not defined.
    TrimOutsideDomain,
    /// An adaptive integral did not converge within its panel cap.
    NoConvergence,
    /// A rational surface's weights vary by a ratio above `1e6` among one
    /// span's poles: its parametrization can change faster than
    /// floating-point evaluation resolves, so the evaluator's error cannot be
    /// bounded.
    IllConditionedSurface,
    /// An integrand or result is not finite.
    NonFinite,
    /// A face's volume term exceeds its area times the body size: the face lies
    /// outside the body's box or its trims are not where its surface is.
    FaceTermExceedsArea,
    /// A face's signed area is negative beyond its bound, or a trim loop's
    /// orientation does not fit its role (outer counter-clockwise, holes
    /// clockwise, winding loops in opposite pairs): its trims are not oriented
    /// with the face on their left.
    TrimOrientation,
    /// A trim loop's pieces do not chain end to start, or the loop does not
    /// close.
    OpenTrimLoop,
    /// Consecutive trims of a loop meet only within the kernel's modelling
    /// tolerance: closing the gap would change the face by an amount no
    /// integration bound covers, so the adapter refuses instead.
    ToleranceGap,
    /// A body bounding material is known not to be closed, or its faces do not
    /// close beyond their integration bounds and the volume bound's widening
    /// by the closure terms and the volume forms' disagreement exceeds four
    /// times the gap volume (file tolerance times area) or, without a file
    /// tolerance, 1e-4 of the volume.
    OpenBody,
    /// A body bounding material whose closure is unknown (`closed` is
    /// `None`): faces with matching holes cancel every closure term, so the
    /// integrals cannot decide it.
    ClosureUnknown,
    /// A body bounding material has a volume that is not positive beyond its
    /// bound.
    NonPositiveVolume,
    /// The volume exceeds the body box's volume beyond its bound.
    VolumeExceedsBox,
    /// The centre of mass lies outside the body box beyond its bound.
    CentroidOutsideBox,
}

impl RefusalCode {
    /// The stable kebab-case code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidDefinition => "invalid-definition",
            Self::UnsupportedSurface => "unsupported-surface",
            Self::UnsupportedTrim => "unsupported-trim",
            Self::ExportMismatch => "export-mismatch",
            Self::TrimOutsideDomain => "trim-outside-domain",
            Self::NoConvergence => "no-convergence",
            Self::IllConditionedSurface => "ill-conditioned-surface",
            Self::NonFinite => "non-finite",
            Self::FaceTermExceedsArea => "face-term-exceeds-area",
            Self::TrimOrientation => "trim-orientation",
            Self::OpenTrimLoop => "open-trim-loop",
            Self::ToleranceGap => "tolerance-gap",
            Self::OpenBody => "open-body",
            Self::ClosureUnknown => "closure-unknown",
            Self::NonPositiveVolume => "non-positive-volume",
            Self::VolumeExceedsBox => "volume-exceeds-box",
            Self::CentroidOutsideBox => "centroid-outside-box",
        }
    }
}

/// A refused face or body.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    /// The class.
    pub code: RefusalCode,
    /// The face's index in its body, when one face is the cause.
    pub face: Option<usize>,
    /// What was found, for a person.
    pub detail: String,
}

impl Refusal {
    /// A refusal not tied to one face.
    #[must_use]
    pub fn new(code: RefusalCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            face: None,
            detail: detail.into(),
        }
    }

    /// The same refusal, attributed to face `face`.
    #[must_use]
    pub fn at_face(mut self, face: usize) -> Self {
        self.face = Some(face);
        self
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.face {
            Some(face) => write!(f, "{} (face {face}): {}", self.code.as_str(), self.detail),
            None => write!(f, "{}: {}", self.code.as_str(), self.detail),
        }
    }
}

impl std::error::Error for Refusal {}
