//! Current-profile-02: the authored overlay that supersedes profile 01's
//! expectation for the empty bounding-box records (ruling ENGINE-FIX-2b /
//! current-profile-02). Profile 01, early-corpus.json and mesh-entry.json stay
//! byte-identical; the overlay binds each record it supersedes to their exact
//! bytes and replaces only that record's expectation with a typed refusal.
// Each conformance test crate uses the part of this module it needs.
#![allow(dead_code)]

use geospec_engine_native_core::sha256_hex;
use serde_json::Value;

pub const OVERLAY: &str = include_str!("../fixtures/current-profile-02/plan-corpus-overlay.json");
pub const OVERLAY_SHA256: &str = "87db731972127e31bfc713f58f6b81004dce80f1bb753f03b76fa6065c2f4ee6";
const PROFILE_01_SHA256: &str = "eb8b42f1591fd2bd695228cdaa3abc4108b411717c468a9e97b724654616221d";
const ORIGINAL_SHA256: &str = "3d43750d055dceec2b7d57c92d4a953c4f7dcd40c2abb1452a82de83ea729476";
const MESH_ENTRY_SHA256: &str = "b7b9b8b166ef8aba4df961aca59c064c3913a5b3833e610c2296c48b144e4bec";
const RECORDS: usize = 9;

/// The pinned overlay records, after checking the overlay's own authority.
pub fn records() -> Vec<Value> {
    assert_eq!(
        sha256_hex(OVERLAY),
        OVERLAY_SHA256,
        "current-profile-02 SHA-256"
    );
    let overlay: Value = serde_json::from_str(OVERLAY).expect("current-profile-02 JSON");
    let authority = &overlay["authority"];
    assert_eq!(overlay["schemaVersion"], 1);
    assert_eq!(authority["id"], "current-profile-02");
    assert_eq!(
        authority["adoptedRuling"],
        "ENGINE-FIX-2b / current-profile-02"
    );
    assert_eq!(authority["classification"], "authored-ruling-overlay");
    let supersedes = &authority["supersedes"];
    assert_eq!(supersedes["id"], "current-profile-01");
    assert_eq!(
        supersedes["adoptedRuling"],
        "W2.C-CURRENT-PROFILE-CONFORMANCE-01"
    );
    assert_eq!(supersedes["sha256"], PROFILE_01_SHA256);
    assert_eq!(supersedes["originalCorpusSha256"], ORIGINAL_SHA256);
    assert_eq!(supersedes["meshEntrySha256"], MESH_ENTRY_SHA256);
    let records = overlay["records"]
        .as_array()
        .expect("overlay records")
        .clone();
    assert_eq!(records.len(), RECORDS, "overlay record count");
    records
}

/// Profile 01 with each overlaid binding's expectation replaced by the
/// overlay's refusal. Panics unless every overlaid record names exactly the
/// profile-01 input and expectation it supersedes.
pub fn apply(profile_01_text: &str) -> Value {
    assert_eq!(sha256_hex(profile_01_text), PROFILE_01_SHA256);
    let mut profile: Value = serde_json::from_str(profile_01_text).expect("profile 01 JSON");
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
            claims[0]["payload"]["expected"], row["authoredExpected"],
            "{id}: authored expectation"
        );
        let superseded = binding["expectedUtf8"].as_str().expect("superseded pass");
        assert_eq!(
            sha256_hex(superseded),
            row["supersededExpectedUtf8Sha256"],
            "{id}: superseded expectation"
        );
        assert_eq!(row["expectedCode"], "invalid-claim", "{id}");
        assert_eq!(
            row["polarities"],
            serde_json::json!(["positive", "negative"])
        );
        let binding = binding.as_object_mut().expect("binding object");
        binding.remove("expectedUtf8");
        binding.insert("expectedCode".into(), row["expectedCode"].clone());
        binding.insert("expectedMessage".into(), row["expectedMessage"].clone());
    }
    profile
}

/// The overlay row for a record ID, if the overlay supersedes it.
pub fn row(id: &str) -> Option<Value> {
    records().into_iter().find(|row| row["id"] == id)
}

/// The same request with its one claim's polarity flipped to negative.
pub fn negative(input: &str) -> String {
    let positive = r#""polarity":"positive""#;
    assert_eq!(input.matches(positive).count(), 1, "one positive claim");
    input.replace(positive, r#""polarity":"negative""#)
}
