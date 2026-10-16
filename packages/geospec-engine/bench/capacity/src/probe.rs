//! One measured run of a generated fixture against one limits profile.
use geospec_engine_native_core::{Engine, EngineConfig};
use serde_json::{json, Value};
use std::{fs, io, path::Path, time::Instant};

/// Peak and current resident set of this process from `/proc/self/status`, in bytes.
fn rss() -> Value {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|kib| kib.parse::<u64>().ok())
            .map(|kib| kib * 1024)
    };
    json!({"peakBytes": field("VmHWM:"), "currentBytes": field("VmRSS:")})
}

fn ms(start: Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 1e6).round() / 1e3
}

fn envelope(method: &str, id: &str) -> Value {
    json!({
        "method": method,
        "requestId": id,
        "protocolVersion": 3,
        "registryVersion": 5,
        "canonicalProfile": "geospec-jcs-v1"
    })
}

/// Keeps a bounded preview of large evidence so the record stays small.
fn bounded(value: &Value) -> Value {
    let text = value.to_string();
    if text.len() <= 2048 {
        value.clone()
    } else {
        json!({"truncatedPreview": &text[..2048], "bytes": text.len()})
    }
}

/// Classifies one canonical claim result; the runner applies the assertions.
fn claim_record(claim: &Value, result: &Value, millis: f64) -> Value {
    let diagnostics = result["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let first = diagnostics.first();
    json!({
        "claimId": claim["claimId"],
        "capability": claim["capability"],
        "expectedTruth": claim["expectedTruth"],
        "status": result["status"],
        "code": first.map(|d| d["code"].clone()).unwrap_or(Value::Null),
        "message": first.map(|d| d["message"].clone()).unwrap_or(Value::Null),
        "details": first.map(|d| d["details"].clone()).unwrap_or(Value::Null),
        "diagnosticCodes": diagnostics.iter().map(|d| d["code"].clone()).collect::<Vec<_>>(),
        "measured": bounded(&result["measured"]),
        "evidence": bounded(&result["evidence"]),
        "workUnits": result.get("workUnits").cloned().unwrap_or(Value::Null),
        "wallMs": millis,
    })
}

fn submit(engine: &Engine, subjects: &[Value], claims: &[Value]) -> Result<Value, Value> {
    let mut request = envelope("submitClaims", "capacity");
    request["plan"] = json!({"subjects": subjects, "claims": claims});
    match engine.process_request(&serde_json::to_vec(&request).expect("request encodes")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).expect("canonical response")),
        Err(error) => Err(json!({"protocolCode": error.code(), "message": error.to_string()})),
    }
}

/// Runs `dir/fixture.json` and returns the measurement record.
/// `only` selects one claim id (`combined` for a combined plan); `Some("")`
/// admits without running claims.
pub fn run(dir: &Path, limits: Option<&str>, budget: u64, only: Option<&str>) -> io::Result<Value> {
    let started = Instant::now();
    let manifest: Value = serde_json::from_slice(&fs::read(dir.join("fixture.json"))?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let config = match limits {
        None => EngineConfig::entry(),
        Some(json) => match EngineConfig::entry().with_limits_json(json.as_bytes()) {
            Ok(config) => config,
            Err(error) => {
                return Ok(
                    json!({"fixture": manifest["id"], "configurationError": {"code": error.code(), "message": error.to_string()}}),
                )
            }
        },
    };
    let mut engine = Engine::with_config(config);
    let configuration: Value = serde_json::from_slice(
        &engine
            .process_request(&serde_json::to_vec(&envelope("initialize", "init")).expect("encodes"))
            .map_err(|e| io::Error::other(e.to_string()))?,
    )
    .expect("canonical initialize");
    let configuration = configuration["result"]["configuration"].clone();

    let mut admissions = Vec::new();
    let mut handles = Vec::new();
    let mut admission_refusal = Value::Null;
    let mut read_bytes = 0_u64;
    let subjects = manifest["subjects"].as_array().cloned().unwrap_or_default();
    for (index, subject) in subjects.iter().enumerate() {
        let read = Instant::now();
        let primary = fs::read(dir.join(subject["primary"].as_str().unwrap_or_default()))?;
        let mut buffers = Vec::new();
        let mut metadata = Vec::new();
        for resource in subject["resources"].as_array().cloned().unwrap_or_default() {
            let name = resource["name"].as_str().unwrap_or_default().to_owned();
            let bytes = fs::read(dir.join(&name))?;
            metadata.push(json!({"name": name, "byteLength": bytes.len()}));
            buffers.push(bytes);
        }
        let read_ms = ms(read);
        read_bytes += primary.len() as u64 + buffers.iter().map(|b| b.len() as u64).sum::<u64>();
        let mut request = envelope("ingestSubject", &format!("admit-{index}"));
        request["format"] = subject["format"].clone();
        request["frame"] =
            json!({"coordinateSystem": "z-up", "sourceUnit": "mm", "outputUnit": "mm"});
        request["ingestOptions"] = json!({});
        request["primaryByteLength"] = json!(primary.len());
        request["resources"] = json!(metadata);
        let admit = Instant::now();
        let outcome = engine.ingest_subject(
            &serde_json::to_vec(&request).expect("encodes"),
            &primary,
            buffers,
        );
        let admit_ms = ms(admit);
        drop(primary);
        match outcome {
            Ok(bytes) => {
                let response: Value = serde_json::from_slice(&bytes).expect("canonical admission");
                let hash = response["result"]["subject"]["subjectHash"].clone();
                handles.push(json!({"slot": format!("s{index}"), "subjectHash": hash}));
                if index < 4 || index + 1 == subjects.len() {
                    admissions.push(json!({"subject": index, "readMs": read_ms, "admitMs": admit_ms, "rss": rss()}));
                }
            }
            Err(error) => {
                admission_refusal = json!({
                    "subject": index,
                    "protocolCode": error.code(),
                    "message": error.to_string(),
                    "admitMs": admit_ms,
                });
                break;
            }
        }
    }
    let after_admission = rss();
    eprintln!(
        "capacity-probe: admitted {} of {} subjects in {} ms",
        handles.len(),
        subjects.len(),
        ms(started)
    );

    let mut claims = Vec::new();
    let mut plan_error = Value::Null;
    if admission_refusal.is_null() {
        let authored: Vec<Value> = if manifest["plan"] == "combined" {
            // Every subject's bounding box and watertight claim in one plan.
            subjects
                .iter()
                .enumerate()
                .flat_map(|(index, subject)| {
                    let bounds = &subject["facts"]["bounds"];
                    [
                        json!({"claimId": format!("s{index}-bounds"), "capability": "toHaveBoundingBox", "slot": format!("s{index}"),
                            "payload": {"kind": "boundingBox", "expected": {"min": bounds[0], "max": bounds[1], "tolerance": 0.001}}, "expectedTruth": true}),
                        json!({"claimId": format!("s{index}-watertight"), "capability": "toBeWatertight", "slot": format!("s{index}"),
                            "payload": {"kind": "watertight", "expected": true}, "expectedTruth": true}),
                    ]
                })
                .collect()
        } else {
            manifest["claims"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut claim| {
                    claim["slot"] = json!("s0");
                    claim
                })
                .collect()
        };
        let authored: Vec<Value> = authored
            .into_iter()
            .filter(|claim| match only {
                None => true,
                Some("") => false,
                Some("combined") => manifest["plan"] == "combined",
                Some(id) => claim["claimId"] == id,
            })
            .collect();
        let wire = |claim: &Value| {
            json!({
                "claimId": claim["claimId"],
                "capability": claim["capability"],
                "subjectSlots": [claim["slot"]],
                "payload": claim["payload"],
                "polarity": "positive",
                "workUnitBudget": budget,
            })
        };
        if manifest["plan"] == "combined" && !authored.is_empty() {
            let wired: Vec<Value> = authored.iter().map(wire).collect();
            let start = Instant::now();
            match submit(&engine, &handles, &wired) {
                Ok(response) => {
                    let millis = ms(start);
                    let results = response["result"]["results"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for (claim, result) in authored.iter().zip(&results) {
                        claims.push(claim_record(claim, result, millis));
                    }
                }
                Err(error) => plan_error = error,
            }
        } else {
            for claim in &authored {
                eprintln!("capacity-probe: claim {} started", claim["claimId"]);
                let start = Instant::now();
                match submit(&engine, &handles[..1], &[wire(claim)]) {
                    Ok(response) => {
                        let millis = ms(start);
                        let result = &response["result"]["results"][0];
                        let mut record = claim_record(claim, result, millis);
                        record["rssAfter"] = rss();
                        claims.push(record);
                    }
                    Err(error) => {
                        claims.push(json!({"claimId": claim["claimId"], "capability": claim["capability"],
                            "expectedTruth": claim["expectedTruth"], "protocolError": error, "wallMs": ms(start)}));
                    }
                }
            }
        }
    }
    Ok(json!({
        "schema": "geospec-capacity-probe-v1",
        "fixture": manifest["id"],
        "configurationProfile": configuration["configurationProfile"],
        "binaryProfileId": configuration["binaryAdmissionLimits"]["profileId"],
        "configuration": configuration,
        "workUnitBudget": budget,
        "subjects": subjects.len(),
        "admittedSubjects": handles.len(),
        "readBytes": read_bytes,
        "admissions": admissions,
        "admissionRefusal": admission_refusal,
        "rssAfterAdmission": after_admission,
        "planError": plan_error,
        "claims": claims,
        "observations": serde_json::from_slice::<Value>(&engine.observations()).unwrap_or(Value::Null),
        "rss": rss(),
        "wallMs": ms(started),
    }))
}
