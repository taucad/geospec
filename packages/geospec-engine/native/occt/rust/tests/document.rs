use geospec_engine_native_core::backend::BackendErrorKind;
use geospec_engine_native_occt::{BrepSubject, Document, SurfaceFacts, TopologyCounts};
use std::path::PathBuf;

mod occt_goldens;

// The bridge reports TopExp_Explorer traversal multiplicities. Each closed box
// has six one-wire faces, four edge uses per face, and two endpoint uses per
// edge use. Historical unique-entity evidence was 12 edges and 8 vertices per
// box; those counts describe a different topology profile.
fn topology_multiplicities(value: TopologyCounts) -> [usize; 6] {
    [
        value.solids,
        value.shells,
        value.faces,
        value.wires,
        value.edges,
        value.vertices,
    ]
}

fn box_topology_multiplicities(boxes: usize) -> [usize; 6] {
    let faces = boxes * 6;
    let edge_uses = faces * 4;
    [boxes, boxes, faces, faces, edge_uses, edge_uses * 2]
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(path).expect("retained fixture must be readable")
}

// The source shape box: the fold of the whole-face AddOptimal boxes (F6).
// The report shape box carries its tolerance expansion instead.
fn source_box(document: &Document) -> ([f64; 3], [f64; 3]) {
    let faces = BrepSubject::faces(document).unwrap();
    (0..faces.len() as u32)
        .map(|face| document.face_optimal_bounds(face).unwrap())
        .fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(min, max), value| {
                (
                    std::array::from_fn(|axis| min[axis].min(value.min[axis])),
                    std::array::from_fn(|axis| max[axis].max(value.max[axis])),
                )
            },
        )
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

fn close3(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for index in 0..3 {
        close(actual[index], expected[index], tolerance);
    }
}

#[test]
fn ap242_box_retains_shape_face_and_mesh_facts() {
    let document = Document::from_step(&fixture("ap242-box.step")).unwrap();
    let admission = document.admission_facts().unwrap();
    let shape = document.reported_shape().unwrap();
    let faces = BrepSubject::faces(&document).unwrap();

    assert_eq!(admission.source_length_unit, "millimetre");
    close(admission.source_unit_to_millimeters, 1.0, 1e-12);
    assert!(admission.all_source_length_contexts_mm);
    assert!(document.validity().unwrap().valid);
    close(shape.volume, 6_000.0, 1e-8);
    close(shape.surface_area, 2_200.0, 1e-8);
    close3(source_box(&document).0, [-5.0, -10.0, -15.0], 1e-8);
    close3(source_box(&document).1, [5.0, 10.0, 15.0], 1e-8);
    close3(shape.center_of_mass, [0.0, 0.0, 0.0], 1e-8);
    assert_eq!(
        topology_multiplicities(shape.topology),
        box_topology_multiplicities(1)
    );
    assert_eq!(faces.len(), 6);
    assert_eq!(
        faces
            .iter()
            .map(|face| face.facts.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert!(faces
        .iter()
        .all(|face| matches!(&face.facts.surface, SurfaceFacts::Plane { .. })));
    // GNE-OCCT-FACE-ORACLE-01: independent STEPControl_Reader / TopExp unique
    // faces / BRepGProp::SurfaceProperties(false, false), frozen before candidate
    // inspection. This exact runtime-byte oracle replaces the mistaken ideal
    // 200.0 predicate; its failure and the rejected fitted tolerance stay in the
    // implementation run evidence. The analytic checks below remain unchanged.
    // OCCT pin: 3d097a0328e71b826377d4814ab05ec3c3d23871
    // Fixture SHA256: 9d3db06c9381009ceeb4c82742f662becbdfce3d3d2084e46e195901d8b3b9bf
    // Raw query SHA256: f8a814833cd6f9365fa46f0ad4f574f73bf10640101dbe966ca349c8a7f276d7
    // Freeze SHA256: 4fcc2801d218bb949545ff87f501e0605daf8fd3915f0f9fd55ef29a1898ec3a
    let expected_face_areas = vec![
        (0, 0x4082_c000_0000_0000_u64),
        (1, 0x4082_c000_0000_0000),
        (2, 0x4072_c000_0000_0000),
        (3, 0x4072_c000_0000_0000),
        (4, 0x4068_ffff_ffff_ffff),
        (5, 0x4068_ffff_ffff_ffff),
    ];
    assert_eq!(
        faces
            .iter()
            .map(|face| (face.facts.index, face.facts.area.to_bits()))
            .collect::<Vec<_>>(),
        expected_face_areas
    );
    for face in faces.iter() {
        if let SurfaceFacts::Plane { normal, .. } = &face.facts.surface {
            close(
                normal.iter().map(|value| value * value).sum::<f64>(),
                1.0,
                1e-12,
            );
        }
    }
    close(
        faces.iter().map(|face| face.facts.area).sum(),
        2_200.0,
        1e-8,
    );
    assert!(document.source_occurrence_structure().unwrap().is_empty());

    let mesh = document.tessellate(0.1, 0.5).unwrap();
    assert_eq!(mesh.positions.len(), 8);
    assert_eq!(mesh.triangles.len(), 12);
    let mesh_min = [
        mesh.positions
            .iter()
            .map(|point| point[0])
            .reduce(f64::min)
            .unwrap(),
        mesh.positions
            .iter()
            .map(|point| point[1])
            .reduce(f64::min)
            .unwrap(),
        mesh.positions
            .iter()
            .map(|point| point[2])
            .reduce(f64::min)
            .unwrap(),
    ];
    let mesh_max = [
        mesh.positions
            .iter()
            .map(|point| point[0])
            .reduce(f64::max)
            .unwrap(),
        mesh.positions
            .iter()
            .map(|point| point[1])
            .reduce(f64::max)
            .unwrap(),
        mesh.positions
            .iter()
            .map(|point| point[2])
            .reduce(f64::max)
            .unwrap(),
    ];
    close3(mesh_min, source_box(&document).0, 1e-8);
    close3(mesh_max, source_box(&document).1, 1e-8);
    assert!(mesh
        .triangles
        .iter()
        .flatten()
        .all(|index| (*index as usize) < mesh.positions.len()));
}

#[test]
fn mixed_length_contexts_keep_first_mm_metadata_but_refuse_minimum_provenance() {
    let source = String::from_utf8(fixture("ap242-box.step")).unwrap();
    let end = source.rfind("ENDSEC;").unwrap();
    let mut mixed = source.clone();
    mixed.insert_str(end,
        "#500000 = ( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#500001,#347,#348)) REPRESENTATION_CONTEXT('mixed-unit-control','') );\n\
         #500001 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );\n");
    let document = Document::from_step(mixed.as_bytes()).unwrap();
    let admission = document.admission_facts().unwrap();
    assert_eq!(admission.source_unit_to_millimeters, 1.0);
    assert!(!admission.all_source_length_contexts_mm);
}

#[test]
fn inch_cube_preserves_declared_units_and_converted_geometry() {
    let document = Document::from_step(&fixture("inch-cube.step")).unwrap();
    let admission = document.admission_facts().unwrap();
    let shape = document.reported_shape().unwrap();
    let faces = BrepSubject::faces(&document).unwrap();

    assert_eq!(admission.source_length_unit, "INCH");
    close(admission.source_unit_to_millimeters, 25.4, 1e-12);
    assert!(document.validity().unwrap().valid);
    close(shape.volume, 25.4_f64.powi(3), 1e-8);
    close(shape.surface_area, 6.0 * 25.4_f64.powi(2), 1e-8);
    close3(source_box(&document).0, [0.0, 0.0, 0.0], 1e-8);
    close3(source_box(&document).1, [25.4, 25.4, 25.4], 1e-8);
    close3(shape.center_of_mass, [12.7, 12.7, 12.7], 1e-8);
    assert_eq!(
        topology_multiplicities(shape.topology),
        box_topology_multiplicities(1)
    );
    assert_eq!(faces.len(), 6);
    assert!(faces
        .iter()
        .all(|face| matches!(&face.facts.surface, SurfaceFacts::Plane { .. })));
    assert_eq!(document.tessellate(0.1, 0.5).unwrap().triangles.len(), 12);
}

#[test]
fn assembly_retains_occurrence_references_composed_placements_and_bounds() {
    let document = Document::from_step(&fixture("two-cube-assembly.step")).unwrap();
    let admission = document.admission_facts().unwrap();
    let shape = document.reported_shape().unwrap();
    let occurrences = document.source_occurrences().unwrap();

    assert_eq!(admission.source_length_unit, "millimetre");
    assert!(document.validity().unwrap().valid);
    close(shape.volume, 2_000.0, 1e-8);
    close(shape.surface_area, 1_200.0, 1e-8);
    close3(source_box(&document).0, [-5.0, -5.0, -5.0], 1e-8);
    close3(source_box(&document).1, [35.0, 5.0, 5.0], 1e-8);
    close3(shape.center_of_mass, [15.0, 0.0, 0.0], 1e-8);
    assert_eq!(
        topology_multiplicities(shape.topology),
        box_topology_multiplicities(2)
    );
    assert_eq!(occurrences.len(), 2);
    close3(
        [
            occurrences[0].placement[3],
            occurrences[0].placement[7],
            occurrences[0].placement[11],
        ],
        [0.0, 0.0, 0.0],
        1e-12,
    );
    close3(
        [
            occurrences[1].placement[3],
            occurrences[1].placement[7],
            occurrences[1].placement[11],
        ],
        [30.0, 0.0, 0.0],
        1e-12,
    );
    close3(occurrences[0].bounds.min, [-5.0, -5.0, -5.0], 1e-8);
    close3(occurrences[0].bounds.max, [5.0, 5.0, 5.0], 1e-8);
    close3(occurrences[1].bounds.min, [25.0, -5.0, -5.0], 1e-8);
    close3(occurrences[1].bounds.max, [35.0, 5.0, 5.0], 1e-8);
    close(
        occurrences[1].bounds.min[0] - occurrences[0].bounds.max[0],
        20.0,
        1e-8,
    );
    assert_eq!(document.tessellate(0.1, 0.5).unwrap().triangles.len(), 24);
}

#[test]
fn nist_document_retains_bspline_faces() {
    let document = Document::from_step(&fixture("nist-pmi-bspline.step")).unwrap();
    let shape = document.reported_shape().unwrap();
    let faces = BrepSubject::faces(&document).unwrap();

    assert!(document.validity().unwrap().valid);
    assert_eq!(shape.topology.faces, 156);
    assert_eq!(faces.len(), 156);
    assert_eq!(
        faces
            .iter()
            .filter(|face| matches!(&face.facts.surface, SurfaceFacts::Bspline { .. }))
            .count(),
        4
    );
    for face in faces
        .iter()
        .filter(|face| matches!(&face.facts.surface, SurfaceFacts::Bspline { .. }))
    {
        match &face.facts.surface {
            SurfaceFacts::Bspline {
                u_degree,
                v_degree,
                u_poles,
                v_poles,
                u_knots,
                v_knots,
                ..
            } => {
                assert!(*u_degree > 0 && *v_degree > 0);
                assert!(u_poles > u_degree && v_poles > v_degree);
                assert!(*u_knots > 1 && *v_knots > 1);
            }
            _ => unreachable!(),
        }
    }
    assert_eq!(
        document.tessellate(0.1, 0.5).unwrap().triangles.len(),
        occt_goldens::pinned().nist_tessellation_triangles
    );
}

#[test]
fn truncated_step_is_a_typed_admission_failure() {
    let error = match Document::from_step(&fixture("truncated.step")) {
        Ok(_) => panic!("truncated STEP must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.kind, BackendErrorKind::InvalidInput);
    assert_eq!(error.message, "STEP read failed.");
}

/// S10 Z01: a record the parser drops, or a geometry or topology parameter
/// that names a record of the wrong type, leaves a null that transfer
/// dereferenced (a native SIGSEGV, a WASM "null function" trap) or a shape
/// silently missing a face. The read refuses with the first fail before
/// transfer.
#[test]
fn step_with_a_dropped_record_is_a_typed_read_failure() {
    for (name, message) in [
        (
            "ap242-box-dropped-point.step",
            "STEP read failed: Unresolved Reference, Ent.Id.#171 Param.n0 4 (Id.#174)",
        ),
        (
            "ap242-box-dropped-face-bound.step",
            "STEP read failed: Undefined Parsing: Line 283: Incorrect syntax: unexpected QUID, expecting '('",
        ),
        (
            "ap242-box-split-record.step",
            "STEP read failed: Undefined Parsing: Line 262: Incorrect syntax: unexpected IDENT, expecting ENDSEC or ENTITY",
        ),
        (
            "ap242-box-direction-typo.step",
            "STEP read failed: #62 StepGeom_Vector: Parameter n0.2 (orientation) : Entity has illegal type",
        ),
        (
            "ap242-box-shell-names-bound.step",
            "STEP read failed: #16 StepShape_ClosedShell: Parameter n0.1 (face) : Entity has illegal type",
        ),
        (
            "ap242-box-bound-names-edge.step",
            "STEP read failed: #18 StepShape_FaceBound: Parameter n0.2 (bound) : Entity has illegal type",
        ),
    ] {
        let error = match Document::from_step(&fixture(name)) {
            Ok(_) => panic!("{name} must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind, BackendErrorKind::InvalidInput, "{name}");
        assert_eq!(error.message, message, "{name}");
    }
}

/// `fixture` with `from`, which it holds exactly once, replaced by `to`.
fn mutated(name: &str, from: &str, to: &str) -> Vec<u8> {
    let source = String::from_utf8(fixture(name)).unwrap();
    assert_eq!(source.matches(from).count(), 1, "{name}: {from}");
    source.replace(from, to).into_bytes()
}

/// A parameter any entity could not read refuses, whatever its schema,
/// unless the bridge admits that exact diagnostic and the record's raw
/// parameter proves the model holds the file's value: `$` for a null, an
/// integral REAL for a rounded INTEGER. The NIST PMI file raises only
/// admitted diagnostics; the same diagnostics over a value the reader
/// dropped or changed refuse.
#[test]
fn step_entity_read_fails_refuse_unless_the_model_holds_the_value() {
    assert!(Document::from_step(&fixture("nist-pmi-bspline.step")).is_ok());
    for (name, from, to, message) in [
        (
            "ap242-box.step",
            "#7 = PRODUCT('COMPOUND','COMPOUND','',(#8));",
            "#7 = PRODUCT('COMPOUND','COMPOUND','',(#9));",
            "STEP read failed: #7 StepBasic_Product: Parameter n0.1 (product_context) : Entity has illegal type",
        ),
        (
            "nist-pmi-bspline.step",
            "#15=GENERAL_PROPERTY_ASSOCIATION('',$,#27,#1090);",
            "#15=GENERAL_PROPERTY_ASSOCIATION('',5,#27,#1090);",
            "STEP read failed: #15 StepBasic_GeneralPropertyAssociation: Parameter n0.2 (description) not a quoted String",
        ),
        (
            "nist-pmi-bspline.step",
            "#39=INTEGER_REPRESENTATION_ITEM('number of annotations',22.);",
            "#39=INTEGER_REPRESENTATION_ITEM('number of annotations',22.5);",
            "STEP read failed: #39 StepRepr_IntegerRepresentationItem: Parameter n0.2 (value) was rounded",
        ),
        (
            "two-cube-assembly.step",
            "#604=ITEM_DEFINED_TRANSFORMATION('','',#513,#514);",
            "#604=ITEM_DEFINED_TRANSFORMATION(5,$,#513,#514);",
            "STEP read failed: #604 StepRepr_ItemDefinedTransformation: Parameter n0.1 (name) not a quoted String",
        ),
        (
            "two-cube-assembly.step",
            "#603=(REPRESENTATION_RELATIONSHIP('','',#10,#505)",
            "#603=(REPRESENTATION_RELATIONSHIP(5,$,#10,#505)",
            "STEP read failed: #603 StepRepr_ShapeRepresentationRelationshipWithTransformation: Parameter n0.1 (name) not a quoted String",
        ),
        (
            "two-cube-assembly.step",
            "#601=PRODUCT_DEFINITION_SHAPE('Placement','Placement of cubeA',#600);",
            "#601=PRODUCT_DEFINITION_SHAPE(5,$,#600);",
            "STEP read failed: #601 StepRepr_ProductDefinitionShape: Parameter n0.1 (property_definition.name) not a quoted String",
        ),
    ] {
        let error = match Document::from_step(&mutated(name, from, to)) {
            Ok(_) => panic!("{to} must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind, BackendErrorKind::InvalidInput, "{to}");
        assert_eq!(error.message, message, "{to}");
    }
}

/// Autodesk's STEP translator writes an unset mandatory name, `$`, on every
/// assembly placement: `ITEM_DEFINED_TRANSFORMATION($,$,…)`, the
/// `REPRESENTATION_RELATIONSHIP($,$,…)` part of its complex record and the
/// occurrence's `PRODUCT_DEFINITION_SHAPE($,$,…)`. The bridge admits all
/// three, and the assembly keeps the placements, bounds and properties of the
/// file with quoted names.
#[test]
fn unset_placement_names_keep_the_assembly() {
    let quoted = Document::from_step(&fixture("two-cube-assembly.step")).unwrap();
    let mut source = String::from_utf8(fixture("two-cube-assembly.step")).unwrap();
    for (from, to, count) in [
        (
            "ITEM_DEFINED_TRANSFORMATION('',''",
            "ITEM_DEFINED_TRANSFORMATION($,$",
            2,
        ),
        (
            "(REPRESENTATION_RELATIONSHIP('',''",
            "(REPRESENTATION_RELATIONSHIP($,$",
            2,
        ),
        (
            "PRODUCT_DEFINITION_SHAPE('Placement','Placement of cubeA'",
            "PRODUCT_DEFINITION_SHAPE($,$",
            1,
        ),
        (
            "PRODUCT_DEFINITION_SHAPE('Placement','Placement of cubeB'",
            "PRODUCT_DEFINITION_SHAPE($,$",
            1,
        ),
    ] {
        assert_eq!(source.matches(from).count(), count, "{from}");
        source = source.replace(from, to);
    }
    let unset = Document::from_step(source.as_bytes()).unwrap();

    let (expected, actual) = (
        quoted.reported_shape().unwrap(),
        unset.reported_shape().unwrap(),
    );
    assert_eq!(actual.volume, expected.volume);
    assert_eq!(actual.surface_area, expected.surface_area);
    assert_eq!(actual.center_of_mass, expected.center_of_mass);
    assert_eq!(source_box(&unset), source_box(&quoted));
    let (expected, actual) = (
        quoted.source_occurrences().unwrap(),
        unset.source_occurrences().unwrap(),
    );
    assert_eq!(actual.len(), 2);
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        assert_eq!(actual.placement, expected.placement);
    }
}

/// ENGINE-FIX-2 ruling 1 (I6): an admitted OCCT schema gap is counted, never
/// silent. CTC-05 targets 40 PROPERTY_DEFINITIONs at
/// CHARACTERIZED_ITEM_WITHIN_REPRESENTATION, outside OCCT's definition
/// select, and holds one COMPOUND_REPRESENTATION_ITEM (#14000). Its
/// presentation styles are CURVE_STYLE and SURFACE_STYLE_USAGE, which OCCT
/// reads, so it raises no PRESENTATION_STYLE_ASSIGNMENT gap.
#[test]
fn nist_admission_counts_each_admitted_schema_gap() {
    let document = Document::from_step(&fixture("nist-pmi-bspline.step")).unwrap();
    let gaps = document.admission_facts().unwrap().schema_gaps;
    let kinds = gaps
        .iter()
        .map(|gap| (gap.entity, gap.attribute))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            ("PROPERTY_DEFINITION", "definition"),
            ("COMPOUND_REPRESENTATION_ITEM", "item_element"),
        ],
        "{gaps:?}"
    );
    // 59 PROPERTY_DEFINITION records in the file bound the first count.
    assert!((1..=59).contains(&gaps[0].count), "{gaps:?}");
    assert_eq!(gaps[1].count, 1, "{gaps:?}");
    // A file that raises no schema gap reports none.
    assert!(Document::from_step(&fixture("ap242-box.step"))
        .unwrap()
        .admission_facts()
        .unwrap()
        .schema_gaps
        .is_empty());
}

#[test]
fn external_step_resource_is_refused_before_transfer() {
    let error = match Document::from_step(&fixture("external-reference.step")) {
        Ok(_) => panic!("STEP external resource must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.kind, BackendErrorKind::Unsupported);
    assert_eq!(
        error.message,
        "STEP external file references are unsupported by byte-only admission."
    );
}
