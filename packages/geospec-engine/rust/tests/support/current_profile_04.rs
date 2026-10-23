//! Current-profile-04: the overlay that supersedes the records a malformed
//! claim expectation used to reject as a whole plan (ruling
//! PER-CLAIM-INVALID-EXPECTATION-01 / current-profile-04). Its generator,
//! `fixtures/current-profile-04/generate.mjs`, derives every expectation from
//! profile 01 and the current-profile-02 overlay alone. Profile 01 and the
//! earlier overlays stay byte-identical; this overlay binds each record it
//! supersedes to its exact input and rejection and replaces only that
//! record's expectation.
// Each conformance test crate uses the part of this module it needs.
#![allow(dead_code)]

use geospec_engine_native_core::sha256_hex;
use serde_json::Value;

pub const OVERLAY: &str = include_str!("../fixtures/current-profile-04/plan-corpus-overlay.json");
pub const OVERLAY_SHA256: &str = "04d860bca41e634ed9137ff347001c7f24867e3c8a85d3b874bf49192c60f080";
const PROFILE_01_SHA256: &str = "eb8b42f1591fd2bd695228cdaa3abc4108b411717c468a9e97b724654616221d";
const OVERLAY_02_SHA256: &str = "87db731972127e31bfc713f58f6b81004dce80f1bb753f03b76fa6065c2f4ee6";
const RECORDS: usize = 74;

/// The pinned overlay records, after checking the overlay's own authority.
pub fn records() -> Vec<Value> {
    assert_eq!(
        sha256_hex(OVERLAY),
        OVERLAY_SHA256,
        "current-profile-04 SHA-256"
    );
    let overlay: Value = serde_json::from_str(OVERLAY).expect("current-profile-04 JSON");
    let authority = &overlay["authority"];
    assert_eq!(overlay["schemaVersion"], 1);
    assert_eq!(authority["id"], "current-profile-04");
    assert_eq!(
        authority["adoptedRuling"],
        "PER-CLAIM-INVALID-EXPECTATION-01 / current-profile-04"
    );
    assert_eq!(authority["classification"], "authored-ruling-overlay");
    let supersedes = &authority["supersedes"];
    assert_eq!(supersedes["id"], "current-profile-01");
    assert_eq!(
        supersedes["adoptedRuling"],
        "W2.C-CURRENT-PROFILE-CONFORMANCE-01"
    );
    assert_eq!(supersedes["sha256"], PROFILE_01_SHA256);
    assert_eq!(supersedes["overlay02Sha256"], OVERLAY_02_SHA256);
    let records = overlay["records"]
        .as_array()
        .expect("overlay records")
        .clone();
    assert_eq!(records.len(), RECORDS, "overlay record count");
    records
}

/// `profile` (profile 01 with current-profile-02 applied, and optionally 03)
/// with each overlaid binding's rejection replaced by the overlay's
/// expectation. Panics unless every overlaid record names exactly the input
/// and the invalid-claim rejection it supersedes.
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
        assert!(
            binding.get("expectedUtf8").is_none(),
            "{id}: superseded rejection"
        );
        assert_eq!(
            binding["expectedCode"], "invalid-claim",
            "{id}: superseded code"
        );
        assert_eq!(
            binding["expectedCode"], row["supersededExpectedCode"],
            "{id}: superseded code"
        );
        assert_eq!(
            binding["expectedMessage"], row["supersededExpectedMessage"],
            "{id}: superseded message"
        );
        let binding = binding.as_object_mut().expect("binding object");
        match row["outcome"].as_str().expect("outcome") {
            "refused" | "canonical" => {
                binding.remove("expectedCode");
                binding.remove("expectedMessage");
                binding.insert("expectedUtf8".into(), row["expectedUtf8"].clone());
            }
            "subject-first" => {
                assert_eq!(row["expectedCode"], "invalid-claim", "{id}");
                binding.insert("expectedMessage".into(), row["expectedMessage"].clone());
            }
            other => panic!("{id}: unknown outcome {other}"),
        }
    }
    profile
}

/// The overlay row for a record ID, if the overlay supersedes it.
pub fn row(id: &str) -> Option<Value> {
    records().into_iter().find(|row| row["id"] == id)
}

/// Whether the overlay pins this record under negative polarity too.
pub fn negates(id: &str) -> bool {
    row(id).is_some_and(|row| row["polarities"] == serde_json::json!(["positive", "negative"]))
}

/// The record's binding with its expectation projected to negative polarity:
/// a refusal holds unchanged and a canonical plan carries the flipped claim.
pub fn negative_binding(binding: &Value) -> Value {
    let mut binding = binding.clone();
    if let Some(expected) = binding["expectedUtf8"].as_str() {
        if expected.contains(r#""polarity":"positive""#) {
            binding["expectedUtf8"] = Value::from(negative(expected));
        }
    }
    binding
}

/// The same request with its one claim's polarity flipped to negative.
pub fn negative(input: &str) -> String {
    let positive = r#""polarity":"positive""#;
    assert_eq!(input.matches(positive).count(), 1, "one positive claim");
    input.replace(positive, r#""polarity":"negative""#)
}
