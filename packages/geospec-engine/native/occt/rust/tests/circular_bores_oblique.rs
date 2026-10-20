//! W5b oblique mouths: a bore whose rim is not a planar coaxial circle, as a
//! cross hole through a pin or a hole through a sloped face leaves it, opens
//! when the face across leaves the whole rim away from the axis at an angle.
//! A curved floor that closes the bore and a hole tangent to the wall stay
//! unqualified (fixture provenance in
//! `fixtures/circular-bores-oblique/provenance.json`).
use geospec_engine_native_core::backend::brep::{
    BrepSubject, CircularBoreDisposition, CircularBoreInventory, CircularBoreTermination,
    CircularBoreUnqualified, SurfaceFacts,
};
use geospec_engine_native_occt::Document;
use std::path::PathBuf;
use CircularBoreTermination::{Mouth, ObliqueMouth, PlanarDiskBottom};

fn observe(name: &str) -> CircularBoreInventory {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/circular-bores-oblique")
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

/// Every qualified bore as (radius, side openings, terminations from `from` to `to`).
fn bores(inventory: &CircularBoreInventory) -> Vec<(f64, u32, [CircularBoreTermination; 2])> {
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
                    topology.ends.map(|end| end.termination),
                ))
            }
            _ => None,
        })
        .collect()
}

fn unqualified(inventory: &CircularBoreInventory) -> Vec<CircularBoreUnqualified> {
    inventory
        .candidates
        .iter()
        .filter_map(|candidate| match candidate.disposition {
            CircularBoreDisposition::Unqualified(reason) => Some(reason),
            _ => None,
        })
        .collect()
}

#[test]
fn a_cross_hole_through_a_pin_opens_at_both_saddles() {
    for name in ["o0-clevis-pin.step", "o1-offset-cross-hole.step"] {
        let inventory = observe(name);
        assert_eq!(
            bores(&inventory),
            [(1.5, 0, [ObliqueMouth, ObliqueMouth])],
            "{name}: {inventory:#?}"
        );
        assert!(unqualified(&inventory).is_empty(), "{name}");
    }
}

#[test]
fn a_blind_side_hole_opens_at_its_saddle() {
    let inventory = observe("o2-blind-side-hole.step");
    let found = bores(&inventory);
    assert_eq!(found.len(), 1, "{inventory:#?}");
    let (radius, openings, mut ends) = found[0];
    ends.sort_by_key(|end| *end as u8);
    assert_eq!((radius, openings), (1.5, 0));
    assert_eq!(ends, [PlanarDiskBottom, ObliqueMouth]);
}

#[test]
fn a_hole_through_a_slope_opens_on_the_slope() {
    let through = observe("o3-hole-through-slope.step");
    assert_eq!(
        bores(&through),
        [(3.0, 0, [Mouth, ObliqueMouth])],
        "{through:#?}"
    );
    let blind = observe("o5-hole-into-slope-blind.step");
    assert_eq!(
        bores(&blind),
        [(3.0, 0, [PlanarDiskBottom, ObliqueMouth])],
        "{blind:#?}"
    );
}

#[test]
fn a_hole_into_a_cross_bore_opens_into_it() {
    let inventory = observe("o7-into-cross-bore.step");
    let mut found = bores(&inventory);
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(found.len(), 2, "{inventory:#?}");
    assert_eq!((found[0].0, found[0].1), (1.5, 0));
    assert!(found[0].2.contains(&ObliqueMouth) && found[0].2.contains(&Mouth));
    assert_eq!(found[1], (4.0, 1, [Mouth, Mouth]));
}

#[test]
fn a_curved_floor_and_a_tangent_hole_stay_unqualified() {
    let floor = observe("o6-curved-floor.step");
    assert!(bores(&floor).is_empty(), "{floor:#?}");
    assert_eq!(
        unqualified(&floor),
        [CircularBoreUnqualified::UnsupportedTermination]
    );
    let tangent = observe("o4-tangent-cross-hole.step");
    assert!(bores(&tangent).is_empty(), "{tangent:#?}");
    assert!(!unqualified(&tangent).is_empty());
}
