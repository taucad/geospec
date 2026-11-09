//! The accuracy record of one numeric fact, in the shape of the engine's
//! protocol record (`rust/src/matchers/accuracy.rs` on the P1 branch): `bound`,
//! method (the protocol's provenance), scale, the file's own tolerance, the
//! file tolerance carried into the fact's unit, and closure.

/// How a fact was computed, which decides what its `bound` means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// Fixed-order integration with no error bound (OCCT's `BRepGProp` without
    /// `Eps`). Never produced here; present so the record maps one to one.
    LegacyFixedOrder,
    /// Exact up to round-off: every inner integral in closed form and every
    /// outer integrand a polynomial the Kronrod rule integrates exactly.
    ExactByDegree,
    /// Adaptive integration. Its `bound` is an estimate, not a guarantee: the
    /// sum over panels of the larger of the Kronrod-Gauss difference and the
    /// difference between a panel's Kronrod value and its halves' (every
    /// piece is bisected at least once), plus the inner integrals' estimates
    /// carried through, round-off from the integrals of `|f|`, the fold's
    /// round-off and the closure terms' effect. A smooth integrand the rules
    /// under-sample can exceed it.
    Adaptive,
    /// A bracket (the box): per side, a value a point of the geometry
    /// attains and a value no point exceeds, both rigorous for the
    /// definition's geometry with rounding carried. `bound` is the largest
    /// difference between them, so it is a guarantee, not an estimate.
    Bracket,
}

impl Method {
    /// The protocol's provenance string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyFixedOrder => "legacy-fixed-order",
            Self::ExactByDegree => "exact-by-degree",
            Self::Adaptive => "adaptive",
            Self::Bracket => "bracket",
        }
    }
}

/// Whether `achieved` is relative (extensive facts: volume, area) or absolute
/// in millimetres (positions: the centre of mass).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale {
    /// Volume, area, mass.
    Extensive,
    /// Positions.
    Positional,
}

/// The accuracy record of one numeric fact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FactAccuracy {
    /// Absolute error bound in the fact's own unit; `None` is "unbounded". For
    /// [`Method::Adaptive`] an estimate (see there).
    pub bound: Option<f64>,
    /// How the fact was computed.
    pub method: Method,
    /// Relative or positional.
    pub scale: Scale,
    /// The file's own maximum topology tolerance in millimetres, when known.
    pub file_tolerance: Option<f64>,
    /// The file tolerance carried into this fact's unit (for volume, the
    /// tolerance times the area), when known. On a box of a bridged body
    /// ([`crate::BoxBody`] with bridges) it is instead the box's gap bound
    /// ([`crate::BodyBox::gap_bound`], millimetres): how far the bridges',
    /// chords' and joints' images lie at most from their vertices. Read it
    /// as the file tolerance only where the box has no gap faces.
    pub gap_bound: Option<f64>,
    /// Whether every shell of the subject is closed, when known.
    pub closed: Option<bool>,
}

impl FactAccuracy {
    /// `achieved`: the bound relative to the measurement for extensive facts,
    /// the bound itself (mm) for positions; `None` without a bound.
    #[must_use]
    pub fn achieved(&self, measured: f64) -> Option<f64> {
        let bound = self.bound?;
        match self.scale {
            Scale::Positional => Some(bound),
            Scale::Extensive if measured != 0.0 && measured.is_finite() => {
                Some(bound / measured.abs())
            }
            Scale::Extensive => None,
        }
    }
}
