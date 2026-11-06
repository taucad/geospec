//! Persisted completed facts (`geospec-facts-v1`): one write-once action per
//! (subject, scope, fact), in the exact-cluster action shape.
//!
//! A fact is a measurement of the admitted subject, never a verdict: claims
//! are decided from the fact and the claim's tolerance on every run. Only a
//! fact the backend computed successfully is stored; a refusal, a failure or
//! an unmeasured part never is. The numeric and facts profile strings, with
//! the subject and producer identities, are the only invalidation.

use serde::{Deserialize, Serialize};

use crate::{
    backend::brep::{
        Bounds, ClosureEdgeSample, ClosureFacts, ClosureGroup, ShapeFacts, ShapeParts, SmallEdge,
        TopologyCounts, ValidityCheck, ValidityFacts,
    },
    cache::{EvidenceAddress, OverlapEvidenceCache},
    identity::sha256_hex,
    protocol::{CANONICAL_PROFILE, NUMERIC_PROFILE, PROTOCOL_VERSION, REGISTRY_VERSION},
    subject::Subject,
};

pub(crate) const FAMILY: &str = "geospec-facts-v1";
pub(crate) const CODEC: &str = "geospec-facts-json-v1";
/// The integrator behind every stored fact. Every fact today comes from the
/// `legacy-fixed-order` paths; a new integrator names a new profile, which
/// invalidates every stored fact by address.
pub(crate) const FACTS_PROFILE: &str = "geospec-facts-v1/legacy-fixed-order";
const PROVENANCE: &str = "legacy-fixed-order";
const PAYLOAD_SCHEMA: &str = "geospec-completed-fact-v1";
/// Facts are small; a larger validity record (many failing checks) is not
/// stored and is recomputed.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

/// One persisted fact of the whole subject (`scope: "subject"`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Fact {
    Validity,
    /// Exact shell closure, with the work units its naming charges.
    Closure,
    Bounds,
    /// Volume and centre of mass: one volume integral.
    Mass,
    Area,
    Counts,
}

impl Fact {
    const fn name(self) -> &'static str {
        match self {
            Self::Validity => "validity",
            Self::Closure => "closure",
            Self::Bounds => "bounds",
            Self::Mass => "mass",
            Self::Area => "area",
            Self::Counts => "counts",
        }
    }
}

/// The shape facts that together answer `parts`, in a fixed order.
fn shape_facts(parts: ShapeParts) -> impl Iterator<Item = (ShapeParts, Fact)> {
    [
        (ShapeParts::VOLUME, Fact::Mass),
        (ShapeParts::AREA, Fact::Area),
        (ShapeParts::BOUNDS, Fact::Bounds),
        (ShapeParts::COUNTS, Fact::Counts),
    ]
    .into_iter()
    .filter(move |(part, _)| parts.contains(*part))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Action<'a> {
    schema: &'static str,
    family: &'static str,
    codec: &'static str,
    protocol_version: u32,
    registry_version: u32,
    canonical_profile: &'static str,
    numeric_profile: &'static str,
    facts_profile: &'static str,
    scope: &'static str,
    fact: &'static str,
    subject: serde_json::Value,
    producer: Producer<'a>,
}

#[derive(Serialize)]
struct Producer<'a> {
    core: &'a str,
    brep: &'a str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Payload {
    schema: String,
    family: String,
    codec: String,
    action_sha256: String,
    fact: String,
    provenance: String,
    /// Always null: no stored fact carries an error bound yet.
    bound: Option<String>,
    units: String,
    value: serde_json::Value,
}

/// Exact IEEE-754 bits, so a stored fact reads back bit for bit.
type Bits = String;

fn bits(value: f64) -> Bits {
    format!("{:016x}", value.to_bits())
}

fn unbits(value: &str) -> Option<f64> {
    (value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| u64::from_str_radix(value, 16).ok().map(f64::from_bits))
    .flatten()
}

fn point(value: [f64; 3]) -> [Bits; 3] {
    value.map(bits)
}

fn unpoint(value: &[Bits; 3]) -> Option<[f64; 3]> {
    Some([unbits(&value[0])?, unbits(&value[1])?, unbits(&value[2])?])
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundsDto {
    min: [Bits; 3],
    max: [Bits; 3],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MassDto {
    volume: Bits,
    center_of_mass: [Bits; 3],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AreaDto {
    surface_area: Bits,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CountsDto {
    compounds: u64,
    solids: u64,
    shells: u64,
    faces: u64,
    wires: u64,
    edges: u64,
    vertices: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckDto {
    shape: String,
    status: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SmallEdgeDto {
    length: Bits,
    shape: Option<String>,
    location: Option<[Bits; 3]>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ValidityDto {
    valid: bool,
    checks: Option<Vec<CheckDto>>,
    max_tolerance: Option<Bits>,
    free_bounds: Option<u32>,
    small_edges: Option<Vec<SmallEdgeDto>>,
    same_parameter: Option<bool>,
    closed_shells: Option<bool>,
    closed_solids: Option<bool>,
    solid_count: Option<u32>,
    invalid_solid_count: Option<u32>,
    open_edge_count: Option<u32>,
    nonmanifold_edge_count: Option<u32>,
    closed_wires: Option<bool>,
    reason: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ClosureDto {
    /// The work units the backend charged to name the failing groups, or
    /// `None` when it charged nothing.
    units: Option<String>,
    shells: u32,
    free_faces: u32,
    open_edges: u32,
    nonmanifold_edges: u32,
    failing: Vec<ClosureGroupDto>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ClosureGroupDto {
    free_faces: bool,
    open_edges: u32,
    nonmanifold_edges: u32,
    occurrences: Vec<u32>,
    samples: Vec<ClosureSampleDto>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ClosureSampleDto {
    face_uses: u32,
    start: [Bits; 3],
    end: [Bits; 3],
    center: [Bits; 3],
}

fn store(subject: &Subject) -> Option<&dyn OverlapEvidenceCache> {
    subject.overlap_cache.as_deref()
}

/// The address of one fact of the whole subject, or `None` when the subject
/// has no store, no verified producer or no established identity.
pub(crate) fn address(subject: &Subject, fact: Fact) -> Option<EvidenceAddress> {
    let producer = subject.producer_identity.as_ref()?;
    if producer.core.is_empty() || producer.brep.is_empty() {
        return None;
    }
    let action = Action {
        schema: "geospec-completed-fact-action-v1",
        family: FAMILY,
        codec: CODEC,
        protocol_version: PROTOCOL_VERSION as u32,
        registry_version: REGISTRY_VERSION as u32,
        canonical_profile: CANONICAL_PROFILE,
        // The constant is the only numeric profile served today. Once v7 (or
        // any second profile) is served, this must key on the profile the
        // subject's facts were computed under, or a v6 fact would answer a
        // v7 demand.
        numeric_profile: NUMERIC_PROFILE,
        facts_profile: FACTS_PROFILE,
        scope: "subject",
        fact: fact.name(),
        subject: serde_json::from_slice(&subject.identity_descriptor_bytes().ok()?).ok()?,
        producer: Producer {
            core: &producer.core,
            brep: &producer.brep,
        },
    };
    let action_bytes = crate::canonicalize(&serde_json::to_vec(&action).ok()?).ok()?;
    let producer_bytes = crate::canonicalize(&serde_json::to_vec(&action.producer).ok()?).ok()?;
    Some(EvidenceAddress {
        action_sha256: sha256_hex(&action_bytes),
        family: FAMILY,
        codec: CODEC,
        producer_profile_sha256: sha256_hex(&producer_bytes),
    })
}

fn payload_bytes(
    address: &EvidenceAddress,
    fact: Fact,
    value: serde_json::Value,
) -> Option<Vec<u8>> {
    let payload = Payload {
        schema: PAYLOAD_SCHEMA.into(),
        family: FAMILY.into(),
        codec: CODEC.into(),
        action_sha256: address.action_sha256.clone(),
        fact: fact.name().into(),
        provenance: PROVENANCE.into(),
        bound: None,
        units: "mm".into(),
        value,
    };
    let bytes = crate::canonicalize(&serde_json::to_vec(&payload).ok()?).ok()?;
    (bytes.len() <= MAX_PAYLOAD_BYTES).then_some(bytes)
}

fn load_value<T: for<'de> Deserialize<'de>>(subject: &Subject, fact: Fact) -> Option<T> {
    let store = store(subject)?;
    let address = address(subject, fact)?;
    let bytes = store.load(&address)?;
    if bytes.len() > MAX_PAYLOAD_BYTES || crate::canonicalize(&bytes).ok()? != bytes {
        return None;
    }
    let payload: Payload = serde_json::from_slice(&bytes).ok()?;
    if payload.schema != PAYLOAD_SCHEMA
        || payload.family != FAMILY
        || payload.codec != CODEC
        || payload.action_sha256 != address.action_sha256
        || payload.fact != fact.name()
        || payload.provenance != PROVENANCE
        || payload.bound.is_some()
        || payload.units != "mm"
    {
        return None;
    }
    serde_json::from_value(payload.value).ok()
}

/// Write-through of one completed fact; the store treats a repeat as a
/// no-op and a divergent repeat as poison. Returns whether it was offered.
fn publish_value(subject: &Subject, fact: Fact, value: impl Serialize) -> bool {
    let (Some(store), Some(address)) = (store(subject), address(subject, fact)) else {
        return false;
    };
    let Some(bytes) = serde_json::to_value(value)
        .ok()
        .and_then(|value| payload_bytes(&address, fact, value))
    else {
        return false;
    };
    store.publish(&address, &bytes);
    true
}

/// Whole-shape facts for `parts` read from the store, the unrequested parts
/// unmeasured (NaN, counts zero) as the backend leaves them; `None` unless
/// every requested fact is stored.
pub(crate) fn load_shape(subject: &Subject, parts: ShapeParts) -> Option<ShapeFacts> {
    store(subject)?;
    let mut shape = ShapeFacts {
        bounds: Bounds {
            min: [f64::NAN; 3],
            max: [f64::NAN; 3],
        },
        volume: f64::NAN,
        surface_area: f64::NAN,
        center_of_mass: [f64::NAN; 3],
        topology: TopologyCounts {
            compounds: 0,
            solids: 0,
            shells: 0,
            faces: 0,
            wires: 0,
            edges: 0,
            vertices: 0,
        },
    };
    let mut any = false;
    for (_, fact) in shape_facts(parts) {
        any = true;
        match fact {
            Fact::Bounds => {
                let value: BoundsDto = load_value(subject, fact)?;
                shape.bounds = Bounds {
                    min: unpoint(&value.min)?,
                    max: unpoint(&value.max)?,
                };
            }
            Fact::Mass => {
                let value: MassDto = load_value(subject, fact)?;
                shape.volume = unbits(&value.volume)?;
                shape.center_of_mass = unpoint(&value.center_of_mass)?;
            }
            Fact::Area => {
                let value: AreaDto = load_value(subject, fact)?;
                shape.surface_area = unbits(&value.surface_area)?;
            }
            Fact::Counts => {
                let value: CountsDto = load_value(subject, fact)?;
                shape.topology = TopologyCounts {
                    compounds: usize::try_from(value.compounds).ok()?,
                    solids: usize::try_from(value.solids).ok()?,
                    shells: usize::try_from(value.shells).ok()?,
                    faces: usize::try_from(value.faces).ok()?,
                    wires: usize::try_from(value.wires).ok()?,
                    edges: usize::try_from(value.edges).ok()?,
                    vertices: usize::try_from(value.vertices).ok()?,
                };
            }
            Fact::Validity | Fact::Closure => unreachable!("not a shape fact"),
        }
    }
    any.then_some(shape)
}

/// Write-through of every requested part of freshly computed shape facts.
/// Returns how many facts were offered to the store.
pub(crate) fn publish_shape(subject: &Subject, parts: ShapeParts, shape: &ShapeFacts) -> u64 {
    if store(subject).is_none() {
        return 0;
    }
    let mut offered = 0;
    for (_, fact) in shape_facts(parts) {
        let published = match fact {
            Fact::Bounds => publish_value(
                subject,
                fact,
                BoundsDto {
                    min: point(shape.bounds.min),
                    max: point(shape.bounds.max),
                },
            ),
            Fact::Mass => publish_value(
                subject,
                fact,
                MassDto {
                    volume: bits(shape.volume),
                    center_of_mass: point(shape.center_of_mass),
                },
            ),
            Fact::Area => publish_value(
                subject,
                fact,
                AreaDto {
                    surface_area: bits(shape.surface_area),
                },
            ),
            Fact::Counts => {
                let counts = shape.topology;
                publish_value(
                    subject,
                    fact,
                    CountsDto {
                        compounds: counts.compounds as u64,
                        solids: counts.solids as u64,
                        shells: counts.shells as u64,
                        faces: counts.faces as u64,
                        wires: counts.wires as u64,
                        edges: counts.edges as u64,
                        vertices: counts.vertices as u64,
                    },
                )
            }
            Fact::Validity | Fact::Closure => unreachable!("not a shape fact"),
        };
        offered += u64::from(published);
    }
    offered
}

pub(crate) fn load_validity(subject: &Subject) -> Option<ValidityFacts> {
    let value: ValidityDto = load_value(subject, Fact::Validity)?;
    Some(ValidityFacts {
        valid: value.valid,
        checks: value.checks.map(|checks| {
            checks
                .into_iter()
                .map(|check| ValidityCheck {
                    shape: check.shape,
                    status: check.status,
                })
                .collect()
        }),
        max_tolerance: match value.max_tolerance {
            Some(value) => Some(unbits(&value)?),
            None => None,
        },
        free_bounds: value.free_bounds,
        small_edges: match value.small_edges {
            Some(edges) => Some(
                edges
                    .into_iter()
                    .map(|edge| {
                        Some(SmallEdge {
                            length: unbits(&edge.length)?,
                            shape: edge.shape,
                            location: match edge.location {
                                Some(location) => Some(unpoint(&location)?),
                                None => None,
                            },
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
            None => None,
        },
        same_parameter: value.same_parameter,
        closed_shells: value.closed_shells,
        closed_solids: value.closed_solids,
        solid_count: value.solid_count,
        invalid_solid_count: value.invalid_solid_count,
        open_edge_count: value.open_edge_count,
        nonmanifold_edge_count: value.nonmanifold_edge_count,
        closed_wires: value.closed_wires,
        reason: value.reason,
    })
}

/// A lower bound on the serialized size of a validity fact's collections,
/// computed without allocating: each check is at least its strings plus the
/// 24 bytes of `{"shape":"","status":""}`, and each small edge at least its
/// shape plus a 16-digit length.
fn validity_size_floor(validity: &ValidityFacts) -> usize {
    let checks = validity
        .checks
        .as_deref()
        .unwrap_or_default()
        .iter()
        .fold(0_usize, |size, check| {
            size.saturating_add(24 + check.shape.len() + check.status.len())
        });
    let edges = validity
        .small_edges
        .as_deref()
        .unwrap_or_default()
        .iter()
        .fold(0_usize, |size, edge| {
            size.saturating_add(16 + edge.shape.as_deref().map_or(0, str::len))
        });
    checks.saturating_add(edges)
}

pub(crate) fn publish_validity(subject: &Subject, validity: &ValidityFacts) -> bool {
    // An oversized fact is never stored: refuse before cloning its collections.
    if store(subject).is_none() || validity_size_floor(validity) > MAX_PAYLOAD_BYTES {
        return false;
    }
    publish_value(
        subject,
        Fact::Validity,
        ValidityDto {
            valid: validity.valid,
            checks: validity.checks.as_ref().map(|checks| {
                checks
                    .iter()
                    .map(|check| CheckDto {
                        shape: check.shape.clone(),
                        status: check.status.clone(),
                    })
                    .collect()
            }),
            max_tolerance: validity.max_tolerance.map(bits),
            free_bounds: validity.free_bounds,
            small_edges: validity.small_edges.as_ref().map(|edges| {
                edges
                    .iter()
                    .map(|edge| SmallEdgeDto {
                        length: bits(edge.length),
                        shape: edge.shape.clone(),
                        location: edge.location.map(point),
                    })
                    .collect()
            }),
            same_parameter: validity.same_parameter,
            closed_shells: validity.closed_shells,
            closed_solids: validity.closed_solids,
            solid_count: validity.solid_count,
            invalid_solid_count: validity.invalid_solid_count,
            open_edge_count: validity.open_edge_count,
            nonmanifold_edge_count: validity.nonmanifold_edge_count,
            closed_wires: validity.closed_wires,
            reason: validity.reason.clone(),
        },
    )
}

/// The stored closure and the work units its completed build charged, which
/// a hit charges again before answering; `None` units when the build charged
/// nothing, so a hit charges nothing either.
pub(crate) fn load_closure(subject: &Subject) -> Option<(Option<u64>, ClosureFacts)> {
    let value: ClosureDto = load_value(subject, Fact::Closure)?;
    let units = match value.units.as_deref() {
        None => None,
        Some(text) => {
            let units = text.parse::<u64>().ok()?;
            if units.to_string() != text {
                return None;
            }
            Some(units)
        }
    };
    // I1: an empty failing list answers watertight, so an entry that counts
    // open or non-manifold edges without naming a failing group is a miss.
    if value.failing.is_empty() && (value.open_edges != 0 || value.nonmanifold_edges != 0) {
        return None;
    }
    let failing = value
        .failing
        .into_iter()
        .map(|group| {
            Some(ClosureGroup {
                free_faces: group.free_faces,
                open_edges: group.open_edges,
                nonmanifold_edges: group.nonmanifold_edges,
                occurrences: group.occurrences,
                samples: group
                    .samples
                    .into_iter()
                    .map(|sample| {
                        Some(ClosureEdgeSample {
                            face_uses: sample.face_uses,
                            start: unpoint(&sample.start)?,
                            end: unpoint(&sample.end)?,
                            center: unpoint(&sample.center)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((
        units,
        ClosureFacts {
            shells: value.shells,
            free_faces: value.free_faces,
            open_edges: value.open_edges,
            nonmanifold_edges: value.nonmanifold_edges,
            failing,
        },
    ))
}

pub(crate) fn publish_closure(
    subject: &Subject,
    units: Option<u64>,
    closure: &ClosureFacts,
) -> bool {
    if store(subject).is_none() {
        return false;
    }
    publish_value(
        subject,
        Fact::Closure,
        ClosureDto {
            units: units.map(|units| units.to_string()),
            shells: closure.shells,
            free_faces: closure.free_faces,
            open_edges: closure.open_edges,
            nonmanifold_edges: closure.nonmanifold_edges,
            failing: closure
                .failing
                .iter()
                .map(|group| ClosureGroupDto {
                    free_faces: group.free_faces,
                    open_edges: group.open_edges,
                    nonmanifold_edges: group.nonmanifold_edges,
                    occurrences: group.occurrences.clone(),
                    samples: group
                        .samples
                        .iter()
                        .map(|sample| ClosureSampleDto {
                            face_uses: sample.face_uses,
                            start: point(sample.start),
                            end: point(sample.end),
                            center: point(sample.center),
                        })
                        .collect(),
                })
                .collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        rc::Rc,
    };

    use super::*;
    use crate::{
        backend::{
            brep::{
                BrepEntity, BrepIdentityProfile, BrepSubject, LocatedFace, OccurrenceFacts,
                PointState, TessellationProfile,
            },
            BackendError, BackendErrorKind, TriangleMesh,
        },
        cache::ProducerIdentity,
        identity::SubjectIdentity,
        subject::SubjectFormat,
    };

    /// Write-once per address, like the host store: a repeat is a no-op and a
    /// divergent repeat poisons the address.
    #[derive(Default)]
    struct MemoryStore {
        entries: RefCell<HashMap<String, Vec<u8>>>,
        poisoned: RefCell<Vec<String>>,
        loads: Cell<u32>,
    }

    impl OverlapEvidenceCache for MemoryStore {
        fn load(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
            self.loads.set(self.loads.get() + 1);
            if self.poisoned.borrow().contains(&address.action_sha256) {
                return None;
            }
            self.entries.borrow().get(&address.action_sha256).cloned()
        }

        fn publish(&self, address: &EvidenceAddress, payload: &[u8]) {
            assert_eq!((address.family, address.codec), (FAMILY, CODEC));
            let mut entries = self.entries.borrow_mut();
            match entries.get(&address.action_sha256) {
                Some(existing) if existing == payload => {}
                Some(_) => self
                    .poisoned
                    .borrow_mut()
                    .push(address.action_sha256.clone()),
                None => {
                    entries.insert(address.action_sha256.clone(), payload.to_vec());
                }
            }
        }
    }

    #[derive(Default)]
    struct Calls {
        shape: Cell<u32>,
        validity: Cell<u32>,
        closure: Cell<u32>,
    }

    fn closure() -> ClosureFacts {
        ClosureFacts {
            shells: 2,
            free_faces: 1,
            open_edges: 3,
            nonmanifold_edges: 0,
            failing: vec![ClosureGroup {
                free_faces: true,
                open_edges: 3,
                nonmanifold_edges: 0,
                occurrences: vec![0, 2],
                samples: vec![ClosureEdgeSample {
                    face_uses: 1,
                    start: [0.0, 0.1, 0.2],
                    end: [1.0 / 3.0, -0.0, 5.5],
                    center: [0.5, 0.05, 2.85],
                }],
            }],
        }
    }

    /// The units the fake backend charges to name its failing groups.
    const CLOSURE_UNITS: u64 = 4;

    struct FakeBrep {
        calls: Rc<Calls>,
        fail: bool,
        /// Closure work units; zero means the build charges nothing.
        closure_units: u64,
    }

    fn shape() -> ShapeFacts {
        ShapeFacts {
            bounds: Bounds {
                min: [-1.5, 0.1 + 0.2, -0.0],
                max: [2.0, 1e-300, 7.25],
            },
            volume: 1.0 / 3.0,
            surface_area: std::f64::consts::PI,
            center_of_mass: [0.1, -0.2, 0.3],
            topology: TopologyCounts {
                compounds: 1,
                solids: 2,
                shells: 3,
                faces: 4,
                wires: 5,
                edges: 6,
                vertices: 7,
            },
        }
    }

    fn validity() -> ValidityFacts {
        ValidityFacts {
            valid: false,
            checks: Some(vec![ValidityCheck {
                shape: "solid 1".into(),
                status: "BRepCheck_UnorientableShape".into(),
            }]),
            max_tolerance: Some(1e-7),
            free_bounds: Some(2),
            small_edges: Some(vec![SmallEdge {
                length: 1e-9,
                shape: None,
                location: Some([1.0, 2.0, f64::from_bits(0x3ff0_0000_0000_0001)]),
            }]),
            same_parameter: Some(true),
            closed_shells: Some(false),
            closed_solids: None,
            solid_count: Some(1),
            invalid_solid_count: Some(1),
            open_edge_count: Some(4),
            nonmanifold_edge_count: Some(0),
            closed_wires: Some(true),
            reason: None,
        }
    }

    fn failure() -> BackendError {
        BackendError {
            kind: BackendErrorKind::ComputationFailed,
            message: "simulated kernel failure".into(),
        }
    }

    impl BrepSubject for FakeBrep {
        fn classify_face_points(
            &self,
            _: BrepEntity,
            _: &[[f64; 3]],
            _: f64,
        ) -> Result<Vec<PointState>, BackendError> {
            unreachable!()
        }
        fn tessellate(
            &self,
            _: BrepEntity,
            _: TessellationProfile,
        ) -> Result<Rc<TriangleMesh>, BackendError> {
            unreachable!()
        }
        fn faces(&self) -> Result<Rc<[LocatedFace]>, BackendError> {
            unreachable!()
        }
        fn source_occurrences(&self) -> Result<Rc<[OccurrenceFacts]>, BackendError> {
            Ok(Rc::from([]))
        }
        fn validity(&self) -> Result<Rc<ValidityFacts>, BackendError> {
            self.calls.validity.set(self.calls.validity.get() + 1);
            if self.fail {
                return Err(failure());
            }
            Ok(Rc::new(validity()))
        }
        fn closure(
            &self,
            charge: &mut crate::backend::brep::Charge<'_>,
        ) -> Result<Option<Rc<ClosureFacts>>, BackendError> {
            self.calls.closure.set(self.calls.closure.get() + 1);
            if self.fail {
                return Err(failure());
            }
            if self.closure_units == 0 {
                return Ok(Some(Rc::new(closure())));
            }
            Ok(charge(self.closure_units).then(|| Rc::new(closure())))
        }
        fn reported_shape_parts(&self, parts: ShapeParts) -> Result<ShapeFacts, BackendError> {
            self.calls.shape.set(self.calls.shape.get() + 1);
            if self.fail {
                return Err(failure());
            }
            // The backend leaves the unrequested parts unmeasured.
            let mut value = shape();
            if !parts.contains(ShapeParts::BOUNDS) {
                value.bounds.min = [f64::NAN; 3];
                value.bounds.max = [f64::NAN; 3];
            }
            Ok(value)
        }
    }

    fn subject_with(
        bytes: &[u8],
        store: &Rc<MemoryStore>,
        calls: &Rc<Calls>,
        fail: bool,
        brep_pin: &str,
    ) -> Subject {
        let identity = SubjectIdentity::step(
            bytes,
            "millimetre",
            1.0,
            BrepIdentityProfile {
                ingest_profile: "facts-test",
                backend_profile: "facts-test",
            },
            None,
        )
        .unwrap();
        let mut subject = Subject::new(
            identity.primary_hash().into(),
            SubjectFormat::Step,
            "mm".into(),
        );
        subject.semantic_identity.set(identity).unwrap();
        subject.producer_identity = Some(Rc::new(ProducerIdentity {
            core: "core-v1".into(),
            csg: "csg-v1".into(),
            brep: brep_pin.into(),
        }));
        subject.overlap_cache = Some(Rc::clone(store) as Rc<dyn OverlapEvidenceCache>);
        subject.brep = Some(Box::new(FakeBrep {
            calls: Rc::clone(calls),
            fail,
            closure_units: CLOSURE_UNITS,
        }));
        subject
    }

    fn subject(store: &Rc<MemoryStore>, calls: &Rc<Calls>) -> Subject {
        subject_with(b"facts-step", store, calls, false, "brep-v1")
    }

    fn same_bits(left: &ShapeFacts, right: &ShapeFacts, parts: ShapeParts) {
        let points = |value: [f64; 3]| value.map(f64::to_bits);
        if parts.contains(ShapeParts::BOUNDS) {
            assert_eq!(points(left.bounds.min), points(right.bounds.min));
            assert_eq!(points(left.bounds.max), points(right.bounds.max));
        }
        if parts.contains(ShapeParts::VOLUME) {
            assert_eq!(left.volume.to_bits(), right.volume.to_bits());
            assert_eq!(points(left.center_of_mass), points(right.center_of_mass));
        }
        if parts.contains(ShapeParts::AREA) {
            assert_eq!(left.surface_area.to_bits(), right.surface_area.to_bits());
        }
        if parts.contains(ShapeParts::COUNTS) {
            assert_eq!(left.topology, right.topology);
        }
    }

    #[test]
    fn warm_subject_reads_every_shape_fact_without_the_backend() {
        let store = Rc::new(MemoryStore::default());
        let cold_calls = Rc::new(Calls::default());
        let cold = subject(&store, &cold_calls);
        let built = cold.report_shape(ShapeParts::ALL).unwrap().unwrap().clone();
        assert_eq!(cold_calls.shape.get(), 1);
        // One write-once action per fact.
        assert_eq!(store.entries.borrow().len(), 4);

        let warm_calls = Rc::new(Calls::default());
        let warm = subject(&store, &warm_calls);
        let read = warm.report_shape(ShapeParts::ALL).unwrap().unwrap().clone();
        assert_eq!(warm_calls.shape.get(), 0, "a warm subject pays no facts");
        same_bits(&built, &read, ShapeParts::ALL);
        // A subset of stored facts is answered from the store too.
        let other = subject(&store, &warm_calls);
        let bounds = other.report_shape(ShapeParts::BOUNDS).unwrap().unwrap();
        assert_eq!(warm_calls.shape.get(), 0);
        same_bits(&built, bounds, ShapeParts::BOUNDS);
        assert!(
            bounds.volume.is_nan(),
            "an unrequested part stays unmeasured"
        );
    }

    #[test]
    fn a_partial_hit_recomputes_the_set_and_rewrites_it_identically() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        subject(&store, &calls)
            .report_shape(ShapeParts::VOLUME)
            .unwrap();
        assert_eq!(store.entries.borrow().len(), 1);
        let wider = ShapeParts::ALL;
        let second = subject(&store, &calls);
        second.report_shape(wider).unwrap();
        assert_eq!(calls.shape.get(), 2, "a missing fact is a miss for the set");
        assert_eq!(store.entries.borrow().len(), 4);
        assert!(store.poisoned.borrow().is_empty());
        let warm_calls = Rc::new(Calls::default());
        subject(&store, &warm_calls).report_shape(wider).unwrap();
        assert_eq!(warm_calls.shape.get(), 0);
    }

    #[test]
    fn validity_round_trips_bit_for_bit_and_is_retained() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let cold = subject(&store, &calls);
        let built = cold.validity().unwrap();
        cold.validity().unwrap();
        assert_eq!(calls.validity.get(), 1);
        let warm_calls = Rc::new(Calls::default());
        let read = subject(&store, &warm_calls).validity().unwrap();
        assert_eq!(warm_calls.validity.get(), 0);
        assert_eq!(*read, *built);
        let location = |facts: &ValidityFacts| {
            facts.small_edges.as_ref().unwrap()[0]
                .location
                .unwrap()
                .map(f64::to_bits)
        };
        assert_eq!(location(&read), location(&built));
        assert_eq!(
            read.max_tolerance.unwrap().to_bits(),
            built.max_tolerance.unwrap().to_bits()
        );
    }

    #[test]
    fn closure_replays_its_charge_and_a_stopped_build_is_not_stored() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        // A build stopped by its charge stores nothing.
        let stopped = subject(&store, &calls);
        assert!(stopped.closure(&mut |_| false).unwrap().is_none());
        assert!(store.entries.borrow().is_empty());
        let mut charged = Vec::new();
        let cold = subject(&store, &calls);
        let built = cold
            .closure(&mut |units| {
                charged.push(units);
                true
            })
            .unwrap()
            .unwrap();
        assert_eq!(charged, [CLOSURE_UNITS]);
        assert_eq!(store.entries.borrow().len(), 1);

        let warm_calls = Rc::new(Calls::default());
        let warm = subject(&store, &warm_calls);
        // A stored closure charges the same units and refuses as the build would.
        assert!(warm.closure(&mut |_| false).unwrap().is_none());
        let mut replayed = Vec::new();
        let read = warm
            .closure(&mut |units| {
                replayed.push(units);
                true
            })
            .unwrap()
            .unwrap();
        assert_eq!(warm_calls.closure.get(), 0);
        assert_eq!(replayed, charged);
        assert_eq!(*read, *built);
        let sample = |facts: &ClosureFacts| facts.failing[0].samples[0].end.map(f64::to_bits);
        assert_eq!(sample(&read), sample(&built));
    }

    #[test]
    fn a_zero_charge_closure_is_stored_and_replays_no_charge() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let zero = |calls: &Rc<Calls>| {
            let mut subject = subject(&store, calls);
            subject.brep = Some(Box::new(FakeBrep {
                calls: Rc::clone(calls),
                fail: false,
                closure_units: 0,
            }));
            subject
        };
        let mut charged = 0;
        let built = zero(&calls)
            .closure(&mut |_| {
                charged += 1;
                true
            })
            .unwrap()
            .unwrap();
        assert_eq!(charged, 0);
        assert_eq!(store.entries.borrow().len(), 1);
        let warm_calls = Rc::new(Calls::default());
        // A refusing charge is never asked: the build charged nothing.
        let read = zero(&warm_calls).closure(&mut |_| false).unwrap().unwrap();
        assert_eq!(warm_calls.closure.get(), 0);
        assert_eq!(*read, *built);
    }

    #[test]
    fn a_closure_with_edge_counts_but_no_failing_group_is_a_miss() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let cold = subject(&store, &calls);
        let mut inconsistent = closure();
        inconsistent.failing.clear();
        for (open_edges, nonmanifold_edges) in [(1, 0), (0, 1)] {
            store.entries.borrow_mut().clear();
            inconsistent.open_edges = open_edges;
            inconsistent.nonmanifold_edges = nonmanifold_edges;
            assert!(publish_closure(&cold, Some(CLOSURE_UNITS), &inconsistent));
            assert!(load_closure(&cold).is_none());
        }
        // A consistent entry with no failing group and no failing edge reads back.
        store.entries.borrow_mut().clear();
        inconsistent.open_edges = 0;
        inconsistent.nonmanifold_edges = 0;
        assert!(publish_closure(&cold, Some(CLOSURE_UNITS), &inconsistent));
        assert!(load_closure(&cold).is_some());
    }

    #[test]
    fn an_oversized_validity_fact_is_not_stored() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let cold = subject(&store, &calls);
        let mut large = validity();
        large.checks = Some(vec![ValidityCheck {
            shape: "s".repeat(MAX_PAYLOAD_BYTES),
            status: "BRepCheck_NoError".into(),
        }]);
        assert!(validity_size_floor(&large) > MAX_PAYLOAD_BYTES);
        assert!(!publish_validity(&cold, &large));
        assert!(store.entries.borrow().is_empty());
        // Many checks with empty strings are still oversized by count alone.
        let mut many = validity();
        many.checks = Some(vec![
            ValidityCheck {
                shape: String::new(),
                status: String::new(),
            };
            MAX_PAYLOAD_BYTES / 24 + 1
        ]);
        assert!(validity_size_floor(&many) > MAX_PAYLOAD_BYTES);
        assert!(!publish_validity(&cold, &many));
        assert!(store.entries.borrow().is_empty());
        assert!(validity_size_floor(&validity()) < 1024);
    }

    #[test]
    fn a_backend_failure_is_never_stored() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let failing = subject_with(b"facts-step", &store, &calls, true, "brep-v1");
        assert!(failing.report_shape(ShapeParts::ALL).is_err());
        assert!(failing.validity().is_err());
        assert!(failing.closure(&mut |_| true).is_err());
        assert!(store.entries.borrow().is_empty());
        // The next process meets the same failure; nothing answers for it.
        let again = subject_with(b"facts-step", &store, &calls, true, "brep-v1");
        assert!(again.report_shape(ShapeParts::ALL).is_err());
        assert!(again.validity().is_err());
        assert_eq!((calls.shape.get(), calls.validity.get()), (2, 2));
    }

    #[test]
    fn subject_producer_and_fact_each_select_their_own_action() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let base = subject(&store, &calls);
        let addresses = [
            address(&base, Fact::Mass),
            address(&base, Fact::Area),
            address(&base, Fact::Bounds),
            address(&base, Fact::Counts),
            address(&base, Fact::Validity),
            address(&base, Fact::Closure),
            address(
                &subject_with(b"other-step", &store, &calls, false, "brep-v1"),
                Fact::Mass,
            ),
            address(
                &subject_with(b"facts-step", &store, &calls, false, "brep-v2"),
                Fact::Mass,
            ),
        ]
        .map(|address| address.unwrap().action_sha256);
        let unique: std::collections::HashSet<_> = addresses.iter().collect();
        assert_eq!(unique.len(), addresses.len());
        // A new producer pin misses what the old one stored.
        base.report_shape(ShapeParts::ALL).unwrap();
        let repinned = subject_with(b"facts-step", &store, &calls, false, "brep-v2");
        repinned.report_shape(ShapeParts::ALL).unwrap();
        assert_eq!(calls.shape.get(), 2);
    }

    #[test]
    fn a_tampered_or_foreign_payload_is_a_miss() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let cold = subject(&store, &calls);
        cold.report_shape(ShapeParts::VOLUME).unwrap();
        let mass = address(&cold, Fact::Mass).unwrap().action_sha256;
        let area = address(&cold, Fact::Area).unwrap().action_sha256;
        let stored = store.entries.borrow()[&mass].clone();
        for tampered in [
            // Another fact's payload under this address.
            String::from_utf8(stored.clone())
                .unwrap()
                .replace("\"fact\":\"mass\"", "\"fact\":\"area\""),
            // A non-canonical spelling of the same payload.
            format!(" {}", String::from_utf8(stored.clone()).unwrap()),
            // A bound the legacy provenance never carries.
            String::from_utf8(stored.clone())
                .unwrap()
                .replace("\"bound\":null", "\"bound\":\"0000000000000000\""),
        ] {
            // Each case starts unpoisoned, so its miss comes from the
            // tampering, not from the previous case's divergent rewrite.
            store.poisoned.borrow_mut().clear();
            store
                .entries
                .borrow_mut()
                .insert(mass.clone(), tampered.into_bytes());
            let before = calls.shape.get();
            subject(&store, &calls)
                .report_shape(ShapeParts::VOLUME)
                .unwrap();
            assert_eq!(calls.shape.get(), before + 1);
        }
        store.entries.borrow_mut().insert(area, stored);
        let before = calls.shape.get();
        subject(&store, &calls)
            .report_shape(ShapeParts::AREA)
            .unwrap();
        assert_eq!(calls.shape.get(), before + 1, "a moved payload misses");
    }

    #[test]
    fn no_store_or_no_producer_never_reads_or_writes() {
        let store = Rc::new(MemoryStore::default());
        let calls = Rc::new(Calls::default());
        let mut unpinned = subject(&store, &calls);
        unpinned.producer_identity = None;
        unpinned.report_shape(ShapeParts::ALL).unwrap();
        unpinned.validity().unwrap();
        assert!(store.entries.borrow().is_empty());
        assert_eq!(store.loads.get(), 0);
        let mut storeless = subject(&store, &calls);
        storeless.overlap_cache = None;
        storeless.report_shape(ShapeParts::ALL).unwrap();
        assert!(store.entries.borrow().is_empty());
    }
}
