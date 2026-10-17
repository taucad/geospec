//! Evaluates the same GeoSpec claims on one STEP file through two engines,
//! one composed with the OCCT connector and one with the Truck connector.
//! Expected values come from the OCCT report (charter D3), so the OCCT engine
//! should pass every positive claim; the question is what the Truck engine
//! answers. Prints one JSON line.
//!
//! Usage: truck-claims <file.step>

use geospec_engine_native_core::backend::brep::BrepConnector;
use geospec_engine_native_core::backend::{csg::*, BackendError, TriangleMesh};
use geospec_engine_native_core::{Engine, EngineConfig};
use serde_json::{json, Value};
use std::time::Instant;

struct NoCsg;
impl CsgConnector for NoCsg {
    fn release(&mut self, _: SolidId) -> Result<(), BackendError> {
        unsupported()
    }
    fn admit(&mut self, _: &TriangleMesh, _: &[[u32; 2]]) -> Result<SolidId, BackendError> {
        unsupported()
    }
    fn boolean(&mut self, _: BooleanOp, _: &[SolidId]) -> Result<SolidId, BackendError> {
        unsupported()
    }
    fn transform(&mut self, _: SolidId, _: [f64; 12]) -> Result<SolidId, BackendError> {
        unsupported()
    }
    fn decompose(&mut self, _: SolidId) -> Result<Vec<SolidId>, BackendError> {
        unsupported()
    }
    fn properties(&self, _: SolidId) -> Result<SolidProperties, BackendError> {
        unsupported()
    }
    fn export(&self, _: SolidId) -> Result<MeshExport, BackendError> {
        unsupported()
    }
    fn slice(&self, _: SolidId, _: f64) -> Result<Section, BackendError> {
        unsupported()
    }
    fn section(&self, _: &[Vec<[f64; 2]>], _: FillRule) -> Result<Section, BackendError> {
        unsupported()
    }
    fn section_boolean(
        &self,
        _: SectionOp,
        _: &Section,
        _: &Section,
    ) -> Result<Section, BackendError> {
        unsupported()
    }
}

fn unsupported<T>() -> Result<T, BackendError> {
    Err(BackendError {
        kind: geospec_engine_native_core::backend::BackendErrorKind::Unsupported,
        message: "No CSG in the parity harness.".into(),
    })
}

fn admit(engine: &mut Engine, source: &[u8]) -> Result<String, String> {
    let ingest = serde_json::to_vec(&json!({
        "method": "ingestSubject", "requestId": "parity", "protocolVersion": 3,
        "registryVersion": 5, "canonicalProfile": "geospec-jcs-v1", "format": "step",
        "frame": {"coordinateSystem": "z-up", "sourceUnit": "auto", "outputUnit": "mm"},
        "ingestOptions": {}, "primaryByteLength": source.len(), "resources": []
    }))
    .unwrap();
    let response = engine
        .ingest_subject(&ingest, source, vec![])
        .map_err(|e| e.to_string())?;
    let admission: Value = serde_json::from_slice(&response).unwrap();
    admission["result"]["subject"]["subjectHash"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| admission.to_string().chars().take(300).collect())
}

fn claim(engine: &Engine, hash: &str, capability: &str, payload: &Value) -> Value {
    let request = serde_json::to_vec(&json!({
        "method": "submitClaims", "requestId": "parity-claim", "protocolVersion": 3,
        "registryVersion": 5, "canonicalProfile": "geospec-jcs-v1",
        "plan": {"subjects": [{"slot": "subject", "subjectHash": hash}], "claims": [{
            "claimId": "c", "capability": capability, "subjectSlots": ["subject"],
            "payload": payload, "polarity": "positive", "workUnitBudget": 80_000_000u64
        }]}
    }))
    .unwrap();
    let t = Instant::now();
    let reply = match engine.process_request(&request) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).unwrap(),
        Err(e) => return json!({"status": "protocol-error", "message": e.to_string()}),
    };
    let result = &reply["result"]["results"][0];
    let diagnostic = result["diagnostics"][0]["message"].as_str().unwrap_or("");
    json!({
        "status": result["status"],
        "ms": t.elapsed().as_secs_f64() * 1000.0,
        "diagnostic": diagnostic.chars().take(200).collect::<String>(),
    })
}

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let source = std::fs::read(&path).expect("readable");
    let mut out = serde_json::Map::new();
    out.insert("file".into(), json!(path));

    // Ground truth from the OCCT report.
    let occt = geospec_engine_native_occt::OcctConnector;
    let truth = occt.open_step(&source).and_then(|s| s.reported_shape());
    let Ok(truth) = truth else {
        out.insert("truth".into(), json!("occt-refused"));
        println!("{}", Value::Object(out));
        return;
    };
    let rel = |v: f64| (v.abs() * 1e-3).max(1e-6);
    let claims: Vec<(&str, &str, Value)> = vec![
        (
            "volume-0.1%",
            "toHaveVolume",
            json!({"kind": "volume", "expected": {"value": truth.volume, "tolerance": rel(truth.volume)}}),
        ),
        (
            "volume-1e-7",
            "toHaveVolume",
            json!({"kind": "volume", "expected": {"value": truth.volume, "tolerance": (truth.volume.abs() * 1e-7).max(1e-6)}}),
        ),
        (
            "area-0.1%",
            "toHaveSurfaceArea",
            json!({"kind": "surfaceArea", "expected": {"value": truth.surface_area, "tolerance": rel(truth.surface_area)}}),
        ),
        (
            "bbox-0.001",
            "toHaveBoundingBox",
            json!({"kind": "boundingBox", "expected": {"min": truth.bounds.min, "max": truth.bounds.max, "tolerance": 0.001}}),
        ),
        (
            "bbox-0.05",
            "toHaveBoundingBox",
            json!({"kind": "boundingBox", "expected": {"min": truth.bounds.min, "max": truth.bounds.max, "tolerance": 0.05}}),
        ),
        (
            "com-0.001",
            "toHaveCenterOfMass",
            json!({"kind": "centerOfMass", "expected": {"point": truth.center_of_mass, "tolerance": 0.001}}),
        ),
        (
            "com-0.05",
            "toHaveCenterOfMass",
            json!({"kind": "centerOfMass", "expected": {"point": truth.center_of_mass, "tolerance": 0.05}}),
        ),
        (
            "watertight",
            "toBeWatertight",
            json!({"kind": "watertight", "expected": true}),
        ),
        ("validBrep", "toBeValidBrep", json!({"kind": "validBrep"})),
        (
            "stepUnits-mm",
            "toHaveStepUnits",
            json!({"kind": "stepUnits", "expected": {"unit": "mm"}}),
        ),
        (
            "meshIntegrity",
            "toHaveMeshIntegrity",
            json!({"kind": "meshIntegrity", "expected": {"watertight": true}}),
        ),
    ];

    for (kernel, connector) in [
        (
            "occt",
            Box::new(geospec_engine_native_occt::OcctConnector) as Box<dyn BrepConnector>,
        ),
        (
            "truck",
            Box::new(geospec_engine_native_truck::TruckConnector),
        ),
    ] {
        let mut engine = Engine::with_backends(EngineConfig::entry(), connector, Box::new(NoCsg));
        let t = Instant::now();
        let hash = admit(&mut engine, &source);
        let mut results = serde_json::Map::new();
        results.insert("admitMs".into(), json!(t.elapsed().as_secs_f64() * 1000.0));
        match hash {
            Err(e) => {
                results.insert("admission".into(), json!(e));
            }
            Ok(hash) => {
                for (label, capability, payload) in &claims {
                    results.insert((*label).into(), claim(&engine, &hash, capability, payload));
                }
            }
        }
        out.insert(kernel.into(), Value::Object(results));
    }
    println!("{}", Value::Object(out));
}
