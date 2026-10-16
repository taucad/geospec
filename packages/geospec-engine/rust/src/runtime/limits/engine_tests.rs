//! Engine-level host limits: entry default, raised admission, refusal and echo.
use crate::{Engine, EngineConfig};
use serde_json::{json, Value};

const MIB: usize = 1024 * 1024;

/// A unit square of four vertices and two triangles.
fn quad_buffer() -> Vec<u8> {
    let vertices: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let mut bytes = vertices
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    bytes.extend(
        [0_u16, 1, 2, 1, 3, 2]
            .into_iter()
            .flat_map(u16::to_le_bytes),
    );
    bytes
}

/// A four-vertex, two-triangle glTF document; `name` varies the subject and
/// trailing whitespace pads the primary bytes without changing the geometry.
fn document(name: &str, padding: usize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "buffers": [{"uri": "mesh.bin", "byteLength": 60}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 48},
            {"buffer": 0, "byteOffset": 48, "byteLength": 12}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
            {"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"}
        ]
    }))
    .unwrap();
    bytes.resize(bytes.len() + padding, b' ');
    bytes
}

/// The admission refusal's protocol code, message, capacity code and details.
type Refusal = (String, String, Option<&'static str>, Option<Value>);

fn ingest(engine: &mut Engine, primary: &[u8]) -> Result<Value, Refusal> {
    let buffer = quad_buffer();
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
        .map_err(|error| {
            (
                error.code().to_owned(),
                error.to_string(),
                error.capacity_code(),
                error
                    .capacity_details()
                    .map(|details| serde_json::from_slice(&details).unwrap()),
            )
        })
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
    let (code, message, capacity, details) = ingest(&mut Engine::new(), &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded", "{message}");
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(
        details.unwrap(),
        json!({"limitClass": "admission", "unit": "bytes", "limit": 64 * MIB, "required": primary.len()})
    );
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
    let (code, message, capacity, details) = ingest(&mut entry, &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded", "{message}");
    assert_eq!(
        message,
        "This admission needs 33 retained subjects; the configured retained subject limit is 32 retained subjects."
    );
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(
        details.unwrap(),
        json!({"limitClass": "admission", "unit": "retained subjects", "limit": 32, "required": 33})
    );
    ingest(&mut host, &primary).unwrap();
    // Re-admitting a retained subject needs no new retention.
    ingest(&mut entry, &document("part-0", 0)).unwrap();
}

#[test]
fn lowered_retention_limit_reaches_admitted_subjects() {
    let config = EngineConfig::entry()
        .with_limits([("maxRetainedSubjects", 1.0)])
        .unwrap();
    let mut engine = Engine::with_config(config);
    ingest(&mut engine, &document("first", 0)).unwrap();
    let (code, _, capacity, details) = ingest(&mut engine, &document("second", 0)).unwrap_err();
    assert_eq!(code, "limit-exceeded");
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(details.unwrap()["limit"], 1);
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

fn with(limits: &[(&str, f64)]) -> Engine {
    Engine::with_config(
        EngineConfig::entry()
            .with_limits(limits.iter().copied())
            .unwrap(),
    )
}

#[test]
fn vertex_limit_admits_at_and_refuses_above_the_host_bound() {
    let primary = document("quad", 0);
    let (code, message, capacity, details) =
        ingest(&mut with(&[("maxVertices", 3.0)]), &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded");
    assert_eq!(
        message,
        "The decoded mesh has 4 vertices; the configured vertex limit is 3 vertices."
    );
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(
        details.unwrap(),
        json!({"limitClass": "admission", "unit": "vertices", "limit": 3, "required": 4})
    );
    ingest(&mut with(&[("maxVertices", 4.0)]), &primary).unwrap();
}

#[test]
fn triangle_limit_admits_at_and_refuses_above_the_host_bound() {
    let primary = document("quad", 0);
    let (code, message, capacity, details) =
        ingest(&mut with(&[("maxTriangles", 1.0)]), &primary).unwrap_err();
    assert_eq!(code, "limit-exceeded");
    assert_eq!(
        message,
        "The decoded mesh has 2 triangles; the configured triangle limit is 1 triangles."
    );
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(
        details.unwrap(),
        json!({"limitClass": "admission", "unit": "triangles", "limit": 1, "required": 2})
    );
    ingest(&mut with(&[("maxTriangles", 2.0)]), &primary).unwrap();
}

#[test]
fn resource_and_total_byte_limits_admit_at_and_refuse_above_the_host_bound() {
    let primary = document("quad", 0);
    let resource = quad_buffer().len() as f64;
    let (_, _, capacity, details) =
        ingest(&mut with(&[("maxResourceBytes", resource - 1.0)]), &primary).unwrap_err();
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    assert_eq!(
        details.unwrap(),
        json!({"limitClass": "admission", "unit": "bytes", "limit": 59, "required": 60})
    );
    ingest(&mut with(&[("maxResourceBytes", resource)]), &primary).unwrap();
    let total = primary.len() as f64 + resource;
    let bounded = |limit: f64| {
        with(&[
            ("maxSubjectBytes", primary.len() as f64),
            ("maxResourceBytes", resource),
            ("maxTotalBinaryBytes", limit),
        ])
    };
    let (_, message, capacity, _) = ingest(&mut bounded(total - 1.0), &primary).unwrap_err();
    assert!(message.starts_with("The binary closure is "), "{message}");
    assert_eq!(capacity, Some("GEOSPEC_RESOURCE_LIMIT"));
    ingest(&mut bounded(total), &primary).unwrap();
}

#[test]
fn initialize_echoes_every_host_selected_bound() {
    let limits = [
        ("maxSubjectBytes", 1000.0),
        ("maxResourceBytes", 2000.0),
        ("maxTotalBinaryBytes", 3000.0),
        ("maxVertices", 4.0),
        ("maxTriangles", 5.0),
        ("maxOccurrences", 6.0),
        ("maxMeshBytes", 7000.0),
        ("maxMeshEntries", 8.0),
        ("maxSolidEntries", 9.0),
        ("maxRetainedSubjects", 10.0),
    ];
    assert_eq!(limits.map(|(name, _)| name), EngineConfig::LIMIT_NAMES);
    let configuration = configuration(&with(&limits));
    for (name, value) in limits {
        let echoed = match name {
            "maxMeshBytes" | "maxMeshEntries" | "maxSolidEntries" => {
                &configuration["analysisRetentionLimits"][name]
            }
            "maxRetainedSubjects" => &configuration[name],
            _ => &configuration["binaryAdmissionLimits"][name],
        };
        assert_eq!(echoed.as_f64(), Some(value), "{name}");
    }
}
