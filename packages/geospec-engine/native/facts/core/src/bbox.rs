//! A body's box with a bracket (program item P5).
//!
//! For each of the box's six sides the result holds two numbers: the `inner`
//! value, which a point of the body attains (so the true side lies at or
//! beyond it), and the `outer` value, which no point of the body exceeds. The
//! true box lies between the inner and the outer box; `bound`, the largest
//! difference over the six sides, is the bracket's width. Both numbers are
//! rigorous for the definition's geometry (the surfaces over their trims),
//! with every rounding error carried into them.
//!
//! Per face, the extreme of a coordinate over the face lies on the face's
//! boundary or at an interior point where the coordinate's gradient in the
//! parameter plane vanishes:
//!
//! - **Boundary.** Each trim piece is refined from its runs between the
//!   knots where it may not be C1, split at the knot nearest an interval's
//!   middle (or the middle): an interval's enclosure is the surface's
//!   enclosure over the box of the piece's control points there (on a plane
//!   or a flat bilinear net, the affine image of those control points), and
//!   its end points are attained. Where the surface's and the trim's first
//!   and second derivatives are bounded (analytic kinds; a net where the
//!   interval's control points cross no C0 knot line and stay within its
//!   domain), an interval is also bounded by its end values
//!   plus `k h^2 / 8`, `k` bounding the coordinate's second derivative along
//!   the piece: near an extreme this converges quadratically. A straight
//!   piece on an affine surface has its extremes at its ends. An
//!   iso-parametric line on an analytic surface is a segment or a circular
//!   arc whose extremes are in closed form. A closing chord between pieces
//!   that do not meet is bounded by its ends (the pieces' attained end
//!   points) plus the surface's first derivatives times its length, or else
//!   refined like a piece.
//! - **Interior, analytic kinds.** On planes, cylinders and cones a
//!   coordinate is affine in v, and on a bilinear net (degree 1 by 1, equal
//!   weights) harmonic, so its extremes lie on the boundary. On
//!   spheres and tori the critical points are in closed form. When none in
//!   the trims' box could move a side beyond its target they only bound the
//!   sides; otherwise each is classified against the trims (every periodic
//!   image in the trims' box) and counts when it is inside (attained) or
//!   cannot be shown outside (outer only).
//! - **Interior, B-spline nets.** The trims' box is split at the knots and
//!   bisected; a sub-patch's Bezier net (from the blossom) encloses it. A cell
//!   is dropped when it lies outside the trims, when its enclosure cannot
//!   beat the side's attained value by more than `epsilon`, or when the
//!   coordinate is monotone over it in u or in v (the rational hodograph's
//!   interval excludes zero) and the edge it increases towards is not a knot
//!   line of C0 continuity: there it has no interior extreme. Towards a C0
//!   knot line the side's extreme over the cell lies on that edge, and the
//!   cell drops when the edge curve's control points cannot beat the target.
//!   A cell inside the trims makes its corners (where the net interpolates
//!   the surface) attained.
//!
//! **Classification** against the trims is exact or undecided, never wrong: a
//! query box that the trims' control-point boxes (refined by bisection near
//! it) do not touch takes the parity of an upward ray against the chords of
//! those boxes, which are homotopic to the trims away from the query. It
//! relies on the loops' structure, so it runs only on faces whose loops pass
//! the facts layer's own loop check ([`FaceDefinition::check_loops`]). A face
//! whose loops do not pass is bounded over the box of its trims in the
//! parameter plane instead (the UV-rectangle hull, a rigorous over-bound of
//! the face; nothing inside it counts as attained), and the body reports it in
//! `fallback_faces`: its bracket is wider.
//!
//! The body's sides fold the faces' in face order. A first pass encloses
//! every face's pieces (each run once) and interior once; the second refines each face only until
//! its outer value is within `epsilon` of the larger of its own attained value
//! and the first pass's, so faces away from a side cost little. Both passes
//! are per-face tasks whose results do not depend on the order or the thread
//! that runs them, so the box is bit-identical for every thread count. A face
//! whose refinement reaches its work cap keeps the outer values it has (still
//! rigorous) and is reported in `capped_faces`.
//!
//! A trim point outside a bounded domain (a net's knots, a sphere's
//! latitudes) by up to a relative 1e-6 is evaluated at the nearest point of
//! the domain, and the face's inner and outer values widen by a bound on how
//! far a kernel that extrapolates the surface there moves from it; the body
//! reports it in `extrapolated_faces`. For a net the bound is exact up to
//! rounding: the end span's polynomial blossomed over the strip the trims
//! enter (corners included), whose control points bound the extension's
//! weights from below and its derivative across the strip. A net whose
//! extrapolated weights there are not shown positive refuses, and so does a
//! trim beyond the tolerance ([`RefusalCode::TrimOutsideDomain`]): the
//! definition does not say where such a face lies. The trims' excursion
//! carries their own evaluation's rounding.
//!
//! The widening bounds the exact extrapolated surface. A kernel's computed
//! value there can differ from it by the kernel's own rounding, which its
//! evaluation outside the span amplifies by the same growth as the blossom's
//! (OCCT's de Boor factors leave `[0, 1]` there): the box does not claim to
//! hold OCCT's computed extrapolated values beyond that rounding. A periodic
//! net whose trims cross its period by at most `1e-12` of it (which the OCCT
//! export leaves in place and makes non-periodic) is extrapolated here where
//! OCCT would wrap it; over that excursion the two differ by far less than
//! the export's own check (1e-9).
//!
//! **Which box holds what.** The outer box ([`BodyBox::outer`]) encloses the
//! definition's geometry: the surfaces over their trims. A kernel's own
//! geometry can lie off that definition by up to its tolerance (OCCT's
//! edges are 3D curves within their tolerance of the face's pcurves, and its
//! face classification counts points within tolerance as on the face), so
//! the box that holds OCCT's sampled points is
//! [`BodyBox::tolerant_outer`] for the body's largest tolerance. On the
//! 2,584 corpus files compared with M3's OCCT samples, 8 have a sample
//! outside the outer box (by at most 8e-6 of the body's tolerance) and none
//! outside the tolerant box. The record ([`BodyBox::accuracy`]) is
//! [`Method::Bracket`] with `bound` the bracket's width.
//!
//! **Bridged bodies.** A kernel's model can join two edges at a vertex
//! within its tolerance while their curves in the parameter plane do not
//! meet. A [`BoxBody`] given with a joint table closes each such gap with a
//! bridge (the straight join in the parameter plane, which the export
//! inserts) and is boxed through [`boxes_of`]: the box is the bracket of the
//! definition with its gaps bridged, never of a definition the facts
//! integrate. Bridges, like closing chords, are not attained: their points
//! raise only the outer values. Before refinement each bridge, each closing
//! chord and each exact joint of the body is tied to the vertex the table
//! gives for it ([`FaceGeo::guard_segment`]): per axis a bound on how far
//! its image lies from the vertex, from its end points, a second-order
//! bound along it (split at a net's C0 knot lines), its ends' rounding and,
//! past a bounded domain, the clamped image's and the extrapolated image's
//! distance. A bridge or chord whose bound exceeds its vertex's tolerance
//! (its cube), that has no vertex or no bound, refuses
//! ([`RefusalCode::ToleranceGap`]); a chord longer than the chaining
//! tolerance is a join left open ([`RefusalCode::OpenTrimLoop`]). The
//! largest bound is the body's [`BodyBox::gap_bound`], and every vertex's
//! ball lies in [`BodyBox::kernel_outer`], the outer box grown by the
//! kernel's tolerance plus that bound. A body without bridges is boxed bit
//! for bit as [`bodies_boxes`] boxes its definition.
//!
//! Preconditions, established by `validate`: degrees within the crate's
//! limits, knot vectors that fit their nets, weights that are positive normal
//! numbers. Indexing below relies on them.
// Coordinate loops index several arrays by axis.
#![allow(clippy::indexing_slicing, clippy::needless_range_loop)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::accuracy::{FactAccuracy, Method, Scale};
use crate::definition::{
    BodyDefinition, Curve2, FaceDefinition, Frame, NurbsSurface, Surface, TrimPiece,
};
use crate::loops;
use crate::refusal::{Refusal, RefusalCode};
use crate::surface::{Prepared, Row};
use crate::trim::PreparedTrim;
use crate::vec::Vec3;

const EPS: f64 = f64::EPSILON;
/// How far, relative to the domain's width, a trim may leave its surface's
/// parameter domain before the face refuses. Within it the surface is
/// evaluated clamped (the definition's semantics) and both ends of the
/// bracket move by a bound on how far the extrapolated surface lies from the
/// clamped one (see `FaceGeo::excursion_widening`), so the bracket also holds
/// a kernel that extrapolates the net instead.
const DOMAIN_TOLERANCE: f64 = 1e-6;
/// Bisections of one trim interval.
const MAX_T_DEPTH: u32 = 64;
/// Bisections of one parameter-plane cell.
const MAX_CELL_DEPTH: u32 = 56;
/// Bisections of one trim segment while classifying.
const MAX_CLASSIFY_DEPTH: u32 = 52;
/// Enclosures per face before its refinement stops.
const WORK_CAP: usize = 250_000;
/// A cell over at most this many knot spans is split at its knots.
const SPLIT_SPANS: usize = 16;
/// Query boxes are widened by this relative distance before the trims'
/// boxes are tested against them.
const CLASSIFY_TOLERANCE: f64 = 1e-12;
/// Samples along a line of critical points.
const LINE_SAMPLES: usize = 16;

/// The six sides in the order `-x, -y, -z, +x, +y, +z`; side `s` is the
/// largest value of `-coordinate` (`s < 3`) or `coordinate` (`s >= 3`).
pub const SIDES: usize = 6;

/// Box settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxOptions {
    /// The bracket's target width relative to the body's size: each side is
    /// refined until its width is under `accuracy` times the size.
    pub accuracy: f64,
    /// Threads for the per-face tasks (1 runs on the caller's thread).
    pub threads: usize,
}

impl Default for BoxOptions {
    fn default() -> Self {
        Self {
            accuracy: 1e-7,
            threads: 1,
        }
    }
}

/// One face's or body's sides: each as the largest value of `-x, -y, -z, x,
/// y, z` that a point attains (`inner`) and that no point exceeds (`outer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides {
    /// Values attained by points of the face or body.
    pub inner: [f64; SIDES],
    /// Values no point of the face or body exceeds.
    pub outer: [f64; SIDES],
}

impl Sides {
    const EMPTY: Self = Self {
        inner: [f64::NEG_INFINITY; SIDES],
        outer: [f64::NEG_INFINITY; SIDES],
    };

    /// The box spanned by the attained values: inside the true box.
    #[must_use]
    pub fn inner_box(&self) -> [Vec3; 2] {
        to_box(&self.inner)
    }

    /// The box spanned by the outer values: around the true box.
    #[must_use]
    pub fn outer_box(&self) -> [Vec3; 2] {
        to_box(&self.outer)
    }

    /// The largest width `outer - inner` over the sides.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.inner
            .iter()
            .zip(&self.outer)
            .fold(0.0_f64, |m, (i, o)| m.max(o - i))
    }
}

fn to_box(s: &[f64; SIDES]) -> [Vec3; 2] {
    [Vec3::new(-s[0], -s[1], -s[2]), Vec3::new(s[3], s[4], s[5])]
}

/// A face's bracket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    /// The sides.
    pub sides: Sides,
    /// Bounded over its trims' parameter box: its loops did not pass the
    /// loop check, so nothing inside the trims was classified.
    pub fallback: bool,
    /// Its refinement reached the work cap.
    pub capped: bool,
    /// Its trims leave the surface's domain (within the tolerance): both
    /// ends of every side moved by `excursion`.
    pub extrapolated: bool,
    /// How far each side's ends moved for trims outside the domain.
    pub excursion: f64,
    /// Enclosures computed.
    pub work: usize,
    /// On a bridged body, the largest bound over the axes and the face's
    /// bridges, closing chords and joints on how far their images lie from
    /// their vertices; 0 otherwise.
    pub gap_bound: f64,
}

/// A body's box with its bracket.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyBox {
    /// The sides, folded over the faces in face order.
    pub sides: Sides,
    /// The bracket's width: the largest `outer - inner` over the sides.
    pub bound: f64,
    /// The target width (`accuracy` times the body's size).
    pub epsilon: f64,
    /// Faces bounded over their trims' parameter box (loops not classified).
    pub fallback_faces: Vec<usize>,
    /// Faces whose refinement reached the work cap.
    pub capped_faces: Vec<usize>,
    /// Faces whose trims leave their surface's domain (within the tolerance).
    pub extrapolated_faces: Vec<usize>,
    /// Enclosures computed over every face.
    pub work: usize,
    /// The kernel's largest tolerance for the body (OCCT's largest edge,
    /// vertex and face tolerance), when the [`BoxBody`] carried one.
    pub kernel_tolerance: Option<f64>,
    /// Faces with bridges: the box is the bracket of the definition with
    /// each tolerance gap closed by its bridge.
    pub gap_faces: Vec<usize>,
    /// On a bridged body, the largest bound over the axes and every face's
    /// bridges, closing chords and joints on how far their images lie from
    /// their vertices; 0 otherwise.
    pub gap_bound: f64,
    /// The largest widening of a face for trims outside its domain.
    pub extrapolation: f64,
    /// The accuracy record: [`Method::Bracket`], `bound` as above, positional,
    /// with the body's file tolerance and closure.
    pub accuracy: FactAccuracy,
}

impl BodyBox {
    /// [`Self::tolerant_outer`] for the kernel's tolerance plus
    /// [`Self::gap_bound`]: the box around the kernel's own geometry for the
    /// body. On a bridged body every vertex lies within the gap bound of a
    /// point of the bridged definition, so its tolerance ball lies in this
    /// box. `None` when the body carried no kernel tolerance.
    #[must_use]
    pub fn kernel_outer(&self) -> Option<[Vec3; 2]> {
        let t = self.kernel_tolerance?;
        let grow = if self.gap_bound > 0.0 {
            (t + self.gap_bound).next_up()
        } else {
            t
        };
        Some(self.tolerant_outer(grow))
    }

    /// The outer box grown by `tolerance` on every side (rounding
    /// included): the box around a kernel's own geometry for the body when
    /// its edges and faces may lie off the definition by up to `tolerance`
    /// (for OCCT, the body's largest edge, vertex and face tolerance). The
    /// outer box bounds the definition only.
    #[must_use]
    pub fn tolerant_outer(&self, tolerance: f64) -> [Vec3; 2] {
        let t = tolerance.max(0.0);
        let grow = |x: f64, d: f64| {
            let y = x + d;
            y + d.signum() * 4.0 * EPS * y.abs()
        };
        let [lo, hi] = self.outer();
        [
            Vec3::new(grow(lo.x, -t), grow(lo.y, -t), grow(lo.z, -t)),
            Vec3::new(grow(hi.x, t), grow(hi.y, t), grow(hi.z, t)),
        ]
    }

    /// The box around the body: `[min, max]` of the outer values.
    #[must_use]
    pub fn outer(&self) -> [Vec3; 2] {
        self.sides.outer_box()
    }

    /// The box the body's attained points span: `[min, max]` of the inner
    /// values.
    #[must_use]
    pub fn inner(&self) -> [Vec3; 2] {
        self.sides.inner_box()
    }
}

/// A joint of the kernel's model: the vertex where a trim's edge ends, and
/// the vertex's tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    /// The vertex.
    pub vertex: Vec3,
    /// Its tolerance (millimetres).
    pub tolerance: f64,
}

/// One face's joint table, for the box alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxJoints {
    /// The trims (indices into the face's trims, ascending) that bridge a
    /// gap the kernel closes within a vertex's tolerance: straight lines in
    /// the parameter plane that are not edges of the model.
    pub bridges: Vec<usize>,
    /// Per trim, the joint at its traversal end (for a bridge, the vertex
    /// whose gap it closes), when the kernel gives one.
    pub ends: Vec<Option<Joint>>,
}

/// A body given for the box alone: its definition, optionally with a joint
/// table per face and the kernel's tolerance. It can only be boxed
/// ([`boxes_of`]): a definition with bridges is not a definition the facts
/// integrate, so the type gives no way back to it.
///
/// ```compile_fail
/// fn facts_from(b: &geospec_facts::BoxBody) -> &geospec_facts::BodyDefinition {
///     &b.definition
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct BoxBody {
    definition: BodyDefinition,
    joints: Vec<BoxJoints>,
    tolerance: Option<f64>,
}

impl BoxBody {
    /// A body with its joint table (one entry per face) and the kernel's
    /// largest tolerance for it.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::InvalidDefinition`] when the table does not fit the
    /// body (an entry per face, an end per trim, bridges ascending, in range
    /// and lines), a joint is not finite or its tolerance negative, or the
    /// tolerance is not finite and non-negative.
    pub fn new(
        definition: BodyDefinition,
        joints: Vec<BoxJoints>,
        tolerance: Option<f64>,
    ) -> Result<Self, Refusal> {
        let invalid = |d: String| Err(Refusal::new(RefusalCode::InvalidDefinition, d));
        if joints.len() != definition.faces.len() {
            return invalid(format!(
                "joint table has {} faces for a body of {}",
                joints.len(),
                definition.faces.len()
            ));
        }
        for (f, (j, face)) in joints.iter().zip(&definition.faces).enumerate() {
            if j.ends.len() != face.trims.len() {
                return invalid(format!(
                    "joint table face {f}: {} ends for {} trims",
                    j.ends.len(),
                    face.trims.len()
                ));
            }
            let ascending = j.bridges.windows(2).all(|w| matches!(w, [a, b] if a < b));
            let lines = j.bridges.iter().all(|&b| {
                matches!(
                    face.trims.get(b).map(|t| &t.curve),
                    Some(Curve2::Line { .. })
                )
            });
            if !(ascending && lines) {
                return invalid(format!(
                    "joint table face {f}: bridges {:?} are not ascending lines of the face",
                    j.bridges
                ));
            }
            let finite = j
                .ends
                .iter()
                .flatten()
                .all(|e| e.vertex.is_finite() && e.tolerance.is_finite() && e.tolerance >= 0.0);
            if !finite {
                return invalid(format!(
                    "joint table face {f}: a joint is not finite or has a negative tolerance"
                ));
            }
        }
        if tolerance.is_some_and(|t| !(t.is_finite() && t >= 0.0)) {
            return invalid("kernel tolerance is not finite and non-negative".to_string());
        }
        Ok(Self {
            definition,
            joints,
            tolerance,
        })
    }

    /// A body without a joint table or a kernel tolerance: boxed exactly as
    /// [`bodies_boxes`] boxes `definition`.
    #[must_use]
    pub fn of(definition: BodyDefinition) -> Self {
        Self {
            definition,
            joints: Vec::new(),
            tolerance: None,
        }
    }

    /// Whether a face of the body has a bridge.
    #[must_use]
    pub fn bridged(&self) -> bool {
        self.joints.iter().any(|j| !j.bridges.is_empty())
    }
}

fn coord(p: Vec3, c: usize) -> f64 {
    match c {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

fn side_value(p: Vec3, s: usize) -> f64 {
    let c = coord(p, s % 3);
    if s < 3 {
        -c
    } else {
        c
    }
}

/// An enclosure of a set of points: per axis `[lo, hi]`, rounding included,
/// and `m`, the rounding margin included in them.
#[derive(Clone, Copy, Debug)]
struct Enc {
    lo: [f64; 3],
    hi: [f64; 3],
    m: [f64; 3],
}

impl Enc {
    const EMPTY: Self = Self {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
        m: [0.0; 3],
    };

    fn upper(&self, s: usize) -> f64 {
        if s < 3 {
            -self.lo[s]
        } else {
            self.hi[s - 3]
        }
    }

    fn add_point(&mut self, p: [f64; 3]) {
        for c in 0..3 {
            self.lo[c] = self.lo[c].min(p[c]);
            self.hi[c] = self.hi[c].max(p[c]);
        }
    }

    fn widen(&mut self, m: [f64; 3]) {
        for c in 0..3 {
            self.lo[c] -= m[c];
            self.hi[c] += m[c];
            self.m[c] = self.m[c].max(m[c]);
        }
    }

    fn union(&mut self, o: &Self) {
        for c in 0..3 {
            self.lo[c] = self.lo[c].min(o.lo[c]);
            self.hi[c] = self.hi[c].max(o.hi[c]);
            self.m[c] = self.m[c].max(o.m[c]);
        }
    }

    fn unbounded() -> Self {
        Self {
            lo: [f64::NEG_INFINITY; 3],
            hi: [f64::INFINITY; 3],
            m: [0.0; 3],
        }
    }
}

/// An interval of angles with its ends' cosines and sines, for the ranges
/// of several `a cos t + b sin t` over it.
struct Arc {
    t0: f64,
    t1: f64,
    /// Under a full turn and not NaN: the ends' `[cos t0, sin t0, cos t1,
    /// sin t1]`.
    ends: Option<[f64; 4]>,
}

impl Arc {
    fn new(t0: f64, t1: f64) -> Self {
        let short = !(t1 - t0 >= TAU || t1.is_nan() || t0.is_nan());
        Self {
            t0,
            t1,
            ends: short.then(|| [libm::cos(t0), libm::sin(t0), libm::cos(t1), libm::sin(t1)]),
        }
    }

    /// The range of `a cos t + b sin t` over the arc, without rounding
    /// margin.
    fn range(&self, a: f64, b: f64) -> [f64; 2] {
        let rho = libm::hypot(a, b);
        let Some([c0, s0, c1, s1]) = self.ends else {
            return [-rho, rho];
        };
        let (t0, t1) = (self.t0, self.t1);
        let e0 = a * c0 + b * s0;
        let e1 = a * c1 + b * s1;
        let (mut lo, mut hi) = (e0.min(e1), e0.max(e1));
        if rho > 0.0 {
            let phi = libm::atan2(b, a);
            let within = |x: f64| x + libm::ceil((t0 - x) / TAU) * TAU <= t1;
            if within(phi) {
                hi = rho;
            }
            if within(phi + PI) {
                lo = -rho;
            }
        }
        [lo, hi]
    }
}

/// A parameter-plane box `[[u0, v0], [u1, v1]]`.
type UvBox = [[f64; 2]; 2];

fn box_of(points: &[[f64; 2]]) -> UvBox {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        for c in 0..2 {
            b[0][c] = b[0][c].min(p[c]);
            b[1][c] = b[1][c].max(p[c]);
        }
    }
    b
}

fn boxes_meet(a: &UvBox, b: &UvBox) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

// ---------------------------------------------------------------------------
// B-spline nets: Bezier sub-patches and local hulls.

/// A net prepared for enclosures: homogeneous poles about the poles' box
/// centre.
#[derive(Debug)]
struct Net {
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
    ku: Vec<f64>,
    kv: Vec<f64>,
    h: Vec<[f64; 4]>,
    origin: Vec3,
    dom: [[f64; 2]; 2],
    /// Interior knots of C0 continuity (multiplicity at least the degree).
    c0u: Vec<f64>,
    c0v: Vec<f64>,
    /// The largest magnitude of a homogeneous coordinate.
    hmax: f64,
    /// The rounding factor of a sub-patch's coordinates.
    gamma: f64,
}

fn find_span(knots: &[f64], degree: usize, count: usize, t: f64) -> usize {
    let n = count - 1;
    if t >= knots[n + 1] {
        let mut span = n;
        while span > degree && knots[span] >= knots[span + 1] {
            span -= 1;
        }
        return span;
    }
    if t <= knots[degree] {
        let mut span = degree;
        while span < n && knots[span] >= knots[span + 1] {
            span += 1;
        }
        return span;
    }
    let (mut lo, mut hi) = (degree, n + 1);
    let mut mid = (lo + hi) / 2;
    while t < knots[mid] || t >= knots[mid + 1] {
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// The non-empty spans meeting `[lo, hi]` (the span holding `lo` when the
/// interval is a point).
fn spans(knots: &[f64], degree: usize, count: usize, lo: f64, hi: f64) -> Vec<usize> {
    if lo >= hi {
        return vec![find_span(knots, degree, count, lo)];
    }
    (degree..count)
        .filter(|&i| knots[i] < knots[i + 1] && knots[i] < hi && knots[i + 1] > lo)
        .collect()
}

fn c0_knots(knots: &[f64], degree: usize, dom: [f64; 2]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut run = 0usize;
    let mut prev = f64::NAN;
    for &k in knots {
        run = if k == prev { run + 1 } else { 1 };
        prev = k;
        if run == degree && k > dom[0] && k < dom[1] {
            out.push(k);
        }
    }
    out
}

/// Blossom points of the curve over the span: Bezier point `m` over `[a, b]`
/// is the blossom at `a` (`degree - m` times) and `b` (`m` times), by de
/// Boor's algorithm with the argument per level.
fn blossom(
    knots: &[f64],
    degree: usize,
    span: usize,
    a: f64,
    b: f64,
    poles: &[[f64; 4]],
    out: &mut [[f64; 4]],
) {
    let p = degree;
    let mut d = [[0.0; 4]; crate::nurbs::MAX_DEGREE + 1];
    for (m, o) in out.iter_mut().enumerate().take(p + 1) {
        d[..=p].copy_from_slice(&poles[..=p]);
        for r in 1..=p {
            let t = if r <= p - m { a } else { b };
            for j in (r..=p).rev() {
                let i = span - p + j;
                let den = knots[i + p + 1 - r] - knots[i];
                let alpha = if den > 0.0 { (t - knots[i]) / den } else { 0.0 };
                for c in 0..4 {
                    d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
                }
            }
        }
        *o = d[p];
    }
}

/// [`blossom`] for arguments that may lie outside the span: the Bezier
/// points of the span's polynomial over `[a, b]`, and the product over the
/// levels of each level's largest `|1 - α| + |α|` (the largest over the
/// points), which bounds how the inputs' magnitude and rounding grow. It is
/// infinite when a denominator is not positive (never in a non-empty span).
fn extend(
    knots: &[f64],
    degree: usize,
    span: usize,
    a: f64,
    b: f64,
    poles: &[[f64; 4]],
    out: &mut [[f64; 4]],
) -> f64 {
    let p = degree;
    let mut d = [[0.0; 4]; crate::nurbs::MAX_DEGREE + 1];
    let mut growth = 1.0_f64;
    for (m, o) in out.iter_mut().enumerate().take(p + 1) {
        d[..=p].copy_from_slice(&poles[..=p]);
        let mut g = 1.0_f64;
        for r in 1..=p {
            let t = if r <= p - m { a } else { b };
            let mut level = 0.0_f64;
            for j in (r..=p).rev() {
                let i = span - p + j;
                let den = knots[i + p + 1 - r] - knots[i];
                if den.is_nan() || den <= 0.0 {
                    return f64::INFINITY;
                }
                let alpha = (t - knots[i]) / den;
                level = level.max((1.0 - alpha).abs() + alpha.abs());
                for c in 0..4 {
                    d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
                }
            }
            g *= level * (1.0 + 8.0 * EPS);
        }
        growth = growth.max(g);
        *o = d[p];
    }
    growth
}

/// What [`Net::extension`] gathers over its Bezier points: the smallest
/// weight, and per axis the largest projected magnitude and the largest
/// control polygon length across the strip (the sum of the differences of
/// consecutive points, per pole of the other direction), each with the
/// points' rounding.
struct Spread {
    wlo: f64,
    x: [f64; 3],
    da: [f64; 3],
    dw: f64,
}

impl Default for Spread {
    fn default() -> Self {
        Self {
            wlo: f64::INFINITY,
            x: [0.0; 3],
            da: [0.0; 3],
            dw: 0.0,
        }
    }
}

impl Spread {
    /// Adds `pts`, `(deg + 1) x n` homogeneous points (`pts[m * n + j]`,
    /// `m` across the strip) each within `err` per coordinate.
    fn add(&mut self, pts: &[[f64; 4]], n: usize, deg: usize, err: [f64; 4]) {
        for h in pts {
            let w = h[3] - err[3];
            // A value that is not finite makes `wlo` NaN for good, which
            // refuses.
            if self.wlo.is_nan() || !h.iter().chain(&err).all(|x| x.is_finite()) {
                self.wlo = f64::NAN;
                continue;
            }
            self.wlo = self.wlo.min(w);
            if w > 0.0 {
                for c in 0..3 {
                    self.x[c] = self.x[c].max((h[c].abs() + err[c]) / w);
                }
            }
        }
        for j in 0..n {
            let (mut da, mut dw) = ([0.0_f64; 3], 0.0_f64);
            for m in 0..deg {
                let (h0, h1) = (pts[m * n + j], pts[(m + 1) * n + j]);
                for c in 0..3 {
                    da[c] += (h1[c] - h0[c]).abs() + 2.0 * err[c];
                }
                dw += (h1[3] - h0[3]).abs() + 2.0 * err[3];
            }
            // Each sum of `deg` terms rounds by at most `deg EPS` of itself.
            let r = 1.0 + 2.0 * deg as f64 * EPS;
            for c in 0..3 {
                self.da[c] = self.da[c].max(da[c] * r);
            }
            self.dw = self.dw.max(dw * r);
        }
    }
}

/// A sub-patch's Bezier net and what it tells.
struct Bezier {
    /// `(p + 1) x (q + 1)` homogeneous points, u-major.
    pts: Vec<[f64; 4]>,
}

impl Net {
    fn new(s: &NurbsSurface) -> Self {
        let (mut lo, mut hi) = (s.poles[0], s.poles[0]);
        for p in &s.poles {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        let origin = 0.5 * (lo + hi);
        let mut hmax = 0.0_f64;
        let h: Vec<[f64; 4]> = s
            .poles
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let w = s.weights.as_ref().map_or(1.0, |w| w[k]);
                let q = *p - origin;
                let r = [q.x * w, q.y * w, q.z * w, w];
                for x in r {
                    hmax = hmax.max(x.abs());
                }
                r
            })
            .collect();
        let dom = [
            [s.knots_u[s.degree_u], s.knots_u[s.count_u]],
            [s.knots_v[s.degree_v], s.knots_v[s.count_v]],
        ];
        Self {
            p: s.degree_u,
            q: s.degree_v,
            nu: s.count_u,
            nv: s.count_v,
            c0u: c0_knots(&s.knots_u, s.degree_u, dom[0]),
            c0v: c0_knots(&s.knots_v, s.degree_v, dom[1]),
            ku: s.knots_u.clone(),
            kv: s.knots_v.clone(),
            h,
            origin,
            dom,
            hmax,
            gamma: 8.0 * (s.degree_u + s.degree_v + 2) as f64 * EPS,
        }
    }

    fn clamp(&self, b: &UvBox) -> UvBox {
        let cl = |x: f64, d: [f64; 2]| x.max(d[0]).min(d[1]);
        [
            [cl(b[0][0], self.dom[0]), cl(b[0][1], self.dom[1])],
            [cl(b[1][0], self.dom[0]), cl(b[1][1], self.dom[1])],
        ]
    }

    fn spans_u(&self, lo: f64, hi: f64) -> Vec<usize> {
        spans(&self.ku, self.p, self.nu, lo, hi)
    }

    fn spans_v(&self, lo: f64, hi: f64) -> Vec<usize> {
        spans(&self.kv, self.q, self.nv, lo, hi)
    }

    /// The Bezier net of span `(i, j)` over `b` (inside the span).
    fn bezier(&self, i: usize, j: usize, b: &UvBox) -> Bezier {
        let (p, q) = (self.p, self.q);
        let mut tmp = vec![[0.0; 4]; (p + 1) * (q + 1)];
        let mut col = vec![[0.0; 4]; p + 1];
        let mut outc = vec![[0.0; 4]; p + 1];
        for l in 0..=q {
            for (k, c) in col.iter_mut().enumerate() {
                *c = self.h[(i - p + k) * self.nv + (j - q + l)];
            }
            blossom(&self.ku, p, i, b[0][0], b[1][0], &col, &mut outc);
            for m in 0..=p {
                tmp[m * (q + 1) + l] = outc[m];
            }
        }
        let mut pts = vec![[0.0; 4]; (p + 1) * (q + 1)];
        let mut row = vec![[0.0; 4]; q + 1];
        let mut outr = vec![[0.0; 4]; q + 1];
        for m in 0..=p {
            row.copy_from_slice(&tmp[m * (q + 1)..(m + 1) * (q + 1)]);
            blossom(&self.kv, q, j, b[0][1], b[1][1], &row, &mut outr);
            pts[m * (q + 1)..(m + 1) * (q + 1)].copy_from_slice(&outr);
        }
        Bezier { pts }
    }

    fn project(&self, h: [f64; 4]) -> [f64; 3] {
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// Whether every point lies in the domain (none is NaN). A net's
    /// derivative bounds ([`net_derivatives`]) hold only there: beyond it the
    /// end span's polynomial is extrapolated and its weights can fall below
    /// every pole's, so a piece whose control points leave the domain gets no
    /// second-order bound and is refined by its enclosures instead.
    fn contains(&self, pts: &[[f64; 2]]) -> bool {
        pts.iter()
            .all(|p| (0..2).all(|c| p[c] >= self.dom[c][0] && p[c] <= self.dom[c][1]))
    }

    /// The enclosure of a Bezier net (its projected points' box, rounding
    /// included).
    fn bezier_enc(&self, bz: &Bezier) -> Enc {
        self.points_enc(bz.pts.iter().copied())
    }

    /// The enclosure of one edge of a Bezier net: in direction `dir` (0 for
    /// u, 1 for v), its high (`high`) or low end; the net's control points
    /// there are the edge curve's.
    fn edge_enc(&self, bz: &Bezier, dir: usize, high: bool) -> Enc {
        let (p, q) = (self.p, self.q);
        let k = if high { [p, q][dir] } else { 0 };
        let n = [q, p][dir] + 1;
        let at = move |j: usize| {
            if dir == 0 {
                k * (q + 1) + j
            } else {
                j * (q + 1) + k
            }
        };
        self.points_enc((0..n).filter_map(|j| bz.pts.get(at(j)).copied()))
    }

    /// The enclosure of the projections of homogeneous points, rounding
    /// included.
    fn points_enc(&self, pts: impl Iterator<Item = [f64; 4]>) -> Enc {
        let mut wmin = f64::INFINITY;
        let mut rel = Enc::EMPTY;
        for h in pts {
            wmin = wmin.min(h[3]);
            rel.add_point(self.project(h));
        }
        let err = self.gamma * self.hmax;
        let w = wmin - err;
        if w.is_nan() || w <= 0.0 || !rel.lo.iter().chain(&rel.hi).all(|x| x.is_finite()) {
            return Enc::unbounded();
        }
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        for c in 0..3 {
            let xmax = rel.lo[c].abs().max(rel.hi[c].abs());
            m[c] = err * (1.0 + xmax) / w + 4.0 * EPS * (xmax + o[c].abs());
            e.lo[c] = rel.lo[c] + o[c];
            e.hi[c] = rel.hi[c] + o[c];
        }
        e.widen(m);
        e
    }

    /// The corners of a Bezier net, which the surface attains (up to the
    /// returned rounding per axis).
    fn corners(&self, bz: &Bezier) -> ([[f64; 3]; 4], [f64; 3]) {
        let (p, q) = (self.p, self.q);
        let idx = [0, q, p * (q + 1), p * (q + 1) + q];
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut out = [[0.0; 3]; 4];
        let mut m = [0.0_f64; 3];
        let err = self.gamma * self.hmax;
        for (k, &i) in idx.iter().enumerate() {
            let h = bz.pts[i];
            let x = self.project(h);
            let w = (h[3] - err).max(f64::MIN_POSITIVE);
            for c in 0..3 {
                out[k][c] = x[c] + o[c];
                m[c] =
                    m[c].max(err * (1.0 + x[c].abs()) / w + 4.0 * EPS * (x[c].abs() + o[c].abs()));
            }
        }
        (out, m)
    }

    /// Per direction (u, v) and axis: `1` when the coordinate increases over
    /// the whole sub-patch in that direction, `-1` when it decreases, `0`
    /// when the hodograph's interval does not decide.
    fn monotone(&self, bz: &Bezier) -> [[i8; 3]; 2] {
        let (p, q) = (self.p, self.q);
        let at = |m: usize, n: usize| bz.pts[m * (q + 1) + n];
        let mut f = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for h in &bz.pts {
            let x = self.project(*h);
            for c in 0..3 {
                f[c][0] = f[c][0].min(x[c]);
                f[c][1] = f[c][1].max(x[c]);
            }
        }
        let mut out = [[0i8; 3]; 2];
        for (dir, o) in out.iter_mut().enumerate() {
            // Hodograph coefficients (up to the positive degree factor).
            let mut a = [[f64::INFINITY, f64::NEG_INFINITY]; 4];
            let (mm, nn) = if dir == 0 { (p, q + 1) } else { (p + 1, q) };
            for m in 0..mm {
                for n in 0..nn {
                    let (x0, x1) = if dir == 0 {
                        (at(m, n), at(m + 1, n))
                    } else {
                        (at(m, n), at(m, n + 1))
                    };
                    for c in 0..4 {
                        let d = x1[c] - x0[c];
                        a[c][0] = a[c][0].min(d);
                        a[c][1] = a[c][1].max(d);
                    }
                }
            }
            let b = a[3];
            for c in 0..3 {
                let prods = [
                    f[c][0] * b[0],
                    f[c][0] * b[1],
                    f[c][1] * b[0],
                    f[c][1] * b[1],
                ];
                let pmin = prods.iter().fold(f64::INFINITY, |m, x| m.min(*x));
                let pmax = prods.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x));
                let lo = a[c][0] - pmax;
                let hi = a[c][1] - pmin;
                let fmag = f[c][0].abs().max(f[c][1].abs());
                let tol = 8.0 * self.gamma * self.hmax * (1.0 + fmag);
                if !(lo.is_finite() && hi.is_finite()) {
                    continue;
                }
                if lo > tol {
                    o[c] = 1;
                } else if hi < -tol {
                    o[c] = -1;
                }
            }
        }
        out
    }

    /// Per axis, a bound on how far the extrapolated surface moves between a
    /// parameter point up to `beyond` past the domain's end in direction
    /// `dir` (its `high` end or low) and that end: `|S_ext(t, s) - S(end,
    /// s)|` for `t` between `end` and `end -+ beyond`, over the other
    /// direction's domain widened by `other` (below it, above it). A kernel
    /// that evaluates there extrapolates the end span's polynomial (OCCT's
    /// `BSplCLib` locates such a parameter in the end span), and so does this
    /// bound: exact up to rounding, with no Lipschitz or Chebyshev factor.
    ///
    /// The end span's poles are blossomed (de Boor's algorithm with the
    /// arguments per level, which holds for any argument) over `[end -
    /// beyond, end]` (or `[end, end + beyond]`): per pole of the other
    /// direction, the Bezier points `b_m` (`m = 0..=p`) of the extrapolated
    /// polynomial over the strip. Over the other direction's domain the
    /// surface is the B-spline with those points as poles; past each of its
    /// ends that the trims leave by (a corner), its end span is blossomed the
    /// same way. Every such net's basis is non-negative and sums to one on
    /// its interval, so with every homogeneous weight above `W_lo > 0` the
    /// surface's coordinate `x` lies within its projected points (`|x| <=
    /// X`), `W >= W_lo`, and its derivative across the strip is `(A' - x W')
    /// / W`. Across the strip the homogeneous coordinate is a blend (by the
    /// other direction's basis) of Bezier polynomials `A_j` with points
    /// `b_{m, j}`, so its variation `∫|A'|` is at most the largest
    /// `Σ_m |b_{m+1, j} - b_{m, j}|` (a Bezier polynomial varies no more than
    /// its control polygon), and likewise for `W`. So
    ///
    /// `|S_ext - S(end)| <= (max_j Σ_m |ΔA| + X max_j Σ_m |ΔW|) / W_lo`.
    ///
    /// The blossom's arguments lie outside the span, so its factors
    /// `(1 - α, α)` are not a convex pair: each level's rounding grows by
    /// `max(|1 - α| + |α|)`, and the product `K` of those over the levels
    /// bounds each point's error by `18 p EPS K` times its inputs' largest
    /// magnitude (each level rounds `α`, `1 - α`, two products and a sum),
    /// plus `8 p EPS K` times the smallest normal number for the absolute
    /// rounding of a value that underflows (weights themselves are normal:
    /// `validate` refuses subnormal ones).
    /// That error is carried into `W_lo`, `X` and the differences. The strip
    /// is widened by a relative `1e-9` plus the rounding of its ends.
    ///
    /// `None` when an extrapolated weight is not shown positive (`W_lo <=
    /// 0`: the extension may pass through infinity) or a value is not finite:
    /// the face then refuses.
    fn extension(&self, dir: usize, high: bool, beyond: f64, other: [f64; 2]) -> Option<[f64; 3]> {
        let o = 1 - dir;
        let degrees = [self.p, self.q];
        let knots = [&self.ku, &self.kv];
        let counts = [self.nu, self.nv];
        let (deg, odeg) = (degrees[dir], degrees[o]);
        let ocount = counts[o];
        // The strip in a direction: past `end` (towards `high`) by `x`, its
        // far end rounded outwards.
        let strip = |c: usize, high: bool, x: f64| -> (f64, f64, usize) {
            let [lo, hi] = self.dom[c];
            let end = if high { hi } else { lo };
            let reach = x * (1.0 + 1e-9) + 2.0 * EPS * (end.abs() + x);
            let span = find_span(knots[c], degrees[c], counts[c], end);
            if high {
                (end, end + reach, span)
            } else {
                (end - reach, end, span)
            }
        };
        let at = |i: usize, j: usize| {
            if dir == 0 {
                self.h[i * self.nv + j]
            } else {
                self.h[j * self.nv + i]
            }
        };
        let (a, b, span) = strip(dir, high, beyond);
        if deg == 0 || span < deg {
            return None;
        }
        // The strip's Bezier points, `grid[m * ocount + j]`, and the
        // inputs' largest magnitudes.
        let mut grid = vec![[0.0; 4]; (deg + 1) * ocount];
        let mut col = [[0.0; 4]; crate::nurbs::MAX_DEGREE + 1];
        let mut out = [[0.0; 4]; crate::nurbs::MAX_DEGREE + 1];
        let mut mag = [0.0_f64; 4];
        let mut k = 1.0_f64;
        for j in 0..ocount {
            for (kk, c) in col.iter_mut().enumerate().take(deg + 1) {
                *c = at(span - deg + kk, j);
                for x in 0..4 {
                    mag[x] = mag[x].max(c[x].abs());
                }
            }
            k = k.max(extend(
                knots[dir],
                deg,
                span,
                a,
                b,
                &col[..=deg],
                &mut out[..=deg],
            ));
            for m in 0..=deg {
                grid[m * ocount + j] = out[m];
            }
        }
        // Each operation also rounds by at most one subnormal ulp, which
        // the growth `k` amplifies: an absolute floor that no relative term
        // gives once a value underflows.
        let floor = 8.0 * deg as f64 * k * EPS * f64::MIN_POSITIVE;
        let err = mag.map(|x| 18.0 * deg as f64 * EPS * k * x + floor);
        let mut st = Spread::default();
        st.add(&grid, ocount, deg, err);
        // Past the other direction's ends (corners).
        for (side, x) in other.into_iter().enumerate() {
            if x.is_nan() || x <= 0.0 {
                continue;
            }
            let (oa, ob, ospan) = strip(o, side == 1, x);
            if odeg == 0 || ospan < odeg {
                return None;
            }
            let mut omag = [0.0_f64; 4];
            for g in &grid {
                for c in 0..4 {
                    omag[c] = omag[c].max(g[c].abs() + err[c]);
                }
            }
            let mut corner = vec![[0.0; 4]; (deg + 1) * (odeg + 1)];
            let mut ok = 1.0_f64;
            for m in 0..=deg {
                let row = &grid[m * ocount + ospan - odeg..=m * ocount + ospan];
                ok = ok.max(extend(
                    knots[o],
                    odeg,
                    ospan,
                    oa,
                    ob,
                    row,
                    &mut out[..=odeg],
                ));
                corner[m * (odeg + 1)..(m + 1) * (odeg + 1)].copy_from_slice(&out[..=odeg]);
            }
            let mut cerr = [0.0; 4];
            for c in 0..4 {
                cerr[c] = ok * err[c]
                    + 18.0 * odeg as f64 * EPS * ok * omag[c]
                    + 8.0 * odeg as f64 * ok * EPS * f64::MIN_POSITIVE;
            }
            st.add(&corner, odeg + 1, deg, cerr);
        }
        let wlo = st.wlo;
        if wlo.is_nan() || wlo <= 0.0 || !st.dw.is_finite() {
            return None;
        }
        let mut d = [0.0; 3];
        for c in 0..3 {
            d[c] = (st.da[c] + st.x[c] * st.dw) / wlo * (1.0 + 1e-9);
        }
        d.iter().all(|x| x.is_finite()).then_some(d)
    }

    /// The hull of the poles that define the surface over spans `su x sv`.
    fn local_hull(&self, su: &[usize], sv: &[usize]) -> Enc {
        let (Some(&i0), Some(&i1), Some(&j0), Some(&j1)) =
            (su.first(), su.last(), sv.first(), sv.last())
        else {
            return Enc::unbounded();
        };
        let mut rel = Enc::EMPTY;
        for i in i0 - self.p..=i1 {
            for j in j0 - self.q..=j1 {
                rel.add_point(self.project(self.h[i * self.nv + j]));
            }
        }
        let o = [self.origin.x, self.origin.y, self.origin.z];
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        for c in 0..3 {
            let xmax = rel.lo[c].abs().max(rel.hi[c].abs());
            m[c] = 4.0 * EPS * (xmax + o[c].abs());
            e.lo[c] = rel.lo[c] + o[c];
            e.hi[c] = rel.hi[c] + o[c];
        }
        e.widen(m);
        e
    }

    /// An enclosure of the surface over `b`: Bezier nets of each span it
    /// meets when they are few, else the hull of the poles that define it.
    fn enclose(&self, b: &UvBox) -> (Enc, usize) {
        let b = self.clamp(b);
        let su = self.spans_u(b[0][0], b[1][0]);
        let sv = self.spans_v(b[0][1], b[1][1]);
        if su.len() * sv.len() > 4 {
            return (self.local_hull(&su, &sv), 1);
        }
        let mut e = Enc::EMPTY;
        let mut work = 0;
        for &i in &su {
            for &j in &sv {
                let sub = [
                    [b[0][0].max(self.ku[i]), b[0][1].max(self.kv[j])],
                    [b[1][0].min(self.ku[i + 1]), b[1][1].min(self.kv[j + 1])],
                ];
                let bz = self.bezier(i, j, &sub);
                e.union(&self.bezier_enc(&bz));
                work += 1;
            }
        }
        (e, work)
    }
}

// ---------------------------------------------------------------------------
// Surfaces: enclosures over parameter boxes.

#[derive(Debug)]
enum Geo {
    Plane { o: Vec3, su: Vec3, sv: Vec3 },
    Ruled { f: Frame, r: f64, s: f64, c: f64 },
    Sphere { f: Frame, r: f64 },
    Torus { f: Frame, big: f64, r: f64 },
    Nurbs(Net),
}

impl Geo {
    fn new(s: &Surface) -> Self {
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => Self::Plane {
                o: *origin,
                su: *u_axis,
                sv: *v_axis,
            },
            Surface::Cylinder { frame, radius } => Self::Ruled {
                f: *frame,
                r: *radius,
                s: 0.0,
                c: 1.0,
            },
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => Self::Ruled {
                f: *frame,
                r: *radius,
                s: libm::sin(*semi_angle),
                c: libm::cos(*semi_angle),
            },
            Surface::Sphere { frame, radius } => Self::Sphere {
                f: *frame,
                r: *radius,
            },
            Surface::Torus {
                frame,
                major_radius,
                minor_radius,
            } => Self::Torus {
                f: *frame,
                big: *major_radius,
                r: *minor_radius,
            },
            Surface::Nurbs(n) => Self::Nurbs(Net::new(n)),
        }
    }

    /// The periods in u and v of the analytic kinds that close.
    fn periods(&self) -> [Option<f64>; 2] {
        match self {
            Self::Ruled { .. } | Self::Sphere { .. } => [Some(TAU), None],
            Self::Torus { .. } => [Some(TAU), Some(TAU)],
            Self::Plane { .. } | Self::Nurbs(_) => [None, None],
        }
    }

    /// An enclosure of the surface over `b`, exact up to rounding for the
    /// analytic kinds, and the enclosures computed.
    fn enclose(&self, b: &UvBox) -> (Enc, usize) {
        let [[u0, v0], [u1, v1]] = *b;
        let umax = u0.abs().max(u1.abs());
        let vmax = v0.abs().max(v1.abs());
        let mut e = Enc::EMPTY;
        let mut m = [0.0; 3];
        match self {
            Self::Nurbs(n) => return n.enclose(b),
            Self::Plane { o, su, sv } => {
                for c in 0..3 {
                    let (oc, a, bb) = (coord(*o, c), coord(*su, c), coord(*sv, c));
                    for (u, v) in [(u0, v0), (u0, v1), (u1, v0), (u1, v1)] {
                        let x = oc + u * a + v * bb;
                        e.lo[c] = e.lo[c].min(x);
                        e.hi[c] = e.hi[c].max(x);
                    }
                    m[c] = 8.0 * EPS * (oc.abs() + umax * a.abs() + vmax * bb.abs());
                }
            }
            Self::Ruled { f, r, s, c: ca } => {
                let arc = Arc::new(u0, u1);
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = arc.range(a, bb);
                    for vk in [v0, v1] {
                        let rho = *r + vk * *s;
                        let base = oc + vk * *ca * z;
                        let (lo, hi) = if rho >= 0.0 {
                            (base + rho * g[0], base + rho * g[1])
                        } else {
                            (base + rho * g[1], base + rho * g[0])
                        };
                        e.lo[c] = e.lo[c].min(lo);
                        e.hi[c] = e.hi[c].max(hi);
                    }
                    m[c] = 32.0
                        * EPS
                        * (oc.abs() + (r.abs() + vmax * s.abs()) * (1.0 + umax) + vmax * z.abs());
                }
            }
            Self::Sphere { f, r } => {
                let va = v0.clamp(-FRAC_PI_2, FRAC_PI_2);
                let vb = v1.clamp(-FRAC_PI_2, FRAC_PI_2);
                let (arc_u, arc_v) = (Arc::new(u0, u1), Arc::new(va, vb));
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = arc_u.range(a, bb);
                    e.lo[c] = oc + *r * arc_v.range(g[0], z)[0];
                    e.hi[c] = oc + *r * arc_v.range(g[1], z)[1];
                    m[c] = 32.0 * EPS * (oc.abs() + *r * (1.0 + umax + vmax));
                }
            }
            Self::Torus { f, big, r } => {
                let (arc_u, arc_v) = (Arc::new(u0, u1), Arc::new(v0, v1));
                for c in 0..3 {
                    let (oc, a, bb, z) = (
                        coord(f.origin, c),
                        coord(f.x, c),
                        coord(f.y, c),
                        coord(f.z, c),
                    );
                    let g = arc_u.range(a, bb);
                    if *big >= *r {
                        e.lo[c] = oc + *big * g[0] + *r * arc_v.range(g[0], z)[0];
                        e.hi[c] = oc + *big * g[1] + *r * arc_v.range(g[1], z)[1];
                    } else {
                        // A spindle torus: the coefficient of g changes sign.
                        let cv = arc_v.range(1.0, 0.0);
                        let sv = arc_v.range(0.0, 1.0);
                        let k = [*big + *r * cv[0], *big + *r * cv[1]];
                        let prods = [k[0] * g[0], k[0] * g[1], k[1] * g[0], k[1] * g[1]];
                        let zs = [*r * z * sv[0], *r * z * sv[1]];
                        e.lo[c] = oc
                            + prods.iter().fold(f64::INFINITY, |m, x| m.min(*x))
                            + zs[0].min(zs[1]);
                        e.hi[c] = oc
                            + prods.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x))
                            + zs[0].max(zs[1]);
                    }
                    m[c] = 32.0 * EPS * (oc.abs() + (big.abs() + *r) * (1.0 + umax + vmax));
                }
            }
        }
        e.widen(m);
        (e, 1)
    }
}

// ---------------------------------------------------------------------------
// The face's boundary pieces and its classifier.

/// A boundary piece: a trim (attained) or a closing chord (bounds only).
/// The coefficients of a B-spline's derivative (degree `deg - 1`, knots
/// without their ends); a zero-length span's coefficient is zero.
fn dcoef<const D: usize>(cs: &[[f64; D]], deg: usize, knots: &[f64]) -> (Vec<[f64; D]>, Vec<f64>) {
    let mut out = Vec::with_capacity(cs.len().saturating_sub(1));
    if deg == 0 || cs.len() < 2 {
        return (out, knots.to_vec());
    }
    for i in 0..cs.len() - 1 {
        let den = knots[i + deg + 1] - knots[i + 1];
        let mut d = [0.0; D];
        if den > 0.0 {
            for c in 0..D {
                d[c] = deg as f64 * (cs[i + 1][c] - cs[i][c]) / den;
            }
        }
        out.push(d);
    }
    (out, knots[1..knots.len() - 1].to_vec())
}

fn max_abs<const D: usize>(cs: &[[f64; D]]) -> [f64; D] {
    let mut m = [0.0_f64; D];
    for x in cs {
        for c in 0..D {
            m[c] = m[c].max(x[c].abs());
        }
    }
    m
}

/// Bounds on `|c'|` and `|c''|` per parameter (u, v) of a trim curve over
/// its whole range, from its homogeneous derivatives' coefficients:
/// `c' = (A' - c w') / w` and `c'' = (A'' - 2 c' w' - c w'') / w`.
fn curve_derivatives(t: &TrimPiece) -> Option<[[f64; 2]; 2]> {
    match &t.curve {
        Curve2::Line { direction, .. } => {
            Some([[direction[0].abs(), direction[1].abs()], [0.0; 2]])
        }
        Curve2::Nurbs(c) => {
            // The derivatives' coefficients (as `dcoef` twice) in one sweep.
            let h = |k: usize| {
                let w = c.weights.as_ref().map_or(1.0, |w| w[k]);
                let p = c.poles[k];
                [p[0] * w, p[1] * w, w]
            };
            let (n, deg, kn) = (c.poles.len(), c.degree, &c.knots);
            let mut wmin = f64::INFINITY;
            for k in 0..n {
                wmin = wmin.min(h(k)[2]);
            }
            if wmin.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return None;
            }
            let x = max_abs(&c.poles);
            let (mut a1, mut a2) = ([0.0_f64; 3], [0.0_f64; 3]);
            let mut prev: Option<[f64; 3]> = None;
            if deg > 0 && n >= 2 {
                for i in 0..n - 1 {
                    let den = kn[i + deg + 1] - kn[i + 1];
                    let mut d = [0.0; 3];
                    if den > 0.0 {
                        let (p0, p1) = (h(i), h(i + 1));
                        for k in 0..3 {
                            d[k] = deg as f64 * (p1[k] - p0[k]) / den;
                        }
                    }
                    for k in 0..3 {
                        a1[k] = a1[k].max(d[k].abs());
                    }
                    if let (Some(q), true) = (prev, deg > 1) {
                        let den = kn[i + deg] - kn[i + 1];
                        if den > 0.0 {
                            for k in 0..3 {
                                let d2 = (deg - 1) as f64 * (d[k] - q[k]) / den;
                                a2[k] = a2[k].max(d2.abs());
                            }
                        }
                    }
                    prev = Some(d);
                }
            }
            let mut out = [[0.0; 2]; 2];
            for k in 0..2 {
                out[0][k] = (a1[k] + x[k] * a1[2]) / wmin;
                out[1][k] = (a2[k] + 2.0 * out[0][k] * a1[2] + x[k] * a2[2]) / wmin;
            }
            let ok = out.iter().flatten().all(|v| v.is_finite());
            ok.then(|| out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
        }
    }
}

/// Bounds per axis on `|S_u|, |S_v|, |S_uu|, |S_uv|, |S_vv|` of a net over
/// its whole domain (its homogeneous derivatives' coefficients, as for a
/// curve).
fn net_derivatives(n: &Net) -> Option<[[f64; 3]; 5]> {
    let grid: Vec<Vec<[f64; 4]>> = (0..n.nu)
        .map(|i| n.h[i * n.nv..(i + 1) * n.nv].to_vec())
        .collect();
    let wmin = n.h.iter().fold(f64::INFINITY, |m, x| m.min(x[3]));
    if wmin.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let mut xmax = [0.0_f64; 3];
    for h in &n.h {
        for c in 0..3 {
            xmax[c] = xmax[c].max((h[c] / h[3]).abs());
        }
    }
    // Along u: each column j over i; along v: each row i over j.
    let du =
        |g: &Vec<Vec<[f64; 4]>>, deg: usize, knots: &[f64]| -> (Vec<Vec<[f64; 4]>>, Vec<f64>) {
            let cols = g.first().map_or(0, Vec::len);
            let mut out: Vec<Vec<[f64; 4]>> =
                vec![Vec::with_capacity(cols); g.len().saturating_sub(1)];
            let mut ks = Vec::new();
            for j in 0..cols {
                let col: Vec<[f64; 4]> = g.iter().map(|r| r[j]).collect();
                let (d, k) = dcoef(&col, deg, knots);
                for (i, x) in d.into_iter().enumerate() {
                    out[i].push(x);
                }
                ks = k;
            }
            (out, ks)
        };
    let dv = |g: &Vec<Vec<[f64; 4]>>, deg: usize, knots: &[f64]| -> Vec<Vec<[f64; 4]>> {
        g.iter().map(|r| dcoef(r, deg, knots).0).collect()
    };
    let flat = |g: &Vec<Vec<[f64; 4]>>| max_abs(&g.iter().flatten().copied().collect::<Vec<_>>());
    let (gu, ku1) = du(&grid, n.p, &n.ku);
    let (guu, _) = du(&gu, n.p.saturating_sub(1), &ku1);
    let gv = dv(&grid, n.q, &n.kv);
    let kv1 = n.kv[1..n.kv.len() - 1].to_vec();
    let gvv = dv(&gv, n.q.saturating_sub(1), &kv1);
    let guv = dv(&gu, n.q, &n.kv);
    let (au, av, auu, auv, avv) = (flat(&gu), flat(&gv), flat(&guu), flat(&guv), flat(&gvv));
    let mut out = [[0.0; 3]; 5];
    for c in 0..3 {
        let su = (au[c] + xmax[c] * au[3]) / wmin;
        let sv = (av[c] + xmax[c] * av[3]) / wmin;
        out[0][c] = su;
        out[1][c] = sv;
        out[2][c] = (auu[c] + 2.0 * su * au[3] + xmax[c] * auu[3]) / wmin;
        out[3][c] = (auv[c] + su * av[3] + sv * au[3] + xmax[c] * auv[3]) / wmin;
        out[4][c] = (avv[c] + 2.0 * sv * av[3] + xmax[c] * avv[3]) / wmin;
    }
    let ok = out.iter().flatten().all(|v| v.is_finite());
    ok.then(|| out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
}

struct Piece {
    trim: PreparedTrim,
    /// The range's ends and the curve's distinct knots inside it; empty for
    /// a line (its range alone).
    cuts: Vec<f64>,
    /// The range's ends and the knots inside it where the curve may not be
    /// C1 (multiplicity at least its degree); empty for a line. The
    /// second-order bound holds between them.
    breaks: Vec<f64>,
    attained: bool,
    straight: bool,
    /// An iso-parametric line (`u` or `v` constant), exact on analytic kinds.
    iso: bool,
    /// Bounds on `|c'|` and `|c''|` per parameter, when known.
    deriv: Option<[[f64; 2]; 2]>,
    /// A closing chord's ends: the trim pieces it joins, each with the end
    /// (0 for the range's start, 1 for its end) it starts or stops at.
    joins: Option<[(usize, usize); 2]>,
}

/// A trim's range ends and its curve's distinct knots strictly inside, and
/// of those the ones where the curve may not be C1.
fn cuts_of(t: &TrimPiece) -> (Vec<f64>, Vec<f64>) {
    let [a, b] = t.range;
    let Curve2::Nurbs(c) = &t.curve else {
        return (Vec::new(), Vec::new());
    };
    let mut cuts = Vec::with_capacity(c.knots.len());
    let mut breaks = Vec::with_capacity(2);
    cuts.push(a);
    breaks.push(a);
    let mut k = 0;
    while k < c.knots.len() {
        let x = c.knots[k];
        let mut m = 1;
        while c.knots.get(k + m) == Some(&x) {
            m += 1;
        }
        if x > a && x < b {
            cuts.push(x);
            if m >= c.degree {
                breaks.push(x);
            }
        }
        k += m;
    }
    cuts.push(b);
    breaks.push(b);
    (cuts, breaks)
}

impl Piece {
    /// The piece's parameter intervals between its cuts.
    fn intervals(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        Self::windows(&self.cuts, self.trim.range)
    }

    /// The piece's parameter intervals between its breaks.
    fn runs(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        Self::windows(&self.breaks, self.trim.range)
    }

    fn windows(at: &[f64], range: [f64; 2]) -> impl Iterator<Item = (f64, f64)> + '_ {
        let whole = at.is_empty().then_some((range[0], range[1]));
        whole
            .into_iter()
            .chain(at.windows(2).filter_map(|w| match *w {
                [a, b] => Some((a, b)),
                _ => None,
            }))
    }

    /// Where to split `[t0, t1]`: the cut inside it nearest its middle, or
    /// the middle.
    fn split(&self, t0: f64, t1: f64) -> f64 {
        let m = 0.5 * (t0 + t1);
        let lo = self.cuts.partition_point(|&x| x <= t0);
        let hi = self.cuts.partition_point(|&x| x < t1);
        self.cuts
            .get(lo..hi)
            .unwrap_or(&[])
            .iter()
            .copied()
            .min_by(|x, y| (x - m).abs().total_cmp(&(y - m).abs()))
            .unwrap_or(m)
    }
}

fn line_piece(a: [f64; 2], b: [f64; 2]) -> Piece {
    Piece {
        trim: PreparedTrim::new(&TrimPiece {
            curve: Curve2::Line {
                origin: a,
                direction: [b[0] - a[0], b[1] - a[1]],
            },
            range: [0.0, 1.0],
            reversed: false,
        }),
        cuts: Vec::new(),
        breaks: Vec::new(),
        attained: false,
        straight: true,
        iso: a[0] == b[0] || a[1] == b[1],
        deriv: Some([[(b[0] - a[0]).abs(), (b[1] - a[1]).abs()], [0.0; 2]]),
        joins: None,
    }
}

fn trim_ends(t: &PreparedTrim) -> ([f64; 2], [f64; 2]) {
    let a = t.point(t.range[0]);
    let b = t.point(t.range[1]);
    if t.reversed {
        (b, a)
    } else {
        (a, b)
    }
}

/// The face's trims as pieces, each attained except the bridges (indices,
/// ascending): a bridge's points are not known to be the model's, so they
/// only bound the outer values, as a closing chord's do.
fn trim_pieces(face: &FaceDefinition, bridges: &[usize]) -> Vec<Piece> {
    face.trims
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let iso = match &t.curve {
                Curve2::Line { direction, .. } => direction[0] == 0.0 || direction[1] == 0.0,
                Curve2::Nurbs(_) => false,
            };
            let (cuts, breaks) = cuts_of(t);
            Piece {
                trim: PreparedTrim::new(t),
                cuts,
                breaks,
                attained: bridges.binary_search(&i).is_err(),
                straight: matches!(t.curve, Curve2::Line { .. }),
                iso,
                deriv: curve_derivatives(t),
                joins: None,
            }
        })
        .collect()
}

/// Appends to the face's trim pieces `out` a chord for every junction
/// where a piece does not end exactly where the next starts (shifted by
/// whole periods to the nearest image), and returns the junctions that
/// meet exactly: the trim pieces whose end is the next one's start.
fn add_chords(
    out: &mut Vec<Piece>,
    face: &FaceDefinition,
    periods: [Option<f64>; 2],
) -> Vec<usize> {
    let mut exact = Vec::new();
    let mut chords = Vec::new();
    let mut start = 0usize;
    for &count in &face.loops {
        let lo = start;
        start += count;
        for k in lo..start {
            let next = if k + 1 == start { lo } else { k + 1 };
            let (Some(a), Some(b)) = (out.get(k), out.get(next)) else {
                continue;
            };
            let p = trim_ends(&a.trim).1;
            let mut q = trim_ends(&b.trim).0;
            for (c, period) in periods.iter().enumerate() {
                if let Some(t) = period {
                    let n = libm::round((p[c] - q[c]) / t);
                    q[c] += n * t;
                }
            }
            if p != q {
                let mut chord = line_piece(p, q);
                // A reversed trim ends at its range's start.
                let (end, start) = (usize::from(!a.trim.reversed), usize::from(b.trim.reversed));
                chord.joins = Some([(k, end), (next, start)]);
                chords.push(chord);
            } else {
                exact.push(k);
            }
        }
    }
    out.extend(chords);
    exact
}

/// One segment of a piece over `[t0, t1]`: its control-point box and end
/// points.
#[derive(Clone, Copy)]
struct Seg {
    piece: usize,
    t0: f64,
    t1: f64,
    b: UvBox,
    a: [f64; 2],
    z: [f64; 2],
}

/// Classifies parameter-plane points and boxes against the face's pieces.
struct Classifier<'a> {
    pieces: &'a [Piece],
    segs: Vec<Seg>,
    period_u: Option<f64>,
    period_v: Option<f64>,
    /// The pieces' box.
    window: UvBox,
}

fn seg_of(pieces: &[Piece], piece: usize, t0: f64, t1: f64) -> Option<Seg> {
    let t = &pieces.get(piece)?.trim;
    let pts = t.hull_points(t0, t1)?;
    let mut b = box_of(&pts);
    let a = t.eval(t0).0;
    let z = t.eval(t1).0;
    for p in [a, z] {
        for c in 0..2 {
            b[0][c] = b[0][c].min(p[c]);
            b[1][c] = b[1][c].max(p[c]);
        }
    }
    let finite = b
        .iter()
        .flatten()
        .chain(&a)
        .chain(&z)
        .all(|x| x.is_finite());
    finite.then_some(Seg {
        piece,
        t0,
        t1,
        b,
        a,
        z,
    })
}

/// Whether segment `a`-`z` meets box `q` (Liang-Barsky clipping).
fn line_meets_box(a: [f64; 2], z: [f64; 2], q: &UvBox) -> bool {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for c in 0..2 {
        let d = z[c] - a[c];
        for (p, r) in [(-d, a[c] - q[0][c]), (d, q[1][c] - a[c])] {
            if p == 0.0 {
                if r < 0.0 {
                    return false;
                }
            } else {
                let t = r / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
    }
    t0 <= t1
}

/// The whole-period shifts `k` with `[lo, hi] + k period` meeting `[a, b]`.
fn shifts(period: Option<f64>, lo: f64, hi: f64, a: f64, b: f64) -> std::ops::RangeInclusive<i64> {
    match period {
        None if hi >= a && lo <= b => 0..=0,
        #[allow(clippy::reversed_empty_ranges)]
        None => 1..=0,
        Some(t) => {
            let k0 = libm::ceil((a - hi) / t);
            let k1 = libm::floor((b - lo) / t);
            if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 64.0 {
                #[allow(clippy::reversed_empty_ranges)]
                return 1..=0;
            }
            (k0 as i64)..=(k1 as i64)
        }
    }
}

impl<'a> Classifier<'a> {
    fn new(pieces: &'a [Piece], periods: [Option<f64>; 2]) -> Option<Self> {
        let mut segs = Vec::new();
        for (i, p) in pieces.iter().enumerate() {
            for (t0, t1) in p.intervals() {
                if p.straight {
                    segs.push(seg_of(pieces, i, t0, t1)?);
                } else {
                    let m = 0.5 * (t0 + t1);
                    segs.push(seg_of(pieces, i, t0, m)?);
                    segs.push(seg_of(pieces, i, m, t1)?);
                }
            }
        }
        let mut window = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for s in &segs {
            for c in 0..2 {
                window[0][c] = window[0][c].min(s.b[0][c]);
                window[1][c] = window[1][c].max(s.b[1][c]);
            }
        }
        Some(Self {
            pieces,
            segs,
            period_u: periods[0],
            period_v: periods[1],
            window,
        })
    }

    /// `Some(true)` when `q` lies inside the face, `Some(false)` outside,
    /// `None` when the trims come too close to decide.
    fn classify(&self, q: &UvBox) -> Option<bool> {
        let mut reps = Vec::new();
        match self.period_v {
            None => reps.push(*q),
            Some(t) => {
                for k in shifts(
                    Some(t),
                    q[0][1],
                    q[1][1],
                    self.window[0][1],
                    self.window[1][1],
                ) {
                    let d = k as f64 * t;
                    reps.push([[q[0][0], q[0][1] + d], [q[1][0], q[1][1] + d]]);
                }
            }
        }
        let mut unknown = false;
        for r in &reps {
            match self.classify_one(r) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => unknown = true,
            }
        }
        if unknown {
            None
        } else {
            Some(false)
        }
    }

    fn classify_one(&self, q: &UvBox) -> Option<bool> {
        if q[1][1] < self.window[0][1] || q[0][1] > self.window[1][1] {
            return Some(false);
        }
        if self.period_u.is_none() && (q[1][0] < self.window[0][0] || q[0][0] > self.window[1][0]) {
            return Some(false);
        }
        let mag = q.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
        let tol = CLASSIFY_TOLERANCE * (1.0 + mag);
        let wide = [
            [q[0][0] - tol, q[0][1] - tol],
            [q[1][0] + tol, q[1][1] + tol],
        ];
        let c = [0.5 * (q[0][0] + q[1][0]), 0.5 * (q[0][1] + q[1][1])];
        let mut crossings = 0usize;
        for s in &self.segs {
            if !self.resolve(*s, &wide, c, 0, &mut crossings) {
                return None;
            }
        }
        Some(crossings % 2 == 1)
    }

    /// Counts `s`'s upward-ray crossings from `c` after bisecting it until no
    /// part of it meets `wide`; `false` when that cannot be done.
    fn resolve(
        &self,
        s: Seg,
        wide: &UvBox,
        c: [f64; 2],
        depth: u32,
        crossings: &mut usize,
    ) -> bool {
        let tu = self.period_u;
        let straight = self.pieces.get(s.piece).is_some_and(|p| p.straight);
        for k in shifts(tu, s.b[0][0], s.b[1][0], wide[0][0], wide[1][0]) {
            let d = tu.map_or(0.0, |t| k as f64 * t);
            let b = [[s.b[0][0] + d, s.b[0][1]], [s.b[1][0] + d, s.b[1][1]]];
            if !boxes_meet(&b, wide) {
                continue;
            }
            if straight {
                if line_meets_box([s.a[0] + d, s.a[1]], [s.z[0] + d, s.z[1]], wide) {
                    return false;
                }
                continue;
            }
            if depth >= MAX_CLASSIFY_DEPTH
                || s.t1.partial_cmp(&s.t0) != Some(std::cmp::Ordering::Greater)
            {
                return false;
            }
            let m = 0.5 * (s.t0 + s.t1);
            let (Some(l), Some(r)) = (
                seg_of(self.pieces, s.piece, s.t0, m),
                seg_of(self.pieces, s.piece, m, s.t1),
            ) else {
                return false;
            };
            return self.resolve(l, wide, c, depth + 1, crossings)
                && self.resolve(r, wide, c, depth + 1, crossings);
        }
        // No image of the segment meets the query: count its chord.
        for k in shifts(tu, s.a[0].min(s.z[0]), s.a[0].max(s.z[0]), c[0], c[0]) {
            let d = tu.map_or(0.0, |t| k as f64 * t);
            let (a, z) = ([s.a[0] + d, s.a[1]], [s.z[0] + d, s.z[1]]);
            if (a[0] <= c[0]) != (z[0] <= c[0]) {
                let v = a[1] + (c[0] - a[0]) * (z[1] - a[1]) / (z[0] - a[0]);
                if v > c[1] {
                    *crossings += 1;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// One face.

/// What one face's refinement keeps.
struct State {
    sides: Sides,
    /// The side values the first pass attained over the body: refinement
    /// stops within `eps` of the larger of these and the face's own.
    floor: [f64; SIDES],
    /// Side values of surface points in the region being bounded that are
    /// not known to lie on the face (a fallback's rectangle, closing
    /// chords): refinement stops near them too, but they are never inner.
    reach: [f64; SIDES],
    /// Bounding a fallback rectangle: its points count towards `reach`.
    loose: bool,
    /// Refining (the second pass); the first pass encloses each piece and
    /// the face's interior once.
    refine: bool,
    eps: f64,
    work: usize,
    capped: bool,
}

impl State {
    fn new(floor: [f64; SIDES], eps: f64) -> Self {
        Self {
            sides: Sides::EMPTY,
            floor,
            reach: [f64::NEG_INFINITY; SIDES],
            loose: false,
            refine: true,
            eps,
            work: 0,
            capped: false,
        }
    }

    fn target(&self, s: usize) -> f64 {
        self.sides.inner[s].max(self.floor[s]).max(self.reach[s]) + self.eps
    }

    fn attain(&mut self, p: [f64; 3], m: [f64; 3]) {
        for s in 0..SIDES {
            let c = s % 3;
            let v = if s < 3 { -p[c] } else { p[c] } - m[c];
            if v > self.sides.inner[s] {
                self.sides.inner[s] = v;
            }
        }
    }

    fn reach(&mut self, p: [f64; 3]) {
        for s in 0..SIDES {
            let v = if s < 3 { -p[s % 3] } else { p[s % 3] };
            if v > self.reach[s] {
                self.reach[s] = v;
            }
        }
    }

    fn bound(&mut self, s: usize, upper: f64) {
        if upper > self.sides.outer[s] {
            self.sides.outer[s] = upper;
        }
    }

    /// Records each side of `e` that is resolved (cannot beat the target)
    /// and returns the sides still open.
    fn settle(&mut self, e: &Enc, skip: [bool; SIDES]) -> [bool; SIDES] {
        let mut open = [false; SIDES];
        for s in 0..SIDES {
            if skip[s] {
                continue;
            }
            let u = e.upper(s);
            if u > self.target(s) {
                open[s] = true;
            } else {
                self.bound(s, u);
            }
        }
        open
    }
}

/// A surface `o + a su + b sv + a b twist` with `a = (u - u0) / du` and
/// `b = (v - v0) / dv`. The affine map stands for the surface only where
/// the twist is negligible, and its margin then covers the twist.
struct Affine {
    o: Vec3,
    su: Vec3,
    sv: Vec3,
    u0: f64,
    du: f64,
    v0: f64,
    dv: f64,
    /// The twist's magnitude per axis; the map adds it to every margin.
    twist: [f64; 3],
    /// Whether the twist is small enough for the affine map.
    flat: bool,
}

impl Affine {
    fn of(s: &Surface) -> Option<Self> {
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => Some(Self {
                o: *origin,
                su: *u_axis,
                sv: *v_axis,
                u0: 0.0,
                du: 1.0,
                v0: 0.0,
                dv: 1.0,
                twist: [0.0; 3],
                flat: true,
            }),
            Surface::Nurbs(n)
                if n.degree_u == 1 && n.degree_v == 1 && n.count_u == 2 && n.count_v == 2 =>
            {
                if let Some(w) = &n.weights {
                    if w.iter().any(|x| *x != w[0]) {
                        return None;
                    }
                }
                let [u0, u1] = [n.knots_u[1], n.knots_u[2]];
                let [v0, v1] = [n.knots_v[1], n.knots_v[2]];
                if !(u1 > u0 && v1 > v0) {
                    return None;
                }
                // Pole (i, j) at i * 2 + j.
                let (p00, p01, p10, p11) = (n.poles[0], n.poles[1], n.poles[2], n.poles[3]);
                let d = p11 - p10 - p01 + p00;
                let size = [p00, p01, p10, p11]
                    .iter()
                    .fold(0.0_f64, |m, p| m.max(p.max_abs()));
                let twist = [
                    d.x.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                    d.y.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                    d.z.abs() * (1.0 + 4.0 * EPS) + 8.0 * EPS * size,
                ];
                Some(Self {
                    o: p00,
                    su: p10 - p00,
                    sv: p01 - p00,
                    u0,
                    du: u1 - u0,
                    v0,
                    dv: v1 - v0,
                    flat: d.max_abs() <= 1e-9 * size,
                    twist,
                })
            }
            _ => None,
        }
    }

    /// The affine part at `uv` and its margin per axis, the twist included.
    fn map(&self, uv: [f64; 2]) -> ([f64; 3], [f64; 3]) {
        let a = (uv[0] - self.u0) / self.du;
        let b = (uv[1] - self.v0) / self.dv;
        let x = self.o + a * self.su + b * self.sv;
        let mut m = [0.0; 3];
        for c in 0..3 {
            // The bilinear term's reach, also just outside the unit square.
            m[c] = 16.0
                * EPS
                * (coord(self.o, c).abs()
                    + (a.abs() + 1.0) * coord(self.su, c).abs()
                    + (b.abs() + 1.0) * coord(self.sv, c).abs())
                + self.twist[c] * (a.abs().max(1.0) * b.abs().max(1.0)).max(1.0) * 1.01;
        }
        ([x.x, x.y, x.z], m)
    }
}

/// The surface prepared both ways: for enclosures and for evaluation.
struct FaceGeo {
    geo: Geo,
    prepared: Prepared,
    domain: [[f64; 2]; 2],
    nurbs: bool,
    /// A plane, or a bilinear net (degree 1 by 1, two by two poles, equal
    /// weights): a coordinate's extremes over a region lie on its boundary.
    affine: Option<Affine>,
    /// A net's derivative bounds over its domain (see [`net_derivatives`]).
    net_d: Option<[[f64; 3]; 5]>,
    /// The analytic kinds' coordinate scale for rounding margins: the
    /// origin's magnitude plus the axes' or radii.
    scale: f64,
}

fn outside(x: f64, [lo, hi]: [f64; 2]) -> bool {
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let slack = DOMAIN_TOLERANCE * (hi - lo);
    x < lo - slack || x > hi + slack
}

impl FaceGeo {
    fn new(s: &Surface) -> Self {
        let prepared = Prepared::new(s);
        let domain = prepared.domain();
        Self {
            scale: match s {
                Surface::Plane {
                    origin,
                    u_axis,
                    v_axis,
                } => origin.max_abs() + u_axis.max_abs() + v_axis.max_abs(),
                Surface::Cylinder { frame, radius } | Surface::Sphere { frame, radius } => {
                    frame.origin.max_abs() + radius.abs() + 1.0
                }
                Surface::Cone { frame, radius, .. } => frame.origin.max_abs() + radius.abs() + 1.0,
                Surface::Torus {
                    frame,
                    major_radius,
                    minor_radius,
                } => frame.origin.max_abs() + major_radius.abs() + minor_radius.abs(),
                Surface::Nurbs(_) => 0.0,
            },
            net_d: match s {
                Surface::Nurbs(n) => net_derivatives(&Net::new(n)),
                _ => None,
            },
            geo: Geo::new(s),
            prepared,
            domain,
            nurbs: matches!(s, Surface::Nurbs(_)),
            affine: Affine::of(s),
        }
    }

    /// The surface point at `uv` and its rounding per axis.
    fn point(&self, uv: [f64; 2], row: &mut Row) -> Result<([f64; 3], [f64; 3]), Refusal> {
        if !(uv[0].is_finite() && uv[1].is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("trim point {uv:?}"),
            ));
        }
        if outside(uv[0], self.domain[0]) || outside(uv[1], self.domain[1]) {
            return Err(Refusal::new(
                RefusalCode::TrimOutsideDomain,
                format!(
                    "trim point ({}, {}) outside the surface's domain {:?}",
                    uv[0], uv[1], self.domain
                ),
            ));
        }
        let cl = |x: f64, d: [f64; 2]| {
            if x < d[0] {
                d[0]
            } else if x > d[1] {
                d[1]
            } else {
                x
            }
        };
        let uv = [cl(uv[0], self.domain[0]), cl(uv[1], self.domain[1])];
        if let (Geo::Plane { .. }, Some(af)) = (&self.geo, &self.affine) {
            // The plane is its affine map, whose margin covers its rounding.
            let (p, m) = af.map(uv);
            if !p.iter().all(|x| x.is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    format!("surface point at {uv:?}"),
                ));
            }
            return Ok((p, m));
        }
        self.prepared.row(uv[0], row);
        let ((p, _, _), err) = self.prepared.eval_bounded(row, uv[1]);
        if !p.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("surface point at {uv:?}"),
            ));
        }
        let mag = match &self.geo {
            Geo::Plane { .. } => {
                // A plane's rounding grows with u and v linearly.
                let m = self.affine.as_ref().map_or([0.0; 3], |a| a.map(uv).1);
                return Ok(([p.x, p.y, p.z], m));
            }
            Geo::Nurbs(n) => n.origin.max_abs() + p.max_abs(),
            _ => self.scale * (1.0 + uv[0].abs()) * (1.0 + uv[1].abs()),
        };
        let m = err[0] + 32.0 * EPS * mag;
        Ok(([p.x, p.y, p.z], [m; 3]))
    }

    /// Bounds per axis on `|S_u|, |S_v|, |S_uu|, |S_uv|, |S_vv|` over `hull`,
    /// `None` where unknown; a net's hold only inside its domain.
    fn derivatives(&self, hull: UvBox) -> Option<[[f64; 3]; 5]> {
        if self.kinked(&hull) {
            return None;
        }
        self.smooth_derivatives(hull)
    }

    /// Whether `b` crosses a net's knot line of C0 continuity, where its
    /// first derivatives jump.
    fn kinked(&self, b: &UvBox) -> bool {
        match &self.geo {
            Geo::Nurbs(n) => {
                n.c0u.iter().any(|&x| x > b[0][0] && x < b[1][0])
                    || n.c0v.iter().any(|&x| x > b[0][1] && x < b[1][1])
            }
            _ => false,
        }
    }

    /// [`Self::derivatives`] within each region of `hull` between a net's
    /// C0 knot lines.
    fn smooth_derivatives(&self, hull: UvBox) -> Option<[[f64; 3]; 5]> {
        let mut out = [[0.0; 3]; 5];
        let vmax = hull[0][1].abs().max(hull[1][1].abs());
        for c in 0..3 {
            let rho_z = |f: &Frame| {
                (
                    libm::hypot(coord(f.x, c), coord(f.y, c)),
                    coord(f.z, c).abs(),
                )
            };
            out[0][c] = match &self.geo {
                Geo::Plane { su, sv, .. } => {
                    out[1][c] = coord(*sv, c).abs();
                    coord(*su, c).abs()
                }
                Geo::Ruled { f, r, s, c: k } => {
                    let (rho, z) = rho_z(f);
                    let big = (r.abs() + s.abs() * vmax) * rho;
                    out[1][c] = s.abs() * rho + k.abs() * z;
                    out[2][c] = big;
                    out[3][c] = s.abs() * rho;
                    big
                }
                Geo::Sphere { f, r } => {
                    let (rho, z) = rho_z(f);
                    let a = r.abs() * (rho + z);
                    for d in 1..5 {
                        out[d][c] = a;
                    }
                    a
                }
                Geo::Torus { f, big, r } => {
                    let (rho, z) = rho_z(f);
                    let a = (big.abs() + r.abs()) * rho;
                    out[1][c] = r.abs() * (rho + z);
                    out[2][c] = a;
                    out[3][c] = r.abs() * rho;
                    out[4][c] = r.abs() * (rho + z);
                    a
                }
                // The caller keeps to the domain.
                Geo::Nurbs(_) => return self.net_d,
            };
        }
        // The frames are unit only to rounding.
        Some(out.map(|r| r.map(|v| v * (1.0 + 1e-9))))
    }

    /// An enclosure of one piece over `[a, b]`, the enclosures computed,
    /// and whether `pts` holds the piece's control points there (otherwise
    /// the corners of its whole hull).
    fn piece_enc(
        &self,
        piece: &Piece,
        a: f64,
        b: f64,
        pts: &mut Vec<[f64; 2]>,
    ) -> (Enc, usize, bool) {
        let points = piece.trim.hull_points_into(a, b, pts);
        if !points {
            let h = piece.trim.hull();
            pts.clear();
            pts.extend([h[0], h[1], [h[0][0], h[1][1]], [h[1][0], h[0][1]]]);
        }
        if let Some(af) = self.affine.as_ref().filter(|a| a.flat) {
            // An affine map takes the control points' hull onto the piece's
            // hull.
            let mut e = Enc::EMPTY;
            let mut m = [0.0_f64; 3];
            for p in pts.iter() {
                let (x, mx) = af.map(*p);
                e.add_point(x);
                for c in 0..3 {
                    m[c] = m[c].max(mx[c]);
                }
            }
            e.widen(m);
            return (e, 1, points);
        }
        let (e, n) = self.geo.enclose(&box_of(pts));
        (e, n, points)
    }
}

/// A surface point and its rounding per axis.
type End = ([f64; 3], [f64; 3]);

/// Buffers one face's pieces reuse.
#[derive(Default)]
struct Scratch {
    pts: Vec<[f64; 2]>,
    ends: Vec<End>,
    stack: Vec<(f64, f64, u32, End, End)>,
}

/// Refines one boundary piece; returns its points at its range's start and
/// end where they were evaluated.
fn bound_piece(
    g: &FaceGeo,
    piece: &Piece,
    st: &mut State,
    row: &mut Row,
    sc: &mut Scratch,
) -> Result<[Option<End>; 2], Refusal> {
    let mut eval = |t: f64, st: &mut State| -> Result<End, Refusal> {
        let uv = piece.trim.point(t);
        let (p, m) = g.point(uv, row)?;
        if piece.attained {
            st.attain(p, m);
        } else {
            st.reach(p);
        }
        Ok((p, m))
    };
    let [a, b] = piece.trim.range;
    if let Some(af) = g.affine.as_ref().filter(|a| piece.straight && a.flat) {
        // A segment of an affine surface: its end points are its extremes.
        let mut out = [None; 2];
        for (k, t) in [a, b].into_iter().enumerate() {
            let uv = piece.trim.point(t);
            let (p, m) = eval(t, st)?;
            out[k] = Some((p, m));
            let (_, ma) = af.map(uv);
            for s in 0..SIDES {
                st.bound(
                    s,
                    side_value(Vec3::new(p[0], p[1], p[2]), s) + m[s % 3] + 2.0 * ma[s % 3],
                );
            }
        }
        st.work += 1;
        return Ok(out);
    }
    if piece.straight && piece.iso && !g.nurbs {
        // An iso line of an analytic surface: a segment or an arc whose
        // enclosure is its own extremes, which it attains.
        let mut out = [None; 2];
        if g.domain.iter().flatten().any(|x| x.is_finite()) {
            out = [Some(eval(a, st)?), Some(eval(b, st)?)];
        }
        let (e, w, _) = g.piece_enc(piece, a, b, &mut sc.pts);
        st.work += w;
        let mut p = [0.0; 3];
        let mut q = [0.0; 3];
        for c in 0..3 {
            p[c] = e.hi[c] - e.m[c];
            q[c] = e.lo[c] + e.m[c];
        }
        if piece.attained {
            st.attain(p, e.m);
            st.attain(q, e.m);
        } else {
            st.reach(p);
            st.reach(q);
        }
        for s in 0..SIDES {
            st.bound(s, e.upper(s));
        }
        return Ok(out);
    }
    // A second-order bound: with `|f''| <= k` per axis along the piece,
    // `f <= max(f(t0), f(t1)) + k h^2 / 8` over `[t0, t1]`.
    let curv = piece.deriv.and_then(|[d1, d2]| {
        let sd = g.smooth_derivatives(piece.trim.hull())?;
        let mut k = [0.0; 3];
        for c in 0..3 {
            k[c] = (sd[2][c] * d1[0] * d1[0]
                + 2.0 * sd[3][c] * d1[0] * d1[1]
                + sd[4][c] * d1[1] * d1[1]
                + sd[0][c] * d2[0]
                + sd[1][c] * d2[1])
                * (1.0 + 1e-9);
        }
        k.iter().all(|x| x.is_finite()).then_some(k)
    });
    let ends = &mut sc.ends;
    ends.clear();
    ends.push(eval(a, st)?);
    for (_, t1) in piece.runs() {
        ends.push(eval(t1, st)?);
    }
    let out = [ends.first().copied(), ends.last().copied()];
    let stack = &mut sc.stack;
    stack.clear();
    for (i, (t0, t1)) in piece.runs().enumerate() {
        stack.push((t0, t1, 0, ends[i], ends[i + 1]));
    }
    // The first interval pops first.
    stack.reverse();
    while let Some((t0, t1, depth, e0, e1)) = stack.pop() {
        let (mut e, w, points) = g.piece_enc(piece, t0, t1, &mut sc.pts);
        st.work += w;
        // A net's bounds hold where the sub-curve does not cross a knot line
        // of C0 continuity and stays in its domain (see `Net::contains`).
        let smooth = match &g.geo {
            Geo::Nurbs(n) if curv.is_some() => {
                points && !g.kinked(&box_of(&sc.pts)) && n.contains(&sc.pts)
            }
            _ => true,
        };
        if let (Some(k), true) = (curv, smooth) {
            let h = t1 - t0;
            for c in 0..3 {
                let q = k[c] * h * h * 0.125 * (1.0 + 1e-9);
                let hi = (e0.0[c] + e0.1[c]).max(e1.0[c] + e1.1[c]) + q;
                let lo = (e0.0[c] - e0.1[c]).min(e1.0[c] - e1.1[c]) - q;
                if hi < e.hi[c] {
                    e.hi[c] = hi;
                }
                if lo > e.lo[c] {
                    e.lo[c] = lo;
                }
            }
        }
        let open = st.settle(&e, [false; SIDES]);
        if !open.iter().any(|o| *o) {
            continue;
        }
        let m = piece.split(t0, t1);
        if !st.refine || depth >= MAX_T_DEPTH || st.work >= WORK_CAP || !(m > t0 && m < t1) {
            st.capped |= st.refine && (depth >= MAX_T_DEPTH || st.work >= WORK_CAP);
            for (s, o) in open.iter().enumerate() {
                if *o {
                    st.bound(s, e.upper(s));
                }
            }
            continue;
        }
        let em = eval(m, st)?;
        stack.push((m, t1, depth + 1, em, e1));
        stack.push((t0, m, depth + 1, e0, em));
    }
    Ok(out)
}

/// Bounds a closing chord from the points of the trims it joins (`a`, `b`,
/// already attained) and the surface's first derivatives over it: along
/// the chord a coordinate moves by at most `|S_u| |du| + |S_v| |dv|`.
/// Returns `false`, having bounded only resolved sides, when that leaves a
/// side open or no derivative bound holds there; the caller then bounds the
/// chord as a piece.
fn bound_chord(g: &FaceGeo, piece: &Piece, a: End, b: End, st: &mut State) -> bool {
    let hull = piece.trim.hull();
    if let Geo::Nurbs(n) = &g.geo {
        let inside = (0..2).all(|k| hull[0][k] >= n.dom[k][0] && hull[1][k] <= n.dom[k][1]);
        if !inside {
            return false;
        }
    }
    let Some(sd) = g.derivatives(hull) else {
        return false;
    };
    let (du, dv) = (hull[1][0] - hull[0][0], hull[1][1] - hull[0][1]);
    let mut e = Enc::EMPTY;
    for c in 0..3 {
        let l = (sd[0][c] * du + sd[1][c] * dv) * (1.0 + 1e-9);
        if !l.is_finite() {
            return false;
        }
        e.hi[c] = (a.0[c] + a.1[c]).max(b.0[c] + b.1[c]) + l;
        e.lo[c] = (a.0[c] - a.1[c]).min(b.0[c] - b.1[c]) - l;
    }
    st.work += 1;
    !st.settle(&e, [false; SIDES]).iter().any(|o| *o)
}

/// The critical points of each coordinate on a sphere or torus: isolated
/// points `(u, v)`, and lines `v = const` (every u) with the sides they
/// count for and their value per side.
struct Critical {
    points: Vec<[f64; 2]>,
    lines: Vec<(f64, [Option<f64>; SIDES])>,
}

fn critical(geo: &Geo) -> Critical {
    let mut out = Critical {
        points: Vec::new(),
        lines: Vec::new(),
    };
    let line_value = |c: usize, v: f64, oc: f64, value: f64| {
        let mut sides = [None; SIDES];
        sides[c] = Some(-(oc + value));
        sides[c + 3] = Some(oc + value);
        (v, sides)
    };
    match geo {
        Geo::Sphere { f, r } => {
            for c in 0..3 {
                let (a, b, z) = (coord(f.x, c), coord(f.y, c), coord(f.z, c));
                let rho = libm::hypot(a, b);
                if rho > 0.0 {
                    let phi = libm::atan2(b, a);
                    let v = libm::atan2(z, rho);
                    out.points.push([phi, v]);
                    out.points.push([phi + PI, -v]);
                } else {
                    let oc = coord(f.origin, c);
                    out.lines.push(line_value(c, FRAC_PI_2, oc, *r * z));
                    out.lines.push(line_value(c, -FRAC_PI_2, oc, -*r * z));
                }
            }
        }
        Geo::Torus { f, big, r } => {
            for c in 0..3 {
                let (a, b, z) = (coord(f.x, c), coord(f.y, c), coord(f.z, c));
                let rho = libm::hypot(a, b);
                let oc = coord(f.origin, c);
                if rho > 0.0 {
                    let phi = libm::atan2(b, a);
                    let v1 = libm::atan2(z, rho);
                    let v2 = libm::atan2(z, -rho);
                    out.points.push([phi, v1]);
                    out.points.push([phi, v1 + PI]);
                    out.points.push([phi + PI, v2]);
                    out.points.push([phi + PI, v2 + PI]);
                } else {
                    out.lines.push(line_value(c, FRAC_PI_2, oc, *r * z));
                    out.lines.push(line_value(c, -FRAC_PI_2, oc, -*r * z));
                }
            }
            if *big < *r {
                // A spindle torus: its apexes, where the u derivative vanishes
                // on a whole line, are points on the axis.
                let v = libm::acos(-*big / *r);
                for v in [v, -v] {
                    let p = f.origin + (*r * libm::sin(v)) * f.z;
                    let mut sides = [None; SIDES];
                    for (s, x) in sides.iter_mut().enumerate() {
                        *x = Some(side_value(p, s));
                    }
                    out.lines.push((v, sides));
                }
            }
        }
        _ => {}
    }
    out
}

/// Counts the interior critical points of a sphere or torus.
fn bound_critical(
    g: &FaceGeo,
    cls: &Classifier<'_>,
    st: &mut State,
    row: &mut Row,
) -> Result<(), Refusal> {
    let crit = critical(&g.geo);
    let w = cls.window;
    let (tu, tv) = (cls.period_u, cls.period_v);
    for p in &crit.points {
        let mut inside = false;
        let mut maybe = false;
        for ku in shifts(tu, p[0], p[0], w[0][0], w[1][0]) {
            for kv in shifts(tv, p[1], p[1], w[0][1], w[1][1]) {
                let q = [
                    p[0] + tu.map_or(0.0, |t| ku as f64 * t),
                    p[1] + tv.map_or(0.0, |t| kv as f64 * t),
                ];
                match cls.classify(&[q, q]) {
                    Some(true) => inside = true,
                    None => maybe = true,
                    Some(false) => {}
                }
            }
        }
        if !(inside || maybe) {
            continue;
        }
        st.work += 1;
        let v = p[1].max(g.domain[1][0]).min(g.domain[1][1]);
        g.prepared.row(p[0], row);
        let ((x, _, _), err) = g.prepared.eval_bounded(row, v);
        let m = err[0] + 64.0 * EPS * (x.max_abs() + 1.0);
        let pt = [x.x, x.y, x.z];
        if inside {
            st.attain(pt, [m; 3]);
        }
        for s in 0..SIDES {
            st.bound(s, side_value(x, s) + m);
        }
    }
    for (v, sides) in &crit.lines {
        // Outer: the line meets the trims' v range; inner: a sample on it
        // lies inside.
        let reps: Vec<f64> = shifts(tv, *v, *v, w[0][1], w[1][1])
            .map(|k| *v + tv.map_or(0.0, |t| k as f64 * t))
            .collect();
        if reps.is_empty() {
            continue;
        }
        let mag = g.prepared_mag();
        let m = 64.0 * EPS * mag;
        let mut inside = false;
        'samples: for &vr in &reps {
            for i in 0..LINE_SAMPLES {
                let u = w[0][0] + (w[1][0] - w[0][0]) * (i as f64 + 0.5) / LINE_SAMPLES as f64;
                st.work += 1;
                if cls.classify(&[[u, vr], [u, vr]]) == Some(true) {
                    inside = true;
                    break 'samples;
                }
            }
        }
        for (s, x) in sides.iter().enumerate() {
            if let Some(x) = x {
                st.bound(s, x + m);
                if inside && x - m > st.sides.inner[s] {
                    st.sides.inner[s] = x - m;
                }
            }
        }
    }
    Ok(())
}

/// Bounds every side by the critical points and lines of a sphere or torus
/// that have an image in `w`, and returns whether one could move a side
/// beyond its target: only then does classifying them against the trims
/// (and checking the loops it needs) matter.
fn critical_open(
    g: &FaceGeo,
    w: &UvBox,
    periods: [Option<f64>; 2],
    st: &mut State,
    row: &mut Row,
) -> bool {
    let [tu, tv] = periods;
    let many = |t: Option<f64>, c: usize| {
        t.is_some_and(|t| {
            let n = (w[1][c] - w[0][c]) / t;
            n.is_nan() || n > 60.0
        })
    };
    if many(tu, 0) || many(tv, 1) {
        return true;
    }
    let crit = critical(&g.geo);
    for p in &crit.points {
        let near = !shifts(tu, p[0], p[0], w[0][0], w[1][0]).is_empty()
            && !shifts(tv, p[1], p[1], w[0][1], w[1][1]).is_empty();
        if !near {
            continue;
        }
        st.work += 1;
        let v = p[1].max(g.domain[1][0]).min(g.domain[1][1]);
        g.prepared.row(p[0], row);
        let ((x, _, _), err) = g.prepared.eval_bounded(row, v);
        let m = err[0] + 64.0 * EPS * (x.max_abs() + 1.0);
        for s in 0..SIDES {
            let u = side_value(x, s) + m;
            if u.is_nan() || u > st.target(s) {
                return true;
            }
            st.bound(s, u);
        }
    }
    let m = 64.0 * EPS * g.prepared_mag();
    for (v, sides) in &crit.lines {
        if shifts(tv, *v, *v, w[0][1], w[1][1]).is_empty() {
            continue;
        }
        for (s, x) in sides.iter().enumerate() {
            if let Some(x) = x {
                if (x + m).is_nan() || x + m > st.target(s) {
                    return true;
                }
                st.bound(s, x + m);
            }
        }
    }
    false
}

impl FaceGeo {
    /// Per axis, the widening of both ends of each side for the trims'
    /// excursion outside the domain (`delta`, per parameter and end: the
    /// trims' control points' excursion, which bounds the face's): a bound
    /// on `|S_ext(u, v) - S(clamp(u, v))|` over the face, `S_ext` the
    /// surface a kernel extrapolates. Moving `u` to its clamp and then `v`
    /// to its clamp, it is at most the sum over the two parameters of the
    /// larger of their two ends' bounds.
    ///
    /// - A net: [`Net::extension`] at each end the trims leave by, exact up
    ///   to rounding (the end span's polynomial blossomed over the strip),
    ///   with the other parameter's excursion for the corners. The face's
    ///   pieces outside the domain take no second-order bound
    ///   ([`Net::contains`]), so its sides bound the clamped surface on
    ///   their own and this bound carries them to the extrapolated one.
    /// - A sphere past its poles: `2 r` times the excursion. `r` bounds each
    ///   coordinate's derivative in latitude; the factor 2 covers the
    ///   second-order bound of a piece there, which holds for the analytic
    ///   continuation rather than the clamped surface.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::TrimOutsideDomain`] when a trim's point at a knot or
    /// a range end, or its refined hull between them, lies outside the
    /// domain beyond the tolerance, or a net's
    /// extrapolated weights over the strip are not shown positive.
    fn excursion_widening(&self, pieces: &[Piece]) -> Result<[f64; 3], Refusal> {
        let bounded = self.domain.iter().flatten().all(|x| x.is_finite())
            || matches!(self.geo, Geo::Sphere { .. });
        if !bounded {
            return Ok([0.0; 3]);
        }
        // Per parameter, below the domain and above it.
        let mut delta = [[0.0_f64; 2]; 2];
        for piece in pieces {
            for (k, (t0, t1)) in piece.intervals().enumerate() {
                // The points the refinement may not evaluate (it starts from
                // the C1 runs) are checked against the tolerance here.
                for t in [t0, t1].into_iter().skip(usize::from(k > 0)) {
                    let uv = piece.trim.point(t);
                    if outside(uv[0], self.domain[0]) || outside(uv[1], self.domain[1]) {
                        return Err(Refusal::new(
                            RefusalCode::TrimOutsideDomain,
                            format!(
                                "trim point ({}, {}) outside the surface's domain {:?}",
                                uv[0], uv[1], self.domain
                            ),
                        ));
                    }
                }
                self.excursion(&piece.trim, t0, t1, 0, &mut delta);
            }
        }
        if delta == [[0.0; 2]; 2] {
            return Ok([0.0; 3]);
        }
        // Between its points at knots a trim's hull can still leave the
        // domain beyond the tolerance: the definition does not say where
        // that part lies either.
        for (c, d) in delta.iter().enumerate() {
            let [lo, hi] = self.domain[c];
            let slack = DOMAIN_TOLERANCE * (hi - lo);
            if lo.is_finite() && hi.is_finite() && (d[0] > slack || d[1] > slack) {
                return Err(Refusal::new(
                    RefusalCode::TrimOutsideDomain,
                    format!(
                        "a trim leaves the surface's domain {:?} by {} in {}, beyond the \
                         tolerance",
                        self.domain,
                        d[0].max(d[1]),
                        ["u", "v"][c]
                    ),
                ));
            }
        }
        self.widening(&delta)
    }

    /// Per axis a bound on `|S_ext(u, v) - S(clamp(u, v))|` over the points
    /// within `delta` (per parameter, below and above) of the domain, as
    /// [`Self::excursion_widening`] gives it.
    ///
    /// # Errors
    ///
    /// [`RefusalCode::TrimOutsideDomain`] when a net's extrapolated weights
    /// there are not shown positive.
    fn widening(&self, delta: &[[f64; 2]; 2]) -> Result<[f64; 3], Refusal> {
        // A clamped point moves by at most one end's excursion per
        // parameter.
        let mut w = [0.0; 3];
        for d in 0..2 {
            let mut wd = [0.0_f64; 3];
            for side in 0..2 {
                let x = delta[d][side];
                if x <= 0.0 {
                    continue;
                }
                let m = match &self.geo {
                    Geo::Nurbs(n) => {
                        n.extension(d, side == 1, x, delta[1 - d]).ok_or_else(|| {
                            Refusal::new(
                                RefusalCode::TrimOutsideDomain,
                                format!(
                                    "the net's extrapolation {x} beyond its domain {:?} in {} has \
                                 weights not shown positive",
                                    self.domain,
                                    ["u", "v"][d]
                                ),
                            )
                        })?
                    }
                    Geo::Sphere { r, .. } => [2.0 * r * x; 3],
                    _ => [f64::INFINITY; 3],
                };
                for c in 0..3 {
                    wd[c] = wd[c].max(m[c]);
                }
            }
            for c in 0..3 {
                w[c] += wd[c];
            }
        }
        Ok(w.map(|x| x * (1.0 + 1e-12)))
    }

    /// Raises `delta` (per parameter and end) to a bound on how far the trim
    /// leaves the domain over `[t0, t1]`: its control points' excursion
    /// plus their rounding ([`PreparedTrim::hull_rounding`]), at most the
    /// excursion of the definition's own poles where it gives them
    /// ([`PreparedTrim::exact_box`]), bisected while the computed excursion
    /// is more than twice the interval's end points' and above round-off.
    fn excursion(&self, t: &PreparedTrim, t0: f64, t1: f64, depth: u32, delta: &mut [[f64; 2]; 2]) {
        let beyond = |p: [f64; 2], c: usize| {
            let [lo, hi] = self.domain[c];
            if lo.is_finite() && hi.is_finite() {
                (lo - p[c]).max(p[c] - hi).max(0.0)
            } else {
                0.0
            }
        };
        // Which end: below the domain (0) or above it (1).
        let side = |p: [f64; 2], c: usize| usize::from(p[c] > self.domain[c][1]);
        let pts = t.hull_points(t0, t1).unwrap_or_else(|| {
            let h = t.hull();
            vec![h[0], h[1]]
        });
        let mut hull = [[0.0_f64; 2]; 2];
        for p in &pts {
            for c in 0..2 {
                let k = side(*p, c);
                hull[c][k] = hull[c][k].max(beyond(*p, c));
            }
        }
        // The exact curve's excursion: the computed points moved outwards
        // by their rounding, within the exact poles' excursion.
        let pad = t.hull_rounding(t0, t1);
        let exact = t.exact_box();
        let mut padded = hull;
        for c in 0..2 {
            let [lo, hi] = self.domain[c];
            if !(lo.is_finite() && hi.is_finite()) {
                continue;
            }
            if pad[c] > 0.0 {
                for p in &pts {
                    padded[c][0] = padded[c][0].max(lo - p[c] + pad[c]);
                    padded[c][1] = padded[c][1].max(p[c] + pad[c] - hi);
                }
            }
            if let Some(e) = exact {
                padded[c][0] = padded[c][0].min((lo - e[0][c]).max(0.0));
                padded[c][1] = padded[c][1].min((e[1][c] - hi).max(0.0));
            }
        }
        if (0..2).all(|c| padded[c][0] <= delta[c][0] && padded[c][1] <= delta[c][1]) {
            return;
        }
        let (a, b) = (t.eval(t0).0, t.eval(t1).0);
        let mut ends = [0.0_f64; 2];
        for c in 0..2 {
            ends[c] = beyond(a, c).max(beyond(b, c));
        }
        let m = 0.5 * (t0 + t1);
        // Beyond the tolerance the hull is refined until it is within it,
        // or an end point (on the curve) is beyond it too, or the depth runs
        // out: the face refuses otherwise.
        let settled = (0..2).all(|c| {
            let [lo, hi] = self.domain[c];
            let h = hull[c][0].max(hull[c][1]);
            let tol = DOMAIN_TOLERANCE * (hi - lo);
            let within = padded[c][0].max(padded[c][1]) <= tol || ends[c] > tol;
            (h <= 2.0 * ends[c] || h <= 1e-13 * (hi - lo).abs()) && within
        });
        if settled || depth >= 48 || !(m > t0 && m < t1) {
            for c in 0..2 {
                for k in 0..2 {
                    delta[c][k] = delta[c][k].max(padded[c][k]);
                }
            }
            return;
        }
        self.excursion(t, t0, m, depth + 1, delta);
        self.excursion(t, m, t1, depth + 1, delta);
    }

    fn prepared_mag(&self) -> f64 {
        match &self.geo {
            Geo::Sphere { f, r } => f.origin.max_abs() + r,
            Geo::Torus { f, big, r } => f.origin.max_abs() + big.abs() + r,
            _ => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Bridged faces: the guard that ties a bridge, a chord or a joint to its
// vertex.

/// Why a guard has no bound; the face then refuses.
type Unbounded = &'static str;

/// `d + rho` rounded upwards, at least 0, for a computed difference `d`
/// (one rounding) and a non-negative `rho`.
fn past(d: f64, rho: f64) -> f64 {
    let d = if d > 0.0 {
        d * (1.0 + 2.0 * EPS)
    } else {
        d * (1.0 - 2.0 * EPS)
    };
    ((d + rho) * (1.0 + 2.0 * EPS)).max(0.0)
}

impl FaceGeo {
    /// `p` clamped to the domain.
    fn clamp_uv(&self, p: [f64; 2]) -> [f64; 2] {
        let cl = |x: f64, [lo, hi]: [f64; 2]| {
            if x < lo {
                lo
            } else if x > hi {
                hi
            } else {
                x
            }
        };
        [cl(p[0], self.domain[0]), cl(p[1], self.domain[1])]
    }

    /// Bounds per axis on `|S_u|` and `|S_v|` over `hull` (clamped to the
    /// domain), which are Lipschitz constants of the surface there: a net's
    /// hold over its whole domain, C0 knot lines included, since it is
    /// continuous.
    fn lipschitz(&self, hull: UvBox) -> Option<[[f64; 3]; 2]> {
        let d = self.smooth_derivatives(hull)?;
        let l = [d[0], d[1]];
        l.iter().flatten().all(|x| x.is_finite()).then_some(l)
    }

    /// Per axis a bound on `|X - v|` over every point `X` of the image of a
    /// straight segment of the parameter plane: the segment between `pa`
    /// and `pb` as computed, whose exact ends lie within `rho` (per
    /// parameter) of them. Every image counts: the definition's, which
    /// evaluates the surface clamped to its domain, and a kernel's that
    /// extrapolates it there.
    ///
    /// The segment `s` between the clamped ends lies in the domain. Along a
    /// part of it that crosses no C0 knot line a coordinate `f` of the
    /// surface has `|f''| <= k = Σ |S_ij| |Δ_i| |Δ_j|` (`Δ` the part's
    /// difference: the bound does not depend on the parameterisation), so
    /// `f` lies within `k / 8` of its chord, which lies between its ends'
    /// values. A net's segment is cut where it crosses its C0 knot lines,
    /// each cut exact in the crossing parameter and within its rounding in
    /// the other, which is carried. The exact segment's clamped points lie
    /// within `rho` plus the excursion past each end of the domain
    /// (`delta`, summed, since clamping moves a point by at most that
    /// towards the segment between the clamped ends) of `s`: the surface
    /// moves by its Lipschitz constant times that. A kernel's extrapolated
    /// point lies within the domain widening for `delta`
    /// ([`Self::widening`]) of the clamped one.
    fn guard_segment(
        &self,
        pa: [f64; 2],
        pb: [f64; 2],
        rho: [f64; 2],
        v: [f64; 3],
        row: &mut Row,
    ) -> Result<Result<[f64; 3], Unbounded>, Refusal> {
        let mut delta = [[0.0_f64; 2]; 2];
        for c in 0..2 {
            let [lo, hi] = self.domain[c];
            for x in [pa[c], pb[c]] {
                if lo.is_finite() {
                    delta[c][0] = delta[c][0].max(past(lo - x, rho[c]));
                }
                if hi.is_finite() {
                    delta[c][1] = delta[c][1].max(past(x - hi, rho[c]));
                }
            }
        }
        let outside = delta != [[0.0; 2]; 2];
        let w = if outside {
            match self.widening(&delta) {
                Ok(w) => w,
                Err(_) => return Ok(Err("leaves the domain where the extension is not bounded")),
            }
        } else {
            [0.0; 3]
        };
        let (ca, cb) = (self.clamp_uv(pa), self.clamp_uv(pb));
        // The cuts at a net's C0 knot lines: (where, point, its rounding).
        let mut cuts: Vec<(f64, [f64; 2], [f64; 2])> = Vec::new();
        if let Geo::Nurbs(n) = &self.geo {
            for (k, lines) in [(0_usize, &n.c0u), (1, &n.c0v)] {
                let o = 1 - k;
                let (lo, hi) = (ca[k].min(cb[k]), ca[k].max(cb[k]));
                for &x in lines.iter().filter(|&&x| x > lo && x < hi) {
                    let s = (x - ca[k]) / (cb[k] - ca[k]);
                    let d = cb[o] - ca[o];
                    let mut q = [x; 2];
                    let [dlo, dhi] = self.domain[o];
                    q[o] = (ca[o] + s * d).clamp(dlo, dhi);
                    let mut e = [0.0; 2];
                    // `s` within 3 ulps, then a product and a sum.
                    e[o] = 8.0 * EPS * (d.abs() + q[o].abs());
                    cuts.push((s, q, e));
                }
            }
            cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let lip = if outside || rho != [0.0; 2] || !cuts.is_empty() {
            let mut hull = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
            for p in [pa, pb, ca, cb] {
                for c in 0..2 {
                    let pad = rho[c] + delta[c][0] + delta[c][1];
                    hull[0][c] = hull[0][c].min(p[c] - pad);
                    hull[1][c] = hull[1][c].max(p[c] + pad);
                }
            }
            match self.lipschitz(hull) {
                Some(l) => l,
                None => return Ok(Err("has no derivative bound")),
            }
        } else {
            [[0.0; 3]; 2]
        };
        let mut points = Vec::with_capacity(cuts.len() + 2);
        points.push((ca, [0.0; 2]));
        points.extend(cuts.iter().map(|&(_, q, e)| (q, e)));
        points.push((cb, [0.0; 2]));
        let mut g = [0.0_f64; 3];
        for pair in points.windows(2) {
            let &[(x0, e0), (x1, e1)] = pair else {
                continue;
            };
            let du = (x1[0] - x0[0]).abs() * (1.0 + 2.0 * EPS);
            let dv = (x1[1] - x0[1]).abs() * (1.0 + 2.0 * EPS);
            let mut k = [0.0; 3];
            if du != 0.0 || dv != 0.0 {
                let hull = [
                    [x0[0].min(x1[0]), x0[1].min(x1[1])],
                    [x0[0].max(x1[0]), x0[1].max(x1[1])],
                ];
                let Some(sd) = self.derivatives(hull) else {
                    return Ok(Err(if self.kinked(&hull) {
                        "crosses a C0 knot line"
                    } else {
                        "has no derivative bound"
                    }));
                };
                for c in 0..3 {
                    k[c] = (sd[2][c] * du * du + 2.0 * sd[3][c] * du * dv + sd[4][c] * dv * dv)
                        * (1.0 + 1e-9);
                }
            }
            let (p0, m0) = self.point(x0, row)?;
            let (p1, m1) = if x1 == x0 {
                (p0, m0)
            } else {
                self.point(x1, row)?
            };
            for c in 0..3 {
                let end = |p: [f64; 3], m: [f64; 3]| {
                    (p[c] - v[c]).abs() + m[c] + 2.0 * EPS * (p[c].abs() + v[c].abs())
                };
                let cut = lip[0][c] * e0[0].max(e1[0]) + lip[1][c] * e0[1].max(e1[1]);
                g[c] = g[c].max(end(p0, m0).max(end(p1, m1)) + k[c] * 0.125 + cut);
            }
        }
        for c in 0..3 {
            let moved = lip[0][c] * (rho[0] + delta[0][0] + delta[0][1])
                + lip[1][c] * (rho[1] + delta[1][0] + delta[1][1]);
            g[c] = (g[c] + moved + w[c]) * (1.0 + 8.0 * EPS);
        }
        if !g.iter().all(|x| x.is_finite()) {
            return Ok(Err("has no finite bound"));
        }
        Ok(Ok(g))
    }
}

/// What guarding a bridged face found.
#[derive(Default)]
struct Guarded {
    /// The largest bound over the axes and the face's bridges, chords and
    /// joints on how far their images lie from their vertices.
    bound: f64,
}

fn max3(x: [f64; 3]) -> f64 {
    x[0].max(x[1]).max(x[2])
}

fn gap(detail: String) -> Refusal {
    Refusal::new(RefusalCode::ToleranceGap, detail)
}

/// Guards each bridge of a face: its image must lie within its vertex's
/// cube (the vertex's tolerance on every axis).
fn guard_bridges(
    g: &FaceGeo,
    ps: &[Piece],
    joints: &BoxJoints,
    row: &mut Row,
    out: &mut Guarded,
) -> Result<(), Refusal> {
    for &i in &joints.bridges {
        let Some(v) = joints.ends.get(i).copied().flatten() else {
            return Err(gap(format!("bridge at trim {i} without a vertex")));
        };
        let Some(piece) = ps.get(i) else { continue };
        let t = &piece.trim;
        let [a, b] = t.range;
        let vv = [v.vertex.x, v.vertex.y, v.vertex.z];
        let bound = match g.guard_segment(t.point(a), t.point(b), t.hull_rounding(a, b), vv, row)? {
            Ok(x) => max3(x),
            Err(why) => return Err(gap(format!("bridge at trim {i} {why}"))),
        };
        if bound > v.tolerance {
            return Err(gap(format!(
                "bridge at trim {i} leaves its vertex cube by {:e} (tolerance {:e})",
                bound - v.tolerance,
                v.tolerance
            )));
        }
        out.bound = out.bound.max(bound);
    }
    Ok(())
}

/// Guards each closing chord of a bridged face like a bridge, and adds each
/// exact joint's end-point term to the bound. A chord longer than the
/// chaining tolerance is a join the export left open: the face refuses as
/// the facts layer would.
fn guard_joins(
    g: &FaceGeo,
    ps: &[Piece],
    exact: &[usize],
    joints: &BoxJoints,
    row: &mut Row,
    out: &mut Guarded,
) -> Result<(), Refusal> {
    let mag = |p: [f64; 2]| p[0].abs().max(p[1].abs());
    for piece in ps {
        let Some([(k, _), _]) = piece.joins else {
            continue;
        };
        let t = &piece.trim;
        let (p, q) = (t.point(0.0), t.point(1.0));
        let diff = [q[0] - p[0], q[1] - p[1]];
        let chained = mag(diff) <= loops::GAP_TOLERANCE * (1.0 + mag(p).max(mag(q)));
        if !chained {
            return Err(Refusal::new(
                RefusalCode::OpenTrimLoop,
                format!("piece {k} ends at {p:?}, the next starts at {q:?}"),
            ));
        }
        let Some(v) = joints.ends.get(k).copied().flatten() else {
            return Err(gap(format!("chord after trim {k} without a vertex")));
        };
        let vv = [v.vertex.x, v.vertex.y, v.vertex.z];
        let bound = match g.guard_segment(p, q, t.hull_rounding(0.0, 1.0), vv, row)? {
            Ok(x) => max3(x),
            Err(why) => return Err(gap(format!("chord after trim {k} {why}"))),
        };
        if bound > v.tolerance {
            return Err(gap(format!(
                "chord after trim {k} leaves its vertex cube by {:e} (tolerance {:e})",
                bound - v.tolerance,
                v.tolerance
            )));
        }
        out.bound = out.bound.max(bound);
    }
    for &k in exact {
        let (Some(piece), Some(v)) = (ps.get(k), joints.ends.get(k).copied().flatten()) else {
            continue;
        };
        let p = trim_ends(&piece.trim).1;
        let vv = [v.vertex.x, v.vertex.y, v.vertex.z];
        match g.guard_segment(p, p, [0.0; 2], vv, row)? {
            Ok(x) => out.bound = out.bound.max(max3(x)),
            Err(why) => return Err(gap(format!("joint after trim {k} {why}"))),
        }
    }
    Ok(())
}

/// Refines the interior cells of a net over `window` (clamped to its
/// domain). With a classifier, cells outside the trims drop, cells inside
/// attain their corners, and monotone cells drop; without one (the
/// fallback), `window`'s edges are pieces of their own and only monotone
/// cells drop.
fn bound_cells(net: &Net, window: &UvBox, cls: Option<&Classifier<'_>>, st: &mut State) {
    let mut stack: Vec<(UvBox, u32)> = vec![(net.clamp(window), 0)];
    while let Some((cell, depth)) = stack.pop() {
        let su = net.spans_u(cell[0][0], cell[1][0]);
        let sv = net.spans_v(cell[0][1], cell[1][1]);
        let n = su.len() * sv.len();
        if n > 1 {
            if n <= SPLIT_SPANS {
                for &i in su.iter().rev() {
                    for &j in sv.iter().rev() {
                        let sub = [
                            [cell[0][0].max(net.ku[i]), cell[0][1].max(net.kv[j])],
                            [cell[1][0].min(net.ku[i + 1]), cell[1][1].min(net.kv[j + 1])],
                        ];
                        stack.push((sub, depth));
                    }
                }
                continue;
            }
            st.work += 1;
            let e = net.local_hull(&su, &sv);
            let open = st.settle(&e, [false; SIDES]);
            if !open.iter().any(|o| *o) {
                continue;
            }
            if st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH {
                st.capped = true;
                for (s, o) in open.iter().enumerate() {
                    if *o {
                        st.bound(s, e.upper(s));
                    }
                }
                continue;
            }
            // Split at the knot nearest the middle of the longer span run.
            let (dir, list, knots) = if su.len() >= sv.len() {
                (0, &su, &net.ku)
            } else {
                (1, &sv, &net.kv)
            };
            let k = list.len() / 2;
            let at = knots[list[k]];
            let mut lo = cell;
            let mut hi = cell;
            lo[1][dir] = at;
            hi[0][dir] = at;
            stack.push((hi, depth + 1));
            stack.push((lo, depth + 1));
            continue;
        }
        let (Some(&i), Some(&j)) = (su.first(), sv.first()) else {
            continue;
        };
        st.work += 1;
        let bz = net.bezier(i, j, &cell);
        let e = net.bezier_enc(&bz);
        // Sides whose coordinate is monotone over the cell, towards an edge
        // that is not a C0 knot line, have no extreme inside it. Towards a
        // C0 knot line the side's extreme over the cell lies on that edge,
        // so the edge curve's hull bounds it; it is dropped when that hull
        // cannot beat the side's target.
        let mono = net.monotone(&bz);
        let mut skip = [false; SIDES];
        for (s, sk) in skip.iter_mut().enumerate() {
            let c = s % 3;
            let sign: i8 = if s < 3 { -1 } else { 1 };
            for (dir, c0) in [(0usize, &net.c0u), (1, &net.c0v)] {
                let d = mono[dir][c] * sign;
                if d == 0 || *sk {
                    continue;
                }
                let edge = if d > 0 { cell[1][dir] } else { cell[0][dir] };
                if !c0.contains(&edge) {
                    *sk = true;
                } else {
                    st.work += 1;
                    let u = net.edge_enc(&bz, dir, d > 0).upper(s);
                    if u <= st.target(s) {
                        st.bound(s, u);
                        *sk = true;
                    }
                }
            }
        }
        let mut open = st.settle(&e, skip);
        if !open.iter().any(|o| *o) {
            continue;
        }
        // Only a cell that could still move a side is classified.
        let class = cls.map(|c| {
            st.work += 1;
            c.classify(&cell)
        });
        if class == Some(Some(false)) {
            continue;
        }
        let inside = class == Some(Some(true));
        if inside || st.loose {
            let (corners, m) = net.corners(&bz);
            for p in corners {
                if inside {
                    st.attain(p, m);
                } else {
                    st.reach(p);
                }
            }
            for (s, o) in open.iter_mut().enumerate() {
                if *o && e.upper(s) <= st.target(s) {
                    *o = false;
                    st.bound(s, e.upper(s));
                }
            }
            if !open.iter().any(|o| *o) {
                continue;
            }
        }
        let mu = 0.5 * (cell[0][0] + cell[1][0]);
        let mv = 0.5 * (cell[0][1] + cell[1][1]);
        let can_u = mu > cell[0][0] && mu < cell[1][0];
        let can_v = mv > cell[0][1] && mv < cell[1][1];
        if st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH || !(can_u || can_v) {
            st.capped |= st.work >= WORK_CAP || depth >= MAX_CELL_DEPTH;
            for (s, o) in open.iter().enumerate() {
                if *o {
                    st.bound(s, e.upper(s));
                }
            }
            continue;
        }
        // Split along the parameter whose edge polygons are longer (the
        // polygons, not the corners: an edge may close on itself or shrink
        // to a pole).
        let span = |a: usize, b: usize| {
            let (x, y) = (bz.pts[a], bz.pts[b]);
            let (p, q) = (net.project(x), net.project(y));
            (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs()
        };
        let (p, q) = (net.p, net.q);
        let at = |m: usize, n: usize| m * (q + 1) + n;
        let lu: f64 = (0..p)
            .map(|m| span(at(m, 0), at(m + 1, 0)) + span(at(m, q), at(m + 1, q)))
            .sum();
        let lv: f64 = (0..q)
            .map(|n| span(at(0, n), at(0, n + 1)) + span(at(p, n), at(p, n + 1)))
            .sum();
        let split_u = can_u && (!can_v || lu >= lv);
        let (mut a, mut b) = (cell, cell);
        if split_u {
            a[1][0] = mu;
            b[0][0] = mu;
        } else {
            a[1][1] = mv;
            b[0][1] = mv;
        }
        stack.push((b, depth + 1));
        stack.push((a, depth + 1));
    }
}

/// The bracket of one face, refined until each side is within `eps` of the
/// larger of its attained value and `floor`'s.
///
/// # Errors
///
/// A typed [`Refusal`]: a malformed definition, a trim point outside a net's
/// domain or a value that is not finite.
fn bracket_face(
    face: &FaceDefinition,
    floor: [f64; SIDES],
    eps: f64,
    refine: bool,
    joints: Option<&BoxJoints>,
) -> Result<FaceBox, Refusal> {
    face.validate()?;
    let g = FaceGeo::new(&face.surface);
    let periods = g.geo.periods();
    let mut row = Row::default();
    let mut ps = trim_pieces(face, joints.map_or(&[], |j| j.bridges.as_slice()));
    let mut guarded = Guarded::default();
    if let Some(j) = joints {
        guard_bridges(&g, &ps, j, &mut row, &mut guarded)?;
    }
    let exact = add_chords(&mut ps, face, periods);
    if let Some(j) = joints {
        guard_joins(&g, &ps, &exact, j, &mut row, &mut guarded)?;
    }
    let mut st = State::new(floor, eps);
    st.refine = refine;
    let mut sc = Scratch::default();
    // Each trim's points at its range's ends, for the chords that join them.
    let mut tips: Vec<[Option<End>; 2]> = Vec::with_capacity(face.trims.len());
    for piece in &ps {
        if let Some(joins) = piece.joins {
            let mut ends = [None; 2];
            for (k, &(trim, end)) in joins.iter().enumerate() {
                ends[k] = match tips.get(trim).and_then(|t| t[end]) {
                    Some(e) => Some(e),
                    // An iso line's ends are not evaluated: they are points of
                    // the face.
                    None => {
                        let t = ps.get(trim).map(|p| &p.trim);
                        match t {
                            Some(t) => {
                                let (p, m) = g.point(t.point(t.range[end]), &mut row)?;
                                // A bridge's end is not known to be the
                                // model's.
                                if ps.get(trim).is_some_and(|p| p.attained) {
                                    st.attain(p, m);
                                } else {
                                    st.reach(p);
                                }
                                Some((p, m))
                            }
                            None => None,
                        }
                    }
                };
            }
            if let [Some(a), Some(b)] = ends {
                if bound_chord(&g, piece, a, b, &mut st) {
                    continue;
                }
            }
        }
        let ends = bound_piece(&g, piece, &mut st, &mut row, &mut sc)?;
        if piece.joins.is_none() {
            tips.push(ends);
        }
    }
    let mut fallback = false;
    match &g.geo {
        Geo::Plane { .. } | Geo::Ruled { .. } => {}
        // A bilinear coordinate is harmonic: its extremes lie on the
        // boundary.
        Geo::Nurbs(_) if g.affine.is_some() => {}
        Geo::Sphere { .. } | Geo::Torus { .. } | Geo::Nurbs(_) if !refine => {
            // The first pass: the surface over the trims' box bounds the
            // interior.
            let (e, n) = g.geo.enclose(&pieces_box(&ps));
            st.work += n;
            for s in 0..SIDES {
                st.bound(s, e.upper(s));
            }
        }
        Geo::Sphere { .. } | Geo::Torus { .. }
            if !could_move(&g.geo.enclose(&pieces_box(&ps)).0, &mut st) => {}
        Geo::Sphere { .. } | Geo::Torus { .. }
            if !critical_open(&g, &pieces_box(&ps), periods, &mut st, &mut row) => {}
        Geo::Sphere { .. } | Geo::Torus { .. } => {
            let classified = loops_pass(face, &g)
                .then(|| Classifier::new(&ps, periods))
                .flatten();
            match classified {
                Some(cls) => bound_critical(&g, &cls, &mut st, &mut row)?,
                None => {
                    fallback = true;
                    let w = pieces_box(&ps);
                    let (e, n) = g.geo.enclose(&w);
                    st.work += n;
                    for s in 0..SIDES {
                        st.bound(s, e.upper(s));
                    }
                }
            }
        }
        Geo::Nurbs(net) if !could_move(&net.enclose(&pieces_box(&ps)).0, &mut st) => {}
        Geo::Nurbs(net) => {
            let classified = loops_pass(face, &g)
                .then(|| Classifier::new(&ps, periods))
                .flatten();
            let w = pieces_box(&ps);
            match classified {
                Some(cls) => bound_cells(net, &w, Some(&cls), &mut st),
                None => {
                    fallback = true;
                    st.loose = true;
                    // The window's edges bound the fallback region.
                    let w = net.clamp(&w);
                    let corners = [
                        [w[0][0], w[0][1]],
                        [w[1][0], w[0][1]],
                        [w[1][0], w[1][1]],
                        [w[0][0], w[1][1]],
                    ];
                    for k in 0..4 {
                        let edge = line_piece(corners[k], corners[(k + 1) % 4]);
                        bound_piece(&g, &edge, &mut st, &mut row, &mut sc)?;
                    }
                    bound_cells(net, &w, None, &mut st);
                }
            }
        }
    }
    let w = g.excursion_widening(&ps)?;
    for s in 0..SIDES {
        // Rounded outwards: a widening below half an ulp of the side would
        // otherwise be lost. A NaN widening still reaches the sides, which
        // then refuse.
        if w[s % 3] != 0.0 {
            st.sides.inner[s] = (st.sides.inner[s] - w[s % 3]).next_down();
            st.sides.outer[s] = (st.sides.outer[s] + w[s % 3]).next_up();
        }
    }
    if !st
        .sides
        .inner
        .iter()
        .chain(&st.sides.outer)
        .all(|x| !x.is_nan())
    {
        return Err(Refusal::new(RefusalCode::NonFinite, "face box not finite"));
    }
    let excursion = w[0].max(w[1]).max(w[2]);
    Ok(FaceBox {
        sides: st.sides,
        fallback,
        capped: st.capped,
        extrapolated: excursion > 0.0,
        excursion,
        work: st.work,
        gap_bound: guarded.bound,
    })
}

/// Whether the surface over the trims' box (enclosure `e`) could move a side
/// beyond its target; when it cannot, `e` bounds every side and the face's
/// interior needs no classification.
fn could_move(e: &Enc, st: &mut State) -> bool {
    st.work += 1;
    if (0..SIDES).any(|s| e.upper(s) > st.target(s)) {
        return true;
    }
    for s in 0..SIDES {
        st.bound(s, e.upper(s));
    }
    false
}

fn pieces_box(ps: &[Piece]) -> UvBox {
    let mut w = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in ps {
        let h = p.trim.hull();
        for c in 0..2 {
            w[0][c] = w[0][c].min(h[0][c]);
            w[1][c] = w[1][c].max(h[1][c]);
        }
    }
    w
}

fn loops_pass(face: &FaceDefinition, g: &FaceGeo) -> bool {
    let trims: Vec<PreparedTrim> = face.trims.iter().map(PreparedTrim::new).collect();
    loops::check(face, &g.prepared, &trims).is_ok()
}

/// The bracket of one face on its own, each side refined until its width is
/// under `epsilon` (absolute, in millimetres).
///
/// # Errors
///
/// A typed [`Refusal`]: a malformed definition, a trim point outside a net's
/// domain or a value that is not finite.
pub fn face_box(face: &FaceDefinition, epsilon: f64) -> Result<FaceBox, Refusal> {
    if !(epsilon.is_finite() && epsilon > 0.0) {
        return Err(Refusal::new(
            RefusalCode::InvalidDefinition,
            "epsilon must be positive and finite",
        ));
    }
    bracket_face(face, [f64::NEG_INFINITY; SIDES], epsilon, true, None)
}

/// The box of `body` with its bracket.
///
/// # Errors
///
/// A typed [`Refusal`] from a face (attributed to it) or a malformed body.
pub fn body_box(body: &BodyDefinition, options: BoxOptions) -> Result<BodyBox, Refusal> {
    bodies_boxes(std::slice::from_ref(body), options)
        .pop()
        .unwrap_or_else(|| Err(Refusal::new(RefusalCode::InvalidDefinition, "no body")))
}

/// A body's target width; the first pass has already checked it.
fn fb_eps(body: &BodyDefinition, accuracy: f64) -> f64 {
    accuracy * body.size
}

/// The boxes of every body, with every face of every body on one pool.
/// The same boxes, bit for bit, as [`boxes_of`] over [`BoxBody::of`].
#[must_use]
pub fn bodies_boxes(
    bodies: &[BodyDefinition],
    options: BoxOptions,
) -> Vec<Result<BodyBox, Refusal>> {
    let inputs: Vec<Input<'_>> = bodies
        .iter()
        .map(|definition| Input {
            definition,
            joints: None,
            tolerance: None,
        })
        .collect();
    boxes_impl(&inputs, options)
}

/// The boxes of bodies given for the box alone, with every face of every
/// body on one pool. A body without bridges is boxed exactly as
/// [`bodies_boxes`] boxes its definition.
#[must_use]
pub fn boxes_of(bodies: &[BoxBody], options: BoxOptions) -> Vec<Result<BodyBox, Refusal>> {
    let inputs: Vec<Input<'_>> = bodies
        .iter()
        .map(|b| Input {
            definition: &b.definition,
            joints: b.bridged().then_some(b.joints.as_slice()),
            tolerance: b.tolerance,
        })
        .collect();
    boxes_impl(&inputs, options)
}

/// One body to box: its definition, its joint table when it has bridges,
/// and the kernel's tolerance.
struct Input<'a> {
    definition: &'a BodyDefinition,
    joints: Option<&'a [BoxJoints]>,
    tolerance: Option<f64>,
}

fn boxes_impl(inputs: &[Input<'_>], options: BoxOptions) -> Vec<Result<BodyBox, Refusal>> {
    let bodies: Vec<&BodyDefinition> = inputs.iter().map(|i| i.definition).collect();
    let index: Vec<(usize, usize)> = bodies
        .iter()
        .enumerate()
        .flat_map(|(b, body)| (0..body.faces.len()).map(move |f| (b, f)))
        .collect();
    let valid = |b: &BodyDefinition| -> Result<f64, Refusal> {
        b.validate()?;
        let a = options.accuracy;
        if !(a.is_finite() && a > 0.0) {
            return Err(Refusal::new(
                RefusalCode::InvalidDefinition,
                "accuracy must be positive and finite",
            ));
        }
        let target = a * b.size;
        if !(target.is_finite() && target > 0.0) {
            return Err(Refusal::new(
                RefusalCode::InvalidDefinition,
                "accuracy times the body's size must be positive and finite",
            ));
        }
        Ok(target)
    };
    // First pass: every face's pieces and interior enclosed once.
    let first = crate::mass::pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        let eps = match valid(body) {
            Ok(e) => e,
            Err(r) => return Some(Err(r)),
        };
        let joints = inputs.get(b)?.joints.and_then(|j| j.get(f));
        Some(
            bracket_face(face, [f64::NEG_INFINITY; SIDES], eps, false, joints)
                .map_err(|r| r.at_face(f)),
        )
    });
    let mut floors: Vec<[f64; SIDES]> = vec![[f64::NEG_INFINITY; SIDES]; bodies.len()];
    for (&(b, _), r) in index.iter().zip(&first) {
        if let (Some(Some(Ok(fb))), Some(fl)) = (r, floors.get_mut(b)) {
            for k in 0..SIDES {
                fl[k] = fl[k].max(fb.sides.inner[k]);
            }
        }
    }
    // Second pass: the faces whose outer values could move a side, refined
    // against their body's first-pass values.
    let second = crate::mass::pool(index.len(), options.threads, |i| {
        let &(b, f) = index.get(i)?;
        let body = bodies.get(b)?;
        let face = body.faces.get(f)?;
        let floor = *floors.get(b)?;
        match first.get(i)?.as_ref()? {
            Some(Ok(fb)) => {
                let eps = fb_eps(body, options.accuracy);
                if (0..SIDES).all(|s| fb.sides.outer[s] <= floor[s] + eps) {
                    return Some(Ok(*fb));
                }
                let joints = inputs.get(b)?.joints.and_then(|j| j.get(f));
                Some(bracket_face(face, floor, eps, true, joints).map_err(|r| r.at_face(f)))
            }
            Some(Err(r)) => Some(Err(r.clone())),
            None => None,
        }
    });
    let mut results = second.into_iter();
    inputs
        .iter()
        .zip(&floors)
        .map(|(input, floor)| {
            let body = input.definition;
            let faces: Vec<Result<FaceBox, Refusal>> = (0..body.faces.len())
                .map(|f| {
                    results.next().flatten().flatten().unwrap_or_else(|| {
                        Err(Refusal::new(RefusalCode::NonFinite, "face task failed").at_face(f))
                    })
                })
                .collect();
            let epsilon = valid(body)?;
            let faces = faces.into_iter().collect::<Result<Vec<_>, _>>()?;
            let mut sides = Sides::EMPTY;
            sides.inner = *floor;
            let mut out = BodyBox {
                sides,
                bound: 0.0,
                epsilon,
                fallback_faces: Vec::new(),
                capped_faces: Vec::new(),
                extrapolated_faces: Vec::new(),
                work: 0,
                kernel_tolerance: input.tolerance,
                gap_faces: Vec::new(),
                gap_bound: 0.0,
                extrapolation: 0.0,
                accuracy: FactAccuracy {
                    bound: None,
                    method: Method::Bracket,
                    scale: Scale::Positional,
                    file_tolerance: body.file_tolerance,
                    gap_bound: body.file_tolerance,
                    closed: body.closed,
                },
            };
            for (f, fb) in faces.iter().enumerate() {
                for s in 0..SIDES {
                    out.sides.inner[s] = out.sides.inner[s].max(fb.sides.inner[s]);
                    out.sides.outer[s] = out.sides.outer[s].max(fb.sides.outer[s]);
                }
                if fb.fallback {
                    out.fallback_faces.push(f);
                }
                if fb.capped {
                    out.capped_faces.push(f);
                }
                if fb.extrapolated {
                    out.extrapolated_faces.push(f);
                }
                out.work += fb.work;
                out.gap_bound = out.gap_bound.max(fb.gap_bound);
                out.extrapolation = out.extrapolation.max(fb.excursion);
            }
            if let Some(joints) = input.joints {
                out.gap_faces = (0..joints.len())
                    .filter(|&f| joints.get(f).is_some_and(|j| !j.bridges.is_empty()))
                    .collect();
                out.accuracy.gap_bound = Some(out.gap_bound);
            }
            out.bound = out.sides.width();
            out.accuracy.bound = Some(out.bound);
            if !(out.bound.is_finite()
                && out
                    .sides
                    .inner
                    .iter()
                    .chain(&out.sides.outer)
                    .all(|x| x.is_finite()))
            {
                return Err(Refusal::new(RefusalCode::NonFinite, "body box not finite"));
            }
            Ok(out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn line(origin: [f64; 2], direction: [f64; 2], range: [f64; 2]) -> PreparedTrim {
        PreparedTrim::new(&TrimPiece {
            curve: Curve2::Line { origin, direction },
            range,
            reversed: false,
        })
    }

    /// The guard of a line trim against `v`.
    fn guard(g: &FaceGeo, t: &PreparedTrim, v: [f64; 3]) -> [f64; 3] {
        let [a, b] = t.range;
        let mut row = Row::default();
        g.guard_segment(t.point(a), t.point(b), t.hull_rounding(a, b), v, &mut row)
            .unwrap()
            .unwrap()
    }

    /// Per axis the largest `|S(l(t)) - v|` over `n + 1` points of the line.
    fn sampled(g: &FaceGeo, t: &PreparedTrim, v: [f64; 3], n: usize) -> [f64; 3] {
        let [a, b] = t.range;
        let mut row = Row::default();
        let mut out = [0.0_f64; 3];
        for i in 0..=n {
            let s = a + (b - a) * (i as f64) / (n as f64);
            let (p, _) = g.point(t.point(s), &mut row).unwrap();
            for c in 0..3 {
                out[c] = out[c].max((p[c] - v[c]).abs());
            }
        }
        out
    }

    fn sphere(r: f64) -> FaceGeo {
        FaceGeo::new(&Surface::Sphere {
            frame: Frame {
                origin: Vec3::new(0.25, -1.0, 2.0),
                x: Vec3::new(1.0, 0.0, 0.0),
                y: Vec3::new(0.0, 1.0, 0.0),
                z: Vec3::new(0.0, 0.0, 1.0),
            },
            radius: r,
        })
    }

    #[test]
    fn the_guard_does_not_depend_on_the_parameterisation() {
        // M1: a bridge 1e-4 long on a sphere of radius 0.5, stored over
        // [0, |d|] with a unit direction and over [0, 1] with direction d,
        // has one bound up to rounding, and it holds dense samples.
        let g = sphere(0.5);
        let p = [0.3, 0.2];
        let unit = [0.6, 0.8];
        for len in [1e-4, 0.5] {
            let a = line(p, unit, [0.0, len]);
            let b = line(p, [unit[0] * len, unit[1] * len], [0.0, 1.0]);
            let mut row = Row::default();
            let (s, _) = g.point(p, &mut row).unwrap();
            let v = [s[0] + 0.3 * len, s[1] - 0.1 * len, s[2]];
            let (ga, gb) = (guard(&g, &a, v), guard(&g, &b, v));
            let truth = sampled(&g, &a, v, 10_000);
            for c in 0..3 {
                assert!(
                    (ga[c] - gb[c]).abs() <= 1e-9 * ga[c].max(1e-300),
                    "len {len} axis {c}: {} vs {}",
                    ga[c],
                    gb[c]
                );
                assert!(
                    truth[c] <= ga[c],
                    "len {len} axis {c}: {} > {}",
                    truth[c],
                    ga[c]
                );
            }
        }
    }

    /// A rational net of revolution about z with double knots at its v
    /// quarters: C0 knot lines at v = 1, 2, 3.
    fn quartered() -> FaceGeo {
        let unit = [
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [-1.0, 1.0],
            [-1.0, 0.0],
            [-1.0, -1.0],
            [0.0, -1.0],
            [1.0, -1.0],
            [1.0, 0.0],
        ];
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let mut poles = Vec::new();
        let mut weights = Vec::new();
        for (scale, z) in [(1.0, 0.0), (1.6, 0.5), (2.0, 2.0)] {
            for (k, p) in unit.iter().enumerate() {
                poles.push(Vec3::new(2.0 * scale * p[0], 2.0 * scale * p[1], z));
                weights.push(if k % 2 == 1 { w } else { 1.0 });
            }
        }
        FaceGeo::new(&Surface::Nurbs(NurbsSurface {
            degree_u: 2,
            degree_v: 2,
            knots_u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            knots_v: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            poles,
            weights: Some(weights),
            count_u: 3,
            count_v: 9,
        }))
    }

    #[test]
    fn a_segment_across_a_c0_knot_line_is_split_there() {
        // Q1: the segment crosses v = 1, where the net's first derivatives
        // jump: no second-order bound holds over it, but one does on each
        // side, and the split bound (with the cut's rounding) holds dense
        // samples on both sides and each side's own bound.
        let g = quartered();
        let t = line([0.2, 0.7], [0.6, 0.6], [0.0, 1.0]);
        assert!(g.derivatives(t.hull()).is_none());
        let mut row = Row::default();
        let (s, _) = g.point([0.5, 1.0], &mut row).unwrap();
        let split = guard(&g, &t, s);
        let truth = sampled(&g, &t, s, 10_000);
        let lo = line([0.2, 0.7], [0.3, 0.3], [0.0, 1.0]);
        let hi = line([0.5, 1.0], [0.3, 0.3], [0.0, 1.0]);
        let (glo, ghi) = (guard(&g, &lo, s), guard(&g, &hi, s));
        for c in 0..3 {
            assert!(
                truth[c] <= split[c],
                "axis {c}: {} > {}",
                truth[c],
                split[c]
            );
            assert!(glo[c].max(ghi[c]) <= split[c], "axis {c}");
        }
    }

    #[test]
    fn a_sphere_segment_past_its_pole_is_bounded() {
        // A segment that leaves a sphere's latitudes by 1e-7 (within the
        // domain tolerance): the clamped image and the analytic continuation
        // both lie within the bound.
        let g = sphere(2.0);
        let v0 = FRAC_PI_2 + 1e-7;
        let t = line([0.1, v0], [0.2, 0.0], [0.0, 1.0]);
        let mut row = Row::default();
        let (s, _) = g.point([0.2, FRAC_PI_2], &mut row).unwrap();
        let bound = guard(&g, &t, s);
        let truth = sampled(&g, &t, s, 1_000);
        for c in 0..3 {
            assert!(truth[c] <= bound[c], "axis {c}");
        }
        // The analytic continuation at the start lies 2 * 1e-7 from the
        // pole, in x and y.
        let off = 2.0 * libm::sin(1e-7);
        assert!(
            off * libm::cos(0.1) <= bound[0] && off * libm::sin(0.1) <= bound[1],
            "{bound:?}"
        );
    }
}
