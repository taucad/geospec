//! S10 cell S02: the owned Rust byte boundary under Miri.
//!
//! Every host binding (NAPI, CPython, Emscripten) forwards caller bytes to
//! these `geospec-engine-native-core` methods. Miri checks them for undefined
//! behaviour on the predeclared malformed, lifecycle and frame inputs. The
//! scope is Rust-only: no OCCT code runs here, so nothing in this file is
//! evidence about OCCT or the C++ bridge.
use geospec_engine_native_core::{canonicalize, sha256_hex, Engine, ProtocolError};
use serde_json::{json, Value};

const TYPED_CODES: [&str; 11] = [
    "duplicate-key",
    "invalid-claim",
    "invalid-json",
    "invalid-number",
    "invalid-request",
    "invalid-utf8",
    "limit-exceeded",
    "unknown-capability",
    "unsupported-version",
    "unsupported-capability",
    "backend-failure",
];

fn typed(result: Result<Vec<u8>, ProtocolError>) -> Result<Vec<u8>, &'static str> {
    result.map_err(|error| {
        let code = error.code();
        assert!(TYPED_CODES.contains(&code), "untyped code {code}");
        assert!(!error.to_string().is_empty(), "empty message for {code}");
        code
    })
}

/// Fixed xorshift32 bytes: the same garbage on every run and host.
fn garbage(seed: u32, length: usize) -> Vec<u8> {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

fn mesh(seed: u32) -> Vec<u8> {
    let mut bytes = b"GSM1".to_vec();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for point in [
        [f64::from(seed), 0.0, 0.0],
        [f64::from(seed) + 1.0, 0.0, 0.0],
        [f64::from(seed), 1.0, 0.0],
    ] {
        for coordinate in point {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    for index in [0_u32, 1, 2] {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn control(method: &str, request_id: &str, fields: Value) -> Vec<u8> {
    let mut value = json!({
        "method": method,
        "requestId": request_id,
        "protocolVersion": 3,
        "registryVersion": 5,
        "canonicalProfile": "geospec-jcs-v1",
    });
    for (key, field) in fields.as_object().expect("control fields") {
        value[key] = field.clone();
    }
    serde_json::to_vec(&value).expect("control bytes")
}

fn admit(engine: &mut Engine, seed: u32) -> String {
    let bytes = mesh(seed);
    let hash = sha256_hex(&bytes);
    let request = control(
        "ingestSubject",
        "admit",
        json!({"contentHash": hash, "format": "mesh-buffer-v1", "frame": {"coordinateSystem": "z-up", "unit": "mm"}}),
    );
    typed(engine.ingest_mesh(&request, &bytes)).expect("admit mesh");
    hash
}

fn claim(hash: &str, budget: u64) -> Vec<u8> {
    control(
        "submitClaims",
        "claim",
        json!({"plan": {
            "subjects": [{"slot": "subject", "contentHash": hash}],
            "claims": [{
                "claimId": "claim",
                "capability": "toHaveBoundingBox",
                "subjectSlots": ["subject"],
                "payload": {"kind": "boundingBox", "expected": {"size": {"x": {"greaterThanOrEqual": 0}}}},
                "polarity": "positive",
                "workUnitBudget": budget
            }]
        }}),
    )
}

fn handle(engine: &Engine, hash: &str) -> Result<Value, &'static str> {
    let bytes = typed(engine.subject_handle(&control(
        "subjectHandle",
        "handle",
        json!({"contentHash": hash}),
    )))?;
    let response: Value = serde_json::from_slice(&bytes).expect("handle response");
    Ok(response["result"]["subjectHandle"].clone())
}

fn release(engine: &mut Engine, subject_handle: &Value) -> Result<bool, &'static str> {
    let bytes = typed(engine.release_subject(&control(
        "releaseSubject",
        "release",
        json!({"subjectHandle": subject_handle}),
    )))?;
    let response: Value = serde_json::from_slice(&bytes).expect("release response");
    Ok(response["result"]["released"]
        .as_bool()
        .expect("released flag"))
}

#[test]
fn canonicalize_contains_malformed_and_nested_input() {
    let mut nested = "[".repeat(48);
    nested.push_str(&"]".repeat(48));
    let inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"{".to_vec(),
        vec![0xff, 0xfe, 0x00],
        br#"{"a":1,"a":2}"#.to_vec(),
        b"[1e400]".to_vec(),
        br#"{"b":[1,2.5,{"c":null}],"a":"\u00e9\n"}"#.to_vec(),
        nested.into_bytes(),
        garbage(0x5310_0001, 97),
    ];
    for input in inputs {
        if let Ok(canonical) = typed(canonicalize(&input)) {
            assert_eq!(typed(canonicalize(&canonical)), Ok(canonical));
        }
    }
}

#[test]
fn every_byte_method_refuses_empty_and_garbage_input() {
    let mut engine = Engine::new();
    let inputs = [Vec::new(), garbage(0x5310_0002, 257), b"null".to_vec()];
    for input in &inputs {
        assert!(typed(engine.process_request(input)).is_err());
        assert!(typed(engine.canonical_plan(input)).is_err());
        assert!(typed(engine.evaluate_plan(input)).is_err());
        assert!(typed(engine.evaluate_claim(input)).is_err());
        assert!(typed(engine.subject_handle(input)).is_err());
        assert!(typed(engine.release_subject(input)).is_err());
        assert!(typed(engine.ingest_mesh(input, &mesh(1))).is_err());
        assert!(typed(engine.ingest_subject(input, input, vec![input.clone()])).is_err());
    }
    let request = control(
        "ingestSubject",
        "admit",
        json!({"contentHash": sha256_hex(b""), "format": "mesh-buffer-v1", "frame": {"coordinateSystem": "z-up", "unit": "mm"}}),
    );
    assert!(typed(engine.ingest_mesh(&request, &[])).is_err());
    assert!(typed(engine.ingest_mesh(&request, &garbage(0x5310_0003, 64))).is_err());
}

#[test]
fn declared_lengths_that_disagree_with_the_buffers_are_refused() {
    let mut engine = Engine::new();
    let primary = garbage(0x5310_0004, 32);
    for (declared, resources) in [
        (json!(primary.len() + 1), json!([])),
        (json!(9_007_199_254_740_991_u64), json!([])),
        (json!(-1), json!([])),
        (
            json!(primary.len()),
            json!([{"name": "mesh.bin", "byteLength": 4}]),
        ),
    ] {
        let request = control(
            "ingestSubject",
            "admit",
            json!({
                "format": "gltf",
                "frame": {"coordinateSystem": "z-up", "outputUnit": "mm", "sourceUnit": "mm"},
                "ingestOptions": {},
                "primaryByteLength": declared,
                "resources": resources
            }),
        );
        assert!(typed(engine.ingest_subject(&request, &primary, vec![vec![0; 3]])).is_err());
    }
}

#[test]
fn subject_lifecycle_rejects_stale_and_foreign_handles() {
    let mut first = Engine::new();
    let mut second = Engine::new();
    let hash = admit(&mut first, 7);
    let owned = handle(&first, &hash).expect("handle");
    assert!(typed(first.process_request(&claim(&hash, 4))).is_ok());
    assert!(typed(first.process_request(&claim(&hash, 0))).is_err());

    // L03: another engine refuses the handle and the subject it names.
    assert!(release(&mut second, &owned).is_err());
    assert!(typed(second.process_request(&claim(&hash, 4))).is_err());

    // L01 and L02: the first release wins, the second reports a stale handle,
    // and every later query is refused before evaluation.
    assert_eq!(release(&mut first, &owned), Ok(true));
    assert_eq!(release(&mut first, &owned), Ok(false));
    assert!(typed(first.process_request(&claim(&hash, 4))).is_err());
    assert!(handle(&first, &hash).is_err());

    // A forged generation never releases the readmitted subject.
    let readmitted = admit(&mut first, 7);
    let current = handle(&first, &readmitted).expect("current handle");
    assert_ne!(current["generation"], owned["generation"]);
    for generation in ["99", "18446744073709551615"] {
        let mut forged = current.clone();
        forged["generation"] = json!(generation);
        assert_ne!(release(&mut first, &forged), Ok(true));
    }
    assert_eq!(release(&mut first, &current), Ok(true));
}

#[test]
fn one_call_claim_frames_agree_with_the_two_call_path() {
    let mut engine = Engine::new();
    let hash = admit(&mut engine, 11);
    let request = claim(&hash, 4);
    let frame = typed(engine.evaluate_claim(&request)).expect("claim frame");
    let length = |offset: usize| {
        let bytes: [u8; 4] = frame[offset..offset + 4].try_into().expect("u32 length");
        u32::from_le_bytes(bytes) as usize
    };
    let claim_start = 8 + length(0);
    let result_start = claim_start + length(4);
    assert!(result_start <= frame.len());
    let plan = typed(engine.canonical_plan(&request)).expect("plan");
    assert_eq!(&frame[8..claim_start], plan.as_slice());
    assert_eq!(
        &frame[result_start..],
        typed(engine.evaluate_plan(&plan))
            .expect("result")
            .as_slice()
    );
}
