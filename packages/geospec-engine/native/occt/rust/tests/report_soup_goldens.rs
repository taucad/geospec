//! Wave 0 report-soup goldens for the mesh authority fixtures: the STEP
//! fixtures of the performance-lab authority set that live in this crate. The
//! six m3 mesh-family authority cases (`toBeWatertight`, `toHaveMeshIntegrity`
//! and `toHaveConnectedComponents`, positive and negative) all measure the
//! `component-interference/original.step` report soup. The pinned soups are
//! written by `examples/write_occt_goldens.rs` in a clean environment.

mod occt_goldens;

#[test]
fn report_soup_matches_the_pinned_generation_on_the_mesh_authority_fixtures() {
    let soups = occt_goldens::pinned().report_soups;
    assert_eq!(
        soups
            .iter()
            .map(|soup| (soup.fixture.as_str(), soup.id.as_str()))
            .collect::<Vec<_>>(),
        occt_goldens::SOUP_FIXTURES
    );
    let mut failures = Vec::new();
    for soup in soups {
        let observed = occt_goldens::report_soup(&soup.fixture);
        if observed != (soup.triangles, soup.fnv1a64.clone()) {
            failures.push(format!(
                "{} ({}): soup {} triangles {} != pinned {} {}",
                soup.id, soup.fixture, observed.0, observed.1, soup.triangles, soup.fnv1a64
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
