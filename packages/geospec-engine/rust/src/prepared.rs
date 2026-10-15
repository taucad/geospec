//! Source-backed authored call lowering and common typed expectations.

pub(crate) mod family;
pub(crate) mod plan;
pub(crate) mod regexp;

use crate::{
    codec::{encode, Json},
    protocol::{array, invalid_claim, object, optional_field, require_fields, string_field},
    registry::{Capability, ExpectedShape},
    ProtocolError,
};

pub(crate) const DEFAULT_LINEAR_TOLERANCE: f64 = 0.02;
pub(crate) const DEFAULT_ANGULAR_TOLERANCE_DEGREES: f64 = 0.5;

/// Ancillary operations return observations rather than negatable predicates.
pub(crate) fn validate_query_polarity(
    capability: Capability,
    polarity: crate::result::Polarity,
) -> Result<(), ProtocolError> {
    if capability.is_query() && polarity == crate::result::Polarity::Negative {
        return invalid_claim(format!(
            "Ancillary capability '{}' requires positive polarity.",
            capability.name()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AnalysisDemand {
    pub connected_components_tolerance_bits: Option<u64>,
    pub selectors: bool,
    pub csg: bool,
}

/// Check one supplied payload and lower authored arguments without geometry.
pub(crate) fn expected(capability: Capability, payload: &Json) -> Result<Json, ProtocolError> {
    let payload = object(payload, "matcher payload")?;
    require_fields(
        payload,
        &["kind", "expected", "arguments"],
        &["kind"],
        "matcher payload",
    )?;
    if Some(string_field(payload, "kind")?) != capability.kind() {
        return invalid_claim(format!(
            "{} requires kind {}.",
            capability.name(),
            capability.kind().unwrap_or("ancillary")
        ));
    }
    let authored = optional_field(payload, "expected");
    let arguments = optional_field(payload, "arguments")
        .map(|value| array(value, "matcher arguments"))
        .transpose()?;
    let from_arguments = if let Some(arguments) =
        arguments.filter(|arguments| !arguments.is_empty())
    {
        Some(match capability.expected_shape() {
            ExpectedShape::Bounds => match arguments {
                [first] => first.clone(),
                [min, max] => Json::object([("min", min.clone()), ("max", max.clone())]),
                _ => {
                    return invalid_claim("Bounding-box arguments must contain at most two values.")
                }
            },
            ExpectedShape::True => {
                return invalid_claim(format!("{} takes no arguments.", capability.name()))
            }
            ExpectedShape::First | ExpectedShape::FirstOrEmpty => match arguments {
                [first] => first.clone(),
                _ => {
                    return invalid_claim(format!(
                        "{} requires at most one argument.",
                        capability.name()
                    ))
                }
            },
        })
    } else {
        None
    };
    if let (Some(authored), Some(lowered)) = (authored, from_arguments.as_ref()) {
        if encode(authored)? != encode(lowered)? {
            return invalid_claim(format!(
                "{} arguments must lower to the supplied expected value.",
                capability.name()
            ));
        }
    }
    let value = from_arguments.or_else(|| authored.cloned());
    match (capability.expected_shape(), value) {
        (ExpectedShape::True, None | Some(Json::Bool(true))) => Ok(Json::Bool(true)),
        (ExpectedShape::True, _) => {
            invalid_claim(format!("{} expected must be true.", capability.name()))
        }
        (ExpectedShape::FirstOrEmpty, None | Some(Json::Null)) => Ok(Json::Object(Vec::new())),
        (_, Some(value)) => Ok(value),
        (_, None) => invalid_claim(format!("{} requires an expected value.", capability.name())),
    }
}

pub(crate) fn normalized_payload(capability: Capability, expected: Json) -> Json {
    Json::object([
        (
            "kind",
            Json::string(capability.kind().unwrap_or("ancillary")),
        ),
        ("expected", expected),
    ])
}

pub(crate) fn finite(value: &Json, label: &str) -> Result<f64, ProtocolError> {
    match value {
        Json::Number(number) if number.is_finite() => Ok(*number),
        _ => invalid_claim(format!("{label} must be a finite number.")),
    }
}

pub(crate) fn nonnegative(value: &Json, label: &str) -> Result<f64, ProtocolError> {
    let number = finite(value, label)?;
    if number < 0.0 {
        invalid_claim(format!("{label} must be nonnegative."))
    } else {
        Ok(number)
    }
}

pub(crate) fn tolerance(
    fields: &[(String, Json)],
    key: &str,
    default: f64,
) -> Result<f64, ProtocolError> {
    optional_field(fields, key).map_or(Ok(default), |value| nonnegative(value, key))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NumericExpectation {
    Equal(f64),
    Conditions {
        value: Option<f64>,
        greater_than: Option<f64>,
        greater_than_or_equal: Option<f64>,
        less_than: Option<f64>,
        less_than_or_equal: Option<f64>,
    },
}

impl NumericExpectation {
    pub(crate) fn parse(value: &Json) -> Result<Self, ProtocolError> {
        if matches!(value, Json::Number(_)) {
            return Ok(Self::Equal(finite(value, "numeric expectation")?));
        }
        let fields = object(value, "numeric expectation")?;
        let names = [
            "value",
            "greaterThan",
            "greaterThanOrEqual",
            "lessThan",
            "lessThanOrEqual",
        ];
        require_fields(fields, &names, &[], "numeric expectation")?;
        if fields.is_empty() {
            return invalid_claim(EmptyExpectation::Comparator.message(None));
        }
        let mut values = [None; 5];
        for (index, key) in names.iter().enumerate() {
            values[index] = optional_field(fields, key)
                .map(|value| finite(value, key))
                .transpose()?;
        }
        Ok(Self::Conditions {
            value: values[0],
            greater_than: values[1],
            greater_than_or_equal: values[2],
            less_than: values[3],
            less_than_or_equal: values[4],
        })
    }

    pub(crate) fn holds(&self, measured: f64, tolerance: f64) -> bool {
        if !measured.is_finite() {
            return false;
        }
        match self {
            Self::Equal(value) => (measured - value).abs() <= tolerance,
            Self::Conditions {
                value,
                greater_than,
                greater_than_or_equal,
                less_than,
                less_than_or_equal,
            } => {
                value.is_none_or(|value| (measured - value).abs() <= tolerance)
                    && greater_than.is_none_or(|value| measured > value)
                    && greater_than_or_equal.is_none_or(|value| measured >= value)
                    && less_than.is_none_or(|value| measured < value)
                    && less_than_or_equal.is_none_or(|value| measured <= value)
            }
        }
    }

    pub(crate) fn to_json(&self) -> Json {
        match self {
            Self::Equal(value) => Json::Number(*value),
            Self::Conditions {
                value,
                greater_than,
                greater_than_or_equal,
                less_than,
                less_than_or_equal,
            } => Json::Object(
                [
                    ("value", value),
                    ("greaterThan", greater_than),
                    ("greaterThanOrEqual", greater_than_or_equal),
                    ("lessThan", less_than),
                    ("lessThanOrEqual", less_than_or_equal),
                ]
                .into_iter()
                .filter_map(|(key, value)| value.map(|number| (key.into(), Json::Number(number))))
                .collect(),
            ),
        }
    }

    pub(crate) fn describe(&self) -> String {
        let number = |value: f64| ryu_js::Buffer::new().format_finite(value).to_owned();
        match self {
            Self::Equal(value) => number(*value),
            Self::Conditions {
                value,
                greater_than,
                greater_than_or_equal,
                less_than,
                less_than_or_equal,
            } => {
                let parts: Vec<_> = [
                    ("", value),
                    ("> ", greater_than),
                    (">= ", greater_than_or_equal),
                    ("< ", less_than),
                    ("<= ", less_than_or_equal),
                ]
                .into_iter()
                .filter_map(|(prefix, value)| {
                    value.map(|value| format!("{prefix}{}", number(value)))
                })
                .collect();
                // `parse` refuses an empty comparator object, so at least one part exists.
                parts.join(" and ")
            }
        }
    }
}

/// A comparator, point, axis or bounds object that declares no constraint at
/// all. Such an object would be satisfied by any measurement, so it is refused
/// as an invalid claim rather than prepared as a vacuous pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmptyExpectation {
    Comparator,
    Point,
    /// A bounding-box `min`, `max`, `size` or `center` axis object with no axis.
    Axes,
    /// A bounding-box expectation with no `min`, `max`, `size` or `center`; a
    /// tolerance alone constrains nothing. Always located at the root.
    Bounds,
}

impl EmptyExpectation {
    /// Kinds whose field path `locate_empty_expectation` names; `Bounds` is
    /// already located at the root.
    const LOCATABLE: [Self; 3] = [Self::Comparator, Self::Point, Self::Axes];

    pub(crate) fn message(self, location: Option<&str>) -> String {
        let location = location.map_or_else(String::new, |location| format!(" at {location}"));
        match self {
            Self::Comparator => format!(
                "GeoSpec numeric expectation{location} requires at least one comparator (value, greaterThan, greaterThanOrEqual, lessThan or lessThanOrEqual)."
            ),
            Self::Point => format!(
                "GeoSpec point{location} requires at least one coordinate (x, y or z)."
            ),
            Self::Axes => format!(
                "GeoSpec bounding-box axes{location} requires at least one axis (x, y or z)."
            ),
            Self::Bounds => "GeoSpec bounding-box expected at the root requires at least one bound (min, max, size or center).".into(),
        }
    }
}

/// Names the field path of an empty comparator or point object. The shared
/// parsers above see only the value, so the family dispatcher calls this with
/// the claim payload to locate the empty object in the lowered expectation.
pub(crate) fn locate_empty_expectation(
    error: ProtocolError,
    capability: Capability,
    payload: &Json,
) -> ProtocolError {
    let Some(kind) = EmptyExpectation::LOCATABLE
        .into_iter()
        .find(|kind| error.message == kind.message(None))
    else {
        return error;
    };
    let Ok(lowered) = expected(capability, payload) else {
        return error;
    };
    let mut paths = Vec::new();
    empty_object_paths(&lowered, "", &mut paths);
    let location = match paths.as_slice() {
        [] => return error,
        [path] => format!("'{path}'"),
        paths => format!(
            "one of {}",
            paths
                .iter()
                .map(|path| format!("'{path}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    ProtocolError::new(error.kind, kind.message(Some(&location)))
}

/// Collects the paths of every empty object below the root, in document order.
fn empty_object_paths(value: &Json, path: &str, paths: &mut Vec<String>) {
    match value {
        Json::Object(fields) if fields.is_empty() && !path.is_empty() => paths.push(path.into()),
        Json::Object(fields) => {
            for (key, value) in fields {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                empty_object_paths(value, &child, paths);
            }
        }
        Json::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                empty_object_paths(value, &format!("{path}[{index}]"), paths);
            }
        }
        _ => {}
    }
}

/// Missing axes are unconstrained, but at least one axis is required; arrays
/// must be exact triples.
pub(crate) fn point(value: &Json) -> Result<[Option<f64>; 3], ProtocolError> {
    let mut point = [None; 3];
    match value {
        Json::Array(values) if values.len() == 3 => {
            for (axis, value) in values.iter().enumerate() {
                point[axis] = Some(finite(value, "point axis")?);
            }
        }
        Json::Object(fields) => {
            require_fields(fields, &["x", "y", "z"], &[], "point")?;
            if fields.is_empty() {
                return invalid_claim(EmptyExpectation::Point.message(None));
            }
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                point[axis] = optional_field(fields, name)
                    .map(|value| finite(value, "point axis"))
                    .transpose()?;
            }
        }
        _ => return invalid_claim("Point requires a length-three array or an axis object."),
    }
    Ok(point)
}

/// Canonical typed expectation point; bbox retains its separate axis-object contract.
pub(crate) fn normalized_point(value: [Option<f64>; 3]) -> Json {
    if let [Some(x), Some(y), Some(z)] = value {
        Json::Array(vec![Json::Number(x), Json::Number(y), Json::Number(z)])
    } else {
        Json::Object(
            ["x", "y", "z"]
                .into_iter()
                .zip(value)
                .filter_map(|(key, value)| value.map(|value| (key.into(), Json::Number(value))))
                .collect(),
        )
    }
}

#[cfg(test)]
mod point_normalization_tests {
    use serde_json::{json, Value};

    fn canonical_plan(
        capability: &str,
        kind: &str,
        expected: Value,
        polarity: &str,
    ) -> Result<Vec<u8>, crate::ProtocolError> {
        crate::Engine::new().canonical_plan(
            &serde_json::to_vec(&json!({
                "method":"submitClaims", "requestId":"point-control", "protocolVersion":3,
                "registryVersion":5, "canonicalProfile":"geospec-jcs-v1",
                "plan":{"subjects":[{"slot":"subject", "subjectHash":"0".repeat(64)}],
                    "claims":[{"claimId":"point-control", "capability":capability,
                        "subjectSlots":["subject"], "polarity":polarity, "workUnitBudget":100,
                        "payload":{"kind":kind,"expected":expected}}]}
            }))
            .unwrap(),
        )
    }

    fn assert_refused(capability: &str, kind: &str, expected: Value, message: &str) {
        for polarity in ["positive", "negative"] {
            let error = canonical_plan(capability, kind, expected.clone(), polarity)
                .expect_err(&format!("{capability} {expected} must be refused"));
            assert_eq!(error.code(), "invalid-claim", "{capability} {expected}");
            assert_eq!(error.to_string(), message, "{capability} {expected}");
        }
    }

    fn comparator_message(path: &str) -> String {
        comparator_message_at(&format!("'{path}'"))
    }

    fn comparator_message_at(location: &str) -> String {
        format!(
            "GeoSpec numeric expectation at {location} requires at least one comparator (value, greaterThan, greaterThanOrEqual, lessThan or lessThanOrEqual)."
        )
    }

    // W5-PF-01: an empty per-axis bounding-box range was prepared as a range
    // and `holds` was vacuously true, so the claim passed.
    #[test]
    fn bounding_box_rejects_empty_axis_range() {
        for field in ["min", "max", "size", "center"] {
            assert_refused(
                "toHaveBoundingBox",
                "boundingBox",
                json!({field: {"x": {}}}),
                &comparator_message(&format!("{field}.x")),
            );
        }
        assert_refused(
            "toHaveBoundingBox",
            "boundingBox",
            json!({"size": {"x": 1, "y": {"lessThan": 9}, "z": {}}}),
            &comparator_message("size.z"),
        );
        assert_refused(
            "toHaveBoundingBox",
            "boundingBox",
            json!({"min": {"x": {}}, "max": [1, 2, 3], "center": {"y": {}}}),
            &comparator_message_at("one of 'center.y', 'min.x'"),
        );
    }

    fn bounding_box_canonical(expected: Value) -> Result<Vec<u8>, crate::ProtocolError> {
        canonical_plan("toHaveBoundingBox", "boundingBox", expected, "positive")
    }

    // ENGINE-FIX-2b / current-profile-02: an empty bounding-box axis object, an
    // empty expectation and a tolerance-only expectation bound nothing, so each
    // passed for any subject. They refuse at plan time, naming the field path.
    #[test]
    fn bounding_box_rejects_empty_axis_object_and_empty_expectation() {
        let axes = |location: &str| {
            format!(
                "GeoSpec bounding-box axes at {location} requires at least one axis (x, y or z)."
            )
        };
        let root = "GeoSpec bounding-box expected at the root requires at least one bound (min, max, size or center).";
        for field in ["min", "max", "size", "center"] {
            assert_refused(
                "toHaveBoundingBox",
                "boundingBox",
                json!({field: {}}),
                &axes(&format!("'{field}'")),
            );
            assert_refused(
                "toHaveBoundingBox",
                "boundingBox",
                json!({field: {}, "tolerance": 0.5}),
                &axes(&format!("'{field}'")),
            );
        }
        assert_refused(
            "toHaveBoundingBox",
            "boundingBox",
            json!({"center": {}, "max": {}, "min": {}, "size": {}}),
            &axes("one of 'center', 'max', 'min', 'size'"),
        );
        assert_refused(
            "toHaveBoundingBox",
            "boundingBox",
            json!({"min": [0, 0, 0], "size": {}}),
            &axes("'size'"),
        );
        for expected in [
            json!({}),
            json!({"tolerance": 0.02}),
            json!({"tolerance": 0}),
        ] {
            assert_refused("toHaveBoundingBox", "boundingBox", expected, root);
        }
        // Authored arguments lower to the same empty expectation.
        let lowered = |arguments: Value| {
            crate::Engine::new().canonical_plan(
                &serde_json::to_vec(&json!({
                    "method":"submitClaims", "requestId":"lowered", "protocolVersion":3,
                    "registryVersion":5, "canonicalProfile":"geospec-jcs-v1",
                    "plan":{"subjects":[{"slot":"subject", "subjectHash":"0".repeat(64)}],
                        "claims":[{"claimId":"lowered", "capability":"toHaveBoundingBox",
                            "subjectSlots":["subject"], "polarity":"negative", "workUnitBudget":100,
                            "payload":{"kind":"boundingBox","arguments":arguments}}]}
                }))
                .unwrap(),
            )
        };
        let error = lowered(json!([{"size": {}}])).unwrap_err();
        assert_eq!(error.code(), "invalid-claim");
        assert_eq!(error.to_string(), axes("'size'"));
        // Earlier refusals keep their messages; the emptiness checks run last.
        for (expected, message) in [
            (
                json!({"tolerance": -0.01}),
                "tolerance must be nonnegative.",
            ),
            (
                json!({"tolerance": null}),
                "tolerance must be a finite number.",
            ),
            (
                json!({"min": {}, "max": null}),
                "Bounding-box fields require axis objects or length-three arrays.",
            ),
            (
                json!({"size": {}, "tolerance": -1}),
                "tolerance must be nonnegative.",
            ),
        ] {
            let error = bounding_box_canonical(expected.clone()).unwrap_err();
            assert_eq!(error.code(), "invalid-claim", "{expected}");
            assert_eq!(error.to_string(), message, "{expected}");
        }
        // One declared axis anywhere is a constraint and is still prepared.
        for expected in [
            json!({"size": {"x": 1}}),
            json!({"min": {"z": {"greaterThan": 0}}, "tolerance": 0}),
            json!({"center": [0, 0, 0]}),
        ] {
            bounding_box_canonical(expected.clone())
                .unwrap_or_else(|error| panic!("{expected}: {error}"));
        }
    }

    // W5-PF-02: an empty numeric comparator object passed as "any value".
    #[test]
    fn numeric_expectation_rejects_empty_comparator_object() {
        let cases = [
            ("toHaveVolume", "volume", json!({"value": {}}), "value"),
            (
                "toHaveSurfaceArea",
                "surfaceArea",
                json!({"value": {}}),
                "value",
            ),
            (
                "toHaveMass",
                "mass",
                json!({"value": {}, "density": 1}),
                "value",
            ),
            (
                "toHaveMeshIntegrity",
                "meshIntegrity",
                json!({"triangleCount": {}}),
                "triangleCount",
            ),
            (
                "toHaveTopologyCounts",
                "topologyCounts",
                json!({"faces": {}}),
                "faces",
            ),
            (
                "toBeValidBrep",
                "validBrep",
                json!({"freeBounds": {"count": {}}}),
                "freeBounds.count",
            ),
            (
                "toHaveProductStructure",
                "productStructure",
                json!({"count": {}}),
                "count",
            ),
            (
                "toHaveAssemblyOccurrences",
                "assemblyOccurrences",
                json!({"occurrences": [{"name": "left", "count": {}}]}),
                "occurrences[0].count",
            ),
            (
                "toHavePlanarFace",
                "planarFace",
                json!({"normal": [0, 0, 1], "offset": 0, "area": {}}),
                "area",
            ),
            (
                "toHaveMinimumWallThickness",
                "minimumWallThickness",
                json!({"value": {}}),
                "value",
            ),
        ];
        for (capability, kind, expected, path) in cases {
            assert_refused(capability, kind, expected, &comparator_message(path));
        }
    }

    // POINT-NORMALIZATION-01: authored forms are predeclared here; no geometry
    // or candidate output supplies the expected canonical point representation.
    #[test]
    fn point_normalization_seven_fields_preserve_shapes_and_refuse_empty() {
        let cases = [
            (
                "toHaveCenterOfMass",
                "centerOfMass",
                json!({"point": null}),
                "/point",
            ),
            (
                "toHavePlanarFace",
                "planarFace",
                json!({"normal": null, "offset": 9}),
                "/normal",
            ),
            (
                "toHaveCircularHole",
                "circularHole",
                json!({"diameter": 2, "center": null}),
                "/center",
            ),
            (
                "toHaveCircularHolePattern",
                "circularHolePattern",
                json!({"count": 4, "holeDiameter": 2, "center": null}),
                "/center",
            ),
            (
                "toHaveAssemblyOccurrences",
                "assemblyOccurrences",
                json!({"occurrences":[{"name":"left", "bounds":{"min":null}}]}),
                "/occurrences/0/bounds/min",
            ),
            (
                "toHaveAssemblyOccurrences",
                "assemblyOccurrences",
                json!({"occurrences":[{"name":"left", "bounds":{"max":null}}]}),
                "/occurrences/0/bounds/max",
            ),
            (
                "toHaveAssemblyOccurrences",
                "assemblyOccurrences",
                json!({"occurrences":[{"name":"left", "bounds":{"center":null}}]}),
                "/occurrences/0/bounds/center",
            ),
        ];
        let canonical = |capability: &str, kind: &str, expected: Value, polarity: &str| {
            canonical_plan(capability, kind, expected, polarity).unwrap()
        };
        let canonical_error = |capability: &str, kind: &str, expected: Value, polarity: &str| {
            canonical_plan(capability, kind, expected, polarity).unwrap_err()
        };
        for (capability, kind, template, path) in cases {
            for polarity in ["positive", "negative"] {
                let mut full_vector = template.clone();
                *full_vector.pointer_mut(path).unwrap() = json!([1, 2, 3]);
                let mut full_axis = template.clone();
                *full_axis.pointer_mut(path).unwrap() = json!({"x":1,"y":2,"z":3});
                let bytes = canonical(capability, kind, full_vector, polarity);
                assert_eq!(
                    bytes,
                    canonical(capability, kind, full_axis, polarity),
                    "{capability} {path}"
                );
                let full: Value = serde_json::from_slice(&bytes).unwrap();
                let output_path = format!("/plan/claims/0/payload/expected{path}");
                assert_eq!(full.pointer(&output_path), Some(&json!([1, 2, 3])));
                let mut partial = template.clone();
                *partial.pointer_mut(path).unwrap() = json!({"x":1,"z":3});
                let normalized: Value =
                    serde_json::from_slice(&canonical(capability, kind, partial, polarity))
                        .unwrap();
                assert_eq!(
                    normalized.pointer(&output_path),
                    Some(&json!({"x":1,"z":3}))
                );
                // Ruling R-2 (W0.8) overrides the earlier acceptance of `{}`:
                // an empty point constrains nothing, so it is refused at plan
                // time with its field path, never prepared as unconstrained.
                let mut empty = template.clone();
                *empty.pointer_mut(path).unwrap() = json!({});
                let error = canonical_error(capability, kind, empty, polarity);
                let field_path = path
                    .trim_start_matches('/')
                    .replace("/0/", "[0].")
                    .replace('/', ".");
                assert_eq!(error.code(), "invalid-claim", "{capability} {path}");
                assert_eq!(
                    error.to_string(),
                    format!(
                        "GeoSpec point at '{field_path}' requires at least one coordinate (x, y or z)."
                    ),
                    "{capability} {path}"
                );
            }
        }
        for field in ["min", "max", "size", "center"] {
            let bytes = canonical(
                "toHaveBoundingBox",
                "boundingBox",
                json!({field:[1,2,3]}),
                "positive",
            );
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                value["plan"]["claims"][0]["payload"]["expected"][field],
                json!({"x":1,"y":2,"z":3})
            );
        }
    }
}
