//! Evaluation-only Truck BRep connector for the GeoSpec engine.
//!
//! This crate answers the engine's `BrepConnector`/`BrepSubject` seam with
//! [Truck](https://github.com/ricosjp/truck) so the Truck replacement charter
//! can measure parity against the OCCT connector through the same contract.
//! It is not part of the published engine.
//!
//! What Truck provides directly is used as is: the STEP table and assembly
//! reader (`truck-stepio`), compressed topology (`truck-topology`) and the
//! trimmed-surface mesher (`truck-meshalgo`). What Truck lacks is either
//! filled with the smallest adapter that keeps the comparison honest (length
//! units, analytic surface identity recovered by evaluation, mesh-integrated
//! properties) or refused with a typed reason, never weakened into a pass.

use geospec_engine_native_core::backend::brep::{
    Bounds, BrepAdmissionFacts, BrepConnector, BrepEntity, BrepIdentityProfile, BrepSubject,
    Charge, ClosureEdgeSample, ClosureFacts, ClosureGroup, FaceFacts, LocatedFace, OccurrenceFacts,
    PointState, ReportedFaces, ShapeFacts, StepSubjectMetadata, SurfaceFacts, TessellationProfile,
    TopologyCounts, ValidityCheck, ValidityFacts,
};
use geospec_engine_native_core::backend::{BackendError, BackendErrorKind, TriangleMesh};
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use truck_base::cgmath64::{InnerSpace, Matrix4, MetricSpace, Point3, Transform, Vector3};
use truck_geotrait::{
    BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter,
};
use truck_meshalgo::tessellation::RobustMeshableShape;
use truck_stepio::r#in::convert::{ProductShape, StepAssembly};
use truck_stepio::r#in::step_geometry::{Curve3D, ElementarySurface, Surface, SweptCurve};
use truck_stepio::r#in::Table;
use truck_topology::compress::CompressedShell;
use truck_topology::shell::ShellCondition;
use truck_topology::Shell;

type CShell = CompressedShell<Point3, Curve3D, Surface>;

/// The connector the engine composes; it owns no state.
pub struct TruckConnector;

impl BrepConnector for TruckConnector {
    fn identity_profile(&self) -> BrepIdentityProfile {
        BrepIdentityProfile {
            ingest_profile: "geospec-truck-step-eval-v0",
            backend_profile: "truck-88ed005-eval-v0",
        }
    }

    fn open_step(&self, bytes: &[u8]) -> Result<Box<dyn BrepSubject>, BackendError> {
        Ok(Box::new(TruckDocument::from_step(bytes)?))
    }
}

fn failure(kind: BackendErrorKind, message: impl Into<String>) -> BackendError {
    BackendError {
        kind,
        message: message.into(),
    }
}

fn unsupported(message: &str) -> BackendError {
    failure(BackendErrorKind::Unsupported, message)
}

/// Runs a Truck call, turning a panic inside Truck into a typed failure.
fn guarded<T>(what: &str, f: impl FnOnce() -> T) -> Result<T, BackendError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let detail = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default();
        failure(
            BackendErrorKind::ComputationFailed,
            format!("Truck panicked during {what}: {detail}"),
        )
    })
}

/// Length unit of the STEP representation contexts. Truck's reader ignores
/// units entirely, so the adapter reads them from the Part 21 text.
#[derive(Clone, Debug, PartialEq)]
pub struct LengthUnit {
    pub name: String,
    pub to_millimeters: f64,
    pub all_contexts_mm: bool,
}

fn length_unit(text: &str) -> LengthUnit {
    let upper = text.to_ascii_uppercase();
    let compact: String = upper.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("CONVERSION_BASED_UNIT('INCH'") {
        return LengthUnit {
            name: "inch".into(),
            to_millimeters: 25.4,
            all_contexts_mm: false,
        };
    }
    let milli = compact.contains("SI_UNIT(.MILLI.,.METRE.)");
    let centi = compact.contains("SI_UNIT(.CENTI.,.METRE.)");
    let metre = compact.contains("SI_UNIT($,.METRE.)");
    match (milli, centi, metre) {
        (true, false, false) | (false, false, false) => LengthUnit {
            name: "mm".into(),
            to_millimeters: 1.0,
            all_contexts_mm: milli,
        },
        (false, true, false) => LengthUnit {
            name: "cm".into(),
            to_millimeters: 10.0,
            all_contexts_mm: false,
        },
        (false, false, true) => LengthUnit {
            name: "m".into(),
            to_millimeters: 1000.0,
            all_contexts_mm: false,
        },
        _ => LengthUnit {
            name: "mixed".into(),
            to_millimeters: f64::NAN,
            all_contexts_mm: false,
        },
    }
}

/// One product's shells as Truck read them, in product coordinates.
pub struct Product {
    pub name: String,
    pub shells: Vec<CShell>,
    pub solid_shells: Vec<bool>,
    pub solids: usize,
    meshes: RefCell<HashMap<u64, Rc<ProductMesh>>>,
}

/// One located product use. Parents own no faces.
pub struct Occurrence {
    pub name: String,
    pub path: String,
    pub parent: Option<u32>,
    pub product: Option<usize>,
    pub ordinal_path: Vec<u32>,
    /// Product to world, already scaled to millimetres.
    pub matrix: Matrix4,
}

#[derive(Clone, Copy)]
struct FaceAddress {
    occurrence: u32,
    shell: usize,
    face: usize,
}

/// Outward-oriented face triangles in product coordinates.
struct FaceMesh {
    positions: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
}

struct ProductMesh {
    /// Indexed `[shell][face]`; `None` when Truck could not mesh the face.
    faces: Vec<Vec<Option<FaceMesh>>>,
}

/// One admitted STEP document.
pub struct TruckDocument {
    pub unit: LengthUnit,
    pub products: Vec<Product>,
    pub occurrences: Vec<Occurrence>,
    pub read_route: &'static str,
    source_len: usize,
    faces: Vec<FaceAddress>,
    occurrence_faces: Vec<Vec<usize>>,
    located: RefCell<Option<Rc<[LocatedFace]>>>,
}

fn product_shells(shapes: &[ProductShape]) -> (Vec<CShell>, Vec<bool>, usize) {
    let mut shells = Vec::new();
    let mut solid_shells = Vec::new();
    let mut solids = 0;
    for shape in shapes {
        match shape {
            ProductShape::Solid(solid) => {
                solids += 1;
                for shell in &solid.boundaries {
                    shells.push(shell.clone());
                    solid_shells.push(true);
                }
            }
            ProductShape::Shells(free) => {
                for shell in free {
                    shells.push(shell.clone());
                    solid_shells.push(false);
                }
            }
            ProductShape::Matrix(_) => {}
        }
    }
    (shells, solid_shells, solids)
}

impl TruckDocument {
    pub fn from_step(bytes: &[u8]) -> Result<Self, BackendError> {
        let text = String::from_utf8_lossy(bytes);
        let unit = length_unit(&text);
        if !unit.to_millimeters.is_finite() {
            return Err(failure(
                BackendErrorKind::Unsupported,
                "The STEP document mixes length units; the Truck adapter reads one.",
            ));
        }
        let table = guarded("STEP parsing", || Table::from_step(&text))?.ok_or_else(|| {
            failure(
                BackendErrorKind::InvalidInput,
                "Truck could not parse the STEP exchange structure.",
            )
        })?;
        let scale = Matrix4::from_scale(unit.to_millimeters);
        let assembly = guarded("assembly extraction", || table.step_assy())?;
        let mut doc = match assembly {
            Ok(assy) if !assy.is_empty() => Self::from_assembly(&assy, scale)?,
            _ => Self::from_flat_table(&table, scale)?,
        };
        doc.unit = unit;
        doc.source_len = bytes.len();
        doc.index_faces();
        if doc.faces.is_empty() {
            return Err(failure(
                BackendErrorKind::Unsupported,
                "Truck admitted no BRep faces from this STEP document.",
            ));
        }
        Ok(doc)
    }

    fn empty(read_route: &'static str) -> Self {
        Self {
            unit: LengthUnit {
                name: "mm".into(),
                to_millimeters: 1.0,
                all_contexts_mm: true,
            },
            products: Vec::new(),
            occurrences: Vec::new(),
            read_route,
            source_len: 0,
            faces: Vec::new(),
            occurrence_faces: Vec::new(),
            located: RefCell::new(None),
        }
    }

    fn from_assembly(assy: &StepAssembly, scale: Matrix4) -> Result<Self, BackendError> {
        let mut doc = Self::empty("truck-step-assy");
        let mut product_of_node = HashMap::new();
        for node in assy.all_nodes() {
            let (shells, solid_shells, solids) = product_shells(node.shape());
            if shells.is_empty() {
                continue;
            }
            product_of_node.insert(node.index(), doc.products.len());
            doc.products.push(Product {
                name: node.attrs().name.clone(),
                shells,
                solid_shells,
                solids,
                meshes: RefCell::new(HashMap::new()),
            });
        }
        for top in assy.top_nodes() {
            // Each path from a top node is one occurrence; its parent is the
            // path one edge shorter, which `paths_iter` yields first.
            let mut by_route: HashMap<Vec<truck_assembly::dag::NodeIndex>, u32> = HashMap::new();
            for path in assy.paths_iter(top.index()) {
                let nodes = path.nodes();
                let edges = path.edges();
                let mut matrix = scale;
                for edge in edges.iter() {
                    let local: Matrix4 = edge.matrix().try_into().map_err(|_| {
                        failure(
                            BackendErrorKind::ComputationFailed,
                            "Truck could not convert an assembly placement.",
                        )
                    })?;
                    matrix = matrix * local;
                }
                let route: Vec<_> = nodes.iter().map(|n| n.index()).collect();
                let parent = by_route.get(&route[..route.len() - 1]).copied();
                let leaf = nodes.last().expect("a path has a node");
                let name = edges
                    .last()
                    .map(|edge| edge.attrs().name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| leaf.attrs().name.clone());
                let ordinal = doc.occurrences.len() as u32;
                let parent_path = parent
                    .map(|p| doc.occurrences[p as usize].path.clone())
                    .unwrap_or_default();
                let mut ordinal_path = parent
                    .map(|p| doc.occurrences[p as usize].ordinal_path.clone())
                    .unwrap_or_default();
                ordinal_path.push(ordinal);
                doc.occurrences.push(Occurrence {
                    path: format!("{parent_path}/{name}"),
                    name,
                    parent,
                    product: product_of_node.get(&leaf.index()).copied(),
                    ordinal_path,
                    matrix,
                });
                by_route.insert(route, ordinal);
            }
        }
        Ok(doc)
    }

    /// No product structure Truck understands: every solid and shell model in
    /// the table becomes one product placed once.
    fn from_flat_table(table: &Table, scale: Matrix4) -> Result<Self, BackendError> {
        let mut doc = Self::empty("truck-flat-table");
        let mut shapes = Vec::new();
        let mut ids: Vec<_> = table.manifold_solid_brep.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let solid = guarded("solid conversion", || {
                table.to_compressed_solid(&table.manifold_solid_brep[&id])
            })?
            .map_err(|e| failure(BackendErrorKind::ComputationFailed, e.to_string()))?;
            shapes.push(ProductShape::Solid(solid));
        }
        let mut ids: Vec<_> = table.shell_based_surface_model.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let shells = guarded("shell conversion", || {
                table.to_compressed_shells(&table.shell_based_surface_model[&id])
            })?
            .map_err(|e| failure(BackendErrorKind::ComputationFailed, e.to_string()))?;
            shapes.push(ProductShape::Shells(shells));
        }
        let (shells, solid_shells, solids) = product_shells(&shapes);
        if !shells.is_empty() {
            doc.products.push(Product {
                name: String::new(),
                shells,
                solid_shells,
                solids,
                meshes: RefCell::new(HashMap::new()),
            });
            doc.occurrences.push(Occurrence {
                name: String::new(),
                path: "/".into(),
                parent: None,
                product: Some(0),
                ordinal_path: vec![0],
                matrix: scale,
            });
        }
        Ok(doc)
    }

    fn index_faces(&mut self) {
        self.occurrence_faces = vec![Vec::new(); self.occurrences.len()];
        let parents: Vec<bool> = {
            let mut is_parent = vec![false; self.occurrences.len()];
            for occurrence in &self.occurrences {
                if let Some(parent) = occurrence.parent {
                    is_parent[parent as usize] = true;
                }
            }
            is_parent
        };
        for (index, occurrence) in self.occurrences.iter().enumerate() {
            let Some(product) = occurrence.product else {
                continue;
            };
            if parents[index] {
                continue;
            }
            for (shell, cshell) in self.products[product].shells.iter().enumerate() {
                for face in 0..cshell.faces.len() {
                    self.occurrence_faces[index].push(self.faces.len());
                    self.faces.push(FaceAddress {
                        occurrence: index as u32,
                        shell,
                        face,
                    });
                }
            }
        }
    }

    /// The default fact tolerance: Truck meshes are the only integration
    /// route, so properties are measured on a fine chordal mesh.
    pub fn fact_tolerance(&self, product: usize) -> f64 {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for shell in &self.products[product].shells {
            for p in &shell.vertices {
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(p[axis]);
                    hi[axis] = hi[axis].max(p[axis]);
                }
            }
        }
        let diagonal = (0..3).map(|a| (hi[a] - lo[a]).powi(2)).sum::<f64>().sqrt();
        if diagonal.is_finite() && diagonal > 0.0 {
            (diagonal * 2.0e-4).max(1.0e-4)
        } else {
            1.0e-3
        }
    }

    fn product_mesh(
        &self,
        product: usize,
        tolerance: f64,
    ) -> Result<Rc<ProductMesh>, BackendError> {
        let key = tolerance.to_bits();
        if let Some(mesh) = self.products[product].meshes.borrow().get(&key) {
            return Ok(mesh.clone());
        }
        let mut faces = Vec::new();
        for cshell in &self.products[product].shells {
            let meshed = guarded("tessellation", || cshell.robust_triangulation(tolerance))?;
            let shell_faces = meshed
                .faces
                .iter()
                .map(|face| {
                    face.surface.as_ref().map(|mesh| {
                        let positions = mesh.positions().to_vec();
                        let triangles = mesh
                            .faces()
                            .triangle_iter()
                            .map(|tri| {
                                let t = [tri[0].pos as u32, tri[1].pos as u32, tri[2].pos as u32];
                                if face.orientation {
                                    t
                                } else {
                                    [t[0], t[2], t[1]]
                                }
                            })
                            .collect();
                        FaceMesh {
                            positions,
                            triangles,
                        }
                    })
                })
                .collect();
            faces.push(shell_faces);
        }
        let mesh = Rc::new(ProductMesh { faces });
        self.products[product]
            .meshes
            .borrow_mut()
            .insert(key, mesh.clone());
        Ok(mesh)
    }

    fn face_triangles(
        &self,
        address: FaceAddress,
        tolerance: Option<f64>,
    ) -> Result<Vec<[Point3; 3]>, BackendError> {
        let occurrence = &self.occurrences[address.occurrence as usize];
        let product = occurrence.product.expect("indexed faces have a product");
        let tolerance = tolerance.unwrap_or_else(|| self.fact_tolerance(product));
        let mesh = self.product_mesh(product, tolerance)?;
        let face = mesh.faces[address.shell][address.face]
            .as_ref()
            .ok_or_else(|| {
                failure(
                    BackendErrorKind::ComputationFailed,
                    "Truck could not tessellate a face.",
                )
            })?;
        Ok(face
            .triangles
            .iter()
            .map(|t| {
                t.map(|i| {
                    occurrence
                        .matrix
                        .transform_point(face.positions[i as usize])
                })
            })
            .collect())
    }

    fn entity_faces(&self, entity: BrepEntity) -> Result<Vec<FaceAddress>, BackendError> {
        let out_of_range = || failure(BackendErrorKind::InvalidInput, "Entity is out of range.");
        Ok(match entity {
            BrepEntity::Whole => self.faces.clone(),
            BrepEntity::WholeFace(index) => {
                vec![*self.faces.get(index as usize).ok_or_else(out_of_range)?]
            }
            BrepEntity::Occurrence(occurrence) => {
                let mut addresses = Vec::new();
                let mut stack = vec![occurrence];
                while let Some(current) = stack.pop() {
                    let faces = self
                        .occurrence_faces
                        .get(current as usize)
                        .ok_or_else(out_of_range)?;
                    addresses.extend(faces.iter().map(|&i| self.faces[i]));
                    stack.extend(
                        self.occurrences
                            .iter()
                            .enumerate()
                            .filter(|(_, o)| o.parent == Some(current))
                            .map(|(i, _)| i as u32),
                    );
                }
                addresses
            }
            BrepEntity::Face { occurrence, face } => {
                let faces = self
                    .occurrence_faces
                    .get(occurrence as usize)
                    .ok_or_else(out_of_range)?;
                vec![self.faces[*faces.get(face as usize).ok_or_else(out_of_range)?]]
            }
        })
    }

    fn surface(&self, address: FaceAddress) -> &Surface {
        let occurrence = &self.occurrences[address.occurrence as usize];
        let product = occurrence.product.expect("indexed faces have a product");
        &self.products[product].shells[address.shell].faces[address.face].surface
    }

    fn located_face(
        &self,
        entity: BrepEntity,
        address: FaceAddress,
    ) -> Result<LocatedFace, BackendError> {
        let triangles = self.face_triangles(address, None)?;
        let integral = integrate(&triangles);
        let occurrence = &self.occurrences[address.occurrence as usize];
        let product = occurrence.product.expect("indexed faces have a product");
        let cshell = &self.products[product].shells[address.shell];
        let cface = &cshell.faces[address.face];
        let hint = cface
            .boundaries
            .first()
            .and_then(|wire| wire.first())
            .map(|edge| cshell.vertices[cshell.edges[edge.index].vertices.0]);
        let surface = guarded("surface classification", || {
            classify_surface(self.surface(address), hint, &occurrence.matrix)
        })?;
        Ok(LocatedFace {
            entity,
            facts: FaceFacts {
                index: match entity {
                    BrepEntity::WholeFace(i) => i,
                    BrepEntity::Face { face, .. } => face,
                    _ => 0,
                },
                area: integral.area,
                center_of_mass: integral.surface_centroid(),
                surface,
            },
            bounds: triangle_bounds(&triangles),
            reversed: !cface.orientation,
        })
    }

    /// Mesh-integrated whole-shape facts at the default fact tolerance.
    pub fn shape_facts(&self) -> Result<ShapeFacts, BackendError> {
        let mut integral = Integral::default();
        let mut triangles_all = Vec::new();
        for &address in &self.faces {
            let triangles = self.face_triangles(address, None)?;
            integral.add(&triangles);
            triangles_all.extend(triangles);
        }
        Ok(ShapeFacts {
            bounds: triangle_bounds(&triangles_all),
            volume: integral.volume,
            surface_area: integral.area,
            center_of_mass: integral.volume_centroid(),
            topology: self.topology(),
        })
    }

    /// Counts per located leaf, matching how OCCT maps located subshapes.
    pub fn topology(&self) -> TopologyCounts {
        let mut counts = TopologyCounts {
            compounds: 0,
            solids: 0,
            shells: 0,
            faces: 0,
            wires: 0,
            edges: 0,
            vertices: 0,
        };
        for (index, occurrence) in self.occurrences.iter().enumerate() {
            if self.occurrence_faces[index].is_empty() {
                continue;
            }
            let product = &self.products[occurrence.product.expect("leaf has a product")];
            counts.solids += product.solids;
            for shell in &product.shells {
                counts.shells += 1;
                counts.faces += shell.faces.len();
                counts.wires += shell
                    .faces
                    .iter()
                    .map(|f| f.boundaries.len())
                    .sum::<usize>();
                // The report counts uses the way `TopExp_Explorer` visits them:
                // each edge once per wire that holds it, two vertices per edge use.
                let edge_uses = shell
                    .faces
                    .iter()
                    .flat_map(|f| &f.boundaries)
                    .map(Vec::len)
                    .sum::<usize>();
                counts.edges += edge_uses;
                counts.vertices += 2 * edge_uses;
            }
        }
        counts
    }

    /// Topological and geometric validity per product shell, through Truck's
    /// own checks: shell extraction, `shell_condition` and
    /// `is_geometric_consistent`.
    pub fn validity_facts(&self) -> Result<ValidityFacts, BackendError> {
        let mut checks = Vec::new();
        let mut open_edges = 0u32;
        let mut nonmanifold_edges = 0u32;
        let mut closed_shells = true;
        let mut closed_solids = true;
        let mut invalid_solids = 0u32;
        let mut solid_count = 0u32;
        let mut valid = true;
        for (pi, product) in self.products.iter().enumerate() {
            for (si, cshell) in product.shells.iter().enumerate() {
                let mut uses = vec![0u32; cshell.edges.len()];
                for face in &cshell.faces {
                    for wire in &face.boundaries {
                        for edge in wire {
                            uses[edge.index] += 1;
                        }
                    }
                }
                let open = uses.iter().filter(|&&n| n == 1).count() as u32;
                let nonmanifold = uses.iter().filter(|&&n| n > 2).count() as u32;
                open_edges += open;
                nonmanifold_edges += nonmanifold;
                let label = format!("product {pi} shell {si}");
                let extracted = guarded("shell extraction", || Shell::extract(cshell.clone()))?;
                let status = match extracted {
                    Err(error) => {
                        valid = false;
                        format!("extract-failed: {error}")
                    }
                    Ok(shell) => {
                        let condition = guarded("shell condition", || shell.shell_condition())?;
                        // `Shell::is_geometric_consistent` needs `IncludeCurve`, which
                        // Truck does not implement for its STEP surface type, so
                        // only the edge-vertex check is available here.
                        let consistent = guarded("geometric consistency", || {
                            shell.edge_iter().all(|edge| edge.is_geometric_consistent())
                        })?;
                        if condition != ShellCondition::Closed {
                            closed_shells = false;
                        }
                        if product.solid_shells[si] {
                            solid_count += 1;
                            if condition != ShellCondition::Closed || !consistent {
                                invalid_solids += 1;
                                closed_solids &= condition == ShellCondition::Closed;
                            }
                        }
                        if !consistent {
                            valid = false;
                        }
                        format!("{condition:?}; geometric-consistent={consistent}")
                    }
                };
                checks.push(ValidityCheck {
                    shape: label,
                    status,
                });
            }
        }
        Ok(ValidityFacts {
            valid: valid && invalid_solids == 0,
            checks: Some(checks),
            max_tolerance: None,
            free_bounds: Some(open_edges),
            small_edges: None,
            same_parameter: None,
            closed_shells: Some(closed_shells),
            closed_solids: Some(closed_solids),
            solid_count: Some(solid_count),
            invalid_solid_count: Some(invalid_solids),
            open_edge_count: Some(open_edges),
            nonmanifold_edge_count: Some(nonmanifold_edges),
            closed_wires: None,
            reason: None,
        })
    }
}

impl BrepSubject for TruckDocument {
    fn admission_facts(&self) -> Result<BrepAdmissionFacts, BackendError> {
        Ok(BrepAdmissionFacts {
            source_length_unit: self.unit.name.clone(),
            source_unit_to_millimeters: self.unit.to_millimeters,
            occurrence_count: self.occurrences.len(),
            surfaceless_faces: 0,
            all_source_length_contexts_mm: self.unit.all_contexts_mm,
        })
    }

    fn step_subject_metadata(&self) -> Result<Option<StepSubjectMetadata>, BackendError> {
        Ok(Some(StepSubjectMetadata {
            schema: None,
            source_byte_length: self.source_len,
            free_shape_count: self
                .occurrences
                .iter()
                .filter(|o| o.parent.is_none())
                .count(),
            native_read_stream: false,
        }))
    }

    fn reported_shape(&self) -> Result<ShapeFacts, BackendError> {
        self.shape_facts()
    }

    /// The OCCT report mesh is 0.01 mm linear and 15 degrees angular; Truck
    /// takes the linear value alone.
    fn reported_mesh(&self) -> Result<TriangleMesh, BackendError> {
        let profile = TessellationProfile {
            linear_deflection_mm: 0.01,
            angular_deflection_rad: 15f64.to_radians(),
        };
        self.tessellate(BrepEntity::Whole, profile)
            .map(|mesh| (*mesh).clone())
    }

    fn reported_faces(&self, _measured: bool) -> Result<ReportedFaces, BackendError> {
        let whole_faces = self.faces()?;
        let occurrence_faces = self
            .occurrence_faces
            .iter()
            .enumerate()
            .map(|(occurrence, faces)| {
                faces
                    .iter()
                    .enumerate()
                    .map(|(face, &whole)| {
                        let mut located = whole_faces[whole].clone();
                        located.entity = BrepEntity::Face {
                            occurrence: occurrence as u32,
                            face: face as u32,
                        };
                        located.facts.index = face as u32;
                        located
                    })
                    .collect::<Vec<_>>()
                    .into()
            })
            .collect();
        Ok(ReportedFaces {
            whole_faces,
            occurrence_faces,
        })
    }

    fn source_occurrences(&self) -> Result<Rc<[OccurrenceFacts]>, BackendError> {
        let mut rows = Vec::with_capacity(self.occurrences.len());
        for (index, occurrence) in self.occurrences.iter().enumerate() {
            let triangles = self
                .entity_faces(BrepEntity::Occurrence(index as u32))?
                .into_iter()
                .map(|address| self.face_triangles(address, None))
                .collect::<Result<Vec<_>, _>>()?
                .concat();
            let m = occurrence.matrix;
            rows.push(OccurrenceFacts {
                name: occurrence.name.clone(),
                placement: [
                    m.x.x, m.y.x, m.z.x, m.w.x, m.x.y, m.y.y, m.z.y, m.w.y, m.x.z, m.y.z, m.z.z,
                    m.w.z,
                ],
                bounds: triangle_bounds(&triangles),
                path: occurrence.path.clone(),
                parent: occurrence.parent,
                product: occurrence.product.unwrap_or(usize::MAX) as u32,
                product_name: occurrence
                    .product
                    .map(|p| self.products[p].name.clone())
                    .unwrap_or_default(),
                instance_name: Some(occurrence.name.clone()),
                ordinal_path: occurrence.ordinal_path.clone(),
                face_count: self.occurrence_faces[index].len() as u32,
            });
        }
        Ok(rows.into())
    }

    fn faces(&self) -> Result<Rc<[LocatedFace]>, BackendError> {
        if let Some(faces) = self.located.borrow().as_ref() {
            return Ok(faces.clone());
        }
        let faces: Rc<[LocatedFace]> = self
            .faces
            .iter()
            .enumerate()
            .map(|(i, &address)| self.located_face(BrepEntity::WholeFace(i as u32), address))
            .collect::<Result<Vec<_>, _>>()?
            .into();
        *self.located.borrow_mut() = Some(faces.clone());
        Ok(faces)
    }

    fn validity(&self) -> Result<Rc<ValidityFacts>, BackendError> {
        self.validity_facts().map(Rc::new)
    }

    /// Edge-use counts per product shell, the same rule the OCCT bridge uses;
    /// Truck reads no free faces, so every group is a shell.
    fn closure(&self, charge: &mut Charge<'_>) -> Result<Option<Rc<ClosureFacts>>, BackendError> {
        let mut facts = ClosureFacts {
            shells: 0,
            free_faces: 0,
            open_edges: 0,
            nonmanifold_edges: 0,
            failing: Vec::new(),
        };
        for (pi, product) in self.products.iter().enumerate() {
            let leaves: Vec<u32> = self
                .occurrences
                .iter()
                .enumerate()
                .filter(|(_, o)| o.product == Some(pi))
                .map(|(i, _)| i as u32)
                .collect();
            let instances = leaves.len().max(1) as u32;
            for cshell in &product.shells {
                facts.shells += instances;
                let mut uses = vec![0u32; cshell.edges.len()];
                for face in &cshell.faces {
                    for edge in face.boundaries.iter().flatten() {
                        uses[edge.index] += 1;
                    }
                }
                let open = uses.iter().filter(|&&n| n == 1).count() as u32;
                let nonmanifold = uses.iter().filter(|&&n| n > 2).count() as u32;
                facts.open_edges += open * instances;
                facts.nonmanifold_edges += nonmanifold * instances;
                if open + nonmanifold == 0 && uses.contains(&2) {
                    continue;
                }
                let matrix = leaves.first().map_or(Matrix4::from_scale(1.0), |&i| {
                    self.occurrences[i as usize].matrix
                });
                let samples = uses
                    .iter()
                    .enumerate()
                    .filter(|(_, &n)| n != 2)
                    .take(4)
                    .map(|(ei, &n)| {
                        let edge = &cshell.edges[ei];
                        let at = |v: usize| matrix.transform_point(cshell.vertices[v]);
                        let (t0, t1) = edge.curve.range_tuple();
                        let mid = matrix.transform_point(edge.curve.subs((t0 + t1) / 2.0));
                        ClosureEdgeSample {
                            face_uses: n,
                            start: at(edge.vertices.0).into(),
                            end: at(edge.vertices.1).into(),
                            center: mid.into(),
                        }
                    })
                    .collect();
                facts.failing.push(ClosureGroup {
                    free_faces: false,
                    open_edges: open,
                    nonmanifold_edges: nonmanifold,
                    occurrences: leaves.clone(),
                    samples,
                });
            }
        }
        let units = if facts.failing.is_empty() {
            0
        } else {
            (facts.failing.len() + self.occurrences.len()) as u64
        };
        Ok(charge(units).then(|| Rc::new(facts)))
    }

    /// Mesh-based classification: Truck has no trimmed-face classifier, so a
    /// point is On within `tolerance_mm` of the face's mesh boundary, In
    /// within tolerance of its triangles and Out otherwise. The chordal
    /// error of the mesh widens every band.
    fn classify_face_points(
        &self,
        face: BrepEntity,
        points: &[[f64; 3]],
        tolerance_mm: f64,
    ) -> Result<Vec<PointState>, BackendError> {
        let addresses = self.entity_faces(face)?;
        let [address] = addresses[..] else {
            return Err(unsupported("Classification needs one face."));
        };
        let triangles = self.face_triangles(address, None)?;
        let boundary = boundary_segments(&triangles);
        Ok(points
            .iter()
            .map(|p| {
                let p = Point3::new(p[0], p[1], p[2]);
                let to_face = triangles
                    .iter()
                    .map(|t| point_triangle_distance(p, t))
                    .fold(f64::INFINITY, f64::min);
                if to_face > tolerance_mm {
                    return PointState::Out;
                }
                let to_boundary = boundary
                    .iter()
                    .map(|s| point_segment_distance(p, s[0], s[1]))
                    .fold(f64::INFINITY, f64::min);
                if to_boundary <= tolerance_mm {
                    PointState::On
                } else {
                    PointState::In
                }
            })
            .collect())
    }

    /// Truck takes one chordal tolerance; the angular deflection has no
    /// Truck counterpart and is ignored.
    fn tessellate(
        &self,
        entity: BrepEntity,
        profile: TessellationProfile,
    ) -> Result<Rc<TriangleMesh>, BackendError> {
        let mut mesh = TriangleMesh {
            positions: Vec::new(),
            triangles: Vec::new(),
        };
        for address in self.entity_faces(entity)? {
            for triangle in self.face_triangles(address, Some(profile.linear_deflection_mm))? {
                let base = mesh.positions.len() as u32;
                mesh.positions
                    .extend(triangle.iter().map(|p| [p.x, p.y, p.z]));
                mesh.triangles.push([base, base + 1, base + 2]);
            }
        }
        Ok(Rc::new(mesh))
    }
}

struct Integral {
    area: f64,
    area_moment: Vector3,
    volume: f64,
    volume_moment: Vector3,
}

impl Default for Integral {
    fn default() -> Self {
        Self {
            area: 0.0,
            area_moment: Vector3::new(0.0, 0.0, 0.0),
            volume: 0.0,
            volume_moment: Vector3::new(0.0, 0.0, 0.0),
        }
    }
}

impl Integral {
    fn add(&mut self, triangles: &[[Point3; 3]]) {
        for [a, b, c] in triangles {
            let (a, b, c) = (
                a.to_homogeneous().truncate(),
                b.to_homogeneous().truncate(),
                c.to_homogeneous().truncate(),
            );
            let area = (b - a).cross(c - a).magnitude() / 2.0;
            self.area += area;
            self.area_moment += (a + b + c) * (area / 3.0);
            let signed = a.dot(b.cross(c)) / 6.0;
            self.volume += signed;
            self.volume_moment += (a + b + c) * (signed / 4.0);
        }
    }

    fn surface_centroid(&self) -> [f64; 3] {
        let c = self.area_moment / self.area;
        [c.x, c.y, c.z]
    }

    fn volume_centroid(&self) -> [f64; 3] {
        let c = self.volume_moment / self.volume;
        [c.x, c.y, c.z]
    }
}

fn integrate(triangles: &[[Point3; 3]]) -> Integral {
    let mut integral = Integral::default();
    integral.add(triangles);
    integral
}

fn triangle_bounds(triangles: &[[Point3; 3]]) -> Bounds {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in triangles.iter().flatten() {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    Bounds { min, max }
}

/// Mesh edges used by exactly one triangle of the face.
fn boundary_segments(triangles: &[[Point3; 3]]) -> Vec<[Point3; 2]> {
    let key = |p: &Point3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    type SegmentKey = ([u64; 3], [u64; 3]);
    let mut uses: HashMap<SegmentKey, (u32, [Point3; 2])> = HashMap::new();
    for t in triangles {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            let (ka, kb) = (key(&a), key(&b));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            uses.entry(k).or_insert((0, [a, b])).0 += 1;
        }
    }
    uses.into_values()
        .filter(|(n, _)| *n == 1)
        .map(|(_, s)| s)
        .collect()
}

fn point_segment_distance(p: Point3, a: Point3, b: Point3) -> f64 {
    let ab = b - a;
    let length2 = ab.magnitude2();
    let t = if length2 > 0.0 {
        ((p - a).dot(ab) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

fn point_triangle_distance(p: Point3, t: &[Point3; 3]) -> f64 {
    let [a, b, c] = *t;
    let n = (b - a).cross(c - a);
    let n2 = n.magnitude2();
    if n2 > 0.0 {
        let d = (p - a).dot(n) / n2;
        let q = p - n * d;
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(u, v)| (v - u).cross(q - u).dot(n) >= 0.0);
        if inside {
            return p.distance(q);
        }
    }
    point_segment_distance(p, a, b)
        .min(point_segment_distance(p, b, c))
        .min(point_segment_distance(p, c, a))
}

fn circle_through(a: Point3, b: Point3, c: Point3) -> Option<(Point3, Vector3, f64)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.magnitude2();
    if n2 <= 1.0e-18 * ab.magnitude2().max(1.0) * ac.magnitude2().max(1.0) {
        return None;
    }
    let center = a + (n.cross(ab) * ac.magnitude2() + ac.cross(n) * ab.magnitude2()) / (2.0 * n2);
    Some((center, n.normalize(), center.distance(a)))
}

/// Recovers analytic surface parameters by evaluating Truck's surface: the
/// STEP reader converts cylinders and cones to revolved lines and drops the
/// original radius and placement, so they are measured back in world space.
fn classify_surface(surface: &Surface, hint: Option<Point3>, m: &Matrix4) -> SurfaceFacts {
    let to_array = |p: Point3| [p.x, p.y, p.z];
    let dir = |v: Vector3| {
        let v = m.transform_vector(v).normalize();
        [v.x, v.y, v.z]
    };
    let scale = m.transform_vector(Vector3::unit_x()).magnitude();
    let (u0, v0) = hint
        .and_then(|p| surface.search_nearest_parameter(p, None, 100))
        .unwrap_or_else(|| {
            let (ur, vr) = surface.parameter_range();
            let mid = |r: truck_geotrait::ParameterRange| match (r.0, r.1) {
                (std::ops::Bound::Included(a), std::ops::Bound::Included(b))
                | (std::ops::Bound::Included(a), std::ops::Bound::Excluded(b)) => (a + b) / 2.0,
                (std::ops::Bound::Included(a), _) => a,
                _ => 0.0,
            };
            (mid(ur), mid(vr))
        });
    let world = |u: f64, v: f64| m.transform_point(surface.subs(u, v));
    let circle_u = || circle_through(world(u0, v0), world(u0 + 0.3, v0), world(u0 + 0.6, v0));
    let circle_v = || circle_through(world(u0, v0), world(u0, v0 + 0.3), world(u0, v0 + 0.6));
    match surface {
        Surface::ElementarySurface(ElementarySurface::Plane(_)) => {
            let n = surface.normal(u0, v0);
            SurfaceFacts::Plane {
                origin: to_array(world(u0, v0)),
                normal: dir(n),
            }
        }
        Surface::ElementarySurface(ElementarySurface::CylindricalSurface(_)) => {
            match circle_u().or_else(circle_v) {
                Some((center, axis, radius)) => SurfaceFacts::Cylinder {
                    origin: to_array(center),
                    axis: [axis.x, axis.y, axis.z],
                    radius,
                },
                None => SurfaceFacts::Other,
            }
        }
        Surface::ElementarySurface(ElementarySurface::ConicalSurface(_)) => {
            let first = circle_u()
                .map(|c| (c, true))
                .or_else(|| circle_v().map(|c| (c, false)));
            match first {
                Some(((center, axis, radius), along_u)) => {
                    let second = if along_u {
                        circle_through(
                            world(u0, v0 + 1.0),
                            world(u0 + 0.3, v0 + 1.0),
                            world(u0 + 0.6, v0 + 1.0),
                        )
                    } else {
                        circle_through(
                            world(u0 + 1.0, v0),
                            world(u0 + 1.0, v0 + 0.3),
                            world(u0 + 1.0, v0 + 0.6),
                        )
                    };
                    let semi_angle = second
                        .map(|(c2, _, r2)| ((r2 - radius) / c2.distance(center)).atan().abs())
                        .unwrap_or(f64::NAN);
                    SurfaceFacts::Cone {
                        origin: to_array(center),
                        axis: [axis.x, axis.y, axis.z],
                        reference_radius: radius,
                        semi_angle,
                    }
                }
                None => SurfaceFacts::Other,
            }
        }
        Surface::ElementarySurface(ElementarySurface::Sphere(_)) => {
            match circle_u().or_else(circle_v) {
                // A meridian through the hint is a great circle of the sphere.
                Some((center, _, radius)) => SurfaceFacts::Sphere {
                    center: to_array(center),
                    radius,
                },
                None => SurfaceFacts::Other,
            }
        }
        Surface::ElementarySurface(ElementarySurface::ToroidalSurface(_)) => {
            match (circle_u(), circle_v()) {
                (Some(a), Some(b)) => {
                    let (major, minor) = if a.2 >= b.2 { (a, b) } else { (b, a) };
                    let center = major.0 + major.1 * (major.1.dot(minor.0 - major.0))
                        - (minor.0 - major.0) * 0.0;
                    let major_radius = (minor.0 - major.0).magnitude().max(0.0);
                    let _ = scale;
                    SurfaceFacts::Torus {
                        center: to_array(center),
                        axis: [major.1.x, major.1.y, major.1.z],
                        major_radius: if major_radius > 0.0 {
                            major_radius
                        } else {
                            major.2 - minor.2
                        },
                        minor_radius: minor.2,
                    }
                }
                _ => SurfaceFacts::Other,
            }
        }
        Surface::BSplineSurface(s) => SurfaceFacts::Bspline {
            u_degree: s.udegree() as u32,
            v_degree: s.vdegree() as u32,
            u_poles: s.control_points().len() as u32,
            v_poles: s.control_points().first().map_or(0, |r| r.len()) as u32,
            u_knots: s.uknot_vec().len() as u32,
            v_knots: s.vknot_vec().len() as u32,
            u_rational: false,
            v_rational: false,
        },
        Surface::NurbsSurface(s) => SurfaceFacts::Bspline {
            u_degree: s.udegree() as u32,
            v_degree: s.vdegree() as u32,
            u_poles: s.control_points().len() as u32,
            v_poles: s.control_points().first().map_or(0, |r| r.len()) as u32,
            u_knots: s.uknot_vec().len() as u32,
            v_knots: s.vknot_vec().len() as u32,
            u_rational: true,
            v_rational: true,
        },
        Surface::SweptCurve(SweptCurve::RevolutedCurve(_)) => SurfaceFacts::Revolution,
        Surface::SweptCurve(SweptCurve::ExtrudedCurve(_)) => SurfaceFacts::Extrusion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_units_are_read_from_the_part21_text() {
        assert_eq!(
            length_unit("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));").to_millimeters,
            1.0
        );
        assert_eq!(
            length_unit("#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));").to_millimeters,
            1000.0
        );
        assert_eq!(
            length_unit("#2=(CONVERSION_BASED_UNIT('INCH',#3)LENGTH_UNIT()NAMED_UNIT(#4));")
                .to_millimeters,
            25.4
        );
    }

    #[test]
    fn three_points_define_a_circle() {
        let (c, n, r) = circle_through(
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(0.0, 1.0, 2.0),
            Point3::new(-1.0, 0.0, 2.0),
        )
        .unwrap();
        assert!(c.distance(Point3::new(0.0, 0.0, 2.0)) < 1e-12);
        assert!((n.z.abs() - 1.0).abs() < 1e-12);
        assert!((r - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_closed_tetrahedron_integrates_exactly() {
        let o = Point3::new(0.0, 0.0, 0.0);
        let x = Point3::new(1.0, 0.0, 0.0);
        let y = Point3::new(0.0, 1.0, 0.0);
        let z = Point3::new(0.0, 0.0, 1.0);
        let i = integrate(&[[o, y, x], [o, x, z], [o, z, y], [x, y, z]]);
        assert!((i.volume - 1.0 / 6.0).abs() < 1e-15);
        let c = i.volume_centroid();
        assert!((c[0] - 0.25).abs() < 1e-15 && (c[1] - 0.25).abs() < 1e-15);
    }
}
