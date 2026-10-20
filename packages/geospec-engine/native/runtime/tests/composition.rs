use geospec_engine_native_runtime::{create_engine, Engine, EngineConfig};
use serde_json::{json, Value};

fn ingest(
    engine: &mut Engine,
    bytes: Vec<u8>,
    buffers: Vec<Vec<u8>>,
    format: &str,
    source: &str,
) -> Value {
    let request = json!({"method":"ingestSubject","requestId":"admit","protocolVersion":3,"registryVersion":5,"canonicalProfile":"geospec-jcs-v1","format":format,"frame":{"coordinateSystem":"z-up","sourceUnit":source,"outputUnit":"mm"},"ingestOptions":{},"primaryByteLength":bytes.len(),"resources":buffers.iter().map(|b|json!({"name":"mesh.bin","byteLength":b.len()})).collect::<Vec<_>>()});
    serde_json::from_slice(
        &engine
            .ingest_subject(&serde_json::to_vec(&request).unwrap(), &bytes, buffers)
            .unwrap(),
    )
    .unwrap()
}
fn evaluate(engine: &Engine, hash: &str, capability: &str, payload: Value) -> Value {
    let request = json!({"method":"submitClaims","requestId":"evaluate","protocolVersion":3,"registryVersion":5,"canonicalProfile":"geospec-jcs-v1","plan":{"subjects":[{"slot":"part","subjectHash":hash}],"claims":[{"claimId":"measure","capability":capability,"subjectSlots":["part"],"payload":payload,"polarity":"positive","workUnitBudget":1000000}]}});
    let plan = engine
        .canonical_plan(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    serde_json::from_slice(&engine.evaluate_plan(&plan).unwrap()).unwrap()
}

#[test]
fn external_resource_identity_and_real_step_share_the_composed_engine() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../conformance/subject-identity-instance-v2-controls.json"
    ))
    .unwrap();
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for row in fixture["rows"].as_array().unwrap() {
        let hex = row["resourceHex"].as_str().unwrap();
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let source = if row["id"].as_str().unwrap().ends_with("scale10") {
            "cm"
        } else {
            "mm"
        };
        let result = ingest(
            &mut engine,
            fixture["primaryUtf8"].as_str().unwrap().as_bytes().to_vec(),
            vec![bytes],
            "gltf",
            source,
        );
        assert_eq!(
            result["result"]["subject"]["subjectHash"],
            row["subjectHash"]
        );
        let expected = if row["id"].as_str().unwrap().starts_with("A") {
            4.0
        } else {
            1.0
        } * if source == "cm" { 100.0 } else { 1.0 };
        let result = evaluate(
            &engine,
            row["subjectHash"].as_str().unwrap(),
            "toHaveSurfaceArea",
            json!({"kind":"surfaceArea","expected":{"value":expected}}),
        );
        assert_eq!(result["results"][0]["status"], "passed", "{result}");
    }
    let step = include_bytes!("../../occt/rust/tests/fixtures/ap242-box.step").to_vec();
    let admitted = ingest(&mut engine, step, vec![], "step", "auto");
    let result = evaluate(
        &engine,
        admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap(),
        "toHaveVolume",
        json!({"kind":"volume","expected":{"value":6000.0}}),
    );
    assert_eq!(result["results"][0]["status"], "passed", "{result}");
}

#[test]
fn ordinary_kernel_fillets_chamfers_and_bolt_patterns_are_decided() {
    // The exact claims refused in the 2026-10-08 kernel-skill acceptance
    // runs, each with a mismatch control so "passed" is a decision, not a
    // leak. The perforated top face of the block stays unknown, so chamfer
    // absence there remains an honest refusal.
    let pattern = |count: u32| json!({"kind":"circularHolePattern","expected":{"count":count,"holeDiameter":5,"boltCircleDiameter":45}});
    let fillet = |radius: u32| json!({"kind":"filletFeature","expected":{"radius":radius}});
    let chamfer = |distance: u32| json!({"kind":"chamferFeature","expected":{"distance":distance}});
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    let mut cases = Vec::new();
    for file in ["replicad-coupling.step", "build123d-coupling.step"] {
        cases.push((
            file,
            "toHaveCircularHolePattern",
            pattern(6),
            "passed",
            Some(1),
        ));
        cases.push((
            file,
            "toHaveCircularHolePattern",
            pattern(5),
            "failed",
            Some(0),
        ));
        cases.push((file, "toHaveFilletFeature", fillet(1), "passed", Some(2)));
        cases.push((file, "toHaveFilletFeature", fillet(2), "failed", Some(0)));
    }
    cases.push((
        "build123d-chamfer-block.step",
        "toHaveChamferFeature",
        chamfer(1),
        "passed",
        Some(4),
    ));
    cases.push((
        "build123d-chamfer-block.step",
        "toHaveChamferFeature",
        chamfer(2),
        "refused",
        None,
    ));
    for (file, capability, payload, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/kernel-skill-acceptance")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, capability, payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file} {capability}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file} {capability}: {result}"
        );
    }
}

#[test]
fn sketched_holes_split_across_two_faces_are_decided() {
    // W5b: replicad's drawCircle is two arcs, so every sketched hole is a band
    // of two half-cylinders. Each case has a mismatch control, and the half
    // notch that opens onto a side face stays an honest refusal.
    let hole = |diameter: u32, through: bool| json!({"kind":"circularHole","expected":{"diameter":diameter,"through":through,"axis":"z"}});
    let centered = json!({"kind":"circularHole","expected":{"diameter":10,"center":[0,0,10]}});
    let pattern = |count: u32| json!({"kind":"circularHolePattern","expected":{"count":count,"holeDiameter":6,"boltCircleDiameter":20}});
    let cases = [
        (
            "s1-sketched-through.step",
            "toHaveCircularHole",
            hole(10, true),
            "passed",
            Some(1),
        ),
        (
            "s1-sketched-through.step",
            "toHaveCircularHole",
            centered,
            "passed",
            Some(1),
        ),
        (
            "s1-sketched-through.step",
            "toHaveCircularHole",
            hole(10, false),
            "failed",
            Some(0),
        ),
        (
            "s1-sketched-through.step",
            "toHaveCircularHole",
            hole(12, true),
            "failed",
            Some(0),
        ),
        (
            "s0-washer.step",
            "toHaveCircularHole",
            hole(8, true),
            "passed",
            Some(1),
        ),
        (
            "s0-washer.step",
            "toHaveCircularHole",
            hole(20, true),
            "failed",
            Some(0),
        ),
        (
            "s2-sketched-pattern.step",
            "toHaveCircularHolePattern",
            pattern(2),
            "passed",
            Some(1),
        ),
        (
            "s2-sketched-pattern.step",
            "toHaveCircularHolePattern",
            pattern(3),
            "failed",
            Some(0),
        ),
        (
            "s5-edge-half-notch.step",
            "toHaveCircularHole",
            hole(12, true),
            "refused",
            None,
        ),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, capability, payload, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/circular-bores-split")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, capability, payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file} {capability}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file} {capability}: {result}"
        );
    }
}

#[test]
fn chamfered_and_counterbored_hole_ends_are_decided() {
    // W5b: a treated mouth still opens the bore, a counterbore band ends on
    // its shoulder, and an internal groove keeps absence claims refused.
    let hole = |diameter: u32, through: bool| json!({"kind":"circularHole","expected":{"diameter":diameter,"through":through,"axis":"z"}});
    let cases = [
        ("t0-chamfered-mouth.step", hole(6, true), "passed", Some(1)),
        ("t0-chamfered-mouth.step", hole(8, true), "failed", Some(0)),
        (
            "t3-chamfered-both-ends.step",
            hole(6, true),
            "passed",
            Some(1),
        ),
        ("t2-rounded-mouth.step", hole(6, false), "failed", Some(0)),
        ("t7-blind-chamfered.step", hole(6, false), "passed", Some(1)),
        ("t4-counterbore.step", hole(10, false), "passed", Some(1)),
        ("t4-counterbore.step", hole(10, true), "failed", Some(0)),
        ("t6-internal-groove.step", hole(10, false), "refused", None),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, payload, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/circular-bores-mouths")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, "toHaveCircularHole", payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file}: {result}"
        );
    }
}

#[test]
fn cross_drilled_bores_and_exterior_cones_are_decided() {
    // W5b: a set-screw hole through a collar's wall keeps the bore and is a
    // through hole itself, so a collar's inventory is complete and a missing
    // hole fails; an outer chamfer no longer leaves the inventory open, and a
    // tangent tee branch keeps its refusal.
    let hole = |diameter: u32, through: bool| json!({"kind":"circularHole","expected":{"diameter":diameter,"through":through,"axis":"z"}});
    let centered = json!({"kind":"circularHole","expected":{"diameter":12,"center":[0,0,10]}});
    let cases = [
        (
            "p0-set-screw-across-seam.step",
            hole(12, true),
            "passed",
            Some(1),
        ),
        ("p0-set-screw-across-seam.step", centered, "passed", Some(1)),
        (
            "p0-set-screw-across-seam.step",
            hole(14, true),
            "failed",
            Some(0),
        ),
        (
            "p3-chamfered-collar.step",
            hole(12, true),
            "passed",
            Some(1),
        ),
        (
            "p3-chamfered-collar.step",
            hole(12, false),
            "failed",
            Some(0),
        ),
        ("p6-chamfered-shaft.step", hole(4, true), "failed", Some(0)),
        ("p4-equal-tee.step", hole(12, true), "refused", None),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, payload, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/circular-bores-pierced")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, "toHaveCircularHole", payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file}: {result}"
        );
    }
}

#[test]
fn oblique_mouths_are_decided() {
    // W5b: a cross hole through a pin, a blind side hole and holes through a
    // slope open on their oblique rims; a curved floor that closes a bore and
    // a hole tangent to a pin's wall keep their refusals.
    let hole = |diameter: f64, through: bool, axis: &str| json!({"kind":"circularHole","expected":{"diameter":diameter,"through":through,"axis":axis}});
    let centered = json!({"kind":"circularHole","expected":{"diameter":3,"center":[0,0,30]}});
    let cases = [
        (
            "o0-clevis-pin.step",
            hole(3.0, true, "x"),
            "passed",
            Some(1),
        ),
        ("o0-clevis-pin.step", centered, "passed", Some(1)),
        (
            "o0-clevis-pin.step",
            hole(3.0, false, "x"),
            "failed",
            Some(0),
        ),
        (
            "o2-blind-side-hole.step",
            hole(3.0, false, "x"),
            "passed",
            Some(1),
        ),
        (
            "o3-hole-through-slope.step",
            hole(6.0, true, "z"),
            "passed",
            Some(1),
        ),
        (
            "o5-hole-into-slope-blind.step",
            hole(6.0, false, "z"),
            "passed",
            Some(1),
        ),
        (
            "o6-curved-floor.step",
            hole(6.0, false, "z"),
            "refused",
            None,
        ),
        (
            "o4-tangent-cross-hole.step",
            hole(3.0, true, "x"),
            "refused",
            None,
        ),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, payload, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/circular-bores-oblique")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let Some(hash) = admitted["result"]["subject"]["subjectHash"].as_str() else {
            // A tangent cross hole leaves an invalid solid, refused at admission.
            assert_eq!(status, "refused", "{file}: {admitted}");
            continue;
        };
        let result = evaluate(&engine, hash, "toHaveCircularHole", payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file}: {result}"
        );
    }
}

#[test]
fn open_arcs_leave_a_complete_inventory() {
    // W5b: rounded cutout corners, slot ends and flutes are open arcs, so a
    // part without the claimed hole fails instead of refusing; a slit bore
    // keeps its refusal.
    let hole = json!({"kind":"circularHole","expected":{"diameter":6,"axis":"z"}});
    let cases = [
        ("a0-rounded-window.step", "failed", Some(0)),
        ("a1-slot.step", "failed", Some(0)),
        ("a2-fluted-shaft.step", "failed", Some(0)),
        ("a3-slit-bore.step", "refused", None),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, status, matches) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/circular-bores-open")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, "toHaveCircularHole", hole.clone());
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        assert_eq!(
            row["evidence"]["measured"]["matchCount"].as_u64(),
            matches,
            "{file}: {result}"
        );
    }
}
