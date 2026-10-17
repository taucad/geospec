//! Evaluation-only solidcore BRep connector for the GeoSpec engine.
//!
//! [solidcore](https://github.com/tangency-labs/solidcore) is a Rust port of
//! NVIDIA's SMLib in progress. At the pinned commit it has no STEP (or other
//! exchange-format) reader, and its B-rep enumeration, bounding boxes and
//! manifold checks are still unported, so the connector cannot admit a source:
//! `open_step` refuses every file with a typed `Unsupported` reason, never a
//! pass. Upstream SMLib itself imports B-reps through OCCT
//! (`source/OCCT_BREP_IMPORT`), which solidcore declares out of scope.
//!
//! [`probe`] exercises the ported pieces that need no topology: exact face
//! integrals over trimmed analytic surfaces and closest-point projection, on
//! geometry built directly in solidcore to match simple corpus parts.

pub mod probe;

use geospec_engine_native_core::backend::brep::{BrepConnector, BrepIdentityProfile, BrepSubject};
use geospec_engine_native_core::backend::{BackendError, BackendErrorKind};

/// The solidcore commit the connector is pinned to.
pub const SOLIDCORE_REV: &str = "56af2a0253c17b4f21c8c01a7f1d887ee30eb441";

/// The refusal every `open_step` returns at the pinned commit.
pub const NO_STEP_READER: &str = "solidcore 56af2a0 has no STEP reader and cannot build a B-rep \
from exchange data: upstream SMLib imports through OCCT (out of scope for solidcore), and \
B-rep enumeration (topology.brep_p1), bounding boxes (query.brep) and manifold checks \
(topology.brep_p2) are unported.";

/// The connector the engine composes; it owns no state.
pub struct SolidcoreConnector;

impl BrepConnector for SolidcoreConnector {
    fn identity_profile(&self) -> BrepIdentityProfile {
        BrepIdentityProfile {
            ingest_profile: "geospec-solidcore-step-eval-v0",
            backend_profile: "solidcore-56af2a0-eval-v0",
        }
    }

    fn open_step(&self, _bytes: &[u8]) -> Result<Box<dyn BrepSubject>, BackendError> {
        Err(BackendError {
            kind: BackendErrorKind::Unsupported,
            message: NO_STEP_READER.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_step_refuses_with_a_typed_reason() {
        let bytes = b"ISO-10303-21;\nEND-ISO-10303-21;\n";
        let Err(error) = SolidcoreConnector.open_step(bytes) else {
            panic!("solidcore must not admit STEP input");
        };
        assert_eq!(error.kind, BackendErrorKind::Unsupported);
        assert!(error.message.contains("no STEP reader"));
    }
}
