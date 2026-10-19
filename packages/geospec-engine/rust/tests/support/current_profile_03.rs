//! Current-profile-03: the authored overlay that supersedes profile 01's
//! expectation for queries that failed without the evidence they read
//! (ruling QUERY-EVIDENCE-REFUSAL-01 / current-profile-03). Profile 01 and the
//! current-profile-02 overlay stay byte-identical; this overlay binds each
//! record it supersedes to its exact input and expectation bytes and replaces
//! only that record's expectation with the authored refusal.
// Each conformance test crate uses the part of this module it needs.
#![allow(dead_code)]

use geospec_engine_native_core::sha256_hex;
use serde_json::Value;

pub const OVERLAY: &str = include_str!("../fixtures/current-profile-03/plan-corpus-overlay.json");
pub const OVERLAY_SHA256: &str = "e8509c121b4c87f7034039eddd04673a191c8bb0c4bf535994122e9b17943867";
const PROFILE_01_SHA256: &str = "eb8b42f1591fd2bd695228cdaa3abc4108b411717c468a9e97b724654616221d";
const RECORDS: usize = 2;

/// The pinned overlay records, after checking the overlay's own authority.
pub fn records() -> Vec<Value> {
    assert_eq!(
        sha256_hex(OVERLAY),
        OVERLAY_SHA256,
        "current-profile-03 SHA-256"
    );
    let overlay: Value = serde_json::from_str(OVERLAY).expect("current-profile-03 JSON");
    let authority = &overlay["authority"];
    assert_eq!(overlay["schemaVersion"], 1);
    assert_eq!(authority["id"], "current-profile-03");
    assert_eq!(
        authority["adoptedRuling"],
        "QUERY-EVIDENCE-REFUSAL-01 / current-profile-03"
    );
    assert_eq!(authority["classification"], "authored-ruling-overlay");
    let supersedes = &authority["supersedes"];
    assert_eq!(supersedes["id"], "current-profile-01");
    assert_eq!(
        supersedes["adoptedRuling"],
        "W2.C-CURRENT-PROFILE-CONFORMANCE-01"
    );
    assert_eq!(supersedes["sha256"], PROFILE_01_SHA256);
    let records = overlay["records"]
        .as_array()
        .expect("overlay records")
        .clone();
    assert_eq!(records.len(), RECORDS, "overlay record count");
    records
}

/// `profile` (profile 01, with any earlier overlay applied) with each
/// overlaid binding's expectation replaced by the overlay's refusal. Panics
/// unless every overlaid record names exactly the input and the failed
/// expectation it supersedes, and only an evidence gap moves: the superseded
/// result failed and the authored one refuses with the same diagnostics.
pub fn apply(mut profile: Value) -> Value {
    let bindings = profile["records"].as_array_mut().expect("profile records");
    for row in records() {
        let id = row["id"].as_str().expect("overlay id");
        let matches: Vec<_> = bindings
            .iter_mut()
            .filter(|binding| binding["id"] == id)
            .collect();
        let [binding] = matches
            .try_into()
            .unwrap_or_else(|_| panic!("{id}: one binding"));
        assert_eq!(binding["operation"], row["operation"], "{id}: operation");
        assert_eq!(
            binding["effectiveInputSha256"], row["effectiveInputSha256"],
            "{id}: input"
        );
        let input: Value =
            serde_json::from_str(binding["effectiveInputUtf8"].as_str().expect("input"))
                .expect("input JSON");
        let claims = input["plan"]["claims"].as_array().expect("claims");
        assert_eq!(claims.len(), 1, "{id}: one claim");
        assert_eq!(
            claims[0]["capability"], row["capability"],
            "{id}: capability"
        );
        assert_eq!(claims[0]["polarity"], "positive", "{id}: query polarity");
        let superseded = binding["expectedUtf8"].as_str().expect("superseded result");
        assert_eq!(
            sha256_hex(superseded),
            row["supersededExpectedUtf8Sha256"],
            "{id}: superseded expectation"
        );
        let expected = row["expectedUtf8"].as_str().expect("authored result");
        let [before, after] = [superseded, expected].map(|text| {
            let value: Value = serde_json::from_str(text).expect("result JSON");
            let results = if id.starts_with("plan/") {
                &value["results"]
            } else {
                &value["result"]["results"]
            };
            let [result] = results
                .as_array()
                .expect("results")
                .clone()
                .try_into()
                .unwrap();
            result
        });
        assert_eq!(before["status"], row["supersededStatus"], "{id}");
        assert_eq!(before["status"], "failed", "{id}: superseded status");
        assert_eq!(after["status"], row["expectedStatus"], "{id}");
        assert_eq!(after["status"], "refused", "{id}: authored status");
        assert_eq!(
            after["diagnostics"], before["diagnostics"],
            "{id}: codes kept"
        );
        assert!(after.get("evidence").is_none(), "{id}: refusal evidence");
        binding["expectedUtf8"] = row["expectedUtf8"].clone();
    }
    profile
}

/// The overlay row for a record ID, if the overlay supersedes it.
pub fn row(id: &str) -> Option<Value> {
    records().into_iter().find(|row| row["id"] == id)
}
