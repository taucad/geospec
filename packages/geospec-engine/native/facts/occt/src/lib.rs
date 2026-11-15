//! The facts layer's OCCT adapter: each face of an OCCT B-rep as a
//! kernel-agnostic [`FaceDefinition`].
//!
//! A [`Subject`] opens a STEP file through the bridge's own admission
//! (`geospec_occt_open_step`: the engine's read profile, authored fixes and
//! threaded admission) and reads it through the export ABI of
//! `export/geospec_occt_facts_export.h`, which is compiled with the bridge's
//! source into this crate alone: no product path links it. Each solid of the
//! admitted shape is one body, in `TopExp_Explorer` order; its faces are the
//! solid's, with their composed orientation. The export passes OCCT's planes,
//! cylinders, cones, spheres and tori through in OCCT's parameterisation (an
//! indirect frame reflected, u negated), B-spline nets as they are (periodic
//! nets with their origin moved to hold the trims, by exact knot insertion)
//! and Bezier nets as B-splines; each edge use's 2D curve over the edge's range
//! (`BRep_Tool::CurveOnSurface`: seams by orientation, degenerate edges
//! included), lines as lines, B-splines as they are, conics as exact rational
//! B-splines. Other surface and curve kinds refuse
//! ([`RefusalCode::UnsupportedSurface`], [`RefusalCode::UnsupportedTrim`]).
//!
//! Every export is checked against the original OCCT geometry: each trim at
//! seven parameters (point, and derivative where the parameter is the edge's;
//! a converted conic's points are projected on the original curve), the
//! surface's point and first derivatives at the trims' points and between
//! them. A difference above a relative 1e-9 refuses
//! ([`RefusalCode::ExportMismatch`]): points against `1 +` their magnitude,
//! a trim's derivative against `1 +` its magnitude, a surface's `S_u` and
//! `S_v` against `1 +` the larger of the two. The loops are OCCT's wires on
//! the forward face, where the material lies on their left. A reflected
//! (indirect) frame mirrors the parameter plane (u negated), which would put
//! the material on the right: the export then reverses every loop (the order
//! of its trims and each trim's direction) so that the material stays on the
//! left, and flips the face's orientation, since the reflected frame's natural
//! normal is the negated one. The facts crate checks the loops' chaining,
//! closure and orientation.
//!
//! Consecutive edges of an OCCT wire meet at their shared vertex within its
//! tolerance, so their 2D curves need not meet exactly. Where a trim ends away
//! from where the next starts by more than the integrator's chaining tolerance
//! (a relative 1e-6) and not by a whole period, and both ends lie on the
//! surface within the vertex's tolerance of the vertex, the export marks the
//! join as a tolerance gap (the straight join in the parameter plane, counted
//! per face in `gap_faces` and in total in `gap_lines`), and the face refuses
//! the facts ([`RefusalCode::ToleranceGap`]): closing it would change the
//! face by an amount no integration bound covers. Ends a whole period apart on a
//! periodic surface are a period wrap, not a gap, and pass to the integrator.
//! Any other gap stays and the integrator refuses the face.
//!
//! Every trim also carries the vertex at its end and that vertex's
//! tolerance (a gap line, the vertex whose gap it closes). Beside the facts'
//! definition each body has one for the box alone ([`BodyExport::boxed`], a
//! [`BoxBody`]): every face with its gaps bridged by their lines, checked
//! against OCCT as any face is, with its joint table (the gap lines and the
//! trims' end vertices) and the body's largest tolerance. The box brackets
//! that bridged definition and ties each bridge, chord and joint to its
//! vertex (see `geospec_facts::boxes_of`); the facts never see it.
//!
//! The body centre is the centre of `BRepBndLib::Add`'s box (no
//! triangulation), which contains the solid; `closed` is the shells'
//! edge-use parity (`BRep_Tool::IsClosed`); a mirrored or scaled placement
//! refuses ([`RefusalCode::InvalidDefinition`]). Per body the export also
//! returns the bridge's measurement of the solid with `BRepGProp` (no `Eps`),
//! for the differential.

mod ffi;

use std::ptr::NonNull;

use geospec_facts::{
    bodies_facts, boxes_within_target, BodyDefinition, BodyFacts, BoxBody, BoxJoints, BoxOptions,
    BoxStop, Curve2, FaceDefinition, FaceDefinitions, Fact, FactAccuracy, Frame, Joint, Material,
    Method, NurbsCurve2, NurbsSurface, Options, Orientation, PositionFact, Refusal, RefusalCode,
    Scale, Surface, TrimPiece, Vec3, SIDES,
};

// The OCCT prefix allocates through mimalloc on native targets.
#[cfg(not(target_os = "emscripten"))]
extern crate libmimalloc_sys as _;

/// Relative tolerance of the export check (positions in the parameter plane
/// and in space, against `1 +` the magnitude), as the solidcore adapter's.
const EXPORT_TOLERANCE: f64 = 1e-9;

/// Trim parameters each export check samples.
const TRIM_SAMPLES: u32 = 7;

/// OCCT's measurement of one solid, the bridge's calls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gprop {
    /// `BRepGProp::VolumeProperties` without `Eps`.
    pub volume: f64,
    /// `BRepGProp::SurfaceProperties` without `Eps`.
    pub area: f64,
    /// The centre of mass of the volume properties.
    pub centre: [f64; 3],
    /// Time of the two calls (ms).
    pub ms: f64,
}

/// What the export knows about one solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyRecord {
    /// Faces of the solid.
    pub faces: usize,
    /// Shells of the solid.
    pub shells: usize,
    /// Every shell closed by edge-use parity.
    pub closed: bool,
    /// `"rigid"`, `"mirrored"` or `"scaled"`.
    pub placement: &'static str,
    /// The largest vertex, edge or face tolerance.
    pub max_tolerance: f64,
    /// OCCT's measurement.
    pub gprop: Gprop,
}

/// Counts of what one body's export met, for the report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportStats {
    /// Faces per OCCT surface kind (`source` names).
    pub surfaces: std::collections::BTreeMap<&'static str, usize>,
    /// Trims per OCCT 2D curve kind.
    pub curves: std::collections::BTreeMap<&'static str, usize>,
    /// Faces whose frame was reflected (an indirect `gp_Ax3`).
    pub reflected_faces: usize,
    /// Faces of periodic nets whose origin moved to hold their trims.
    pub moved_origin_faces: usize,
    /// Of those, faces whose trims moved by whole periods.
    pub shifted_faces: usize,
    /// Seam edge uses.
    pub seams: usize,
    /// Degenerate edge uses.
    pub degenerate: usize,
    /// Tolerance gaps between consecutive trims (each a refusal of its face).
    pub gap_lines: usize,
    /// Faces (in solid order) with at least one tolerance gap, and their count.
    pub gap_faces: Vec<(usize, usize)>,
    /// Trims on a surface of revolution reaching u outside `[0, 2 pi]`.
    pub past_two_pi: usize,
    /// Faces whose orientation in the solid is reversed.
    pub reversed_faces: usize,
}

/// One body's export.
#[derive(Debug)]
pub struct BodyExport {
    /// The solid's record.
    pub record: BodyRecord,
    /// The definition, or the first face's refusal.
    pub definition: Result<BodyDefinition, Refusal>,
    /// The body for the box alone ([`boxes_of`](geospec_facts::boxes_of)):
    /// every face's definition with its tolerance gaps bridged and its joint
    /// table, and the body's largest tolerance, or the first refusal of a
    /// face's export or check in face order. A face that refuses the facts
    /// only for its gaps is here.
    pub boxed: Result<BoxBody, Refusal>,
    /// What the export met.
    pub stats: ExportStats,
}

/// The bridge's whole-shape facts (its source shape facts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeFacts {
    /// Volume (`BRepGProp::VolumeProperties` of the whole shape).
    pub volume: f64,
    /// Surface area (the bridge's `surface_area`).
    pub area: f64,
    /// Centre of mass (NaN for a volume within epsilon of zero).
    pub centre: [f64; 3],
    /// Time (ms).
    pub ms: f64,
}

/// `BRepGProp`'s adaptive measurement of one solid at a relative accuracy
/// (M1's `volumeRef` method).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EpsReference {
    /// Volume.
    pub volume: f64,
    /// Surface area.
    pub area: f64,
    /// Centre of mass of the volume properties.
    pub centre: [f64; 3],
    /// `VolumeProperties`' error estimate.
    pub volume_error: f64,
    /// `SurfaceProperties`' error estimate.
    pub area_error: f64,
    /// Time of the two calls (ms).
    pub ms: f64,
}

/// The bridge's box of one solid: `BRepBndLib::AddOptimal(face, box, false,
/// false)` per face, folded (P5's reference).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AddOptimalBox {
    /// `[min, max]`, `None` when every face box is void.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// Time of the face boxes (ms).
    pub ms: f64,
    /// Faces whose box is void.
    pub void_faces: usize,
}

/// What the shape holds outside its solids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeCounts {
    /// Faces outside every solid.
    pub faces: usize,
    /// Edges outside every face.
    pub edges: usize,
    /// Vertices outside every edge.
    pub vertices: usize,
}

/// The box of a whole shape with its bracket (P5): every solid boxed
/// ([`boxes_within_target`]) and folded in solid order, for a shape with
/// nothing outside its solids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeBox {
    /// `[min, max]` spanned by values points of the shape attain: inside the
    /// true box.
    pub inner: [Vec3; 2],
    /// `[min, max]` no point of the shape's definition exceeds: around the
    /// true box.
    pub outer: [Vec3; 2],
    /// The union of the solids' [`kernel_outer`](geospec_facts::BodyBox::kernel_outer)
    /// boxes: the outer box grown per solid by its largest tolerance plus
    /// its gap bound, the box around OCCT's own geometry.
    pub outer_tolerant: [Vec3; 2],
    /// The largest vertex, edge or face tolerance over the solids.
    pub kernel_tolerance: f64,
    /// Solids boxed.
    pub bodies: usize,
}

/// The mass facts of a whole shape (P6): every solid's area, volume and
/// centre of mass ([`bodies_facts`], with the solid's largest tolerance as
/// its file tolerance), combined in solid order. One solid's values and
/// bounds are its own, bit for bit; the centre's gap is the shape's
/// ([`centroid_gap`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeMass {
    /// The sum of the solids' areas.
    pub area: Fact,
    /// The sum of the solids' volumes; its `gap_bound` is the sum of theirs.
    pub volume: Fact,
    /// The volume-weighted centre of mass; `None` when a solid has none.
    /// Its `gap_bound` is [`centroid_gap`], how far the file's tolerance can
    /// move it.
    pub centroid: Option<PositionFact>,
    /// Solids measured.
    pub bodies: usize,
}

/// Why a shape has no [`ShapeBox`].
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeBoxRefusal {
    /// A [`RefusalCode`] name, or `free-geometry` (faces outside solids,
    /// edges outside faces or vertices outside edges), `no-solid` or
    /// `bracket-too-wide` (a solid's bracket wider than its target).
    pub code: &'static str,
    /// The refusal's detail.
    pub detail: String,
    /// The solid that refused, in solid order.
    pub body: Option<usize>,
    /// Its face that refused, in the solid's face order.
    pub face: Option<usize>,
}

impl ShapeBoxRefusal {
    fn of(refusal: Refusal, body: usize) -> Self {
        Self {
            code: refusal.code.as_str(),
            detail: refusal.detail,
            body: Some(body),
            face: refusal.face,
        }
    }

    /// `bracket-too-wide` for solid `body` whose bracket's width
    /// ([`geospec_facts::BodyBox::bound`]) exceeds the target it is refined
    /// towards (`target`, `accuracy` times the solid's size): faces that
    /// fell back, reached the work cap or were widened for trims outside
    /// their domain can leave a side wider than asked, and such a box is
    /// not reported. With `at_least`, `width` is a lower bound found before
    /// the solid was fully refined.
    #[must_use]
    pub fn too_wide(body: usize, width: f64, target: f64, at_least: bool, accuracy: f64) -> Self {
        let width = if at_least {
            format!("is at least {width:e}, which")
        } else {
            format!("{width:e}")
        };
        Self {
            code: "bracket-too-wide",
            detail: format!(
                "the bracket's width {width} exceeds its target {target:e} ({accuracy:e} of the solid's size)"
            ),
            body: Some(body),
            face: None,
        }
    }

    fn shape(code: &'static str, detail: String) -> Self {
        Self {
            code,
            detail,
            body: None,
            face: None,
        }
    }
}

/// An admitted OCCT shape and its export.
pub struct Subject {
    document: Option<NonNull<ffi::Document>>,
    export: NonNull<ffi::Export>,
    error: ffi::ErrorBuffer,
}

fn surface_name(code: i32) -> &'static str {
    [
        "plane",
        "cylinder",
        "cone",
        "sphere",
        "torus",
        "bspline",
        "periodic-bspline",
        "bezier",
        "revolution",
        "extrusion",
        "offset",
        "other",
    ]
    .get(usize::try_from(code).unwrap_or(usize::MAX))
    .copied()
    .unwrap_or("unknown")
}

fn curve_name(code: i32) -> &'static str {
    [
        "line",
        "circle",
        "ellipse",
        "hyperbola",
        "parabola",
        "bspline",
        "periodic-bspline",
        "bezier",
        "offset",
        "other",
    ]
    .get(usize::try_from(code).unwrap_or(usize::MAX))
    .copied()
    .unwrap_or("unknown")
}

/// One face's export: the definition the box takes with its joint table
/// (the gaps bridged), and the face's tolerance-gap refusal, which the facts
/// take first.
struct FaceExport {
    gapped: Option<Refusal>,
    boxed: Result<(FaceDefinition, BoxJoints), Refusal>,
}

impl FaceExport {
    /// A face whose export check refused `r`: the facts refuse the gap
    /// first when the face has one (decided before any check, as always).
    fn refused(gapped: Option<Refusal>, r: Refusal) -> Self {
        Self {
            gapped,
            boxed: Err(r),
        }
    }

    /// The definition the facts take: the gap's refusal when the face has
    /// one, else the box's definition or refusal. Only the facts' path
    /// copies the definition.
    fn facts(&self) -> Result<FaceDefinition, Refusal> {
        match (&self.gapped, &self.boxed) {
            (Some(r), _) | (None, Err(r)) => Err(r.clone()),
            (None, Ok((d, _))) => Ok(d.clone()),
        }
    }
}

/// The code of an export's face refusal.
fn refusal_code(refusal: i32) -> RefusalCode {
    match refusal {
        ffi::REFUSAL_UNSUPPORTED_SURFACE => RefusalCode::UnsupportedSurface,
        ffi::REFUSAL_UNSUPPORTED_TRIM => RefusalCode::UnsupportedTrim,
        _ => RefusalCode::InvalidDefinition,
    }
}

/// The export's `source` code of a periodic B-spline surface, which OCCT
/// evaluates as periodic past the exported net's domain.
const SOURCE_PERIODIC_BSPLINE: i32 = 6;

/// A face's joint table from its exported trims: the gap lines as bridges,
/// each trim's end vertex where its numbers are finite, and whether OCCT
/// evaluates the face's surface as periodic.
fn joints_of(raw: &ffi::Face, trims: &[ffi::Trim]) -> BoxJoints {
    BoxJoints {
        periodic: raw.source == SOURCE_PERIODIC_BSPLINE,
        bridges: trims
            .iter()
            .enumerate()
            .filter(|(_, t)| t.gap != 0)
            .map(|(i, _)| i)
            .collect(),
        ends: trims
            .iter()
            .map(|t| {
                let finite = t.vertex.iter().all(|x| x.is_finite())
                    && t.vertex_tolerance.is_finite()
                    && t.vertex_tolerance >= 0.0;
                finite.then(|| Joint {
                    vertex: v3(t.vertex),
                    tolerance: t.vertex_tolerance,
                })
            })
            .collect(),
    }
}

fn refuse(code: RefusalCode, detail: impl Into<String>) -> Refusal {
    Refusal::new(code, detail)
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn close(a: &[f64], b: &[f64], scale: f64) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= EXPORT_TOLERANCE * (1.0 + scale))
}

fn max_abs(a: &[f64]) -> f64 {
    a.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
}

fn mismatch(what: &str, kernel: &[f64], export: &[f64]) -> Refusal {
    refuse(
        RefusalCode::ExportMismatch,
        format!("{what}: OCCT {kernel:?}, export {export:?}"),
    )
}

/// How far the file's tolerance can move the centre of mass, per axis: the
/// surface may lie anywhere within each solid's tolerance `t` of where it is
/// integrated, which moves the moments about the centre `c` by at most
/// `t ∫ |x - c| dA <= t A R` and the volume by at most `t A`, so the centre
/// moves by at most `t A R / (V - b_V - t A)`. `t A` is the volume's gap
/// (summed over the solids); `R` is the farthest corner of the solids' boxes
/// from `c`, widened by the centre's own bound and the largest tolerance.
/// Infinite when the volume cannot bound the denominator away from zero;
/// `None` without a file tolerance.
#[must_use]
pub fn centroid_gap(volume: &Fact, centroid: &PositionFact, boxes: &[[Vec3; 2]]) -> Option<f64> {
    let gap = volume.accuracy.gap_bound?;
    let t = volume.accuracy.file_tolerance.unwrap_or(0.0);
    let c = centroid.value;
    let corner = |[lo, hi]: &[Vec3; 2]| {
        let far = |c: f64, lo: f64, hi: f64| (c - lo).abs().max((hi - c).abs());
        Vec3::new(
            far(c.x, lo.x, hi.x),
            far(c.y, lo.y, hi.y),
            far(c.z, lo.z, hi.z),
        )
        .length()
    };
    let reach = boxes.iter().map(corner).fold(0.0, f64::max) + centroid.axis_bounds.length() + t;
    let floor = volume.value - volume.accuracy.bound.unwrap_or(f64::INFINITY) - gap;
    let moved = gap * reach / floor;
    Some(if floor > 0.0 && moved.is_finite() && reach.is_finite() {
        (moved * (1.0 + 8.0 * f64::EPSILON)).next_up()
    } else {
        f64::INFINITY
    })
}

/// Several solids' facts combined in solid order (see [`ShapeMass`]). The
/// sums carry their fold's round-off; the centre of mass is taken about the
/// first solid's, so each solid's distance from it scales its volume bound.
/// `boxes` holds each solid's box, for the centre's gap ([`centroid_gap`]).
///
/// # Errors
///
/// [`RefusalCode::InvalidDefinition`] for no solid, [`RefusalCode::NonFinite`]
/// or [`RefusalCode::NonPositiveVolume`] for sums that cannot be bounded.
pub fn combine(facts: &[BodyFacts], boxes: &[[Vec3; 2]]) -> Result<ShapeMass, Refusal> {
    let mut mass = combine_values(facts)?;
    if let Some(c) = mass.centroid.as_mut() {
        c.accuracy.gap_bound = centroid_gap(&mass.volume, c, boxes);
    }
    Ok(mass)
}

fn combine_values(facts: &[BodyFacts]) -> Result<ShapeMass, Refusal> {
    let [first, rest @ ..] = facts else {
        return Err(refuse(RefusalCode::InvalidDefinition, "no solid"));
    };
    if rest.is_empty() {
        return Ok(ShapeMass {
            area: first.area,
            volume: first.volume,
            centroid: first.centroid,
            bodies: 1,
        });
    }
    let n = facts.len() as f64;
    let fold = f64::EPSILON * n;
    let exact = facts.iter().all(|f| {
        f.area.accuracy.method == Method::ExactByDegree
            && f.volume.accuracy.method == Method::ExactByDegree
    });
    let method = if exact {
        Method::ExactByDegree
    } else {
        Method::Adaptive
    };
    let file_tolerance = facts
        .iter()
        .filter_map(|f| f.volume.accuracy.file_tolerance)
        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
    let sum = |fact: fn(&BodyFacts) -> &Fact| {
        let (mut value, mut bound, mut magnitude) = (0.0, 0.0, 0.0);
        let mut gap = Some(0.0);
        for f in facts {
            let x = fact(f);
            value += x.value;
            magnitude += x.value.abs();
            bound += x.accuracy.bound.unwrap_or(f64::INFINITY);
            gap = gap.zip(x.accuracy.gap_bound).map(|(a, b)| a + b);
        }
        (value, bound + fold * magnitude, gap)
    };
    let record = |bound: f64, scale: Scale, gap_bound: Option<f64>| FactAccuracy {
        bound: Some(bound),
        method,
        scale,
        file_tolerance,
        gap_bound,
        closed: Some(true),
    };
    let (area, area_b, _) = sum(|f| &f.area);
    let (volume, volume_b, volume_gap) = sum(|f| &f.volume);
    if !(area.is_finite() && area_b.is_finite() && volume.is_finite() && volume_b.is_finite()) {
        return Err(refuse(RefusalCode::NonFinite, "shape integrals not finite"));
    }
    let mut centroid = None;
    let centres: Option<Vec<&PositionFact>> = facts.iter().map(|f| f.centroid.as_ref()).collect();
    if let (Some(centres), Some(origin)) = (centres, first.centroid.map(|c| c.value)) {
        if volume - volume_b <= 0.0 {
            return Err(refuse(
                RefusalCode::NonPositiveVolume,
                format!("volume {volume} within ±{volume_b} is not positive"),
            ));
        }
        // Per axis, M = sum V_i (c_i - o), with
        // |dM| <= sum |V_i| b_ci + |c_i - o| b_Vi + b_Vi b_ci plus rounding.
        let axis = |get: fn(Vec3) -> f64| {
            let o = get(origin);
            let (mut moment, mut moment_b, mut moment_abs) = (0.0, 0.0, 0.0);
            for (f, c) in facts.iter().zip(&centres) {
                let v = f.volume.value;
                let bv = f.volume.accuracy.bound.unwrap_or(f64::INFINITY);
                let (p, b) = (get(c.value), get(c.axis_bounds));
                let d = p - o;
                moment += v * d;
                moment_abs += (v * d).abs();
                moment_b += v.abs() * b
                    + d.abs() * bv
                    + bv * b
                    + f64::EPSILON * (p.abs() + o.abs()) * v.abs();
            }
            let rel = moment / volume;
            let bound = (moment_b + fold * moment_abs + rel.abs() * volume_b) / (volume - volume_b)
                + 2.0 * f64::EPSILON * (o.abs() + 2.0 * rel.abs());
            (o + rel, bound)
        };
        let (x, y, z) = (axis(|v| v.x), axis(|v| v.y), axis(|v| v.z));
        let value = Vec3::new(x.0, y.0, z.0);
        let axis_bounds = Vec3::new(x.1, y.1, z.1);
        if !(value.is_finite() && axis_bounds.is_finite()) {
            return Err(refuse(
                RefusalCode::NonFinite,
                format!("centre of mass {value:?} ±{axis_bounds:?} not finite"),
            ));
        }
        centroid = Some(PositionFact {
            value,
            axis_bounds,
            // The shape's gap is set by `combine` from the solids' boxes.
            accuracy: record(axis_bounds.max_abs(), Scale::Positional, None),
        });
    }
    Ok(ShapeMass {
        area: Fact {
            value: area,
            accuracy: record(area_b, Scale::Extensive, None),
        },
        volume: Fact {
            value: volume,
            accuracy: record(volume_b, Scale::Extensive, volume_gap),
        },
        centroid,
        bodies: facts.len(),
    })
}

impl Subject {
    /// Opens STEP bytes through the bridge's admission.
    ///
    /// # Errors
    ///
    /// The bridge's message when admission or the export fails.
    pub fn open_step(bytes: &[u8]) -> Result<Self, String> {
        let mut error = ffi::ErrorBuffer::new();
        let mut document = std::ptr::null_mut();
        // SAFETY: the bytes are borrowed for the call; the out pointer is valid.
        let status = unsafe {
            ffi::geospec_occt_open_step(bytes.as_ptr(), bytes.len(), &mut document, error.raw())
        };
        if status != ffi::OK {
            return Err(format!("admission {status}: {}", error.message()));
        }
        let document = NonNull::new(document).ok_or("admission returned no document")?;
        let mut export = std::ptr::null_mut();
        // SAFETY: the document is live; it is released after the export.
        let status = unsafe {
            ffi::geospec_occt_facts_export_new(document.as_ptr(), &mut export, error.raw())
        };
        let Some(export) = NonNull::new(export).filter(|_| status == ffi::OK) else {
            // SAFETY: the document came from the bridge and is released once.
            unsafe { ffi::geospec_occt_release(document.as_ptr()) };
            return Err(format!("export {status}: {}", error.message()));
        };
        Ok(Self {
            document: Some(document),
            export,
            error,
        })
    }

    /// An export of a document the caller owns and keeps alive, and uses on
    /// this thread only, for as long as the subject lives: the engine's own
    /// admitted document.
    ///
    /// # Errors
    ///
    /// The export's message.
    ///
    /// # Safety
    ///
    /// `document` is a live `geospec_occt_document` that outlives the
    /// subject; nothing else uses it while the subject does.
    pub unsafe fn of_document(document: *const std::ffi::c_void) -> Result<Self, String> {
        let mut error = ffi::ErrorBuffer::new();
        let mut export = std::ptr::null_mut();
        // SAFETY: the caller keeps the document live; the out pointer is valid.
        let status = unsafe {
            ffi::geospec_occt_facts_export_new(
                document.cast::<ffi::Document>(),
                &mut export,
                error.raw(),
            )
        };
        let export = NonNull::new(export)
            .filter(|_| status == ffi::OK)
            .ok_or_else(|| format!("export {status}: {}", error.message()))?;
        Ok(Self {
            document: None,
            export,
            error,
        })
    }

    /// The box of the whole shape with its bracket: every solid's box-only
    /// body ([`Subject::box_body`]) boxed by [`boxes_within_target`] and
    /// folded in solid order. The bodies are exported on the calling thread
    /// and their faces bracketed on `options.threads`; the result does not
    /// depend on the thread count. Solids are exported and boxed in order
    /// and the first refusing solid stops the shape: no later solid is
    /// exported or refined.
    ///
    /// # Errors
    ///
    /// The first refusing solid's refusal (its export's, its box's, or
    /// `bracket-too-wide` when its bracket is, or a lower bound found while
    /// it is refined shows it must be, wider than its target), or
    /// `free-geometry` / `no-solid` for a shape whose box the solids alone
    /// do not give.
    pub fn shape_box(&mut self, options: BoxOptions) -> Result<ShapeBox, ShapeBoxRefusal> {
        let free = self
            .free_counts()
            .map_err(|m| ShapeBoxRefusal::shape(RefusalCode::InvalidDefinition.as_str(), m))?;
        if free.faces + free.edges + free.vertices > 0 {
            return Err(ShapeBoxRefusal::shape(
                "free-geometry",
                format!(
                    "{} face(s) outside solids, {} edge(s) outside faces, {} vertex(es) outside edges",
                    free.faces, free.edges, free.vertices
                ),
            ));
        }
        let count = self.body_count();
        if count == 0 {
            return Err(ShapeBoxRefusal::shape(
                "no-solid",
                "the shape has no solid".to_string(),
            ));
        }
        let boxes = boxes_within_target(count, |index| self.box_body(index), options).map_err(
            |(index, stop)| match stop {
                BoxStop::Export(r) | BoxStop::Refused(r) => ShapeBoxRefusal::of(r, index),
                BoxStop::TooWide {
                    width,
                    target,
                    at_least,
                } => ShapeBoxRefusal::too_wide(index, width, target, at_least, options.accuracy),
            },
        )?;
        let mut inner = [f64::NEG_INFINITY; SIDES];
        let mut outer = [f64::NEG_INFINITY; SIDES];
        let mut tolerant = [f64::NEG_INFINITY; SIDES];
        let mut kernel_tolerance = 0.0_f64;
        for b in boxes {
            let [lo, hi] = b.kernel_outer().unwrap_or_else(|| b.outer());
            let grown = [-lo.x, -lo.y, -lo.z, hi.x, hi.y, hi.z];
            for s in 0..SIDES {
                if let (Some(i), Some(o), Some(t), Some(bi), Some(bo), Some(g)) = (
                    inner.get_mut(s),
                    outer.get_mut(s),
                    tolerant.get_mut(s),
                    b.sides.inner.get(s),
                    b.sides.outer.get(s),
                    grown.get(s),
                ) {
                    *i = i.max(*bi);
                    *o = o.max(*bo);
                    *t = t.max(*g);
                }
            }
            kernel_tolerance = kernel_tolerance.max(b.kernel_tolerance.unwrap_or(0.0));
        }
        let to_box = |s: [f64; SIDES]| {
            let [a, b, c, d, e, f] = s;
            [Vec3::new(-a, -b, -c), Vec3::new(d, e, f)]
        };
        Ok(ShapeBox {
            inner: to_box(inner),
            outer: to_box(outer),
            outer_tolerant: to_box(tolerant),
            kernel_tolerance,
            bodies: count,
        })
    }

    /// The mass facts of the whole shape (P6): each solid's facts body
    /// ([`Subject::mass_body`]) integrated by [`bodies_facts`] on
    /// `options.threads` and combined in solid order ([`ShapeMass`]). Solids
    /// are exported in order on the calling thread; an export refusal stops
    /// the export, and only the solids before it are integrated. The first
    /// refusing solid in order decides the refusal, so the result does not
    /// depend on the thread count.
    ///
    /// # Errors
    ///
    /// The first refusing solid's refusal: `open-shell` for a solid whose
    /// shells are not closed (edge-use parity), else its export's or its
    /// facts' [`RefusalCode`]; or `free-geometry` / `no-solid` for a shape
    /// whose mass properties its solids alone do not give.
    pub fn shape_mass(&mut self, options: Options) -> Result<ShapeMass, ShapeBoxRefusal> {
        let free = self
            .free_counts()
            .map_err(|m| ShapeBoxRefusal::shape(RefusalCode::InvalidDefinition.as_str(), m))?;
        if free.faces + free.edges + free.vertices > 0 {
            return Err(ShapeBoxRefusal::shape(
                "free-geometry",
                format!(
                    "{} face(s) outside solids, {} edge(s) outside faces, {} vertex(es) outside edges",
                    free.faces, free.edges, free.vertices
                ),
            ));
        }
        let count = self.body_count();
        if count == 0 {
            return Err(ShapeBoxRefusal::shape(
                "no-solid",
                "the shape has no solid".to_string(),
            ));
        }
        let mut definitions = Vec::with_capacity(count);
        let mut stop = None;
        for index in 0..count {
            match self.mass_body(index) {
                Ok(d) => definitions.push(d),
                Err(r) => {
                    stop = Some(r);
                    break;
                }
            }
        }
        let mut facts = Vec::with_capacity(definitions.len());
        for (index, r) in bodies_facts(&definitions, options).into_iter().enumerate() {
            facts.push(r.map_err(|r| ShapeBoxRefusal::of(r, index))?);
        }
        if let Some(r) = stop {
            return Err(r);
        }
        let boxes: Vec<[Vec3; 2]> = definitions.iter().filter_map(|d| d.bounds).collect();
        combine(&facts, &boxes).map_err(|r| ShapeBoxRefusal::shape(r.code.as_str(), r.detail))
    }

    /// Body `index` for the mass facts: the facts' definition of every face
    /// (a face with a tolerance gap refuses), the solid's largest tolerance
    /// as its file tolerance, stopping at the first refusing face.
    ///
    /// # Errors
    ///
    /// `open-shell` for a solid whose shells are not closed, or the solid's
    /// or the first face's refusal.
    pub fn mass_body(&mut self, index: usize) -> Result<BodyDefinition, ShapeBoxRefusal> {
        let mut b = ffi::Body::default();
        // SAFETY: the export is live and the output valid.
        let status = unsafe {
            ffi::geospec_occt_facts_body_box(self.export.as_ptr(), index, &mut b, self.error.raw())
        };
        let of = |r: Refusal| ShapeBoxRefusal::of(r, index);
        if status != ffi::OK {
            return Err(of(refuse(
                RefusalCode::InvalidDefinition,
                format!("OCCT body: {}", self.error.message()),
            )));
        }
        if b.closed == 0 {
            return Err(ShapeBoxRefusal {
                code: "open-shell",
                detail:
                    "the solid's shells are not closed (an edge has an odd number of face uses)"
                        .into(),
                body: Some(index),
                face: None,
            });
        }
        let invalid = |d: &str| of(refuse(RefusalCode::InvalidDefinition, d.to_string()));
        match b.placement {
            ffi::RIGID => {}
            ffi::MIRRORED => return Err(invalid("mirrored placement")),
            _ => return Err(invalid("scaled placement")),
        }
        if b.box_void != 0 {
            return Err(invalid("solid without a box"));
        }
        if !(b.max_tolerance.is_finite() && b.max_tolerance >= 0.0) {
            return Err(invalid("the solid's tolerance is not finite"));
        }
        let (lo, hi) = (v3(b.box_min), v3(b.box_max));
        let size = (hi.x - lo.x)
            .max(hi.y - lo.y)
            .max(hi.z - lo.z)
            .max(f64::MIN_POSITIVE);
        let mut stats = ExportStats::default();
        let mut faces = Vec::with_capacity(b.face_count);
        for f in 0..b.face_count {
            let d = self
                .face_definition(index, f, size, &mut stats)
                .facts()
                .map_err(|r| of(r.at_face(f)))?;
            faces.push(d);
        }
        Ok(BodyDefinition {
            centre: Vec3::new(
                0.5 * (lo.x + hi.x),
                0.5 * (lo.y + hi.y),
                0.5 * (lo.z + hi.z),
            ),
            size,
            bounds: Some([lo, hi]),
            faces,
            file_tolerance: Some(b.max_tolerance),
            closed: Some(true),
        })
    }

    /// Body `index` for the box alone: as [`BodyExport::boxed`], without
    /// OCCT's measurement of the solid, stopping at the first face whose box
    /// export refuses.
    ///
    /// # Errors
    ///
    /// The solid's or the first face's refusal.
    pub fn box_body(&mut self, index: usize) -> Result<BoxBody, Refusal> {
        let mut b = ffi::Body::default();
        // SAFETY: the export is live and the output valid.
        let status = unsafe {
            ffi::geospec_occt_facts_body_box(self.export.as_ptr(), index, &mut b, self.error.raw())
        };
        if status != ffi::OK {
            return Err(refuse(
                RefusalCode::InvalidDefinition,
                format!("OCCT body: {}", self.error.message()),
            ));
        }
        let invalid = |d: &str| refuse(RefusalCode::InvalidDefinition, d.to_string());
        match b.placement {
            ffi::RIGID => {}
            ffi::MIRRORED => return Err(invalid("mirrored placement")),
            _ => return Err(invalid("scaled placement")),
        }
        if b.box_void != 0 {
            return Err(invalid("solid without a box"));
        }
        let (lo, hi) = (v3(b.box_min), v3(b.box_max));
        let size = (hi.x - lo.x)
            .max(hi.y - lo.y)
            .max(hi.z - lo.z)
            .max(f64::MIN_POSITIVE);
        let mut stats = ExportStats::default();
        let mut faces = Vec::with_capacity(b.face_count);
        let mut joints = Vec::with_capacity(b.face_count);
        for f in 0..b.face_count {
            let (d, j) = self
                .face_definition(index, f, size, &mut stats)
                .boxed
                .map_err(|r| r.at_face(f))?;
            faces.push(d);
            joints.push(j);
        }
        let definition = BodyDefinition {
            centre: Vec3::new(
                0.5 * (lo.x + hi.x),
                0.5 * (lo.y + hi.y),
                0.5 * (lo.z + hi.z),
            ),
            size,
            bounds: Some([lo, hi]),
            faces,
            file_tolerance: None,
            closed: Some(b.closed != 0),
        };
        BoxBody::new(definition, joints, Some(b.max_tolerance))
    }

    /// A synthetic shape of the export's test kinds.
    ///
    /// # Errors
    ///
    /// The export's message for an unknown kind.
    pub fn synthetic(kind: i32) -> Result<Self, String> {
        let mut error = ffi::ErrorBuffer::new();
        let mut export = std::ptr::null_mut();
        // SAFETY: the out pointer is valid.
        let status =
            unsafe { ffi::geospec_occt_facts_export_synthetic(kind, &mut export, error.raw()) };
        let export = NonNull::new(export)
            .filter(|_| status == ffi::OK)
            .ok_or_else(|| format!("synthetic {kind}: {}", error.message()))?;
        Ok(Self {
            document: None,
            export,
            error,
        })
    }

    /// Solids in the shape.
    #[must_use]
    pub fn body_count(&self) -> usize {
        // SAFETY: the export is live.
        unsafe { ffi::geospec_occt_facts_body_count(self.export.as_ptr()) }
    }

    /// Faces outside every solid (not exported).
    #[must_use]
    pub fn free_faces(&self) -> usize {
        // SAFETY: the export is live.
        unsafe { ffi::geospec_occt_facts_free_face_count(self.export.as_ptr()) }
    }

    /// The bridge's whole-shape facts.
    ///
    /// # Errors
    ///
    /// The bridge's message.
    pub fn shape_facts(&mut self) -> Result<ShapeFacts, String> {
        let mut f = ShapeFacts {
            volume: f64::NAN,
            area: f64::NAN,
            centre: [f64::NAN; 3],
            ms: 0.0,
        };
        // SAFETY: the export is live and every output is valid.
        let status = unsafe {
            ffi::geospec_occt_facts_shape(
                self.export.as_ptr(),
                &mut f.volume,
                &mut f.area,
                f.centre.as_mut_ptr(),
                &mut f.ms,
                self.error.raw(),
            )
        };
        if status == ffi::OK {
            Ok(f)
        } else {
            Err(self.error.message())
        }
    }

    /// `BRepGProp` at relative accuracy `eps` on solid `index`.
    ///
    /// # Errors
    ///
    /// The bridge's message.
    pub fn eps_reference(&mut self, index: usize, eps: f64) -> Result<EpsReference, String> {
        let mut out = [f64::NAN; 8];
        // SAFETY: the export is live and `out` holds the 8 outputs.
        let status = unsafe {
            ffi::geospec_occt_facts_body_eps(
                self.export.as_ptr(),
                index,
                eps,
                out.as_mut_ptr(),
                self.error.raw(),
            )
        };
        if status != ffi::OK {
            return Err(self.error.message());
        }
        let [volume, area, cx, cy, cz, volume_error, area_error, ms] = out;
        Ok(EpsReference {
            volume,
            area,
            centre: [cx, cy, cz],
            volume_error,
            area_error,
            ms,
        })
    }

    /// `BRepGProp` without `Eps` on solid `index`, as the export measures it
    /// (`BodyRecord::gprop`), for repeated timing.
    ///
    /// # Errors
    ///
    /// The bridge's message.
    pub fn gprop(&mut self, index: usize) -> Result<Gprop, String> {
        let mut out = [f64::NAN; 6];
        // SAFETY: the export is live and `out` holds the 6 outputs.
        let status = unsafe {
            ffi::geospec_occt_facts_body_gprop(
                self.export.as_ptr(),
                index,
                out.as_mut_ptr(),
                self.error.raw(),
            )
        };
        if status != ffi::OK {
            return Err(self.error.message());
        }
        let [volume, area, cx, cy, cz, ms] = out;
        Ok(Gprop {
            volume,
            area,
            centre: [cx, cy, cz],
            ms,
        })
    }

    /// The bridge's per-face `AddOptimal` box of solid `index`, folded.
    ///
    /// # Errors
    ///
    /// The bridge's message.
    pub fn addoptimal_box(&mut self, index: usize) -> Result<AddOptimalBox, String> {
        let mut out = [f64::NAN; 8];
        // SAFETY: the export is live and `out` holds the 8 outputs.
        let status = unsafe {
            ffi::geospec_occt_facts_body_addoptimal(
                self.export.as_ptr(),
                index,
                out.as_mut_ptr(),
                self.error.raw(),
            )
        };
        if status != ffi::OK {
            return Err(self.error.message());
        }
        let [x0, y0, z0, x1, y1, z1, ms, voids] = out;
        let bounds = [x0, y0, z0, x1, y1, z1]
            .iter()
            .all(|x| x.is_finite())
            .then_some([[x0, y0, z0], [x1, y1, z1]]);
        Ok(AddOptimalBox {
            bounds,
            ms,
            // A count the export wrote as a double.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            void_faces: voids as usize,
        })
    }

    /// Faces outside solids, edges outside faces and vertices outside edges.
    ///
    /// # Errors
    ///
    /// The bridge's message.
    pub fn free_counts(&mut self) -> Result<FreeCounts, String> {
        let mut out = [0usize; 3];
        // SAFETY: the export is live and `out` holds the 3 outputs.
        let status = unsafe {
            ffi::geospec_occt_facts_free_counts(
                self.export.as_ptr(),
                out.as_mut_ptr(),
                self.error.raw(),
            )
        };
        if status != ffi::OK {
            return Err(self.error.message());
        }
        let [faces, edges, vertices] = out;
        Ok(FreeCounts {
            faces,
            edges,
            vertices,
        })
    }

    fn body_record(&mut self, index: usize) -> Result<(BodyRecord, ffi::Body), Refusal> {
        let mut b = ffi::Body::default();
        // SAFETY: the export is live and the output valid.
        let status = unsafe {
            ffi::geospec_occt_facts_body(self.export.as_ptr(), index, &mut b, self.error.raw())
        };
        if status != ffi::OK {
            return Err(refuse(
                RefusalCode::InvalidDefinition,
                format!("OCCT body: {}", self.error.message()),
            ));
        }
        let record = BodyRecord {
            faces: b.face_count,
            shells: b.shell_count,
            closed: b.closed != 0,
            placement: match b.placement {
                ffi::RIGID => "rigid",
                ffi::MIRRORED => "mirrored",
                _ => "scaled",
            },
            max_tolerance: b.max_tolerance,
            gprop: Gprop {
                volume: b.gprop_volume,
                area: b.gprop_area,
                centre: b.gprop_centre,
                ms: b.gprop_ms,
            },
        };
        Ok((record, b))
    }

    /// Body `index`: its record, its definition (or the first refusal) and
    /// the export's counts over every face.
    ///
    /// # Errors
    ///
    /// A refusal when the solid itself cannot be read.
    pub fn export_body(&mut self, index: usize) -> Result<BodyExport, Refusal> {
        let (record, raw) = self.body_record(index)?;
        let mut stats = ExportStats::default();
        let (definition, boxed) = self.body_definition(index, &record, &raw, &mut stats);
        Ok(BodyExport {
            record,
            definition,
            boxed,
            stats,
        })
    }

    /// The joint table of face `face` of body `body`, after
    /// [`Subject::export_body`] exported that body: its gap lines, and per
    /// trim the vertex at its traversal end with its tolerance (for a gap
    /// line, the vertex whose gap it closes).
    ///
    /// # Errors
    ///
    /// The face's refusal when the export has no trims for it.
    pub fn export_face_joints(&mut self, body: usize, face: usize) -> Result<BoxJoints, Refusal> {
        let raw = self.raw_face(body, face)?;
        if raw.refusal != 0 {
            return Err(refuse(refusal_code(raw.refusal), self.error.message()));
        }
        // SAFETY: the export keeps the face's arrays until its next call.
        let trims = unsafe { ffi::slice(raw.trims, raw.trim_count) };
        Ok(joints_of(&raw, trims))
    }

    /// Face `face` of body `body` alone, after [`Subject::export_body`]
    /// exported that body (for diagnostics).
    ///
    /// # Errors
    ///
    /// The face's refusal.
    pub fn export_face(&mut self, body: usize, face: usize) -> Result<FaceDefinition, Refusal> {
        let mut stats = ExportStats::default();
        self.face_definition(body, face, f64::MIN_POSITIVE, &mut stats)
            .facts()
    }

    fn body_definition(
        &mut self,
        index: usize,
        record: &BodyRecord,
        raw: &ffi::Body,
        stats: &mut ExportStats,
    ) -> (Result<BodyDefinition, Refusal>, Result<BoxBody, Refusal>) {
        let invalid = |d: &str| refuse(RefusalCode::InvalidDefinition, d.to_string());
        let placement = if record.placement == "rigid" {
            Ok(())
        } else {
            Err(invalid(&format!("{} placement", record.placement)))
        };
        if raw.box_void != 0 {
            let r = invalid("solid without a box");
            return (Err(r.clone()), Err(r));
        }
        let (lo, hi) = (v3(raw.box_min), v3(raw.box_max));
        let centre = Vec3::new(
            0.5 * (lo.x + hi.x),
            0.5 * (lo.y + hi.y),
            0.5 * (lo.z + hi.z),
        );
        let size = (hi.x - lo.x)
            .max(hi.y - lo.y)
            .max(hi.z - lo.z)
            .max(f64::MIN_POSITIVE);
        let mut faces = Vec::with_capacity(record.faces);
        let mut first: Option<Refusal> = placement.err();
        let mut boxed_faces = Vec::with_capacity(record.faces);
        let mut joints = Vec::with_capacity(record.faces);
        let mut boxed_first = first.clone();
        for f in 0..record.faces {
            let e = self.face_definition(index, f, size, stats);
            match e.facts() {
                Ok(d) => faces.push(d),
                Err(r) => {
                    if first.is_none() {
                        first = Some(r.at_face(f));
                    }
                }
            }
            match e.boxed {
                Ok((d, j)) => {
                    boxed_faces.push(d);
                    joints.push(j);
                }
                Err(r) => {
                    if boxed_first.is_none() {
                        boxed_first = Some(r.at_face(f));
                    }
                }
            }
        }
        let body = |faces| BodyDefinition {
            centre,
            size,
            bounds: Some([lo, hi]),
            faces,
            file_tolerance: None,
            closed: Some(record.closed),
        };
        let boxed = match boxed_first {
            Some(r) => Err(r),
            None => BoxBody::new(body(boxed_faces), joints, Some(record.max_tolerance)),
        };
        let definition = match first {
            Some(r) => Err(r),
            None => Ok(body(faces)),
        };
        (definition, boxed)
    }

    /// One face's definition for the facts, and for the box with its joint
    /// table, from one export of the face.
    fn face_definition(
        &mut self,
        body: usize,
        face: usize,
        size: f64,
        stats: &mut ExportStats,
    ) -> FaceExport {
        match self.face_export(body, face, size, stats) {
            Ok(e) => e,
            Err(r) => FaceExport {
                gapped: None,
                boxed: Err(r),
            },
        }
    }

    fn face_export(
        &mut self,
        body: usize,
        face: usize,
        size: f64,
        stats: &mut ExportStats,
    ) -> Result<FaceExport, Refusal> {
        let raw = self.raw_face(body, face)?;
        if raw.orientation == ffi::REVERSED {
            stats.reversed_faces += 1;
        }
        // Coverage counts first, so that a refused face is counted too.
        *stats.surfaces.entry(surface_name(raw.source)).or_default() += 1;
        if raw.reflected != 0 {
            stats.reflected_faces += 1;
        }
        if raw.moved_origin != 0 {
            stats.moved_origin_faces += 1;
        }
        if raw.shift != [0.0, 0.0] {
            stats.shifted_faces += 1;
        }
        // SAFETY: the export keeps the face's arrays until its next call;
        // a refused face's are empty or null.
        let (trims_raw, loops) = unsafe {
            (
                ffi::slice(raw.trims, raw.trim_count),
                ffi::slice(raw.loops, raw.loop_count).to_vec(),
            )
        };
        let mut gaps = 0_usize;
        let mut widest_gap = 0.0_f64;
        for t in trims_raw {
            stats.seams += usize::from(t.seam != 0);
            stats.degenerate += usize::from(t.degenerate != 0);
            if t.gap != 0 {
                gaps += 1;
                let [a, b] = t.range;
                widest_gap = widest_gap.max(
                    (b - a)
                        * (t.direction[0] * t.direction[0] + t.direction[1] * t.direction[1])
                            .sqrt(),
                );
            } else {
                *stats.curves.entry(curve_name(t.source)).or_default() += 1;
            }
        }
        if gaps > 0 {
            stats.gap_faces.push((face, gaps));
            stats.gap_lines += gaps;
        }
        if raw.refusal != 0 {
            return Err(refuse(refusal_code(raw.refusal), self.error.message()));
        }
        // Closing a gap within the vertex tolerance changes the face by an
        // amount no integration bound covers: the facts refuse (fail
        // closed). The box takes the bridged face, checked as any other.
        let gapped = (gaps > 0).then(|| {
            refuse(
                RefusalCode::ToleranceGap,
                format!(
                    "{gaps} gap(s) between consecutive trims within the vertex tolerance, \
                     the widest {widest_gap:e} in the parameter plane"
                ),
            )
        });
        let joints = joints_of(&raw, trims_raw);
        let surface = match export_surface(&raw.surface) {
            Ok(s) => s,
            Err(r) => return Ok(FaceExport::refused(gapped, r)),
        };
        let revolution = !matches!(surface, Surface::Plane { .. } | Surface::Nurbs(_));
        let domain = match &surface {
            Surface::Nurbs(n) => Some(nurbs_domain(n)),
            _ => None,
        };
        let mut trims = Vec::with_capacity(trims_raw.len());
        let mut uvs = Vec::new();
        let mut past_two_pi = 0;
        for (i, t) in trims_raw.iter().enumerate() {
            let checked = export_trim(t).and_then(|piece| {
                piece.validate()?;
                let points = self.check_trim(i, t, &piece)?;
                Ok((piece, points))
            });
            let (piece, points) = match checked {
                Ok(c) => c,
                Err(r) => return Ok(FaceExport::refused(gapped, r)),
            };
            let tau = std::f64::consts::TAU;
            if revolution
                && points
                    .iter()
                    .any(|p| p[0] < -1e-9 * tau || p[0] > tau * (1.0 + 1e-9))
            {
                past_two_pi += 1;
            }
            for pair in points.windows(2) {
                if let [p, q] = pair {
                    uvs.push(*p);
                    uvs.push([0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])]);
                }
            }
            if let Some(last) = points.last() {
                uvs.push(*last);
            }
            trims.push(piece);
        }
        if let Err(r) = self.check_surface(&surface, &uvs, domain, size) {
            return Ok(FaceExport::refused(gapped, r));
        }
        // Counted as before: on the faces the facts take.
        if gapped.is_none() {
            stats.past_two_pi += past_two_pi;
        }
        let reflected = raw.reflected != 0;
        let (forward, material) = match raw.orientation {
            ffi::FORWARD => (true, Material::OneSide),
            ffi::REVERSED => (false, Material::OneSide),
            ffi::INTERNAL => (true, Material::BothSides),
            _ => (true, Material::Neither),
        };
        // A reflected frame negates the natural normal.
        let orientation = if forward != reflected {
            Orientation::Forward
        } else {
            Orientation::Reversed
        };
        let definition = FaceDefinition {
            surface,
            trims,
            loops,
            orientation,
            material,
        };
        Ok(FaceExport {
            gapped,
            boxed: Ok((definition, joints)),
        })
    }

    /// The export's record of one face; its arrays live until the next
    /// call.
    fn raw_face(&mut self, body: usize, face: usize) -> Result<ffi::Face, Refusal> {
        // SAFETY: zeroed is a valid bit pattern for the plain record (null
        // pointers, zero counts).
        let mut raw: ffi::Face = unsafe { std::mem::zeroed() };
        // SAFETY: the export is live and the output valid.
        let status = unsafe {
            ffi::geospec_occt_facts_face(
                self.export.as_ptr(),
                body,
                face,
                &mut raw,
                self.error.raw(),
            )
        };
        if status != ffi::OK {
            return Err(refuse(
                RefusalCode::InvalidDefinition,
                format!("OCCT face: {}", self.error.message()),
            ));
        }
        Ok(raw)
    }

    /// Checks a trim's export at seven parameters against the original 2D
    /// curve and returns its points there.
    fn check_trim(
        &mut self,
        index: usize,
        raw: &ffi::Trim,
        t: &TrimPiece,
    ) -> Result<Vec<[f64; 2]>, Refusal> {
        let [a, b] = t.range;
        let mut points = Vec::new();
        for i in 0..TRIM_SAMPLES {
            let s = a + (b - a) * f64::from(i) / f64::from(TRIM_SAMPLES - 1);
            let mut out = [0.0_f64; 5];
            // SAFETY: the export is live and `out` holds five values.
            let status = unsafe {
                ffi::geospec_occt_facts_trim_d1(
                    self.export.as_ptr(),
                    index,
                    s,
                    out.as_mut_ptr(),
                    self.error.raw(),
                )
            };
            if status != ffi::OK {
                return Err(refuse(
                    RefusalCode::ExportMismatch,
                    format!("OCCT trim evaluation: {}", self.error.message()),
                ));
            }
            let (ep, ed) = t.curve.evaluate(s)?;
            let (p, d, distance) = ([out[0], out[1]], [out[2], out[3]], out[4]);
            let scale = max_abs(&p);
            if !close(&p, &ep, scale) {
                return Err(mismatch("trim point", &p, &ep));
            }
            if raw.reparameterised != 0 {
                // A NaN distance refuses too.
                let on_curve = distance <= EXPORT_TOLERANCE * (1.0 + scale);
                if !on_curve {
                    return Err(mismatch(
                        "converted trim off its curve",
                        &[distance],
                        &[0.0],
                    ));
                }
            } else if !close(&d, &ed, max_abs(&d).max(max_abs(&ed))) {
                return Err(mismatch("trim derivative", &d, &ed));
            }
            points.push(ep);
        }
        Ok(points)
    }

    /// Checks the surface's export at `uvs` against the original surface: the
    /// point, `S_u` and `S_v`. Points outside a net's domain are left to the
    /// integrator, which refuses them.
    fn check_surface(
        &mut self,
        export: &Surface,
        uvs: &[[f64; 2]],
        domain: Option<[[f64; 2]; 2]>,
        size: f64,
    ) -> Result<(), Refusal> {
        for uv in uvs {
            if let Some([du, dv]) = domain {
                if uv[0] < du[0] || uv[0] > du[1] || uv[1] < dv[0] || uv[1] > dv[1] {
                    continue;
                }
            }
            let mut out = [0.0_f64; 9];
            // SAFETY: the export is live and `out` holds nine values.
            let status = unsafe {
                ffi::geospec_occt_facts_surface_d1(
                    self.export.as_ptr(),
                    uv[0],
                    uv[1],
                    out.as_mut_ptr(),
                    self.error.raw(),
                )
            };
            if status != ffi::OK {
                return Err(refuse(
                    RefusalCode::ExportMismatch,
                    format!("OCCT surface evaluation: {}", self.error.message()),
                ));
            }
            let (e, eu, ev) = export.evaluate(uv[0], uv[1])?;
            let p = out.get(0..3).unwrap_or(&[]);
            let scale = size.max(max_abs(p));
            if !close(p, &[e.x, e.y, e.z], scale) {
                return Err(mismatch(
                    &format!("surface point at {uv:?}"),
                    p,
                    &[e.x, e.y, e.z],
                ));
            }
            // Both derivatives against the larger one's magnitude: at a pole
            // one vanishes, and its error is relative to the other.
            let derivatives = max_abs(out.get(3..9).unwrap_or(&[]))
                .max(max_abs(&[eu.x, eu.y, eu.z, ev.x, ev.y, ev.z]));
            for (what, got, want) in [
                ("S_u", out.get(3..6).unwrap_or(&[]), eu),
                ("S_v", out.get(6..9).unwrap_or(&[]), ev),
            ] {
                if !close(got, &[want.x, want.y, want.z], derivatives) {
                    return Err(mismatch(
                        &format!("surface {what} at {uv:?}"),
                        got,
                        &[want.x, want.y, want.z],
                    ));
                }
            }
        }
        Ok(())
    }
}

fn nurbs_domain(n: &NurbsSurface) -> [[f64; 2]; 2] {
    let at = |k: &[f64], i: usize| k.get(i).copied().unwrap_or(f64::NAN);
    [
        [at(&n.knots_u, n.degree_u), at(&n.knots_u, n.count_u)],
        [at(&n.knots_v, n.degree_v), at(&n.knots_v, n.count_v)],
    ]
}

fn export_surface(s: &ffi::Surface) -> Result<Surface, Refusal> {
    let frame = Frame {
        origin: v3(s.origin),
        x: v3(s.x),
        y: v3(s.y),
        z: v3(s.z),
    };
    Ok(match s.kind {
        ffi::SURFACE_PLANE => Surface::Plane {
            origin: v3(s.origin),
            u_axis: v3(s.x),
            v_axis: v3(s.y),
        },
        ffi::SURFACE_CYLINDER => Surface::Cylinder {
            frame,
            radius: s.radius,
        },
        ffi::SURFACE_CONE => Surface::Cone {
            frame,
            radius: s.radius,
            semi_angle: s.semi_angle,
        },
        ffi::SURFACE_SPHERE => Surface::Sphere {
            frame,
            radius: s.radius,
        },
        ffi::SURFACE_TORUS => Surface::Torus {
            frame,
            major_radius: s.radius,
            minor_radius: s.minor_radius,
        },
        ffi::SURFACE_NURBS => {
            let count = s
                .count_u
                .checked_mul(s.count_v)
                .ok_or_else(|| refuse(RefusalCode::InvalidDefinition, "net size overflows"))?;
            // SAFETY: the export keeps the net's arrays until its next call,
            // with these lengths.
            let (knots_u, knots_v, poles, weights) = unsafe {
                (
                    ffi::slice(s.knots_u, s.knot_count_u).to_vec(),
                    ffi::slice(s.knots_v, s.knot_count_v).to_vec(),
                    ffi::slice(s.poles, 3 * count),
                    (!s.weights.is_null()).then(|| ffi::slice(s.weights, count).to_vec()),
                )
            };
            Surface::Nurbs(NurbsSurface {
                degree_u: s.degree_u,
                degree_v: s.degree_v,
                knots_u,
                knots_v,
                poles: poles
                    .chunks_exact(3)
                    .map(|p| match p {
                        [x, y, z] => Vec3::new(*x, *y, *z),
                        _ => Vec3::new(f64::NAN, f64::NAN, f64::NAN),
                    })
                    .collect(),
                weights,
                count_u: s.count_u,
                count_v: s.count_v,
            })
        }
        k => {
            return Err(refuse(
                RefusalCode::UnsupportedSurface,
                format!("export kind {k}"),
            ))
        }
    })
}

fn export_trim(t: &ffi::Trim) -> Result<TrimPiece, Refusal> {
    let curve = match t.kind {
        ffi::TRIM_LINE => Curve2::Line {
            origin: t.origin,
            direction: t.direction,
        },
        ffi::TRIM_NURBS => {
            // SAFETY: the export keeps the trim's arrays until its next call,
            // with these lengths.
            let (knots, poles, weights) = unsafe {
                (
                    ffi::slice(t.knots, t.knot_count).to_vec(),
                    ffi::slice(t.poles, 2 * t.pole_count),
                    (!t.weights.is_null()).then(|| ffi::slice(t.weights, t.pole_count).to_vec()),
                )
            };
            Curve2::Nurbs(NurbsCurve2 {
                degree: t.degree,
                knots,
                poles: poles
                    .chunks_exact(2)
                    .map(|p| match p {
                        [u, v] => [*u, *v],
                        _ => [f64::NAN, f64::NAN],
                    })
                    .collect(),
                weights,
            })
        }
        k => {
            return Err(refuse(
                RefusalCode::UnsupportedTrim,
                format!("export trim kind {k}"),
            ))
        }
    };
    Ok(TrimPiece {
        curve,
        range: t.range,
        reversed: t.reversed != 0,
    })
}

impl Drop for Subject {
    fn drop(&mut self) {
        // SAFETY: both came from the bridge; the export goes before the
        // document it reads.
        unsafe {
            ffi::geospec_occt_facts_export_release(self.export.as_ptr());
            if let Some(d) = self.document {
                ffi::geospec_occt_release(d.as_ptr());
            }
        }
    }
}

impl FaceDefinitions for Subject {
    fn body_count(&self) -> usize {
        Subject::body_count(self)
    }

    fn body(&mut self, index: usize) -> Result<BodyDefinition, Refusal> {
        self.export_body(index)?.definition
    }
}
