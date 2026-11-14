//! GeoSpec's kernel-agnostic facts layer (program item S8).
//!
//! A kernel adapter turns each body of a B-rep into a [`BodyDefinition`]: per
//! face a [`FaceDefinition`] (the surface's kind and parameters or its NURBS
//! net, the oriented 2D trim pieces with their parameter ranges, the face's
//! orientation and which of its sides bound material), plus the body centre the
//! moments are taken about and the body's size. Adapters implement
//! [`FaceDefinitions`]; this crate never sees a kernel.
//!
//! [`body_facts`] integrates area, volume and centre of mass with the adaptive
//! Gauss-Kronrod 7-15 scheme of the solidcore evaluation's patches 0001, 0012
//! and 0014: Green's theorem reduces each face to a boundary integral of iso-u
//! integrals in v, split at trim knots, at crossings of the surface's u knot
//! lines and at its v knots; planes, cylinders and cones integrate their inner
//! (v) integrals in closed form. Every fact carries a [`FactAccuracy`] in the
//! shape of the engine's accuracy record (`bound`, method, scale, file
//! tolerance): for adaptive facts the bound is an estimate (see
//! [`Method::Adaptive`]): the panels' error estimates (each piece bisected at
//! least once), the inner errors carried through the outer quadrature,
//! round-off from the integrals of `|f|`, closing segments for trim gaps, the
//! fold's round-off and the closure terms' effect on the volume and moments.
//! Trim loops must chain, close and be oriented by role. A body bounding
//! material is checked for closure: its closure integrals should vanish and its
//! three volume forms agree; a residual beyond the integration bounds widens
//! the volume bound by the forms' disagreement and the closure terms, and the
//! body refuses when that widening exceeds four times the gap volume (or,
//! without a file tolerance, 1e-4 of the volume), when it is known open, or
//! when its closure is unknown. A face or body the integrator cannot decide
//! refuses with a typed [`Refusal`]; nothing falls back to another integrator.
//!
//! Determinism: faces are independent tasks on any number of threads and the
//! results fold in face order, so every fact is bit-identical for every thread
//! count. The arithmetic is scalar with separate multiply and add (no fused
//! multiply-add: `mul_add` is a disallowed method) and the transcendental
//! functions come from `libm`, so native and WASM builds compute the same bits.

mod accuracy;
mod bbox;
mod definition;
mod gk;
mod loops;
mod mass;
mod nurbs;
mod refusal;
mod surface;
mod trim;
mod vec;

pub use accuracy::{FactAccuracy, Method, Scale};
pub use bbox::{
    bodies_boxes, body_box, boxes_of, boxes_within_target, face_box, BodyBox, BoxBody, BoxJoints,
    BoxOptions, BoxStop, FaceBox, Joint, Sides, SIDES,
};
pub use definition::{
    BodyDefinition, Curve2, FaceDefinition, FaceDefinitions, Frame, Material, NurbsCurve2,
    NurbsSurface, Orientation, Surface, TrimPiece,
};
pub use loops::LoopOrientation;
pub use mass::{
    bodies_facts, body_facts, face_mass, BodyFacts, FaceMass, Fact, Options, PositionFact,
};
pub use refusal::{Refusal, RefusalCode};
pub use vec::Vec3;
