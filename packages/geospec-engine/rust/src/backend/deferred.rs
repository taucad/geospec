//! A STEP document opened on first demand.
//!
//! A subject admitted from a stored admission record has its identity and
//! admission facts without reading the file. Its document is opened from the
//! retained source bytes by the same connector the first time a query needs
//! the shape, and every query is then the opened document's own. A subject
//! whose claims are all answered from the persisted facts store never opens
//! it.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use super::brep::*;
use super::{pmi, BackendError, BackendErrorKind, TriangleMesh};
use crate::protocol::{Observations, WorkCounter};

pub(crate) struct DeferredBrep {
    // Declared first, so the document drops before the connector it came from.
    document: OnceCell<Result<Box<dyn BrepSubject>, BackendError>>,
    source: RefCell<Option<Vec<u8>>>,
    connector: Rc<dyn BrepConnector>,
    observations: Rc<Observations>,
}

impl DeferredBrep {
    pub(crate) fn new(
        connector: Rc<dyn BrepConnector>,
        source: Vec<u8>,
        observations: Rc<Observations>,
    ) -> Self {
        Self {
            document: OnceCell::new(),
            source: RefCell::new(Some(source)),
            connector,
            observations,
        }
    }

    /// The opened document. An open that fails stays failed: every later
    /// query returns the same error.
    fn open(&self) -> Result<&dyn BrepSubject, BackendError> {
        let opened = self.document.get_or_init(|| {
            let Some(source) = self.source.borrow_mut().take() else {
                return Err(BackendError {
                    kind: BackendErrorKind::ComputationFailed,
                    message: "The deferred STEP source is unavailable.".into(),
                });
            };
            self.observations.add(WorkCounter::Parses, 1);
            self.connector.open_step(&source)
        });
        match opened {
            Ok(document) => Ok(document.as_ref()),
            Err(error) => Err(error.clone()),
        }
    }

    /// The retained source bytes until the document opens.
    #[cfg(test)]
    fn pending_source_bytes(&self) -> usize {
        self.source.borrow().as_ref().map_or(0, Vec::len)
    }
}

impl BrepSubject for DeferredBrep {
    fn occurrence_minimum_distance(
        &self,
        a: u32,
        b: u32,
        charge: &mut Charge<'_>,
    ) -> Result<Option<BrepMinimumDistance>, BackendError> {
        self.open()?.occurrence_minimum_distance(a, b, charge)
    }

    fn occurrence_separation(
        &self,
        a: u32,
        b: u32,
        charge: &mut Charge<'_>,
    ) -> Result<Option<OccurrenceSeparation>, BackendError> {
        self.open()?.occurrence_separation(a, b, charge)
    }

    fn void_witness(
        &self,
        materials: &[u32],
        points: &[[f64; 3]],
        charge: &mut Charge<'_>,
    ) -> Result<Option<VoidWitness>, BackendError> {
        self.open()?.void_witness(materials, points, charge)
    }

    fn finite_contact_face(&self, face: BrepEntity) -> Result<FiniteContactFace, BackendError> {
        self.open()?.finite_contact_face(face)
    }

    fn admission_facts(&self) -> Result<BrepAdmissionFacts, BackendError> {
        self.open()?.admission_facts()
    }

    fn step_subject_metadata(&self) -> Result<Option<StepSubjectMetadata>, BackendError> {
        self.open()?.step_subject_metadata()
    }

    fn reported_mesh(&self) -> Result<TriangleMesh, BackendError> {
        self.open()?.reported_mesh()
    }

    fn reported_shape(&self) -> Result<ShapeFacts, BackendError> {
        self.open()?.reported_shape()
    }

    fn reported_shape_parts(&self, parts: ShapeParts) -> Result<ShapeFacts, BackendError> {
        self.open()?.reported_shape_parts(parts)
    }

    fn bracket_bounds(&self) -> Result<Result<BracketBounds, BracketRefusal>, BackendError> {
        self.open()?.bracket_bounds()
    }

    fn mass_facts(&self) -> Result<MassOutcome, BackendError> {
        self.open()?.mass_facts()
    }

    fn reported_faces(&self, measured: bool) -> Result<ReportedFaces, BackendError> {
        self.open()?.reported_faces(measured)
    }

    fn document_rows(&self) -> Result<DocumentRows, BackendError> {
        self.open()?.document_rows()
    }

    fn face_optimal_bounds(&self, public_face: u32) -> Result<Bounds, BackendError> {
        self.open()?.face_optimal_bounds(public_face)
    }

    fn cylinder_axial_extent(&self, face: BrepEntity) -> Result<CylinderAxialExtent, BackendError> {
        self.open()?.cylinder_axial_extent(face)
    }

    fn nominal_cylindrical_band(
        &self,
        face: BrepEntity,
    ) -> Result<NominalCylindricalBand, BackendError> {
        self.open()?.nominal_cylindrical_band(face)
    }

    fn circular_bores(&self, max_candidates: usize) -> Result<CircularBoreInventory, BackendError> {
        self.open()?.circular_bores(max_candidates)
    }

    fn selected_interference_material(
        &self,
        face: BrepEntity,
    ) -> Result<SelectedInterferenceMaterial, BackendError> {
        self.open()?.selected_interference_material(face)
    }

    fn selected_bore_void(&self, face: BrepEntity) -> Result<SelectedBoreVoid, BackendError> {
        self.open()?.selected_bore_void(face)
    }

    fn edge_treatment_counts(&self) -> Result<EdgeTreatmentCounts, BackendError> {
        self.open()?.edge_treatment_counts()
    }

    fn edge_treatments(&self, max_rows: usize) -> Result<EdgeTreatmentInventory, BackendError> {
        self.open()?.edge_treatments(max_rows)
    }

    /// The opened document's memo; an empty one when the open failed, which
    /// no memoized query reads because each returns the open's error first.
    fn operand_memo(&self) -> OperandMemo {
        match self.open() {
            Ok(document) => document.operand_memo(),
            Err(_) => Box::new(()),
        }
    }

    fn selected_interference_material_memoized(
        &self,
        face: BrepEntity,
        memo: &mut OperandMemo,
    ) -> Result<SelectedInterferenceMaterial, BackendError> {
        self.open()?
            .selected_interference_material_memoized(face, memo)
    }

    fn selected_bore_void_memoized(
        &self,
        face: BrepEntity,
        memo: &mut OperandMemo,
    ) -> Result<SelectedBoreVoid, BackendError> {
        self.open()?.selected_bore_void_memoized(face, memo)
    }

    fn regular_solid_containment_memoized(
        &self,
        subject: BrepEntity,
        target: BrepEntity,
        memo: &mut OperandMemo,
    ) -> Result<RegularSolidContainment, BackendError> {
        self.open()?
            .regular_solid_containment_memoized(subject, target, memo)
    }

    fn occurrence_overlap_memoized(
        &self,
        left: u32,
        right: u32,
        tolerance: f64,
        memo: &mut OperandMemo,
        charge: &mut Charge<'_>,
    ) -> Result<Option<OccurrenceOverlap>, BackendError> {
        self.open()?
            .occurrence_overlap_memoized(left, right, tolerance, memo, charge)
    }

    fn regular_solid_containment(
        &self,
        subject: BrepEntity,
        target: BrepEntity,
    ) -> Result<RegularSolidContainment, BackendError> {
        self.open()?.regular_solid_containment(subject, target)
    }

    fn continuous_wall_domain(
        &self,
        entity: BrepEntity,
    ) -> Result<ContinuousWallDomain, BackendError> {
        self.open()?.continuous_wall_domain(entity)
    }

    fn selected_continuous_domain(
        &self,
        occurrence: u32,
    ) -> Result<SelectedContinuousDomain, BackendError> {
        self.open()?.selected_continuous_domain(occurrence)
    }

    fn resolve_source_face(&self, key: &SourceFaceKey) -> Result<ResolvedSourceFace, BackendError> {
        self.open()?.resolve_source_face(key)
    }

    fn source_occurrences(&self) -> Result<Rc<[OccurrenceFacts]>, BackendError> {
        self.open()?.source_occurrences()
    }

    fn source_occurrence_structure(&self) -> Result<Rc<[OccurrenceFacts]>, BackendError> {
        self.open()?.source_occurrence_structure()
    }

    fn occurrence_box_units(&self, occurrence: u32) -> Result<u64, BackendError> {
        self.open()?.occurrence_box_units(occurrence)
    }

    fn occurrence_bounds(&self, occurrence: u32) -> Result<Bounds, BackendError> {
        self.open()?.occurrence_bounds(occurrence)
    }

    fn pmi_source_faces(
        &self,
        source_face_id: u32,
    ) -> Result<pmi::PmiField<Vec<pmi::PmiFaceAssociation>>, BackendError> {
        self.open()?.pmi_source_faces(source_face_id)
    }

    fn faces(&self) -> Result<Rc<[LocatedFace]>, BackendError> {
        self.open()?.faces()
    }

    fn validity(&self) -> Result<Rc<ValidityFacts>, BackendError> {
        self.open()?.validity()
    }

    fn closure(&self, charge: &mut Charge<'_>) -> Result<Option<Rc<ClosureFacts>>, BackendError> {
        self.open()?.closure(charge)
    }

    fn component_bodies(
        &self,
        occurrences: &[u32],
        charge: &mut Charge<'_>,
    ) -> Result<Option<Box<dyn ComponentBodies + '_>>, BackendError> {
        self.open()?.component_bodies(occurrences, charge)
    }

    fn classify_face_points(
        &self,
        face: BrepEntity,
        points: &[[f64; 3]],
        tolerance_mm: f64,
    ) -> Result<Vec<PointState>, BackendError> {
        self.open()?
            .classify_face_points(face, points, tolerance_mm)
    }

    fn tessellate(
        &self,
        entity: BrepEntity,
        profile: TessellationProfile,
    ) -> Result<Rc<TriangleMesh>, BackendError> {
        self.open()?.tessellate(entity, profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Every `BrepSubject` method the trait declares must be forwarded, or a
    /// deferred subject would answer a defaulted method with the trait's
    /// `Unsupported` default instead of the opened document's answer.
    #[test]
    fn every_trait_method_is_forwarded() {
        let trait_source = include_str!("brep.rs");
        let start = trait_source.find("pub trait BrepSubject {").expect("trait");
        let body = &trait_source[start..];
        let end = body.find("\n}\n").expect("trait end");
        let declared = method_names(&body[..end]);
        let forwarded = method_names(include_str!("deferred.rs"));
        assert!(declared.len() > 30, "{declared:?}");
        for name in &declared {
            assert!(forwarded.contains(name), "{name} is not forwarded");
        }
    }

    fn method_names(source: &str) -> Vec<String> {
        source
            .split("fn ")
            .skip(1)
            .filter_map(|rest| {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            })
            .collect()
    }

    struct Connector {
        opens: Rc<Cell<u32>>,
        fail: bool,
    }

    impl BrepConnector for Connector {
        fn identity_profile(&self) -> BrepIdentityProfile {
            BrepIdentityProfile {
                ingest_profile: "deferred-test",
                backend_profile: "deferred-test",
            }
        }

        fn open_step(&self, bytes: &[u8]) -> Result<Box<dyn BrepSubject>, BackendError> {
            self.opens.set(self.opens.get() + 1);
            if self.fail {
                return Err(BackendError {
                    kind: BackendErrorKind::InvalidInput,
                    message: "unreadable".into(),
                });
            }
            Ok(Box::new(Document(bytes.len())))
        }
    }

    struct Document(usize);

    impl BrepSubject for Document {
        fn faces(&self) -> Result<Rc<[LocatedFace]>, BackendError> {
            Ok(Rc::from(Vec::new()))
        }

        fn validity(&self) -> Result<Rc<ValidityFacts>, BackendError> {
            Err(BackendError {
                kind: BackendErrorKind::ComputationFailed,
                message: format!("{} bytes", self.0),
            })
        }

        fn classify_face_points(
            &self,
            _face: BrepEntity,
            _points: &[[f64; 3]],
            _tolerance_mm: f64,
        ) -> Result<Vec<PointState>, BackendError> {
            Ok(Vec::new())
        }

        fn tessellate(
            &self,
            _entity: BrepEntity,
            _profile: TessellationProfile,
        ) -> Result<Rc<TriangleMesh>, BackendError> {
            Err(BackendError {
                kind: BackendErrorKind::Unsupported,
                message: "no mesh".into(),
            })
        }
    }

    fn deferred(fail: bool) -> (DeferredBrep, Rc<Cell<u32>>, Rc<Observations>) {
        let opens = Rc::new(Cell::new(0));
        let observations = Rc::new(Observations::default());
        let brep = DeferredBrep::new(
            Rc::new(Connector {
                opens: Rc::clone(&opens),
                fail,
            }),
            b"ISO-10303-21;".to_vec(),
            Rc::clone(&observations),
        );
        (brep, opens, observations)
    }

    #[test]
    fn opens_once_on_first_query_and_releases_its_source() {
        let (brep, opens, _) = deferred(false);
        assert_eq!(opens.get(), 0);
        assert_eq!(brep.pending_source_bytes(), 13);
        assert!(brep.faces().unwrap().is_empty());
        assert_eq!(brep.validity().unwrap_err().message, "13 bytes");
        assert_eq!(opens.get(), 1);
        assert_eq!(brep.pending_source_bytes(), 0);
    }

    #[test]
    fn a_failed_open_stays_failed() {
        let (brep, opens, _) = deferred(true);
        assert_eq!(brep.faces().unwrap_err().message, "unreadable");
        assert_eq!(brep.validity().unwrap_err().message, "unreadable");
        assert_eq!(opens.get(), 1);
    }
}
