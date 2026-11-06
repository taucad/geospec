//! The facts layer's solidcore adapter: each face of a solidcore B-rep as a
//! kernel-agnostic [`FaceDefinition`].
//!
//! Per face, as the solidcore evaluation's patch 0001 prepares its tasks: the
//! surface (`topology::face::p2::surface`) exported as a plane or as the NURBS
//! net solidcore itself evaluates (analytic kinds through their NURB), the
//! oriented UV trims of `create_oriented_trim_boundary` (each edge use's
//! `uv_trim_curve`, gap lines included) exported as B-spline curves over their
//! natural intervals, and the volume signs of the face uses on non-void sides.
//! The body centre is the centre of solidcore's tight box, as patch 0001
//! integrates about.
//!
//! Every export is checked against solidcore's own evaluation: each trim at
//! seven parameters, the surface at the trims' points and between them. A
//! mismatch refuses ([`RefusalCode::ExportMismatch`]); a kind with no export
//! refuses ([`RefusalCode::UnsupportedSurface`], [`RefusalCode::UnsupportedTrim`]).
//! The loops are solidcore's; the facts crate checks their chaining, closure
//! and per-loop orientation ([`FaceDefinition::check_loops`]). When solidcore's
//! loops fail that check and their reversal passes, every loop is reversed so
//! that the face lies on their left, with the volume signs flipped to match;
//! when neither passes the face refuses.

use std::borrow::Cow;

use geospec_facts::{
    BodyDefinition, Curve2, FaceDefinition, FaceDefinitions, Material, NurbsCurve2, NurbsSurface,
    Orientation, Refusal, RefusalCode, Surface, TrimPiece, Vec3,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::nurbs::b_spline_curve::p1::BSplineCurve;
use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surface::{Surface as ScSurface, SurfaceKind};
use solidcore::topology::{FaceId, FaceuseId, RegionId};
use solidcore::{Brep, EntityRef, Kernel, Point2};

/// Relative tolerance of the export check (positions in the parameter plane
/// and in space, against `1 +` the magnitude).
const EXPORT_TOLERANCE: f64 = 1e-9;

fn refuse(code: RefusalCode, detail: impl Into<String>) -> Refusal {
    Refusal::new(code, detail)
}

fn kernel_error(e: impl std::fmt::Display) -> Refusal {
    refuse(RefusalCode::InvalidDefinition, format!("solidcore: {e}"))
}

fn v3(p: solidcore::Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

/// The NURB a B-spline-based surface evaluates on.
fn nurbs_surface(b: &BSplineSurface) -> Result<Surface, Refusal> {
    let s = b.gw_nurb().ok_or_else(|| {
        refuse(
            RefusalCode::UnsupportedSurface,
            "B-spline surface without NURB",
        )
    })?;
    let (count_u, count_v) = (s.n() + 1, s.m() + 1);
    let mut poles = Vec::with_capacity(count_u * count_v);
    let mut weights = Vec::with_capacity(count_u * count_v);
    let mut rational = false;
    for row in s.rows().take(count_u) {
        let row = row.get(..count_v).ok_or_else(|| {
            refuse(
                RefusalCode::InvalidDefinition,
                "net row shorter than its count",
            )
        })?;
        for c in row {
            let (p, w) = homogeneous3(c);
            rational |= w != 1.0;
            poles.push(p);
            weights.push(w);
        }
    }
    Ok(Surface::Nurbs(NurbsSurface {
        degree_u: s.p(),
        degree_v: s.q(),
        knots_u: s.u_knots().to_vec(),
        knots_v: s.v_knots().to_vec(),
        poles,
        weights: rational.then_some(weights),
        count_u,
        count_v,
    }))
}

/// A control point's Euclidean position and weight.
fn homogeneous3(c: &CPoint) -> (Vec3, f64) {
    if c.w == NOW {
        (Vec3::new(c.x, c.y, c.z), 1.0)
    } else {
        (Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w), c.w)
    }
}

/// The surface as a definition.
fn export_surface(k: &Kernel, s: &ScSurface) -> Result<Surface, Refusal> {
    let unsupported = |what: &str| refuse(RefusalCode::UnsupportedSurface, what.to_string());
    match s.kind() {
        SurfaceKind::Plane(p) => {
            let pos = p.position();
            let scale = p.uv_scale();
            Ok(Surface::Plane {
                origin: v3(pos.origin()),
                u_axis: v3(pos.x_axis() * scale.x),
                v_axis: v3(pos.y_axis() * scale.y),
            })
        }
        SurfaceKind::BSpline(b) => nurbs_surface(b),
        SurfaceKind::BallPatch(b) => nurbs_surface(b.net()),
        SurfaceKind::Cone(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::SurfOfExtrusion(_) => match s.kind_with_nurb(k).map_err(kernel_error)? {
            SurfaceKind::Cone(c) => nurbs_surface(c.rev().base()),
            SurfaceKind::Sphere(c) => nurbs_surface(c.base().base()),
            SurfaceKind::Torus(c) => nurbs_surface(c.base().base()),
            SurfaceKind::SurfOfRevolution(c) => nurbs_surface(c.base()),
            SurfaceKind::SurfOfExtrusion(c) => nurbs_surface(c.base()),
            _ => Err(unsupported("analytic surface without NURB")),
        },
        SurfaceKind::CurveBounded(_) => Err(unsupported("curve-bounded surface")),
        SurfaceKind::Step(_) => Err(unsupported("STEP-parameterised surface")),
        SurfaceKind::OffsetSurface(_) => Err(unsupported("offset surface")),
        SurfaceKind::SrfInVolume(_) => Err(unsupported("surface in volume")),
        SurfaceKind::ProceduralBlend(_) => Err(unsupported("procedural blend")),
    }
}

/// The B-spline part of a trim curve.
fn trim_base(c: &Curve) -> Result<Cow<'_, BSplineCurve>, Refusal> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb().map_err(kernel_error)?,
        CurveKind::Ellipse(e) => Cow::Borrowed(e.base()),
        CurveKind::Hyperbola(h) => Cow::Borrowed(h.base()),
        CurveKind::Parabola(p) => Cow::Borrowed(p.base()),
        _ => {
            return Err(refuse(
                RefusalCode::UnsupportedTrim,
                "trim curve without a B-spline form",
            ))
        }
    })
}

/// The trim as a definition over its natural interval.
fn export_trim(c: &Curve) -> Result<TrimPiece, Refusal> {
    let b = trim_base(c)?;
    let degree = b.degree().map_err(kernel_error)?;
    let cps = b.control_points().map_err(kernel_error)?;
    let (distinct, mults) = b.knots(None).map_err(kernel_error)?;
    let mut knots = Vec::new();
    for (k, m) in distinct.iter().zip(&mults) {
        knots.extend(std::iter::repeat_n(*k, *m));
    }
    let mut poles = Vec::with_capacity(cps.len());
    let mut weights = Vec::with_capacity(cps.len());
    let mut rational = false;
    for c in cps {
        let w = if c.w == NOW { 1.0 } else { c.w };
        rational |= w != 1.0;
        poles.push([c.x / w, c.y / w]);
        weights.push(w);
    }
    let ivl = c.natural_interval();
    Ok(TrimPiece {
        curve: Curve2::Nurbs(NurbsCurve2 {
            degree,
            knots,
            poles,
            weights: rational.then_some(weights),
        }),
        range: [ivl.min(), ivl.max()],
        reversed: false,
    })
}

fn mismatch(what: &str, kernel: [f64; 3], export: [f64; 3]) -> Refusal {
    refuse(
        RefusalCode::ExportMismatch,
        format!("{what}: solidcore {kernel:?}, export {export:?}"),
    )
}

fn close(a: [f64; 3], b: [f64; 3], scale: f64) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= EXPORT_TOLERANCE * (1.0 + scale))
}

/// Checks a trim's export at seven parameters (points and derivatives) and
/// returns its points there.
fn check_trim(k: &Kernel, c: &Curve, t: &TrimPiece) -> Result<Vec<[f64; 2]>, Refusal> {
    let [a, b] = t.range;
    let mut points = Vec::with_capacity(7);
    for i in 0..7 {
        let s = a + (b - a) * f64::from(i) / 6.0;
        let kv = c.evaluate(k, s, 1, true, true).map_err(kernel_error)?;
        let (Some(p), Some(d)) = (kv.first(), kv.get(1)) else {
            return Err(kernel_error("trim evaluation"));
        };
        let (ep, ed) = t.curve.evaluate(s)?;
        let scale = p.x.abs().max(p.y.abs());
        if !close([p.x, p.y, 0.0], [ep[0], ep[1], 0.0], scale) {
            return Err(mismatch("trim point", [p.x, p.y, 0.0], [ep[0], ep[1], 0.0]));
        }
        let dscale = d.x.abs().max(d.y.abs());
        if !close([d.x, d.y, 0.0], [ed[0], ed[1], 0.0], 1e3 * dscale) {
            return Err(mismatch(
                "trim derivative",
                [d.x, d.y, 0.0],
                [ed[0], ed[1], 0.0],
            ));
        }
        points.push(ep);
    }
    Ok(points)
}

/// Checks the surface's export at `uvs` against solidcore's first-derivative
/// evaluation (the one patch 0001's integrand calls): the point, `S_u` and
/// `S_v`.
fn check_surface(
    k: &Kernel,
    s: &ScSurface,
    export: &Surface,
    uvs: &[[f64; 2]],
    size: f64,
) -> Result<(), Refusal> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: true,
    };
    for uv in uvs {
        let mut d = [[solidcore::Vec3::new(0.0, 0.0, 0.0); 2]; 2];
        s.evaluate_simple_first_into(k, Point2::new(uv[0], uv[1]), flags, &mut d)
            .map_err(kernel_error)?;
        let p = d[0][0];
        let (e, eu, ev) = export.evaluate(uv[0], uv[1])?;
        let scale = size.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
        if !close([p.x, p.y, p.z], [e.x, e.y, e.z], scale) {
            return Err(mismatch(
                &format!("surface point at {uv:?}"),
                [p.x, p.y, p.z],
                [e.x, e.y, e.z],
            ));
        }
        for (what, got, want) in [("S_u", d[1][0], eu), ("S_v", d[0][1], ev)] {
            let dscale = got.x.abs().max(got.y.abs()).max(got.z.abs());
            if !close(
                [got.x, got.y, got.z],
                [want.x, want.y, want.z],
                1e3 * dscale,
            ) {
                return Err(mismatch(
                    &format!("surface {what} at {uv:?}"),
                    [got.x, got.y, got.z],
                    [want.x, want.y, want.z],
                ));
            }
        }
    }
    Ok(())
}

/// `trims` with every loop reversed: its pieces in the opposite order, each
/// traversed the other way.
fn reversed_loops(trims: &[TrimPiece], loops: &[usize]) -> Vec<TrimPiece> {
    let mut out = Vec::with_capacity(trims.len());
    let mut start = 0;
    for &n in loops {
        let piece = trims.get(start..start + n).unwrap_or(&[]);
        start += n;
        out.extend(piece.iter().rev().map(|t| TrimPiece {
            reversed: !t.reversed,
            ..t.clone()
        }));
    }
    out
}

fn region_of(b: &Brep, fu: FaceuseId) -> Result<RegionId, Refusal> {
    let shell = b
        .faceuse(fu)
        .map_err(kernel_error)?
        .shell
        .ok_or_else(|| kernel_error("face use without shell"))?;
    b.shell(shell)
        .map_err(kernel_error)?
        .region
        .ok_or_else(|| kernel_error("shell without region"))
}

/// The face's volume signs as patch 0001 counts them: one per face use on a
/// non-void side of a separating face, `-1` where the use's orientation is
/// `Same`.
fn volume_signs(b: &Brep, f: FaceId) -> Result<Vec<f64>, Refusal> {
    let up = b
        .face(f)
        .map_err(kernel_error)?
        .faceuse
        .ok_or_else(|| kernel_error("face without face use"))?;
    let mate = b
        .faceuse(up)
        .map_err(kernel_error)?
        .mate
        .ok_or_else(|| kernel_error("face use without mate"))?;
    let mut signs = Vec::new();
    if region_of(b, up)? != region_of(b, mate)? {
        for fu in [up, mate] {
            let region = region_of(b, fu)?;
            if b.region(region).map_err(kernel_error)?.is_void == 0 {
                let orientation = b.faceuse(fu).map_err(kernel_error)?.orientation;
                signs.push(if orientation == OrientType::Same {
                    -1.0
                } else {
                    1.0
                });
            }
        }
    }
    Ok(signs)
}

/// Face `f` of `b` as a definition.
fn face_definition(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    size: f64,
) -> Result<FaceDefinition, Refusal> {
    let signs = volume_signs(b, f)?;
    let key_of = |e: EntityRef| solidcore::geom::object::ObjectRef {
        type_code: 0,
        key: solidcore::geom::attribute::ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    let boundary = solidcore::topology::face::p1::create_oriented_trim_boundary(
        k, b, f, true, true, None, &key_of,
    )
    .map_err(|e| kernel_error(format!("trims: {e}")))?;
    let (curves, loops) = (boundary.curves, boundary.loops);
    let surface = solidcore::topology::face::p2::surface(b, f)
        .map_err(kernel_error)?
        .ok_or_else(|| kernel_error("face without surface"))?;
    let export = export_surface(k, surface)?;
    export.validate()?;
    let mut trims = Vec::with_capacity(curves.len());
    let mut uvs = Vec::new();
    for c in &curves {
        let t = export_trim(c)?;
        t.validate()?;
        let points = check_trim(k, c, &t)?;
        for pair in points.windows(2) {
            if let [p, q] = pair {
                uvs.push(*p);
                uvs.push([0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])]);
            }
        }
        trims.push(t);
    }
    check_surface(k, surface, &export, &uvs, size)?;
    let mut face = FaceDefinition {
        surface: export,
        trims,
        loops,
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    // solidcore's loops as they are, else all reversed; neither refuses with
    // the first's reason. Loops that fit either way (every closed loop
    // degenerate) keep solidcore's orientation; the face's signed area is then
    // checked against its bound by the integrator.
    let flip = match face.check_loops() {
        Ok(_) => false,
        Err(r) if r.code == RefusalCode::TrimOrientation => {
            let flipped = FaceDefinition {
                trims: reversed_loops(&face.trims, &face.loops),
                ..face.clone()
            };
            if flipped.check_loops().is_err() {
                return Err(r);
            }
            face = flipped;
            true
        }
        Err(r) => return Err(r),
    };
    let flip_sign = if flip { -1.0 } else { 1.0 };
    let signs: Vec<f64> = signs.iter().map(|s| s * flip_sign).collect();
    let (orientation, material) = match signs.as_slice() {
        [] => (Orientation::Forward, Material::Neither),
        [s] => (orientation_of(*s), Material::OneSide),
        [s, t] if s * t < 0.0 => (orientation_of(*s), Material::BothSides),
        [_, _] => {
            return Err(refuse(
                RefusalCode::InvalidDefinition,
                "both face uses bound material with the same sign",
            ))
        }
        _ => return Err(kernel_error("more than two face uses")),
    };
    face.orientation = orientation;
    face.material = material;
    Ok(face)
}

fn orientation_of(sign: f64) -> Orientation {
    if sign > 0.0 {
        Orientation::Forward
    } else {
        Orientation::Reversed
    }
}

/// `b` as a body definition: its faces in `topology::brep::p1::faces` order,
/// about the centre of its tight box (the loose box where the tight one
/// fails, as patch 0001).
///
/// # Errors
///
/// A typed [`Refusal`], attributed to the face that caused it.
pub fn body_definition(k: &Kernel, b: &mut Brep) -> Result<BodyDefinition, Refusal> {
    let bounds = solidcore::query::brep::calculate_tight_bounding_box_threads(k, b, 1)
        .or_else(|_| solidcore::query::api_queries::brep_bounding_box(k, b, false))
        .map_err(kernel_error)?;
    if !bounds.is_bounded() {
        return Err(refuse(RefusalCode::InvalidDefinition, "unbounded B-rep"));
    }
    let (mid, s) = (bounds.mid(), bounds.size());
    let size = s.x.max(s.y).max(s.z).max(f64::MIN_POSITIVE);
    let half = Vec3::new(s.x / 2.0, s.y / 2.0, s.z / 2.0);
    let centre = v3(mid);
    let faces = solidcore::topology::brep::p1::faces(b, None).map_err(kernel_error)?;
    let mut definitions = Vec::with_capacity(faces.len());
    for (i, f) in faces.into_iter().enumerate() {
        definitions.push(face_definition(k, b, f, size).map_err(|r| r.at_face(i))?);
    }
    let closed = solidcore::query::api_queries::brep_is_manifold_solid(b).ok();
    Ok(BodyDefinition {
        centre,
        size,
        bounds: Some([centre - half, centre + half]),
        faces: definitions,
        file_tolerance: None,
        closed,
    })
}

/// Bodies of one subject, each with the kernel that built it.
pub struct SolidcoreBodies<'a> {
    kernel: &'a Kernel,
    breps: Vec<&'a mut Brep>,
}

impl<'a> SolidcoreBodies<'a> {
    /// The adapter over `breps`, evaluated with `kernel`.
    pub fn new(kernel: &'a Kernel, breps: Vec<&'a mut Brep>) -> Self {
        Self { kernel, breps }
    }
}

impl FaceDefinitions for SolidcoreBodies<'_> {
    fn body_count(&self) -> usize {
        self.breps.len()
    }

    fn body(&mut self, index: usize) -> Result<BodyDefinition, Refusal> {
        let b = self
            .breps
            .get_mut(index)
            .ok_or_else(|| refuse(RefusalCode::InvalidDefinition, "no such body"))?;
        body_definition(self.kernel, b)
    }
}
