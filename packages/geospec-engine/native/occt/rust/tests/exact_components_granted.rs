//! M2: a granted whole-body distance keeps the serial connected-component
//! verdicts. The granted connector fixes the OCCT pool width before any STEP
//! is read, so it runs in a test binary of its own: a serial test in the same
//! process could open a document first and fix the pool at one thread.
mod component_claims;

use component_claims::{components, workspace, DESIGNED};
use geospec_engine_native_occt::ParallelOcctConnector;

#[test]
fn a_granted_whole_body_distance_keeps_the_serial_verdicts() {
    // SAFETY: this test binary owns its OCCT closure and its only test uses
    // one caller-inclusive two-CPU grant on one thread.
    for (fixture, count) in DESIGNED {
        let source = workspace(&format!(
            "packages/geospec-engine/fixtures/{fixture}/model.step"
        ));
        let connector = unsafe { ParallelOcctConnector::new().unwrap() };
        let result = components(Box::new(connector), &source, count, 8_000_000, false);
        assert_eq!(result["status"], "passed", "{fixture}: {result}");
    }
}
