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
    /// Adaptive integration with a checked, heuristic error bound.
    Adaptive,
}

impl Method {
    /// The protocol's provenance string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyFixedOrder => "legacy-fixed-order",
            Self::ExactByDegree => "exact-by-degree",
            Self::Adaptive => "adaptive",
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
    /// Absolute error bound in the fact's own unit; `None` is "unbounded".
    pub bound: Option<f64>,
    /// How the fact was computed.
    pub method: Method,
    /// Relative or positional.
    pub scale: Scale,
    /// The file's own maximum topology tolerance in millimetres, when known.
    pub file_tolerance: Option<f64>,
    /// The file tolerance carried into this fact's unit (for volume, the
    /// tolerance times the area), when known.
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
