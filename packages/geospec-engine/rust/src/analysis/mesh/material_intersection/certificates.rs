//! One-sided certificates. A miss preserves the exact signed-volume fallback.
//!
//! Both regular-material qualifications run before these predicates. Neither
//! a classification refusal nor a separating-axis miss is an overlap verdict.
use super::super::Primitive;
use super::*;

/// Classify a separately labelled union of the two qualified operands. Every
/// spatial material cluster must belong entirely to one operand to certify
/// zero. The classifier joins boundary contact and solid containment, while
/// correctly keeping an object in a cavity separate. No positive scalar is
/// inferred from cluster membership.
///
/// This requires the caller's combined 128-bit span check, strictly inside the
/// staged classifier's 256-bit arithmetic domain. Classification uses its
/// existing predicates, refusal checks and scratch accounting at tolerance 0.
/// All buffers are call-local; an incomplete certificate is never cached.
pub(super) fn zero_material(
    left: &MeshAnalysisRecord,
    right: &MeshAnalysisRecord,
    work: &Work<'_>,
    available: u64,
) -> Result<bool> {
    // The combined record labels each whole operand as one primitive, which
    // would collapse an operand's own primitive partition: a multi-primitive
    // operand's inward shell would read as a same-primitive cavity instead of
    // the occupied union that independently closed authored operands form, and
    // a genuine positive overlap could then look like a pure cluster. A
    // validated record with one primitive has one label on every triangle, so
    // that is the only partition this classification preserves. Anything else
    // declines before any charge or allocation.
    if left.primitives.len() != 1 || right.primitives.len() != 1 {
        return Ok(false);
    }
    let positions = left.positions.len().saturating_add(right.positions.len());
    let triangles = left.triangles.len().saturating_add(right.triangles.len());
    if positions > u32::MAX as usize {
        return Ok(false);
    }
    // Exact-capacity vectors, two short labels and their headers. No cloned
    // original primitive names or reallocating copies live beside this record.
    let reserve = (positions as u64)
        .saturating_mul(size_of::<[f64; 3]>() as u64)
        .saturating_add((triangles as u64).saturating_mul(16))
        .saturating_add(1024);
    if reserve > available {
        return Ok(false);
    }
    let spare = available - reserve;
    // The optional trace already holds more than the certificate would leave
    // it. Lowering the ceiling here would discard a trace the scalar path
    // would have kept complete, which would stop the overlap being cached, so
    // the certificate declines instead of spending another call's caching.
    let ceiling = work.1.borrow().limit_bytes();
    if work.1.borrow().retained_bytes() > spare {
        return Ok(false);
    }
    let certified = classify_combined(left, right, work, spare);
    // The certificate's buffers are released either way, so the rest of the
    // call keeps the ceiling the scalar path would have had.
    work.1.borrow_mut().restore_limit_to_bytes(ceiling);
    certified
}

/// Classify the two operands as one separately labelled record.
fn classify_combined(
    left: &MeshAnalysisRecord,
    right: &MeshAnalysisRecord,
    work: &Work<'_>,
    spare: u64,
) -> Result<bool> {
    let positions = left.positions.len().saturating_add(right.positions.len());
    let triangles = left.triangles.len().saturating_add(right.triangles.len());
    work.1.borrow_mut().limit_to_bytes(spare);
    work.charge((positions as u64).saturating_add(triangles as u64))?;
    let offset = left.positions.len() as u32;
    let split = left.triangles.len();
    let mut combined = MeshAnalysisRecord {
        positions: Vec::with_capacity(positions),
        triangles: Vec::with_capacity(triangles),
        triangle_primitives: Vec::with_capacity(triangles),
        primitives: vec![
            Primitive {
                name: "left".into(),
                vertex_start: 0,
                vertex_count: offset,
            },
            Primitive {
                name: "right".into(),
                vertex_start: offset,
                vertex_count: right.positions.len() as u32,
            },
        ],
    };
    combined.positions.extend_from_slice(&left.positions);
    combined.positions.extend_from_slice(&right.positions);
    combined.triangles.extend_from_slice(&left.triangles);
    combined
        .triangles
        .extend(right.triangles.iter().map(|t| t.map(|v| v + offset)));
    combined
        .triangle_primitives
        .extend(std::iter::repeat_n(0, split));
    combined
        .triangle_primitives
        .extend(std::iter::repeat_n(1, right.triangles.len()));
    match material::clusters_traced(&combined, 0.0, work.0, spare, &mut work.1.borrow_mut()) {
        Ok(groups) => Ok(groups
            .iter()
            .all(|g| g.iter().all(|&i| i < split) || g.iter().all(|&i| i >= split))),
        Err(error) if matches!(error.kind, BackendErrorKind::BudgetExceeded { .. }) => Err(error),
        // A certificate is optional. Ambiguous combined topology or insufficient
        // certificate scratch falls back; neither can become a successful claim.
        Err(_) => Ok(false),
    }
}

pub(super) fn coordinates(record: &MeshAnalysisRecord, triangle: usize) -> [[f64; 3]; 4] {
    let t = record.triangles[triangle];
    [
        record.positions[0],
        record.positions[t[0] as usize],
        record.positions[t[1] as usize],
        record.positions[t[2] as usize],
    ]
}

/// A face plane with the other tetrahedron on the opposite side certifies
/// empty interior. Signs are exact degree-three dyadic integer polynomials;
/// no floating epsilon or positive-overlap approximation is introduced.
///
/// Only empty, point and edge intersections are filtered (at most two other
/// vertices on the separating plane). Coplanar faces retain the scalar path.
///
/// This also bounds the scalar arithmetic the filter skips, so the filter
/// cannot stand in for a cell whose exact arithmetic would have refused. With
/// grid integers of at most S=128 bits including the binary point, normalized
/// plane normals have height <=2S+3 and offsets <=3S+5. A Cramer vertex has a
/// common denominator and numerator of height <=7S+16; intermediate
/// determinant/vertex division and inside tests, including preflight products,
/// stay below 32S+128=4224 bits, well inside the 8192-bit arithmetic guard.
///
/// The scalar path returns zero before the centroid, face-sorting and volume
/// stages, whose heights that bound does not cover, because it admits at most
/// two vertices here. The filtered cell lies entirely in the separating plane:
/// one tetrahedron is in the closed halfspace on one side and the other is in
/// the open halfspace on the far side, so their intersection is the separating
/// face triangle met with the hull of the at most two far vertices on the
/// plane, which is empty, a point or a segment. A vertex the scalar path
/// admits is the unique common point of three constraint planes whose normals
/// are independent. Such a point cannot sit in the relative interior of a
/// segment intersection: a constraint plane through an interior point that did
/// not contain the whole segment would exclude part of it, so all three planes
/// would contain the segment, share its line and have coplanar normals, which
/// the solver rejects as singular. So only the segment's two endpoints can be
/// admitted, fewer than the four the centroid stage needs.
///
/// Ineligible spans and every uncertain cell execute the guarded scalar path
/// unchanged.
pub(super) fn separated(a: &[[f64; 3]; 4], b: &[[f64; 3]; 4], work: &Work<'_>) -> Result<bool> {
    for (own, other) in [(a, b), (b, a)] {
        for opposite in 0..4 {
            let mut face = [[0.; 3]; 3];
            let mut at = 0;
            for (i, p) in own.iter().enumerate() {
                if i != opposite {
                    face[at] = *p;
                    at += 1;
                }
            }
            work.charge(23)?;
            let side = material::scalar_face_sign(face, &own[opposite]);
            if side == Ordering::Equal {
                continue;
            }
            let mut apart = true;
            let mut on_plane = 0;
            for p in other {
                work.charge(23)?;
                let sign = material::scalar_face_sign(face, p);
                if sign == side {
                    apart = false;
                    break;
                }
                on_plane += usize::from(sign == Ordering::Equal);
            }
            if apart && on_plane <= 2 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
