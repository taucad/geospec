//! T-PF-01: which STEP name labels an occurrence when the product, its
//! next-assembly-usage occurrence (NAUO) and its shape representation all
//! carry different names. The precedence is the bridge's and OCCT's
//! `STEPCAFControl_Reader::ReadNames`, pinned here so a change is deliberate:
//!
//! - `instance_name`: NAUO description, else NAUO name, else NAUO id; absent
//!   when all three are empty.
//! - `path` segment: the instance name, else the product's XCAF name (PRODUCT
//!   name, else PRODUCT id).
//! - `product_name`: PRODUCT id, else PRODUCT name.
//! - The shape representation's name labels nothing.

use geospec_engine_native_occt::{BrepSubject, Document};

/// The two-cube assembly with every naming slot of both leaves replaced.
/// `a` and `b` are each `[product id, product name, representation name,
/// NAUO id, NAUO name, NAUO description]`.
fn renamed(a: [&str; 6], b: [&str; 6]) -> Vec<u8> {
    let mut text =
        String::from_utf8(include_bytes!("fixtures/two-cube-assembly.step").to_vec()).unwrap();
    for (from, to) in [
        (
            "#4=PRODUCT('cubeA','cubeA',",
            format!("#4=PRODUCT('{}','{}',", a[0], a[1]),
        ),
        (
            "#10=ADVANCED_BREP_SHAPE_REPRESENTATION('cubeA',",
            format!("#10=ADVANCED_BREP_SHAPE_REPRESENTATION('{}',", a[2]),
        ),
        (
            "#600=NEXT_ASSEMBLY_USAGE_OCCURRENCE('NAUO1','cubeA','cubeA',",
            format!(
                "#600=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{}','{}','{}',",
                a[3], a[4], a[5]
            ),
        ),
        (
            "#2004=PRODUCT('cubeB','cubeB',",
            format!("#2004=PRODUCT('{}','{}',", b[0], b[1]),
        ),
        (
            "#2010=ADVANCED_BREP_SHAPE_REPRESENTATION('cubeB',",
            format!("#2010=ADVANCED_BREP_SHAPE_REPRESENTATION('{}',", b[2]),
        ),
        (
            "#610=NEXT_ASSEMBLY_USAGE_OCCURRENCE('NAUO2','cubeB','cubeB',",
            format!(
                "#610=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{}','{}','{}',",
                b[3], b[4], b[5]
            ),
        ),
    ] {
        assert_eq!(text.matches(from).count(), 1, "{from}");
        text = text.replace(from, &to);
    }
    text.into_bytes()
}

/// `(path, instance_name, product_name)` of each leaf occurrence, ordered by
/// product name so the assertion does not depend on component order.
fn leaves(source: &[u8]) -> Vec<(String, Option<String>, String)> {
    let document = Document::from_step(source).expect("OCCT imports the renamed assembly");
    let rows = document.source_occurrence_structure().unwrap();
    let mut leaves: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| !rows.iter().any(|row| row.parent == Some(*index as u32)))
        .map(|(_, row)| {
            (
                row.path.clone(),
                row.instance_name.clone(),
                row.product_name.clone(),
            )
        })
        .collect();
    leaves.sort_by(|left, right| left.2.cmp(&right.2));
    leaves
}

#[test]
fn nauo_description_labels_the_path_and_product_id_the_product() {
    let rows = leaves(&renamed(
        ["pid-A", "pname-A", "rep-A", "nid-A", "nname-A", "ndesc-A"],
        ["pid-B", "pname-B", "rep-B", "", "", ""],
    ));
    assert_eq!(
        rows,
        vec![
            ("ndesc-A".into(), Some("ndesc-A".into()), "pid-A".into()),
            // An unnamed NAUO falls back to the product's XCAF name, which
            // OCCT takes from PRODUCT name before id.
            ("pname-B".into(), None, "pid-B".into()),
        ]
    );
}

#[test]
fn nauo_name_then_id_and_product_name_without_id() {
    let rows = leaves(&renamed(
        ["", "pname-A", "rep-A", "nid-A", "nname-A", ""],
        ["", "pname-B", "rep-B", "nid-B", "", ""],
    ));
    assert_eq!(
        rows,
        vec![
            ("nname-A".into(), Some("nname-A".into()), "pname-A".into()),
            ("nid-B".into(), Some("nid-B".into()), "pname-B".into()),
        ]
    );
}

#[test]
fn shape_representation_names_label_nothing() {
    let rows = leaves(&renamed(
        ["pid-A", "pname-A", "rep-A", "", "", ""],
        ["pid-B", "pname-B", "rep-B", "", "", ""],
    ));
    for (path, instance, product) in &rows {
        assert!(!path.contains("rep-"), "{rows:?}");
        assert!(instance.is_none(), "{rows:?}");
        assert!(!product.contains("rep-"), "{rows:?}");
    }
}
