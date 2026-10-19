//! W5b split-bore admission: a replicad `drawCircle` is two sagitta arcs, so a
//! sketched and extruded hole is a band of two half-cylinder faces instead of
//! one seamed cylinder. The band's first face in public order carries the bore;
//! every other face of it is a `SplitBandMember` of that row. A partial band
//! that opens onto another face stays `IncompleteBand` (fixture provenance in
//! `fixtures/circular-bores-split/provenance.json`).
use geospec_engine_native_core::backend::brep::{
    BrepSubject, CircularBoreDisposition, CircularBoreInventory, CircularBoreNonMember,
    CircularBoreTermination, CircularBoreUnqualified, SurfaceFacts,
};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

fn observe(name: &str) -> CircularBoreInventory {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/circular-bores-split")
            .join(name),
    )
    .expect("retained fixture must be readable");
    let document = Document::from_step(&bytes).expect("fixture must be admitted");
    let faces = document.faces().expect("faces");
    let nonplanar = faces
        .iter()
        .filter(|face| !matches!(face.facts.surface, SurfaceFacts::Plane { .. }))
        .count();
    let inventory = document.circular_bores(nonplanar).expect("bore query");
    assert_eq!(
        inventory.candidates.len(),
        nonplanar,
        "{name}: one row per face"
    );
    inventory
}

/// Each qualified split band, with its members, as (representative, members, radius, through).
fn split_bands(inventory: &CircularBoreInventory) -> Vec<(u32, Vec<u32>, f64, bool)> {
    let mut bands = Vec::new();
    for candidate in &inventory.candidates {
        if let CircularBoreDisposition::Qualified(topology) = &candidate.disposition {
            let members: Vec<u32> = inventory
                .candidates
                .iter()
                .filter(|other| {
                    other.disposition
                        == CircularBoreDisposition::NonMember(
                            CircularBoreNonMember::SplitBandMember {
                                representative_public_face_ordinal: candidate.public_face_ordinal,
                            },
                        )
                })
                .map(|other| other.public_face_ordinal)
                .collect();
            assert_eq!(
                topology.band_face_count as usize,
                members.len() + 1,
                "every member answers to its representative"
            );
            assert!(members
                .iter()
                .all(|&member| member > candidate.public_face_ordinal));
            assert_eq!(topology.interior_residual_solid_count, 0);
            bands.push((
                candidate.public_face_ordinal,
                members,
                topology.band.radius,
                topology
                    .ends
                    .iter()
                    .all(|end| end.termination == CircularBoreTermination::Mouth),
            ));
        }
    }
    bands
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn sketched_through_bore_split_in_two_faces_is_one_qualified_bore() {
    let inventory = observe("s1-sketched-through.step");
    let bands = split_bands(&inventory);
    assert_eq!(bands.len(), 1, "{inventory:#?}");
    let (_, members, radius, through) = &bands[0];
    assert_eq!(members.len(), 1);
    close(*radius, 5.0);
    assert!(through);
    let CircularBoreDisposition::Qualified(topology) = &inventory
        .candidates
        .iter()
        .find(|candidate| matches!(candidate.disposition, CircularBoreDisposition::Qualified(_)))
        .unwrap()
        .disposition
    else {
        unreachable!()
    };
    close(topology.band.to - topology.band.from, 20.0);
    assert_ne!(
        topology.ends[0].adjacent_public_face_ordinal,
        topology.ends[1].adjacent_public_face_ordinal
    );
}

#[test]
fn sketched_pattern_bores_each_qualify_once() {
    let inventory = observe("s2-sketched-pattern.step");
    let bands = split_bands(&inventory);
    assert_eq!(bands.len(), 2, "{inventory:#?}");
    for (_, members, radius, through) in &bands {
        assert_eq!(members.len(), 1);
        close(*radius, 3.0);
        assert!(through);
    }
    assert_eq!(inventory.candidates.len(), 4);
}

#[test]
fn split_washer_keeps_its_exterior_out_and_its_bore_in() {
    let inventory = observe("s0-washer.step");
    let bands = split_bands(&inventory);
    assert_eq!(bands.len(), 1, "{inventory:#?}");
    close(bands[0].2, 4.0);
    assert!(bands[0].3);
    let exterior = inventory
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.disposition
                == CircularBoreDisposition::NonMember(CircularBoreNonMember::ExteriorCylinder)
        })
        .count();
    assert_eq!(exterior, 2);
}

#[test]
fn half_notch_open_to_a_side_face_stays_an_incomplete_band() {
    let inventory = observe("s5-edge-half-notch.step");
    assert_eq!(inventory.candidates.len(), 2);
    for candidate in &inventory.candidates {
        assert_eq!(
            candidate.disposition,
            CircularBoreDisposition::Unqualified(CircularBoreUnqualified::IncompleteBand),
            "{inventory:#?}"
        );
    }
}

#[test]
fn split_bore_whose_arcs_meet_two_chamfer_cones_has_an_unsupported_end() {
    let inventory = observe("s6-split-chamfered-mouth.step");
    let representative = inventory
        .candidates
        .iter()
        .find(|candidate| {
            candidate.disposition
                == CircularBoreDisposition::Unqualified(
                    CircularBoreUnqualified::UnsupportedTermination,
                )
        })
        .unwrap_or_else(|| panic!("{inventory:#?}"));
    assert!(inventory.candidates.iter().any(|candidate| {
        candidate.disposition
            == CircularBoreDisposition::NonMember(CircularBoreNonMember::SplitBandMember {
                representative_public_face_ordinal: representative.public_face_ordinal,
            })
    }));
}
