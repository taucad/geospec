//! W5b bore terminations: a chamfered, countersunk or rounded rim is a
//! `TreatedMouth` and its cone or torus a `MouthTreatment` of that bore; a
//! counterbore band ends on a `Shoulder` opposite its mouth. An internal groove
//! between two annuli stays `UnsupportedTermination` (fixture provenance in
//! `fixtures/circular-bores-mouths/provenance.json`).
use geospec_engine_native_core::backend::brep::{
    BrepSubject, CircularBoreDisposition, CircularBoreInventory, CircularBoreNonMember,
    CircularBoreTermination, CircularBoreUnqualified, SurfaceFacts,
};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

use CircularBoreTermination::{Mouth, PlanarDiskBottom, Shoulder, TreatedMouth};

fn observe(name: &str) -> CircularBoreInventory {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/circular-bores-mouths")
            .join(name),
    )
    .expect("retained fixture must be readable");
    let document = Document::from_step(&bytes).expect("fixture must be admitted");
    let nonplanar = document
        .faces()
        .expect("faces")
        .iter()
        .filter(|face| !matches!(face.facts.surface, SurfaceFacts::Plane { .. }))
        .count();
    document.circular_bores(nonplanar).expect("bore query")
}

/// Every qualified bore as (radius, sorted terminations, treatment count), and
/// the rows that are neither a bore nor one of its treatments.
fn bores(inventory: &CircularBoreInventory) -> Vec<(f64, Vec<CircularBoreTermination>, usize)> {
    let mut out = Vec::new();
    for candidate in &inventory.candidates {
        let CircularBoreDisposition::Qualified(topology) = &candidate.disposition else {
            continue;
        };
        assert_eq!(topology.interior_residual_solid_count, 0);
        let mut ends: Vec<_> = topology.ends.iter().map(|end| end.termination).collect();
        ends.sort_by_key(|termination| *termination as u8);
        let treatments = inventory
            .candidates
            .iter()
            .filter(|other| {
                other.disposition
                    == CircularBoreDisposition::NonMember(CircularBoreNonMember::MouthTreatment {
                        bore_public_face_ordinal: candidate.public_face_ordinal,
                    })
            })
            .count();
        out.push(((topology.band.radius * 1e6).round() / 1e6, ends, treatments));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn complete(inventory: &CircularBoreInventory) -> bool {
    inventory.candidates.iter().all(|candidate| {
        !matches!(
            candidate.disposition,
            CircularBoreDisposition::Unqualified(_)
        )
    })
}

#[test]
fn chamfered_countersunk_and_rounded_mouths_open_their_bores() {
    for name in [
        "t0-chamfered-mouth.step",
        "t1-countersink-2mm.step",
        "t2-rounded-mouth.step",
    ] {
        let inventory = observe(name);
        assert_eq!(
            bores(&inventory),
            [(3.0, vec![Mouth, TreatedMouth], 1)],
            "{name}: {inventory:#?}"
        );
        assert!(complete(&inventory), "{name}: {inventory:#?}");
    }
}

#[test]
fn a_bore_chamfered_at_both_ends_is_through_with_two_treatments() {
    let inventory = observe("t3-chamfered-both-ends.step");
    assert_eq!(
        bores(&inventory),
        [(3.0, vec![TreatedMouth, TreatedMouth], 2)]
    );
    assert!(complete(&inventory));
}

#[test]
fn a_chamfered_blind_bore_keeps_its_floor() {
    let inventory = observe("t7-blind-chamfered.step");
    assert_eq!(
        bores(&inventory),
        [(3.0, vec![PlanarDiskBottom, TreatedMouth], 1)]
    );
    assert!(complete(&inventory));
}

#[test]
fn a_counterbore_ends_on_its_shoulder() {
    let inventory = observe("t4-counterbore.step");
    assert_eq!(
        bores(&inventory),
        [
            (3.0, vec![Mouth, Mouth], 0),
            (5.0, vec![Mouth, Shoulder], 0)
        ]
    );
    assert!(complete(&inventory));
}

#[test]
fn an_internal_groove_between_two_annuli_is_no_hole() {
    let inventory = observe("t6-internal-groove.step");
    let groove = inventory
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.disposition
                == CircularBoreDisposition::Unqualified(
                    CircularBoreUnqualified::UnsupportedTermination,
                )
        })
        .count();
    assert_eq!(groove, 1, "{inventory:#?}");
}
