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

#[test]
fn void_paths_through_general_solids_are_decided() {
    // W5f: a bore through a bend, a slit collar and a blocked bore are
    // outside the box and cylinder domains. Clear straight segments prove a
    // path, a waypoint in the wall fails it, and a segment through the web
    // or an isolation constraint refuses.
    let claim = |path: Value, min: [f64; 3], max: [f64; 3], extra: Value| {
        let mut expected = json!({"path": path, "bounds": {"min": min, "max": max}});
        for (key, value) in extra.as_object().unwrap() {
            expected[key] = value.clone();
        }
        json!({"kind": "voidContinuity", "expected": expected})
    };
    let bend = json!([
        [0, 0, 1],
        [0, 0, 20],
        [4.019237886466843, 0, 35],
        [15, 0, 45.98076211353316],
        [30, 0, 50],
        [49, 0, 50]
    ]);
    let none = json!({});
    let cases = [
        (
            "w0-elbow.step",
            claim(bend, [-7.0, -7.0, 0.5], [49.5, 7.0, 57.0], none.clone()),
            "passed",
        ),
        (
            "w0-elbow.step",
            claim(
                json!([[0, 0, 1], [49, 0, 50]]),
                [-7.0, -7.0, 0.5],
                [49.5, 7.0, 57.0],
                none.clone(),
            ),
            "refused",
        ),
        (
            "w0-elbow.step",
            claim(
                json!([[0, 0, 1], [0, 8, 10]]),
                [-9.0, -9.0, 0.5],
                [9.0, 9.0, 20.0],
                none.clone(),
            ),
            "failed",
        ),
        (
            "w0-elbow.step",
            claim(
                json!([[0, 0, 1], [0, 0, 19]]),
                [-6.0, -6.0, 0.5],
                [6.0, 6.0, 20.0],
                json!({"minCrossSection": 113}),
            ),
            "passed",
        ),
        (
            "w0-elbow.step",
            claim(
                json!([[0, 0, 1], [0, 0, 19]]),
                [-6.0, -6.0, 0.5],
                [6.0, 6.0, 20.0],
                json!({"minCrossSection": 114}),
            ),
            "refused",
        ),
        (
            "w1-split-collar.step",
            claim(
                json!([[0, 0, 1], [0, 0, 19]]),
                [-5.0, -5.0, 0.0],
                [5.0, 5.0, 20.0],
                none.clone(),
            ),
            "passed",
        ),
        (
            "w2-blocked-bore.step",
            claim(
                json!([[0, 0, 1], [0, 0, 29]]),
                [-3.0, -3.0, 0.0],
                [3.0, 3.0, 30.0],
                none.clone(),
            ),
            "refused",
        ),
        (
            "w2-blocked-bore.step",
            claim(
                json!([[0, 0, 1], [0, 0, 15]]),
                [-3.0, -3.0, 0.0],
                [3.0, 3.0, 30.0],
                none.clone(),
            ),
            "failed",
        ),
        (
            "w2-blocked-bore.step",
            claim(
                json!([[0, 0, 1], [0, 0, 13]]),
                [-3.0, -3.0, 0.0],
                [3.0, 3.0, 30.0],
                json!({"isolatedFrom": [[0, 0, 20]]}),
            ),
            "refused",
        ),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, payload, status) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/void-witness")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let result = evaluate(&engine, hash, "toHaveVoidContinuity", payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        if status != "refused" {
            assert_eq!(
                row["evidence"]["measured"]["profile"], "geospec-void-segment-witness-v1",
                "{file}: {result}"
            );
        }
    }
}

#[test]
fn separated_step_components_are_decided_clear() {
    // W5a: two complete regular solid sets whose faces stay apart, neither
    // holding the other, share no material. Touching pairs keep the
    // certificate refusal; a part in a closed cavity is apart, not nested.
    // W5-PF-06: overlapping and embedded pairs are decided by their exact
    // Common.
    let cases = [
        ("s0-apart.step", "passed"),
        ("s1-touching.step", "refused"),
        ("s2-overlapping.step", "failed"),
        ("s3-embedded.step", "failed"),
        ("s4-caged.step", "passed"),
        ("s5-curved.step", "passed"),
        ("../box-clearance/two-mm.step", "passed"),
    ];
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    for (file, status) in cases {
        let step = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../occt/rust/tests/fixtures/separation")
                .join(file),
        )
        .unwrap();
        let admitted = ingest(&mut engine, step, vec![], "step", "auto");
        let hash = admitted["result"]["subject"]["subjectHash"]
            .as_str()
            .unwrap();
        let payload = json!({"kind": "componentInterference", "expected": {}});
        let result = evaluate(&engine, hash, "toHaveNoComponentInterference", payload);
        let row = &result["results"][0];
        assert_eq!(row["status"], status, "{file}: {result}");
        if status == "passed" {
            let pair = &row["evidence"]["measured"]["pairs"][0];
            assert_eq!(pair["criterion"], "separated-regular-solid-sets", "{file}: {result}");
        }
    }
}

fn interference(engine: &mut Engine, directory: &str, file: &str) -> Value {
    let step = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../occt/rust/tests/fixtures")
            .join(directory)
            .join(file),
    )
    .unwrap();
    let admitted = ingest(engine, step, vec![], "step", "auto");
    let hash = admitted["result"]["subject"]["subjectHash"]
        .as_str()
        .unwrap()
        .to_owned();
    let payload = json!({"kind": "componentInterference", "expected": {}});
    let result = evaluate(engine, &hash, "toHaveNoComponentInterference", payload);
    result["results"][0].clone()
}

#[test]
fn step_interference_decides_ordinary_pairs() {
    // W5-PF-06: a STEP assembly with no allowance. Case A, two disjoint box
    // occurrences, passes on the separation certificate; case B, two boxes
    // sharing a 1 mm3 corner cube, fails on the exact regular-solid Common.
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    let apart = interference(&mut engine, "interference-decided", "boxes-apart.step");
    assert_eq!(apart["status"], "passed", "{apart}");
    let pair = &apart["evidence"]["measured"]["pairs"][0];
    assert_eq!(pair["criterion"], "separated-regular-solid-sets", "{apart}");
    assert!(
        (pair["clearance"].as_f64().unwrap() - 5.0).abs() < 1e-6,
        "{apart}"
    );

    let overlap = interference(&mut engine, "interference-decided", "corner-overlap.step");
    assert_eq!(overlap["status"], "failed", "{overlap}");
    assert_eq!(
        overlap["diagnostics"][0]["code"], "GEOSPEC_COMPONENT_INTERFERENCE",
        "{overlap}"
    );
    let measured = &overlap["evidence"]["measured"];
    assert_eq!(measured["interferingPairs"], 1, "{overlap}");
    assert_eq!(measured["undecidedPairs"], 0, "{overlap}");
    let pair = &measured["pairs"][0];
    assert_eq!(pair["criterion"], "exact-regular-solid-common", "{overlap}");
    assert_eq!(pair["residualSolids"], 1, "{overlap}");
    let volume = pair["intersectionVolume"].as_f64().unwrap();
    assert!((volume - 1.0).abs() < 1e-6, "{overlap}");
    for axis in 0..3 {
        let min = pair["residualBounds"]["min"][axis].as_f64().unwrap();
        let max = pair["residualBounds"]["max"][axis].as_f64().unwrap();
        assert!(
            (min - 19.0).abs() < 1e-3 && (max - 20.0).abs() < 1e-3,
            "{overlap}"
        );
    }
}

#[test]
fn step_interference_keeps_refusing_touching_pairs() {
    // Operator ruling 2026-10-22: a contact pair (a zero-volume Common) is
    // neither clear nor interfering; no tolerance turns it into a pass.
    let mut engine = create_engine(EngineConfig::entry()).unwrap();
    let touching = interference(&mut engine, "separation", "s1-touching.step");
    assert_eq!(touching["status"], "refused", "{touching}");
    assert_eq!(
        touching["diagnostics"][0]["message"],
        "This selected component pair has no bounded complete-material noninterference certificate.",
        "{touching}"
    );
    assert_eq!(
        touching["diagnostics"][0]["code"], "GEOSPEC_UNSUPPORTED_EVIDENCE",
        "{touching}"
    );
}
