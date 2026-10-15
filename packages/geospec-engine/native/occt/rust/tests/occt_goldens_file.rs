//! The generated OCCT goldens file: its bytes are the generator's rendering,
//! and the campaign subject's centre of mass equals it bit for bit, so every
//! producer measures the point the WASM build does.

mod occt_goldens;

#[test]
fn the_goldens_file_is_the_generator_rendering() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(occt_goldens::PATH);
    let written = std::fs::read_to_string(path).unwrap();
    assert_eq!(written, occt_goldens::render(&occt_goldens::pinned()));
}

#[test]
fn the_campaign_center_of_mass_matches_the_generated_bits() {
    let pinned = occt_goldens::pinned().center_of_mass;
    assert_eq!(pinned.fixture, occt_goldens::CENTER_OF_MASS_FIXTURE);
    assert_eq!(occt_goldens::observe().center_of_mass, pinned);
}
