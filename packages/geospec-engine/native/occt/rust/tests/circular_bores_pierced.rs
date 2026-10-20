//! W5b pierced bores and exterior cones: a seamed bore whose wall is pierced
//! strictly between its rims, as a set-screw hole pierces a collar, is still a
//! qualified bore that counts its openings; a cone with material on its own
//! axis side is an `ExteriorCone` non-member. A branch that meets the bore
//! tangentially, a window whose planes cut the wall in coaxial arcs and a
//! drill-point cone stay unqualified (fixture provenance in
//! `fixtures/circular-bores-pierced/provenance.json`).
use geospec_engine_native_core::backend::brep::{
    BrepSubject, CircularBoreDisposition, CircularBoreInventory, CircularBoreNonMember,
    CircularBoreTermination, CircularBoreUnqualified, SurfaceFacts,
};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

fn observe(name: &str) -> CircularBoreInventory {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/circular-bores-pierced")
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

/// Every qualified bore as (radius, side openings, through).
fn bores(inventory: &CircularBoreInventory) -> Vec<(f64, u32, bool)> {
    inventory
        .candidates
        .iter()
        .filter_map(|candidate| match &candidate.disposition {
            CircularBoreDisposition::Qualified(topology) => {
                assert_eq!(topology.interior_residual_solid_count, 0);
                assert_eq!(topology.band_face_count, 1);
                Some((
                    topology.band.radius,
                    topology.side_opening_count,
                    topology
                        .ends
                        .iter()
                        .all(|end| end.termination == CircularBoreTermination::Mouth),
                ))
            }
            _ => None,
        })
        .collect()
}

fn count(inventory: &CircularBoreInventory, disposition: CircularBoreDisposition) -> usize {
    inventory
        .candidates
        .iter()
        .filter(|candidate| candidate.disposition == disposition)
        .count()
}

const INCOMPLETE: CircularBoreDisposition =
    CircularBoreDisposition::Unqualified(CircularBoreUnqualified::IncompleteBand);

#[test]
fn a_set_screw_hole_in_the_wall_keeps_the_bore() {
    for name in [
        "p0-set-screw-across-seam.step",
        "p1-set-screw-beside-seam.step",
    ] {
        let inventory = observe(name);
        assert_eq!(
            bores(&inventory),
            [(6.0, 1, true)],
            "{name}: {inventory:#?}"
        );
        // The screw hole opens onto two curved faces: not a decided bore yet.
        assert_eq!(count(&inventory, INCOMPLETE), 1, "{name}");
    }
}

#[test]
fn two_cross_holes_through_both_walls_are_four_openings() {
    let inventory = observe("p2-cross-drilled-through.step");
    assert_eq!(bores(&inventory), [(6.0, 4, true)], "{inventory:#?}");
}

#[test]
fn outer_chamfers_of_a_collar_are_exterior_cones() {
    let inventory = observe("p3-chamfered-collar.step");
    assert_eq!(bores(&inventory), [(6.0, 1, true)], "{inventory:#?}");
    assert_eq!(
        count(
            &inventory,
            CircularBoreDisposition::NonMember(CircularBoreNonMember::ExteriorCone)
        ),
        2
    );
}

#[test]
fn a_chamfered_shaft_leaves_a_complete_inventory() {
    let inventory = observe("p6-chamfered-shaft.step");
    assert!(bores(&inventory).is_empty());
    assert!(inventory.candidates.iter().all(|candidate| matches!(
        candidate.disposition,
        CircularBoreDisposition::NonMember(
            CircularBoreNonMember::ExteriorCone | CircularBoreNonMember::ExteriorCylinder
        )
    )));
}

#[test]
fn tangent_branches_windows_and_drill_points_stay_unqualified() {
    let tee = observe("p4-equal-tee.step");
    assert!(bores(&tee).is_empty(), "{tee:#?}");
    assert_eq!(count(&tee, INCOMPLETE), 2);
    let window = observe("p5-window.step");
    assert!(bores(&window).is_empty(), "{window:#?}");
    assert_eq!(count(&window, INCOMPLETE), 1);
    let drill = observe("p7-drill-point.step");
    assert!(bores(&drill).is_empty(), "{drill:#?}");
    assert_eq!(
        count(
            &drill,
            CircularBoreDisposition::Unqualified(CircularBoreUnqualified::UnsupportedSurface)
        ),
        1
    );
}
