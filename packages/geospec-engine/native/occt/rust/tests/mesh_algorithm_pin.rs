//! The bridge pins the Watson mesher, so `CSF_MeshAlgo` cannot move the soup
//! (GeoSpec policy §16). This binary holds one test because it mutates the
//! process environment that OCCT reads on every mesh.

mod occt_goldens;

/// This target's generated report soups (`CSF_MeshAlgo` unset): triangles and
/// FNV-1a 64 of position bits, then indices. With `delabella` exported, M0 gave
/// `0x0df9_ed6e_9c74_92e5` for the PMI source.
#[test]
fn report_soup_ignores_a_delabella_csf_mesh_algo() {
    let pinned: Vec<_> = occt_goldens::pinned_soups()
        .into_iter()
        .map(|soup| (soup.fixture, (soup.triangles, soup.fnv1a64)))
        .collect();
    std::env::remove_var("CSF_MeshAlgo");
    let clean: Vec<_> = pinned
        .iter()
        .map(|(name, _)| (name.clone(), occt_goldens::report_soup(name)))
        .collect();
    std::env::set_var("CSF_MeshAlgo", "delabella");
    let delabella: Vec<_> = pinned
        .iter()
        .map(|(name, _)| (name.clone(), occt_goldens::report_soup(name)))
        .collect();
    std::env::remove_var("CSF_MeshAlgo");
    assert_eq!(clean, pinned);
    assert_eq!(delabella, pinned);
}
