//! Engine-level host limits: entry default, raised admission, refusal and echo.
use crate::{Engine, EngineConfig};
use serde_json::{json, Value};

const MIB: usize = 1024 * 1024;

fn triangle_buffer() -> Vec<u8> {
    let vertices: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bytes = vertices
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    bytes.extend([0_u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    bytes
}

/// A one-triangle glTF document; `name` varies the subject and trailing
/// whitespace pads the primary bytes without changing the geometry.
fn document(name: &str, padding: usize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "buffers": [{"uri": "mesh.bin", "byteLength": 42}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 6}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
            {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
        ]
    }))
    .unwrap();
    bytes.resize(bytes.len() + padding, b' ');
    bytes
}

fn ingest(engine: &mut Engine, primary: &[u8]) -> Result<Value, (String, String)> {
    let buffer = triangle_buffer();
    let request = json!({
        "method": "ingestSubject",
        "requestId": "admit",
        "protocolVersion": 3,
        "registryVersion": 5,
        "canonicalProfile": "geospec-jcs-v1",
        "format": "gltf",
        "frame": {"coordinateSystem": "z-up", "sourceUnit": "mm", "outputUnit": "mm"},
        "ingestOptions": {},
        "primaryByteLength": primary.len(),
        "resources": [{"name": "mesh.bin", "byteLength": buffer.len()}]
    });
    engine
        .ingest_subject(
            &serde_json::to_vec(&request).unwrap(),
            primary,
            vec![buffer],
        )
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .map_err(|error| (error.code().to_owned(), error.to_string()))
}

fn configuration(engine: &Engine) -> Value {
    let request = json!({
        "method": "initialize",
        "requestId": "init",
        "protocolVersion": 3,
        "registryVersion": 5,
        "canonicalProfile": "geospec-jcs-v1"
    });
    let response: Value = serde_json::from_slice(
        &engine
            .process_request(&serde_json::to_vec(&request).unwrap())
            .unwrap(),
    )
    .unwrap();
    response["result"]["configuration"].clone()
}

fn raised() -> EngineConfig {
    EngineConfig::entry()
        .with_limits([
            ("maxSubjectBytes", (96 * MIB) as f64),
            ("maxTotalBinaryBytes", (192 * MIB) as f64),
            ("maxRetainedSubjects", 64.0),
        ])
        .unwrap()
}

#[test]
fn default_engine_reports_the_entry_profile() {
    let configuration = configuration(&Engine::new());
    assert_eq!(
        configuration["configurationProfile"],
        "geospec-entry-config-v1"
    );
    assert_eq!(
        configuration["binaryAdmissionLimits"],
        json!({
            "profileId": "geospec-entry-binary-v1",
            "maxSubjectBytes": 64 * MIB,
            "maxResourceBytes": 64 * MIB,
            "maxTotalBinaryBytes": 128 * MIB,
            "maxVertices": 2_000_000,
            "maxTriangles": 4_000_000,
            "maxOccurrences": 65_536
        })
    );
    assert_eq!(
        configuration["analysisRetentionLimits"],
        json!({"maxMeshBytes": 256 * MIB, "maxMeshEntries": 256, "maxSolidEntries": 256})
    );
    assert_eq!(configuration["maxRetainedSubjects"], 32);
    assert_eq!(
        configuration,
        self::configuration(&Engine::with_config(EngineConfig::entry()))
    );
    assert_eq!(
        configuration,
        self::configuration(&Engine::with_execution_permits(1.0).unwrap())
    );
}

#[test]
fn initialize_echoes_host_limits_under_a_distinct_profile() {
    let config = raised();
    let configuration = configuration(&Engine::with_config(config.clone()));
    let profile = configuration["configurationProfile"].as_str().unwrap();
    assert!(profile.starts_with("geospec-host-config-v1-"), "{profile}");
    assert_eq!(profile, config.configuration_profile_id());
    let binary = &configuration["binaryAdmissionLimits"];
    assert!(binary["profileId"]
        .as_str()
        .unwrap()
        .starts_with("geospec-host-binary-v1-"));
    assert_eq!(binary["maxSubjectBytes"], 96 * MIB);
    assert_eq!(binary["maxTotalBinaryBytes"], 192 * MIB);
    assert_eq!(binary["maxResourceBytes"], 64 * MIB);
    assert_eq!(configuration["maxRetainedSubjects"], 64);
    assert_eq!(
        configuration["analysisRetentionLimits"]["maxMeshBytes"],
        256 * MIB
    );
    assert_eq!(configuration["productionCapacityQualified"], false);
}

#[test]
fn raised_subject_limit_admits_what_the_entry_profile_refuses() {
    let primary = document("padded", 65 * MIB);
    let (code, message) = ingest(&mut Engine::new(), &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded", "{message}");
    let admitted = ingest(&mut Engine::with_config(raised()), &primary).unwrap();
    assert!(
        admitted["result"]["subject"]["subjectHash"].is_string(),
        "{admitted}"
    );
}

#[test]
fn raised_retained_subject_limit_admits_what_the_entry_profile_refuses() {
    let mut entry = Engine::new();
    let mut host = Engine::with_config(raised());
    for index in 0..32 {
        let primary = document(&format!("part-{index}"), 0);
        ingest(&mut entry, &primary).unwrap();
        ingest(&mut host, &primary).unwrap();
    }
    let primary = document("part-32", 0);
    let (code, message) = ingest(&mut entry, &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded", "{message}");
    ingest(&mut host, &primary).unwrap();
}

#[test]
fn lowered_retention_limit_reaches_admitted_subjects() {
    let config = EngineConfig::entry()
        .with_limits([("maxRetainedSubjects", 1.0)])
        .unwrap();
    let mut engine = Engine::with_config(config);
    ingest(&mut engine, &document("first", 0)).unwrap();
    let (code, _) = ingest(&mut engine, &document("second", 0)).unwrap_err();
    assert_eq!(code, "limit-exceeded");
}

#[test]
fn invalid_limits_are_refused_before_an_engine_exists() {
    for limits in [
        ("maxMeshBytes", 0.0),
        ("maxTriangles", f64::NAN),
        ("maxRetainedSubjects", 2.5),
        ("maxResourceBytes", (256 * MIB) as f64),
    ] {
        let error = EngineConfig::entry().with_limits([limits]).unwrap_err();
        assert_eq!(error.code(), "invalid-request", "{limits:?}");
    }
}
