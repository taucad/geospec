//! Host-selected engine limits over the entry profile.
//!
//! A host may replace any admission, retention or retained-subject limit of the
//! entry profile when it constructs an engine. Omitted limits keep their entry
//! value. The effective limits and a
//! profile id are echoed by `initialize`, so a run records the limits it used. Any
//! configuration that differs from the entry profile reports a derived `host`
//! profile id and never the entry id.

use sha2::{Digest, Sha256};

use super::EngineConfig;
use crate::{
    backend::{resources::BinaryAdmissionLimits, AnalysisRetentionLimits},
    codec::{decode, Json},
    protocol::{object, require_fields},
    ErrorKind, ProtocolError,
};

/// Configuration profile id of the unmodified entry limits.
const ENTRY_CONFIGURATION_PROFILE: &str = "geospec-entry-config-v1";
/// Binary admission profile id of the unmodified entry limits.
const ENTRY_BINARY_PROFILE: &str = "geospec-entry-binary-v1";

/// Optional host replacements for entry limits; each absent value keeps the current value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct EngineLimits {
    max_subject_bytes: Option<f64>,
    max_resource_bytes: Option<f64>,
    max_total_binary_bytes: Option<f64>,
    max_vertices: Option<f64>,
    max_triangles: Option<f64>,
    max_occurrences: Option<f64>,
    max_mesh_bytes: Option<f64>,
    max_mesh_entries: Option<f64>,
    max_solid_entries: Option<f64>,
    max_retained_subjects: Option<f64>,
}

fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorKind::InvalidRequest, message)
}

impl EngineLimits {
    fn from_pairs<'a>(
        pairs: impl IntoIterator<Item = (&'a str, f64)>,
    ) -> Result<Self, ProtocolError> {
        let mut limits = Self::default();
        for (name, value) in pairs {
            let slot = limits
                .slot(name)
                .ok_or_else(|| invalid(format!("Unknown engine limit '{name}'.")))?;
            if slot.replace(value).is_some() {
                return Err(invalid(format!("Engine limit {name} is supplied twice.")));
            }
        }
        Ok(limits)
    }

    fn slot(&mut self, name: &str) -> Option<&mut Option<f64>> {
        Some(match name {
            "maxSubjectBytes" => &mut self.max_subject_bytes,
            "maxResourceBytes" => &mut self.max_resource_bytes,
            "maxTotalBinaryBytes" => &mut self.max_total_binary_bytes,
            "maxVertices" => &mut self.max_vertices,
            "maxTriangles" => &mut self.max_triangles,
            "maxOccurrences" => &mut self.max_occurrences,
            "maxMeshBytes" => &mut self.max_mesh_bytes,
            "maxMeshEntries" => &mut self.max_mesh_entries,
            "maxSolidEntries" => &mut self.max_solid_entries,
            "maxRetainedSubjects" => &mut self.max_retained_subjects,
            _ => return None,
        })
    }
}

/// Largest byte limit this host can address and a JSON number can carry exactly.
fn byte_ceiling() -> u64 {
    (usize::MAX as u64).min(9_007_199_254_740_991)
}

fn positive_integer(name: &str, value: f64, ceiling: u64) -> Result<u64, ProtocolError> {
    if !value.is_finite() || value.fract() != 0.0 || value < 1.0 || value > ceiling as f64 {
        return Err(invalid(format!(
            "Engine limit {name} must be a positive integer no greater than {ceiling}."
        )));
    }
    Ok(value as u64)
}

fn bytes(name: &str, value: Option<f64>, current: u64) -> Result<u64, ProtocolError> {
    value.map_or(Ok(current), |value| {
        positive_integer(name, value, byte_ceiling())
    })
}

fn count(name: &str, value: Option<f64>, current: u32) -> Result<u32, ProtocolError> {
    value.map_or(Ok(current), |value| {
        positive_integer(name, value, u64::from(u32::MAX)).map(|value| value as u32)
    })
}

fn digest(prefix: &str, values: &[(&str, u64)]) -> String {
    let mut hash = Sha256::new();
    hash.update(prefix.as_bytes());
    for (name, value) in values {
        hash.update(b"\n");
        hash.update(name.as_bytes());
        hash.update(b"=");
        hash.update(value.to_string().as_bytes());
    }
    let hex = crate::identity::hex(&hash.finalize());
    format!("{prefix}-{}", &hex[..16])
}

fn binary_values(binary: &BinaryAdmissionLimits) -> [(&'static str, u64); 6] {
    [
        ("maxSubjectBytes", binary.max_subject_bytes),
        ("maxResourceBytes", binary.max_resource_bytes),
        ("maxTotalBinaryBytes", binary.max_total_binary_bytes),
        ("maxVertices", u64::from(binary.max_vertices)),
        ("maxTriangles", u64::from(binary.max_triangles)),
        ("maxOccurrences", u64::from(binary.max_occurrences)),
    ]
}

fn analysis_values(analysis: &AnalysisRetentionLimits) -> [(&'static str, u64); 3] {
    [
        ("maxMeshBytes", analysis.max_mesh_bytes),
        ("maxMeshEntries", u64::from(analysis.max_mesh_entries)),
        ("maxSolidEntries", u64::from(analysis.max_solid_entries)),
    ]
}

impl EngineConfig {
    /// Wire names of the host-selectable limits, in their canonical order.
    pub const LIMIT_NAMES: [&'static str; 10] = [
        "maxSubjectBytes",
        "maxResourceBytes",
        "maxTotalBinaryBytes",
        "maxVertices",
        "maxTriangles",
        "maxOccurrences",
        "maxMeshBytes",
        "maxMeshEntries",
        "maxSolidEntries",
        "maxRetainedSubjects",
    ];

    /// Replace the named limits (camelCase wire names), validating each value and
    /// their relations, and recompute the binary profile id. Omitted limits and
    /// execution permits are unchanged. Unknown or repeated names are refused.
    pub fn with_limits<'a>(
        self,
        limits: impl IntoIterator<Item = (&'a str, f64)>,
    ) -> Result<Self, ProtocolError> {
        self.apply(&EngineLimits::from_pairs(limits)?)
    }

    /// Replace limits given as one strict JSON object with camelCase names.
    pub fn with_limits_json(self, input: &[u8]) -> Result<Self, ProtocolError> {
        let value = decode(input)?;
        let fields = object(&value, "engine limits")?;
        require_fields(fields, &Self::LIMIT_NAMES, &[], "engine limits")?;
        let mut pairs = Vec::with_capacity(fields.len());
        for (name, value) in fields {
            let Json::Number(number) = value else {
                return Err(invalid(format!(
                    "Engine limit {name} must be a positive integer."
                )));
            };
            pairs.push((name.as_str(), *number));
        }
        self.with_limits(pairs)
    }

    fn apply(mut self, limits: &EngineLimits) -> Result<Self, ProtocolError> {
        let binary = BinaryAdmissionLimits {
            profile_id: String::new(),
            max_subject_bytes: bytes(
                "maxSubjectBytes",
                limits.max_subject_bytes,
                self.binary.max_subject_bytes,
            )?,
            max_resource_bytes: bytes(
                "maxResourceBytes",
                limits.max_resource_bytes,
                self.binary.max_resource_bytes,
            )?,
            max_total_binary_bytes: bytes(
                "maxTotalBinaryBytes",
                limits.max_total_binary_bytes,
                self.binary.max_total_binary_bytes,
            )?,
            max_vertices: count("maxVertices", limits.max_vertices, self.binary.max_vertices)?,
            max_triangles: count(
                "maxTriangles",
                limits.max_triangles,
                self.binary.max_triangles,
            )?,
            max_occurrences: count(
                "maxOccurrences",
                limits.max_occurrences,
                self.binary.max_occurrences,
            )?,
        };
        if binary.max_subject_bytes > binary.max_total_binary_bytes
            || binary.max_resource_bytes > binary.max_total_binary_bytes
        {
            return Err(invalid(
                "Engine limits maxSubjectBytes and maxResourceBytes must not exceed maxTotalBinaryBytes.",
            ));
        }
        self.analysis.max_mesh_bytes = bytes(
            "maxMeshBytes",
            limits.max_mesh_bytes,
            self.analysis.max_mesh_bytes,
        )?;
        self.analysis.max_mesh_entries = count(
            "maxMeshEntries",
            limits.max_mesh_entries,
            self.analysis.max_mesh_entries,
        )?;
        self.analysis.max_solid_entries = count(
            "maxSolidEntries",
            limits.max_solid_entries,
            self.analysis.max_solid_entries,
        )?;
        self.max_retained_subjects = count(
            "maxRetainedSubjects",
            limits.max_retained_subjects,
            self.max_retained_subjects,
        )?;
        self.binary = binary;
        self.binary.profile_id = self.binary_profile_id();
        Ok(self)
    }

    /// Whether every limit equals the entry profile; execution permits are not limits.
    pub fn has_entry_limits(&self) -> bool {
        let entry = Self::entry();
        binary_values(&self.binary) == binary_values(&entry.binary)
            && analysis_values(&self.analysis) == analysis_values(&entry.analysis)
            && self.max_retained_subjects == entry.max_retained_subjects
    }

    /// The entry binary profile id, or a derived host id for any other binary limits.
    pub fn binary_profile_id(&self) -> String {
        if binary_values(&self.binary) == binary_values(&Self::entry().binary) {
            ENTRY_BINARY_PROFILE.into()
        } else {
            digest("geospec-host-binary-v1", &binary_values(&self.binary))
        }
    }

    /// The entry configuration profile id, or a derived host id for any other limits.
    pub fn configuration_profile_id(&self) -> String {
        if self.has_entry_limits() {
            return ENTRY_CONFIGURATION_PROFILE.into();
        }
        let mut values = binary_values(&self.binary).to_vec();
        values.extend(analysis_values(&self.analysis));
        values.push(("maxRetainedSubjects", u64::from(self.max_retained_subjects)));
        digest("geospec-host-config-v1", &values)
    }
}

#[cfg(test)]
mod engine_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const RAISED: [(&str, f64); 2] = [
        ("maxSubjectBytes", 256.0 * 1024.0 * 1024.0),
        ("maxTotalBinaryBytes", 512.0 * 1024.0 * 1024.0),
    ];

    #[test]
    fn omitted_limits_keep_the_entry_profile() {
        let config = EngineConfig::entry().with_limits([]).unwrap();
        assert!(config.has_entry_limits());
        assert_eq!(config.binary, EngineConfig::entry().binary);
        assert_eq!(
            config.configuration_profile_id(),
            ENTRY_CONFIGURATION_PROFILE
        );
        assert_eq!(config.binary_profile_id(), ENTRY_BINARY_PROFILE);
    }

    #[test]
    fn explicit_entry_values_are_the_entry_profile() {
        let config = EngineConfig::entry()
            .with_limits([
                ("maxSubjectBytes", 64.0 * 1024.0 * 1024.0),
                ("maxMeshBytes", 256.0 * 1024.0 * 1024.0),
                ("maxRetainedSubjects", 32.0),
            ])
            .unwrap();
        assert_eq!(
            config.configuration_profile_id(),
            ENTRY_CONFIGURATION_PROFILE
        );
    }

    #[test]
    fn raised_limits_report_a_distinct_reproducible_profile() {
        let config = EngineConfig::entry().with_limits(RAISED).unwrap();
        assert_eq!(config.binary.max_subject_bytes, 256 * 1024 * 1024);
        assert_eq!(config.binary.max_total_binary_bytes, 512 * 1024 * 1024);
        assert_eq!(config.binary.max_resource_bytes, 64 * 1024 * 1024);
        assert!(!config.has_entry_limits());
        let profile = config.configuration_profile_id();
        assert!(profile.starts_with("geospec-host-config-v1-"), "{profile}");
        assert!(config
            .binary
            .profile_id
            .starts_with("geospec-host-binary-v1-"));
        let again = EngineConfig::entry().with_limits(RAISED).unwrap();
        assert_eq!(again.configuration_profile_id(), profile);
        let retention_only = EngineConfig::entry()
            .with_limits([("maxMeshBytes", 1024.0 * 1024.0 * 1024.0)])
            .unwrap();
        assert_eq!(retention_only.binary_profile_id(), ENTRY_BINARY_PROFILE);
        assert_ne!(retention_only.configuration_profile_id(), profile);
        assert_ne!(
            retention_only.configuration_profile_id(),
            ENTRY_CONFIGURATION_PROFILE
        );
    }

    /// Each limit's entry value.
    const ENTRY: [(&str, f64); 10] = [
        ("maxSubjectBytes", 64.0 * 1024.0 * 1024.0),
        ("maxResourceBytes", 64.0 * 1024.0 * 1024.0),
        ("maxTotalBinaryBytes", 128.0 * 1024.0 * 1024.0),
        ("maxVertices", 2_000_000.0),
        ("maxTriangles", 4_000_000.0),
        ("maxOccurrences", 65_536.0),
        ("maxMeshBytes", 256.0 * 1024.0 * 1024.0),
        ("maxMeshEntries", 256.0),
        ("maxSolidEntries", 256.0),
        ("maxRetainedSubjects", 32.0),
    ];

    #[test]
    fn every_entry_value_supplied_explicitly_is_the_entry_profile() {
        assert_eq!(ENTRY.map(|(name, _)| name), EngineConfig::LIMIT_NAMES);
        let config = EngineConfig::entry().with_limits(ENTRY).unwrap();
        assert!(config.has_entry_limits());
        assert_eq!(config.binary, EngineConfig::entry().binary);
        assert_eq!(config.analysis, EngineConfig::entry().analysis);
        assert_eq!(
            config.configuration_profile_id(),
            ENTRY_CONFIGURATION_PROFILE
        );
        assert_eq!(config.binary_profile_id(), ENTRY_BINARY_PROFILE);
    }

    #[test]
    fn every_limit_feeds_a_distinct_profile_id() {
        let mut profiles = std::collections::BTreeSet::new();
        profiles.insert(ENTRY_CONFIGURATION_PROFILE.to_owned());
        for (name, entry) in ENTRY {
            // Lower each limit alone (byte limits stay within the total).
            for value in [entry - 1.0, (entry / 2.0).floor()] {
                let config = EngineConfig::entry().with_limits([(name, value)]).unwrap();
                assert!(!config.has_entry_limits(), "{name}={value}");
                let profile = config.configuration_profile_id();
                assert!(profile.starts_with("geospec-host-config-v1-"), "{profile}");
                assert!(profiles.insert(profile), "{name}={value} repeats a profile");
                let binary = config.binary_profile_id();
                let admission = !matches!(
                    name,
                    "maxMeshBytes" | "maxMeshEntries" | "maxSolidEntries" | "maxRetainedSubjects"
                );
                assert_eq!(binary != ENTRY_BINARY_PROFILE, admission, "{name}");
                assert_eq!(config.binary.profile_id, binary);
            }
        }
        // The same limits give the same id whatever order they are supplied in.
        let forward = EngineConfig::entry()
            .with_limits([("maxOccurrences", 70_000.0), ("maxMeshEntries", 512.0)])
            .unwrap();
        let reverse = EngineConfig::entry()
            .with_limits([("maxMeshEntries", 512.0), ("maxOccurrences", 70_000.0)])
            .unwrap();
        assert_eq!(
            forward.configuration_profile_id(),
            reverse.configuration_profile_id()
        );
        assert_eq!(forward.binary.max_occurrences, 70_000);
        assert_eq!(forward.analysis.max_mesh_entries, 512);
    }

    #[test]
    fn existing_host_profile_ids_are_unchanged() {
        // Values a host could already select before occurrences and entries
        // became selectable keep their profile ids.
        let config = EngineConfig::entry().with_limits(RAISED).unwrap();
        assert_eq!(
            config.configuration_profile_id(),
            digest(
                "geospec-host-config-v1",
                &[
                    ("maxSubjectBytes", 256 * 1024 * 1024),
                    ("maxResourceBytes", 64 * 1024 * 1024),
                    ("maxTotalBinaryBytes", 512 * 1024 * 1024),
                    ("maxVertices", 2_000_000),
                    ("maxTriangles", 4_000_000),
                    ("maxOccurrences", 65_536),
                    ("maxMeshBytes", 256 * 1024 * 1024),
                    ("maxMeshEntries", 256),
                    ("maxSolidEntries", 256),
                    ("maxRetainedSubjects", 32),
                ]
            )
        );
    }

    #[test]
    fn refuses_nonsense_limits() {
        let invalid_values = [
            0.0,
            -1.0,
            1.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e300,
        ];
        for name in EngineConfig::LIMIT_NAMES {
            for value in invalid_values {
                let error = EngineConfig::entry()
                    .with_limits([(name, value)])
                    .unwrap_err();
                assert_eq!(error.code(), "invalid-request", "{name}={value}");
                assert!(error.to_string().contains(name), "{error}");
            }
        }
        for limits in [
            &[("maxVertices", f64::from(u32::MAX) + 1.0)][..],
            &[("maxOccurrences", f64::from(u32::MAX) + 1.0)],
            &[("maxMeshEntries", f64::from(u32::MAX) + 1.0)],
            &[("maxSolidEntries", f64::from(u32::MAX) + 1.0)],
            &[("maxRetainedSubjects", f64::from(u32::MAX) + 1.0)],
            &[("maxSubjectBytes", 256.0 * 1024.0 * 1024.0)],
            &[("maxSubjectByte", 1.0)],
            &[("maxVertices", 1.0), ("maxVertices", 2.0)],
        ] {
            let error = EngineConfig::entry()
                .with_limits(limits.iter().copied())
                .unwrap_err();
            assert_eq!(error.code(), "invalid-request", "{limits:?}");
        }
    }

    #[test]
    fn parses_strict_json_limits() {
        let config = EngineConfig::entry()
            .with_limits_json(br#"{"maxSubjectBytes":268435456,"maxTotalBinaryBytes":536870912}"#)
            .unwrap();
        assert_eq!(
            config.configuration_profile_id(),
            EngineConfig::entry()
                .with_limits(RAISED)
                .unwrap()
                .configuration_profile_id()
        );
        assert!(EngineConfig::entry()
            .with_limits_json(b"{}")
            .unwrap()
            .has_entry_limits());
        for input in [
            &br#"{"maxSubjectByte":1}"#[..],
            br#"{"maxVertices":"1"}"#,
            br#"{"maxVertices":null}"#,
            br#"{"maxVertices":0}"#,
            br#"{"maxOccurrences":-1}"#,
            br#"{"maxMeshEntries":2.5}"#,
            br#"{"maxSolidEntries":0}"#,
            br#"{"maxOccurrences":65536,"maxOccurrences":65536}"#,
            b"[]",
            b"{",
        ] {
            assert!(EngineConfig::entry().with_limits_json(input).is_err());
        }
    }
}
