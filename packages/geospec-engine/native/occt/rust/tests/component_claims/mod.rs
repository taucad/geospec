//! Connected-component claim helpers shared by the exact_components and
//! exact_components_granted test binaries.
use std::path::PathBuf;

use geospec_engine_native_core::{
    backend::{csg::*, BackendError, TriangleMesh},
    Engine, EngineConfig,
};
use geospec_engine_native_occt::BrepConnector;
use serde_json::{json, Value};

pub struct NoCsg;
impl CsgConnector for NoCsg {
    fn release(&mut self, _: SolidId) -> Result<(), BackendError> {
        panic!("unexpected CSG")
    }
    fn admit(&mut self, _: &TriangleMesh, _: &[[u32; 2]]) -> Result<SolidId, BackendError> {
        panic!("unexpected CSG")
    }
    fn boolean(&mut self, _: BooleanOp, _: &[SolidId]) -> Result<SolidId, BackendError> {
        panic!("unexpected CSG")
    }
    fn transform(&mut self, _: SolidId, _: [f64; 12]) -> Result<SolidId, BackendError> {
        panic!("unexpected CSG")
    }
    fn decompose(&mut self, _: SolidId) -> Result<Vec<SolidId>, BackendError> {
        panic!("unexpected CSG")
    }
    fn properties(&self, _: SolidId) -> Result<SolidProperties, BackendError> {
        panic!("unexpected CSG")
    }
    fn export(&self, _: SolidId) -> Result<MeshExport, BackendError> {
        panic!("unexpected CSG")
    }
    fn slice(&self, _: SolidId, _: f64) -> Result<Section, BackendError> {
        panic!("unexpected CSG")
    }
    fn section(&self, _: &[Vec<[f64; 2]>], _: FillRule) -> Result<Section, BackendError> {
        panic!("unexpected CSG")
    }
    fn section_boolean(
        &self,
        _: SectionOp,
        _: &Section,
        _: &Section,
    ) -> Result<Section, BackendError> {
        panic!("unexpected CSG")
    }
}

pub fn workspace(relative: &str) -> Vec<u8> {
    let root = std::env::var_os("GEOSPEC_ADAPTER_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(5)
                .expect("OCCT crate must remain below the workspace root")
                .to_path_buf()
        });
    std::fs::read(root.join(relative)).expect("workspace fixture must be readable")
}

/// One toHaveConnectedComponents result at 0.001 mm on a fresh engine.
pub fn components(
    connector: Box<dyn BrepConnector>,
    source: &[u8],
    count: u64,
    budget: u64,
    bounded: bool,
) -> Value {
    charged_components(connector, source, count, budget, bounded).0
}

/// The result and the work units its claim charged.
pub fn charged_components(
    connector: Box<dyn BrepConnector>,
    source: &[u8],
    count: u64,
    budget: u64,
    bounded: bool,
) -> (Value, u64) {
    let mut engine = Engine::with_backends(EngineConfig::entry(), connector, Box::new(NoCsg));
    let ingest = serde_json::to_vec(&json!({
        "method": "ingestSubject", "requestId": "cc", "protocolVersion": 3,
        "registryVersion": 5, "canonicalProfile": "geospec-jcs-v1", "format": "step",
        "frame": {"coordinateSystem": "z-up", "sourceUnit": "auto", "outputUnit": "mm"},
        "ingestOptions": {}, "primaryByteLength": source.len(), "resources": []
    }))
    .unwrap();
    let admission: Value =
        serde_json::from_slice(&engine.ingest_subject(&ingest, source, vec![]).unwrap()).unwrap();
    let mut plan = json!({
        "subjects": [{"slot": "part",
            "subjectHash": admission["result"]["subject"]["subjectHash"]}],
        "claims": [{
            "claimId": "cc", "capability": "toHaveConnectedComponents", "subjectSlots": ["part"],
            "payload": {"kind": "connectedComponents",
                "expected": {"count": count, "toleranceMm": 0.001}},
            "polarity": "positive", "workUnitBudget": budget
        }]
    });
    if bounded {
        plan["evidenceProfile"] = json!("bounded");
    }
    let request = serde_json::to_vec(&json!({
        "method": "submitClaims", "requestId": "cc", "protocolVersion": 3,
        "registryVersion": 5, "canonicalProfile": "geospec-jcs-v1", "plan": plan
    }))
    .unwrap();
    let response: Value =
        serde_json::from_slice(&engine.process_request(&request).unwrap()).unwrap();
    let observations: Value = serde_json::from_slice(&engine.observations()).unwrap();
    let charged = observations["logical"]["chargedUnits"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    (response["result"]["results"][0].clone(), charged)
}

/// The designed fixtures and the disjoint bodies each one models.
pub const DESIGNED: [(&str, u64); 9] = [
    ("clearance/bolt-clearance-hole-positive", 2),
    ("clearance/shaft-bore-radial-positive", 2),
    ("clearance/shaft-bore-too-loose-negative", 2),
    ("clearance/shaft-bore-too-tight-negative", 2),
    ("clearance/piston-bore-skirt-positive", 2),
    ("containment/filter-inside-housing-positive", 2),
    ("containment/aabb-inside-false-positive", 2),
    ("containment/pin-through-boss-positive", 4),
    ("containment/pin-partial-insertion-negative", 4),
];
