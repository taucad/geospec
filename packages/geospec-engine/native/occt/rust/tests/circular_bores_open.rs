//! W5b open arcs: a concave cylinder wall spanning at most half a turn whose
//! axial edges both meet other surfaces (a rounded cutout or pocket corner, a
//! slot end, a flute) is an `OpenArc` non-member, so it no longer leaves the
//! inventory open. A slit bore, whose wall spans more than half a turn, stays
//! unqualified (fixture provenance in
//! `fixtures/circular-bores-open/provenance.json`).
use geospec_engine_native_core::backend::brep::{
    BrepSubject, CircularBoreDisposition, CircularBoreInventory, CircularBoreNonMember,
    CircularBoreUnqualified, SurfaceFacts,
};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

fn observe(name: &str) -> CircularBoreInventory {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/circular-bores-open")
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

fn count(inventory: &CircularBoreInventory, disposition: CircularBoreDisposition) -> usize {
    inventory
        .candidates
        .iter()
        .filter(|candidate| candidate.disposition == disposition)
        .count()
}

const OPEN_ARC: CircularBoreDisposition =
    CircularBoreDisposition::NonMember(CircularBoreNonMember::OpenArc);

#[test]
fn rounded_corners_and_slot_ends_are_open_arcs() {
    for (name, arcs) in [
        ("a0-rounded-window.step", 4),
        ("a1-slot.step", 2),
        ("a4-rounded-pocket.step", 4),
    ] {
        let inventory = observe(name);
        assert_eq!(inventory.candidates.len(), arcs, "{name}: {inventory:#?}");
        assert_eq!(count(&inventory, OPEN_ARC), arcs, "{name}: {inventory:#?}");
    }
}

#[test]
fn flutes_on_a_shaft_are_open_arcs() {
    let inventory = observe("a2-fluted-shaft.step");
    assert_eq!(count(&inventory, OPEN_ARC), 6, "{inventory:#?}");
    assert_eq!(
        count(
            &inventory,
            CircularBoreDisposition::NonMember(CircularBoreNonMember::ExteriorCylinder)
        ),
        6
    );
}

#[test]
fn a_slit_bore_stays_unqualified() {
    let inventory = observe("a3-slit-bore.step");
    assert_eq!(count(&inventory, OPEN_ARC), 0, "{inventory:#?}");
    assert_eq!(
        count(
            &inventory,
            CircularBoreDisposition::Unqualified(CircularBoreUnqualified::IncompleteBand)
        ),
        1
    );
}
