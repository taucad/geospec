//! P5 on the product path: the facts layer's bracket box over the engine's
//! own admitted document, against the bridge's `AddOptimal` box.

use geospec_engine_native_occt::{Bounds, BracketBounds, BrepSubject, Document, ShapeParts};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("retained fixture must be readable")
}

fn bracket(name: &str) -> BracketBounds {
    let document = Document::from_step(&fixture(name)).unwrap();
    document.bracket_bounds().unwrap().unwrap()
}

fn sides(bounds: Bounds) -> [f64; 6] {
    let [a, b, c] = bounds.min;
    let [d, e, f] = bounds.max;
    [-a, -b, -c, d, e, f]
}

/// Every side: the attained value within the outer one, the outer value
/// within the tolerant one, and the true side (`truth`) between the
/// attained and the outer value.
fn assert_brackets(name: &str, value: &BracketBounds, truth: Bounds, width: f64) {
    let (inner, outer, tolerant, truth) = (
        sides(value.inner),
        sides(value.outer),
        sides(value.outer_tolerant),
        sides(truth),
    );
    for side in 0..6 {
        assert!(inner[side] <= truth[side], "{name} side {side}: {value:?}");
        assert!(truth[side] <= outer[side], "{name} side {side}: {value:?}");
        assert!(
            outer[side] - inner[side] <= width,
            "{name} side {side}: {value:?}"
        );
        assert!(
            outer[side] <= tolerant[side],
            "{name} side {side}: {value:?}"
        );
        assert!(
            tolerant[side] - outer[side] >= value.kernel_tolerance,
            "{name} side {side}: {value:?}"
        );
    }
}

#[test]
fn analytic_solids_box_within_their_bracket_of_the_closed_form() {
    // A 10 x 20 x 30 block and a cylinder of radius 1 and height 10 about z.
    let block = bracket("ap242-box.step");
    let document = Document::from_step(&fixture("ap242-box.step")).unwrap();
    let optimal = document
        .reported_shape_parts(ShapeParts::BOUNDS)
        .unwrap()
        .bounds;
    assert_brackets("block", &block, optimal, 1e-7 * 30.0);
    let cylinder = bracket("ap242-radius1-height10.step");
    let truth = Bounds {
        min: [-1.0, -1.0, 0.0],
        max: [1.0, 1.0, 10.0],
    };
    assert_brackets("cylinder", &cylinder, truth, 1e-7 * 10.0);
}

#[test]
fn a_bracket_is_the_same_on_every_call_and_document() {
    let first = bracket("ap242-radius1-height10.step");
    let document = Document::from_step(&fixture("ap242-radius1-height10.step")).unwrap();
    let again = document.bracket_bounds().unwrap().unwrap();
    let repeat = document.bracket_bounds().unwrap().unwrap();
    let bits = |b: &BracketBounds| {
        [b.inner, b.outer, b.outer_tolerant]
            .iter()
            .flat_map(|x| x.min.iter().chain(&x.max).map(|v| v.to_bits()))
            .chain([b.kernel_tolerance.to_bits()])
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&first), bits(&again));
    assert_eq!(bits(&again), bits(&repeat));
}

#[test]
fn geometry_outside_solids_refuses_the_bracket_with_its_counts() {
    let document = Document::from_step(&fixture("bounds-free-edges.step")).unwrap();
    let refusal = document.bracket_bounds().unwrap().unwrap_err();
    assert_eq!(refusal.code, "free-geometry", "{refusal:?}");
    assert!(refusal.detail.contains("outside"), "{refusal:?}");
    assert_eq!((refusal.body, refusal.face), (None, None));
    // The bridge's box is untouched by the attempt.
    assert!(document.reported_shape_parts(ShapeParts::BOUNDS).is_ok());
}
