//! Material connectivity of the retained dyadic triangle representation.
//! No spatial weld or envelope overlap is verdict-bearing here.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

use num_rational::BigRational as Q;
use num_traits::{Signed, Zero};

use crate::{
    analysis::continuous::{exact as arithmetic, ContinuousError, ContinuousErrorKind},
    backend::{BackendError, BackendErrorKind, CapacityLimit, LimitUnit},
    budget::Budget,
};

use super::exact::{ChargeStage, ChargeStep, ChargeTrace};
use super::{empty_aabb, expand, find, Aabb, BoxTree, MeshAnalysisRecord};
use scratch::EdgeUse;

mod integer;
mod scratch;
mod staged;

pub(super) type Point = [Q; 3];
pub(super) type Triangle = [Point; 3];
type Result<T> = std::result::Result<T, BackendError>;

pub(super) fn invalid(message: &str) -> BackendError {
    BackendError {
        kind: BackendErrorKind::InvalidInput,
        message: message.into(),
    }
}

pub(super) fn unresolved(message: &str) -> BackendError {
    BackendError {
        kind: BackendErrorKind::Unsupported,
        message: message.into(),
    }
}

/// Exact scratch for `record` that needs `required` of `byte_limit` bytes.
fn scratch_limit(
    subject: &str,
    record: &MeshAnalysisRecord,
    required: u64,
    byte_limit: u64,
) -> BackendError {
    BackendError::limit(
        subject,
        CapacityLimit::analysis(LimitUnit::Bytes, required, byte_limit)
            .with_mesh(record.positions.len() as u64, record.triangles.len() as u64),
    )
}

pub(super) fn exact(error: ContinuousError) -> BackendError {
    BackendError {
        kind: if error.kind == ContinuousErrorKind::InvalidInput {
            BackendErrorKind::InvalidInput
        } else {
            BackendErrorKind::Unsupported
        },
        message: error.message,
    }
}

pub(super) fn add(a: &Q, b: &Q) -> Result<Q> {
    arithmetic::add(a, b).map_err(exact)
}
pub(super) fn sub(a: &Q, b: &Q) -> Result<Q> {
    arithmetic::subtract(a, b).map_err(exact)
}
pub(super) fn mul(a: &Q, b: &Q) -> Result<Q> {
    arithmetic::multiply(a, b).map_err(exact)
}
pub(super) fn div(a: &Q, b: &Q) -> Result<Q> {
    arithmetic::divide(a, b).map_err(exact)
}
pub(super) fn difference(a: &Point, b: &Point) -> Result<Point> {
    Ok([sub(&a[0], &b[0])?, sub(&a[1], &b[1])?, sub(&a[2], &b[2])?])
}
pub(super) fn dot(a: &Point, b: &Point) -> Result<Q> {
    add(
        &add(&mul(&a[0], &b[0])?, &mul(&a[1], &b[1])?)?,
        &mul(&a[2], &b[2])?,
    )
}
pub(super) fn cross(a: &Point, b: &Point) -> Result<Point> {
    Ok([
        sub(&mul(&a[1], &b[2])?, &mul(&a[2], &b[1])?)?,
        sub(&mul(&a[2], &b[0])?, &mul(&a[0], &b[2])?)?,
        sub(&mul(&a[0], &b[1])?, &mul(&a[1], &b[0])?)?,
    ])
}
fn along(a: &Point, direction: &Point, t: &Q) -> Result<Point> {
    Ok([
        add(&a[0], &mul(&direction[0], t)?)?,
        add(&a[1], &mul(&direction[1], t)?)?,
        add(&a[2], &mul(&direction[2], t)?)?,
    ])
}
fn norm(a: &Point) -> Result<Q> {
    dot(a, a)
}
fn distance(a: &Point, b: &Point) -> Result<Q> {
    norm(&difference(a, b)?)
}
fn normal(t: &Triangle) -> Result<Point> {
    cross(&difference(&t[1], &t[0])?, &difference(&t[2], &t[0])?)
}
fn unit() -> Q {
    Q::from_integer(1.into())
}
fn clamp(q: Q) -> Q {
    q.max(Q::zero()).min(unit())
}

/// Barycentric inclusion on the plane, including its boundary.
fn in_triangle(p: &Point, triangle: &Triangle) -> Result<bool> {
    let n = normal(triangle)?;
    if !dot(&n, &difference(p, &triangle[0])?)?.is_zero() {
        return Ok(false);
    }
    for i in 0..3 {
        let side = cross(
            &difference(&triangle[(i + 1) % 3], &triangle[i])?,
            &difference(p, &triangle[i])?,
        )?;
        if dot(&n, &side)?.is_negative() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn point_segment(p: &Point, a: &Point, b: &Point) -> Result<(Q, Point)> {
    let edge = difference(b, a)?;
    let denominator = norm(&edge)?;
    let t = if denominator.is_zero() {
        Q::zero()
    } else {
        clamp(div(&dot(&difference(p, a)?, &edge)?, &denominator)?)
    };
    let witness = along(a, &edge, &t)?;
    Ok((distance(p, &witness)?, witness))
}

fn point_triangle(p: &Point, t: &Triangle) -> Result<(Q, Point)> {
    let n = normal(t)?;
    let n2 = norm(&n)?;
    if n2.is_zero() {
        return Err(invalid(
            "Material premises require nondegenerate triangles.",
        ));
    }
    let projection = along(p, &n, &div(&(-dot(&difference(p, &t[0])?, &n)?), &n2)?)?;
    if in_triangle(&projection, t)? {
        return Ok((distance(p, &projection)?, projection));
    }
    let mut best = point_segment(p, &t[0], &t[1])?;
    for i in 1..3 {
        let candidate = point_segment(p, &t[i], &t[(i + 1) % 3])?;
        if candidate.0 < best.0 {
            best = candidate;
        }
    }
    Ok(best)
}

fn segment_segment(a: &Point, b: &Point, c: &Point, d: &Point) -> Result<(Q, Point, Point)> {
    let u = difference(b, a)?;
    let v = difference(d, c)?;
    let w = difference(a, c)?;
    let uu = norm(&u)?;
    let vv = norm(&v)?;
    let uv = dot(&u, &v)?;
    let uw = dot(&u, &w)?;
    let vw = dot(&v, &w)?;
    let determinant = sub(&mul(&uu, &vv)?, &mul(&uv, &uv)?)?;
    let mut candidates = Vec::with_capacity(5);
    for (p, x, y, swapped) in [
        (a, c, d, false),
        (b, c, d, false),
        (c, a, b, true),
        (d, a, b, true),
    ] {
        let (squared, other) = point_segment(p, x, y)?;
        candidates.push(if swapped {
            (squared, other, p.clone())
        } else {
            (squared, p.clone(), other)
        });
    }
    if !determinant.is_zero() {
        let s = div(&sub(&mul(&uv, &vw)?, &mul(&vv, &uw)?)?, &determinant)?;
        let t = div(&sub(&mul(&uu, &vw)?, &mul(&uv, &uw)?)?, &determinant)?;
        if s >= Q::zero() && s <= unit() && t >= Q::zero() && t <= unit() {
            let p = along(a, &u, &s)?;
            let q = along(c, &v, &t)?;
            candidates.push((distance(&p, &q)?, p, q));
        }
    }
    Ok(candidates.into_iter().min_by(|a, b| a.0.cmp(&b.0)).unwrap())
}

/// Includes the segment-through-face case missed by feature endpoint distances.
fn pierce(a: &Point, b: &Point, t: &Triangle) -> Result<Option<Point>> {
    let n = normal(t)?;
    let direction = difference(b, a)?;
    let denominator = dot(&n, &direction)?;
    if denominator.is_zero() {
        return Ok(None);
    }
    let parameter = div(&dot(&n, &difference(&t[0], a)?)?, &denominator)?;
    if parameter < Q::zero() || parameter > unit() {
        return Ok(None);
    }
    let p = along(a, &direction, &parameter)?;
    Ok(in_triangle(&p, t)?.then_some(p))
}

fn intersections(a: &Triangle, b: &Triangle) -> Result<Vec<Point>> {
    let mut contacts = Vec::new();
    let coplanar = (0..3)
        .map(|i| dot(&normal(a)?, &difference(&b[i], &a[0])?))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .all(Zero::is_zero);
    for (left, right) in [(a, b), (b, a)] {
        for i in 0..3 {
            if coplanar && in_triangle(&left[i], right)? {
                contacts.push(left[i].clone());
            }
            if let Some(p) = pierce(&left[i], &left[(i + 1) % 3], right)? {
                contacts.push(p);
            }
        }
    }
    if coplanar {
        for i in 0..3 {
            for j in 0..3 {
                let (squared, p, _) =
                    segment_segment(&a[i], &a[(i + 1) % 3], &b[j], &b[(j + 1) % 3])?;
                if squared.is_zero() {
                    contacts.push(p);
                }
            }
        }
    }
    Ok(contacts)
}

fn triangle_pair(a: &Triangle, b: &Triangle) -> Result<(Q, Vec<Point>)> {
    let contacts = intersections(a, b)?;
    if !contacts.is_empty() {
        return Ok((Q::zero(), contacts));
    }
    let mut minimum: Option<Q> = None;
    let mut contacts = Vec::new();
    for (left, right) in [(a, b), (b, a)] {
        for point in left {
            let (squared, _) = point_triangle(point, right)?;
            if squared.is_zero() {
                contacts.push(point.clone());
            }
            minimum = Some(minimum.map_or(squared.clone(), |value| value.min(squared)));
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            let (squared, p, _) = segment_segment(&a[i], &a[(i + 1) % 3], &b[j], &b[(j + 1) % 3])?;
            if squared.is_zero() {
                contacts.push(p);
            }
            minimum = Some(minimum.map_or(squared.clone(), |value| value.min(squared)));
        }
    }
    Ok((
        if contacts.is_empty() {
            minimum.unwrap()
        } else {
            Q::zero()
        },
        contacts,
    ))
}

fn box_distance(a: &Triangle, b: &Triangle) -> Result<Q> {
    let mut squared = Q::zero();
    for axis in 0..3 {
        let amin = a.iter().map(|p| &p[axis]).min().unwrap();
        let amax = a.iter().map(|p| &p[axis]).max().unwrap();
        let bmin = b.iter().map(|p| &p[axis]).min().unwrap();
        let bmax = b.iter().map(|p| &p[axis]).max().unwrap();
        let gap = if amin > bmax {
            sub(amin, bmax)?
        } else if bmin > amax {
            sub(bmin, amax)?
        } else {
            Q::zero()
        };
        squared = add(&squared, &mul(&gap, &gap)?)?;
    }
    Ok(squared)
}

/// One same-shell triangle pair whose boxes touch: adjacent triangles may not
/// fold into one plane, and other pairs may meet only on their shared vertex
/// or edge.
fn self_pair(ta: &Triangle, tb: &Triangle) -> Result<()> {
    let shared: Vec<_> = ta.iter().filter(|p| tb.contains(p)).collect();
    if shared.len() == 2 {
        let na = normal(ta)?;
        let nb = normal(tb)?;
        // Distinct planes meet only on their common edge. In one plane,
        // consistent normals put opposite-edge triangles on opposite sides;
        // inverse normals overlap there.
        if !norm(&cross(&na, &nb)?)?.is_zero() || dot(&na, &nb)?.is_positive() {
            return Ok(());
        }
        return Err(invalid(
            "Adjacent material triangles overlap in their plane.",
        ));
    }
    let contacts = intersections(ta, tb)?;
    for p in contacts {
        let allowed = match shared.as_slice() {
            [] => false,
            [point] => p == **point,
            [a, b] => point_segment(&p, a, b)?.0.is_zero(),
            _ => false,
        };
        if !allowed {
            return Err(invalid(
                "A material shell has self-intersecting or duplicated boundary triangles.",
            ));
        }
    }
    Ok(())
}

/// The exact squared distance of two triangles of distinct shells, or `None`
/// when their boxes alone are farther apart than `threshold`.
fn proximity(ta: &Triangle, tb: &Triangle, threshold: &Q) -> Result<Option<Q>> {
    if box_distance(ta, tb)? > *threshold {
        return Ok(None);
    }
    Ok(Some(triangle_pair(ta, tb)?.0))
}

/// One triangle of a ray-parity pass: `None` when the ray is parallel to its
/// plane or meets its boundary, else whether the ray crosses it.
fn ray_step(p: &Point, direction: &Point, t: &Triangle) -> Result<Option<bool>> {
    let n = normal(t)?;
    let denominator = dot(&n, direction)?;
    if denominator.is_zero() {
        return Ok(None);
    }
    let parameter = div(&dot(&n, &difference(&t[0], p)?)?, &denominator)?;
    if parameter <= Q::zero() {
        return Ok(Some(false));
    }
    let hit = along(p, direction, &parameter)?;
    if !in_triangle(&hit, t)? {
        return Ok(Some(false));
    }
    for i in 0..3 {
        if point_segment(&hit, &t[i], &t[(i + 1) % 3])?.0.is_zero() {
            return Ok(None);
        }
    }
    Ok(Some(true))
}

struct Work<'a> {
    budget: &'a Budget,
    trace: RefCell<&'a mut ChargeTrace>,
    stage: Cell<ChargeStage>,
}
impl Work<'_> {
    fn charge(&self, units: u64) -> std::result::Result<(), crate::budget::BudgetExceeded> {
        super::exact::charge(self.budget, units)?;
        self.trace.borrow_mut().record(ChargeStep {
            units,
            pair: None,
            stage: self.stage.get(),
        });
        Ok(())
    }
}

fn charge(budget: &Work<'_>) -> Result<()> {
    budget.charge(1).map_err(|error| BackendError {
        kind: BackendErrorKind::BudgetExceeded {
            limit: error.limit,
            used: error.used,
        },
        message: "Material triangle work exceeds the declared budget.".into(),
    })
}

/// Outward-rounded addition can only retain extra candidates. Original box
/// extrema are original binary64 coordinates, so comparison needs no rounding.
fn candidate(a: Aabb, b: Aabb, tolerance: f64) -> bool {
    (0..3).all(|axis| {
        a.min[axis] <= (b.max[axis] + tolerance).next_up()
            && b.min[axis] <= (a.max[axis] + tolerance).next_up()
    })
}

fn candidates(
    tree: &BoxTree,
    bounds: &[Aabb],
    current: usize,
    tolerance: f64,
    budget: &Work<'_>,
) -> Result<Vec<usize>> {
    let mut stack = vec![0];
    let mut result = Vec::new();
    while let Some(index) = stack.pop() {
        charge(budget)?;
        let node = &tree.nodes[index];
        if !candidate(bounds[current], node.bounds, tolerance) {
            continue;
        }
        if let Some(children) = node.children {
            stack.extend(children);
        } else {
            for &other in &tree.items[node.items.clone()] {
                if other > current && candidate(bounds[current], bounds[other], tolerance) {
                    result.push(other);
                }
            }
        }
    }
    result.sort_unstable();
    Ok(result)
}

struct Shell {
    primitive: u32,
    triangles: Vec<usize>,
    sign: bool,
    bounds: [Point; 2],
    aabb: Aabb,
}

/// One regular material body in an immutable authored primitive. A source-order
/// key is not stable across changed content and is never a proximity cluster.
#[derive(Clone)]
pub(crate) struct MaterialRegion {
    pub(crate) primitive: u32,
    pub(crate) root: u32,
    pub(crate) triangles: Vec<usize>,
    pub(crate) label: String,
    pub(crate) bounds: crate::backend::brep::Bounds,
    pub(crate) local_triangles: Vec<usize>,
    pub(crate) operand: std::rc::Rc<super::material_intersection::QualifiedMaterial>,
}

pub(crate) fn region_boundary_distance(
    record: &MeshAnalysisRecord,
    left: &MaterialRegion,
    right: &MaterialRegion,
    budget: &Budget,
) -> Result<(Q, Option<[f64; 3]>)> {
    use num_traits::ToPrimitive;
    let mut minimum: Option<Q> = None;
    let mut witness = None;
    for &a in &left.triangles {
        for &b in &right.triangles {
            super::exact::charge(budget, 1).map_err(|error| BackendError {
                kind: BackendErrorKind::BudgetExceeded {
                    limit: error.limit,
                    used: error.used,
                },
                message: "Material boundary-distance work exceeds the declared budget.".into(),
            })?;
            let triangle = |index: usize| -> Result<Triangle> {
                let vertices = record.triangles[index];
                let point = |vertex: u32| -> Result<Point> {
                    let p = record.positions[vertex as usize];
                    Ok([
                        arithmetic::rational(p[0]).map_err(exact)?,
                        arithmetic::rational(p[1]).map_err(exact)?,
                        arithmetic::rational(p[2]).map_err(exact)?,
                    ])
                };
                Ok([
                    point(vertices[0])?,
                    point(vertices[1])?,
                    point(vertices[2])?,
                ])
            };
            let (squared, contacts) = triangle_pair(&triangle(a)?, &triangle(b)?)?;
            if minimum.as_ref().is_none_or(|value| squared < *value) {
                witness = contacts.first().and_then(|point| {
                    Some([point[0].to_f64()?, point[1].to_f64()?, point[2].to_f64()?])
                });
                minimum = Some(squared);
            }
        }
    }
    minimum
        .map(|value| (value, witness))
        .ok_or_else(|| invalid("A material region has no boundary triangles."))
}

/// Returns triangle membership of qualified spatial material clusters.
/// Scratch is call-local; neither invalid premises nor incomplete work is cached.
#[cfg(test)]
pub(super) fn clusters(
    record: &MeshAnalysisRecord,
    tolerance: f64,
    budget: &Budget,
    byte_limit: u64,
) -> Result<Vec<Vec<usize>>> {
    clusters_traced(
        record,
        tolerance,
        budget,
        byte_limit,
        &mut ChargeTrace::disabled(),
    )
}

pub(super) fn clusters_traced(
    record: &MeshAnalysisRecord,
    tolerance: f64,
    budget: &Budget,
    byte_limit: u64,
    trace: &mut ChargeTrace,
) -> Result<Vec<Vec<usize>>> {
    classify(record, tolerance, budget, byte_limit, false, trace).map(|value| value.0)
}

/// Orientation normalization per independently qualified material-root stack.
/// Spatial clusters are deliberately not scalar union operands.
pub(super) fn regular_orientation_traced(
    record: &MeshAnalysisRecord,
    budget: &Budget,
    byte_limit: u64,
    trace: &mut ChargeTrace,
) -> Result<(Vec<bool>, Vec<Vec<usize>>)> {
    classify(record, 0.0, budget, byte_limit, true, trace).map(|value| (value.1, value.2))
}

type Classification = (Vec<Vec<usize>>, Vec<bool>, Vec<Vec<usize>>);

/// The bytes `classify` reserves for a record whose `shells` shells include
/// one of `largest` triangles, through its shell-pair scratch with every
/// shell pair a candidate.
#[cfg(test)]
pub(super) fn reserved(record: &MeshAnalysisRecord, shells: u64, largest: u64) -> u64 {
    if staged::eligible(record) {
        return scratch::staged_shells(
            scratch::staged_admission(record),
            record.triangles.len() as u64,
            shells,
            largest,
            record.primitives.len() as u64,
        )
        .saturating_add(scratch::staged_pairs(
            shells,
            shells.saturating_mul(shells.saturating_sub(1)) / 2,
            record.triangles.len() as u64,
        ));
    }
    let admission = scratch::admission(record);
    scratch::shells(
        &admission,
        record.triangles.len() as u64,
        shells,
        largest,
        record.primitives.len() as u64,
    )
    .saturating_add(scratch::pairs(shells))
}

/// The staged path for records whose coordinates keep every rational
/// intermediate inside the arithmetic guard; the rational path otherwise.
/// Both return the same classification and the same semantic refusals.
fn classify(
    record: &MeshAnalysisRecord,
    tolerance: f64,
    budget: &Budget,
    byte_limit: u64,
    regular_operand: bool,
    trace: &mut ChargeTrace,
) -> Result<Classification> {
    if staged::eligible(record) {
        staged::classify(
            record,
            tolerance,
            budget,
            byte_limit,
            regular_operand,
            trace,
        )
    } else {
        classify_rational(
            record,
            tolerance,
            budget,
            byte_limit,
            regular_operand,
            trace,
        )
    }
}

/// Every predicate in exact rationals: the reference the staged path is
/// tested against, and the path for coordinates outside its span.
fn classify_rational(
    record: &MeshAnalysisRecord,
    tolerance: f64,
    budget: &Budget,
    byte_limit: u64,
    regular_operand: bool,
    trace: &mut ChargeTrace,
) -> Result<Classification> {
    let work = Work {
        budget,
        trace: RefCell::new(trace),
        stage: Cell::new(ChargeStage::BodySetup),
    };
    let budget = &work;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(invalid(
            "Material tolerance must be finite and nonnegative.",
        ));
    }
    record.validate().map_err(invalid)?;
    // Reserve an upper bound of every retained coordinate limb, topology
    // container and the fixed predicate working set before constructing any
    // of them (see `scratch`). Per-shell scratch is reserved once the shell
    // count is known, before shells exist. No retained cache.
    let admission = scratch::admission(record);
    let reservation = admission.bytes;
    if reservation > byte_limit {
        return Err(scratch_limit(
            "Material exact scratch",
            record,
            reservation,
            byte_limit,
        ));
    }
    budget
        .trace
        .borrow_mut()
        .limit_to_bytes(byte_limit.saturating_sub(reservation));
    budget
        .charge(record.positions.len() as u64)
        .map_err(|error| BackendError {
            kind: BackendErrorKind::BudgetExceeded {
                limit: error.limit,
                used: error.used,
            },
            message: "Material coordinate work exceeds the declared budget.".into(),
        })?;
    let mut points: Vec<Point> = Vec::with_capacity(record.positions.len());
    for p in &record.positions {
        points.push([
            arithmetic::rational(p[0]).map_err(exact)?,
            arithmetic::rational(p[1]).map_err(exact)?,
            arithmetic::rational(p[2]).map_err(exact)?,
        ]);
    }
    // Each vertex maps to the first index holding its exact point. Sorting
    // indices by (point, index) finds it without cloning any point.
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_unstable_by(|&a, &b| points[a].cmp(&points[b]).then(a.cmp(&b)));
    let mut vertices: Vec<usize> = (0..points.len()).collect();
    for run in order.chunk_by(|&a, &b| points[a] == points[b]) {
        for &index in run {
            vertices[index] = run[0];
        }
    }
    // Every exact edge use, sorted by (edge, primitive, triangle): the order
    // in which the edge-incidence and primitive groups are visited below.
    let mut edges: Vec<EdgeUse> = Vec::with_capacity(record.triangles.len().saturating_mul(3));
    let mut parent: Vec<usize> = (0..record.triangles.len()).collect();
    let triangle = |index: usize| record.triangles[index].map(|v| points[v as usize].clone());
    let triangle_bounds: Vec<Aabb> = record
        .triangles
        .iter()
        .map(|triangle| {
            let mut bounds = empty_aabb();
            for &vertex in triangle {
                expand(&mut bounds, record.positions[vertex as usize]);
            }
            bounds
        })
        .collect();
    budget.stage.set(ChargeStage::Sweep);
    budget
        .charge(
            (record.triangles.len() as u64)
                .saturating_mul(u64::from(record.triangles.len().max(1).ilog2()) + 1),
        )
        .map_err(|error| BackendError {
            kind: BackendErrorKind::BudgetExceeded {
                limit: error.limit,
                used: error.used,
            },
            message: "Material broad-phase construction exceeds the declared budget.".into(),
        })?;
    let tree = BoxTree::new(&triangle_bounds);
    budget.stage.set(ChargeStage::BodySetup);
    for (index, row) in record.triangles.iter().enumerate() {
        charge(budget)?;
        if norm(&normal(&triangle(index))?)?.is_zero() {
            return Err(invalid(
                "Material premises require nondegenerate triangles.",
            ));
        }
        for i in 0..3 {
            let a = vertices[row[i] as usize];
            let b = vertices[row[(i + 1) % 3] as usize];
            edges.push((
                a.min(b),
                a.max(b),
                record.triangle_primitives[index],
                index,
                a < b,
            ));
        }
    }
    edges.sort_unstable_by_key(|&(low, high, primitive, index, _)| (low, high, primitive, index));
    // Coincident boundaries of distinct admitted primitive partitions are not
    // a self-intersection of either shell. Labels are unused.
    for incident in edges.chunk_by(|a, b| (a.0, a.1, a.2) == (b.0, b.1, b.2)) {
        if incident.len() != 2 || incident[0].4 == incident[1].4 {
            return Err(invalid(
                "Material shells require exactly two oppositely directed triangles per exact edge.",
            ));
        }
        let a = find(&mut parent, incident[0].3);
        let b = find(&mut parent, incident[1].3);
        parent[a] = b;
    }
    // Shell memberships sorted by (root, triangle): roots ascending, each
    // shell's triangles ascending.
    let mut memberships: Vec<(usize, usize)> = Vec::with_capacity(record.triangles.len());
    for index in 0..record.triangles.len() {
        memberships.push((find(&mut parent, index), index));
    }
    memberships.sort_unstable();
    let runs = || memberships.chunk_by(|a, b| a.0 == b.0);
    let shell_scratch = scratch::shells(
        &admission,
        record.triangles.len() as u64,
        runs().count() as u64,
        runs().map(<[_]>::len).max().unwrap_or(0) as u64,
        record.primitives.len() as u64,
    );
    if shell_scratch > byte_limit {
        return Err(scratch_limit(
            "Material exact scratch",
            record,
            shell_scratch,
            byte_limit,
        ));
    }
    let reservation = shell_scratch;
    budget
        .trace
        .borrow_mut()
        .limit_to_bytes(byte_limit.saturating_sub(reservation));
    let mut shells = Vec::with_capacity(runs().count());
    for members in runs() {
        let members: Vec<usize> = members.iter().map(|&(_, index)| index).collect();
        // Every vertex link in a closed two-manifold shell is connected. Uses
        // sorted by (vertex, triangle) visit each link in vertex order.
        let mut links: Vec<(usize, usize)> = Vec::with_capacity(members.len() * 3);
        for &index in &members {
            for &v in &record.triangles[index] {
                links.push((vertices[v as usize], index));
            }
        }
        links.sort_unstable();
        for incident in links.chunk_by(|a, b| a.0 == b.0) {
            let vertex = incident[0].0;
            charge(budget)?;
            let mut linked: Vec<usize> = (0..incident.len()).collect();
            for a in 0..incident.len() {
                for b in a + 1..incident.len() {
                    charge(budget)?;
                    let ta = record.triangles[incident[a].1].map(|v| vertices[v as usize]);
                    let tb = record.triangles[incident[b].1].map(|v| vertices[v as usize]);
                    if ta.iter().any(|&v| v != vertex && tb.contains(&v)) {
                        let left = find(&mut linked, a);
                        let right = find(&mut linked, b);
                        linked[left] = right;
                    }
                }
            }
            if (0..linked.len()).any(|index| find(&mut linked, index) != find(&mut linked, 0)) {
                return Err(invalid("A material shell has a disconnected vertex link."));
            }
        }
        drop(links);
        let mut volume = Q::zero();
        for &index in &members {
            let t = triangle(index);
            volume = add(&volume, &dot(&t[0], &cross(&t[1], &t[2])?)?)?;
        }
        if volume.is_zero() {
            return Err(invalid("Material shells require nonzero oriented volume."));
        }
        budget.stage.set(ChargeStage::FaceDistance);
        for &a in &members {
            for b in candidates(&tree, &triangle_bounds, a, 0.0, budget)? {
                if find(&mut parent, a) != find(&mut parent, b) {
                    continue;
                }
                charge(budget)?;
                let ta = triangle(a);
                let tb = triangle(b);
                if !box_distance(&ta, &tb)?.is_zero() {
                    continue;
                }
                self_pair(&ta, &tb)?;
            }
        }
        let first = triangle(members[0]);
        let mut bounds = [first[0].clone(), first[0].clone()];
        let mut aabb = empty_aabb();
        for &index in &members {
            for p in triangle(index) {
                for (axis, coordinate) in p.iter().enumerate() {
                    bounds[0][axis] = bounds[0][axis].clone().min(coordinate.clone());
                    bounds[1][axis] = bounds[1][axis].clone().max(coordinate.clone());
                }
            }
        }
        for &index in &members {
            expand(&mut aabb, triangle_bounds[index].min);
            expand(&mut aabb, triangle_bounds[index].max);
        }
        shells.push(Shell {
            primitive: record.triangle_primitives[members[0]],
            triangles: members,
            sign: volume.is_positive(),
            bounds,
            aabb,
        });
    }
    let count = shells.len();
    let pairs = scratch::pairs(count as u64);
    if reservation.saturating_add(pairs) > byte_limit {
        return Err(scratch_limit(
            "Material pair scratch",
            record,
            reservation.saturating_add(pairs),
            byte_limit,
        ));
    }
    budget
        .trace
        .borrow_mut()
        .limit_to_bytes(byte_limit.saturating_sub(reservation).saturating_sub(pairs));
    let mut contains = vec![vec![false; count]; count];
    let mut near = vec![vec![false; count]; count];
    let mut boundary_contacts = vec![vec![false; count]; count];
    let threshold = arithmetic::rational(tolerance).map_err(exact)?;
    let threshold = mul(&threshold, &threshold)?;
    for a in 0..count {
        for b in a + 1..count {
            budget.stage.set(ChargeStage::FaceDistance);
            charge(budget)?;
            if !candidate(shells[a].aabb, shells[b].aabb, tolerance) {
                continue;
            }
            let left = [
                shells[a].bounds[0].clone(),
                shells[a].bounds[1].clone(),
                shells[a].bounds[1].clone(),
            ];
            let right = [
                shells[b].bounds[0].clone(),
                shells[b].bounds[1].clone(),
                shells[b].bounds[1].clone(),
            ];
            if box_distance(&left, &right)? > threshold {
                // Positive box separation proves both no nesting and no
                // threshold candidate, not contact or containment.
                continue;
            }
            let mut boundary_contact = false;
            for &i in &shells[a].triangles {
                for &j in &shells[b].triangles {
                    charge(budget)?;
                    let Some(squared) = proximity(&triangle(i), &triangle(j), &threshold)? else {
                        continue;
                    };
                    boundary_contact |= squared.is_zero();
                    near[a][b] |= squared <= threshold;
                }
            }
            if !boundary_contact {
                contains[a][b] = inside(
                    &triangle(shells[b].triangles[0])[0],
                    &shells[a],
                    &triangle,
                    budget,
                )?;
                contains[b][a] = inside(
                    &triangle(shells[a].triangles[0])[0],
                    &shells[b],
                    &triangle,
                    budget,
                )?;
                if contains[a][b] && contains[b][a] {
                    return Err(invalid("Material shell nesting is ambiguous."));
                }
            }
            boundary_contacts[a][b] = boundary_contact;
        }
    }
    let mut nesting = vec![None; count];
    for (child, slot) in nesting.iter_mut().enumerate() {
        let candidates: Vec<_> = (0..count)
            .filter(|&a| shells[a].primitive == shells[child].primitive && contains[a][child])
            .collect();
        let nearest: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|&a| !candidates.iter().any(|&b| a != b && contains[a][b]))
            .collect();
        if nearest.len() > 1 {
            return Err(unresolved(
                "Crossing containers require complete material-cell classification.",
            ));
        }
        *slot = nearest.first().copied();
    }
    let mut parent: Vec<_> = (0..count).collect();
    for (a, row) in boundary_contacts.iter().enumerate() {
        for (b, &touching) in row.iter().enumerate().skip(a + 1) {
            if touching
                && shells[a].primitive == shells[b].primitive
                && nesting[a].is_none()
                && nesting[b].is_none()
                && shells[a].sign != shells[b].sign
            {
                return Err(unresolved("Intersecting opposite-oriented root shells require complete material-cell classification."));
            }
        }
    }
    for child in 0..count {
        if let Some(container) = nesting[child] {
            if shells[child].sign != shells[container].sign {
                // Relative inversion creates a cavity only when its parent
                // bounds material; an island inside a cavity is a new body.
                let depth = depth(child, &nesting)?;
                if depth % 2 == 1 {
                    parent[child] = container;
                }
            } else {
                return Err(unresolved("Nested shells with inconsistent relative orientation require explicit material-cell classification."));
            }
        }
    }
    if regular_operand
        && boundary_contacts
            .iter()
            .any(|row| row.iter().any(|&contact| contact))
    {
        return Err(unresolved("A scalar material operand requires disjoint or strictly nested shell boundaries, not crossing or tangent shell stacks."));
    }
    let mut orientation = vec![false; record.triangles.len()];
    for (index, shell) in shells.iter().enumerate() {
        let mut root = index;
        while let Some(container) = nesting[root] {
            root = container;
        }
        for &triangle in &shell.triangles {
            orientation[triangle] = !shells[root].sign;
        }
    }
    // Nesting between independently closed authored operands is occupied union,
    // not a cavity sign relation. The deepest containing shell of that operand
    // determines material/void; a legitimate global inversion is immaterial.
    for child in 0..count {
        if depth(child, &nesting)? % 2 == 1 {
            continue;
        }
        let mut deepest = BTreeMap::<u32, (usize, usize)>::new();
        for container in 0..count {
            charge(budget)?;
            if shells[container].primitive == shells[child].primitive || !contains[container][child]
            {
                continue;
            }
            let level = depth(container, &nesting)?;
            let value = deepest
                .entry(shells[container].primitive)
                .or_insert((container, level));
            if level > value.1 {
                *value = (container, level);
            }
        }
        for (_, (container, level)) in deepest {
            if level % 2 == 0 {
                let a = find(&mut parent, container);
                let b = find(&mut parent, child);
                parent[a] = b;
            }
        }
    }
    // Material bodies precede the spatial proximity graph: each even-depth
    // shell owns its immediate cavity shells, while an island is another body.
    let mut bodies = BTreeMap::<usize, Vec<usize>>::new();
    for (index, shell) in shells.iter().enumerate() {
        let owner = if depth(index, &nesting)? % 2 == 1 {
            nesting[index].expect("odd depth has parent")
        } else {
            index
        };
        bodies
            .entry(owner)
            .or_default()
            .extend(shell.triangles.iter().copied());
    }
    let mut bodies: Vec<_> = bodies
        .into_iter()
        .map(|(owner, mut body)| {
            body.sort_unstable();
            let root = *shells[owner]
                .triangles
                .iter()
                .min()
                .expect("nonempty shell");
            let position = body
                .iter()
                .position(|&index| index == root)
                .expect("root belongs to body");
            body.swap(0, position);
            body
        })
        .collect();
    bodies.sort_by_key(|body| body[0]);
    for (a, row) in near.iter().enumerate() {
        for (b, &is_near) in row.iter().enumerate().skip(a + 1) {
            if is_near {
                let left = find(&mut parent, a);
                let right = find(&mut parent, b);
                parent[left] = right;
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for (index, shell) in shells.into_iter().enumerate() {
        groups
            .entry(find(&mut parent, index))
            .or_default()
            .extend(shell.triangles);
    }
    Ok((groups.into_values().collect(), orientation, bodies))
}

fn depth(mut child: usize, parents: &[Option<usize>]) -> Result<usize> {
    let mut depth = 0;
    while let Some(parent) = parents[child] {
        depth += 1;
        if depth > parents.len() {
            return Err(invalid("Cyclic material nesting."));
        }
        child = parent;
    }
    Ok(depth)
}

fn inside(
    p: &Point,
    shell: &Shell,
    triangle: &impl Fn(usize) -> Triangle,
    budget: &Work<'_>,
) -> Result<bool> {
    budget.stage.set(ChargeStage::NestedClassify);
    // Each forbidden ray is a root of a degree-at-most-two polynomial in k.
    // Bound attempts independently; exhaustion is unresolved, never a panic.
    for k in 1..=shell.triangles.len().saturating_mul(8).saturating_add(1) {
        let k = Q::from_integer(k.into());
        let direction = [unit(), k.clone(), mul(&k, &k)?];
        let mut crossings = 0;
        let mut ambiguous = false;
        for &index in &shell.triangles {
            charge(budget)?;
            match ray_step(p, &direction, &triangle(index))? {
                None => {
                    ambiguous = true;
                    break;
                }
                Some(true) => crossings += 1,
                Some(false) => {}
            }
        }
        if !ambiguous {
            return Ok(crossings % 2 == 1);
        }
    }
    Err(unresolved(
        "Exact material classification exhausted its bounded nondegenerate ray directions.",
    ))
}

/// Combined dyadic span, including the binary point, for scalar certificates.
/// A single shared span is necessary: two individually small spans can have
/// widely separated exponents when their records are combined.
pub(super) fn scalar_certificate_eligible<'a>(points: impl Iterator<Item = &'a [f64; 3]>) -> bool {
    let (mut low, mut high) = (0i32, 0i32);
    for &value in points.flatten() {
        if !value.is_finite() {
            return false;
        }
        if let Some((mantissa, exponent)) = integer::decompose(value) {
            low = low.min(exponent);
            high = high.max(exponent + 64 - mantissa.unsigned_abs().leading_zeros() as i32);
        }
    }
    high - low <= 128
}

/// Exact oriented face sign: checked i128, then arbitrary precision integers.
pub(super) fn scalar_face_sign(face: [[f64; 3]; 3], point: &[f64; 3]) -> std::cmp::Ordering {
    let least = integer::least(face.iter().flatten().chain(point));
    integer::volume(|| std::iter::once(face), point, least)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: i64, y: i64, z: i64) -> Point {
        [x, y, z].map(|v| Q::from_integer(v.into()))
    }
    fn boxes(specs: &[(f64, bool, [f64; 3], u32)]) -> MeshAnalysisRecord {
        use super::super::Primitive;
        let mut record = MeshAnalysisRecord {
            positions: Vec::new(),
            triangles: Vec::new(),
            triangle_primitives: Vec::new(),
            primitives: Vec::new(),
        };
        for &(radius, inward, translation, primitive) in specs {
            let cube = super::super::tests::box_record(0.0);
            let start = record.positions.len() as u32;
            record.positions.extend(cube.positions.into_iter().map(|p| {
                std::array::from_fn(|axis| (2.0 * p[axis] - 1.0) * radius + translation[axis])
            }));
            record.triangles.extend(cube.triangles.into_iter().map(|t| {
                let [a, b, c] = t.map(|v| v + start);
                if inward {
                    [c, b, a]
                } else {
                    [a, b, c]
                }
            }));
            record.triangle_primitives.extend([primitive; 12]);
        }
        let count = specs.iter().map(|spec| spec.3).max().map_or(0, |v| v + 1);
        record.primitives = (0..count)
            .map(|_| Primitive {
                name: "same-label".into(),
                vertex_start: 0,
                vertex_count: record.positions.len() as u32,
            })
            .collect();
        record
    }
    fn answer(
        record: &MeshAnalysisRecord,
        tolerance: f64,
        budget: &Budget,
    ) -> Result<Vec<Vec<usize>>> {
        clusters(record, tolerance, budget, u64::MAX)
    }
    #[test]
    fn cavity_island_and_uniform_inversion_preserve_material_connectivity() {
        let mut record = boxes(&[
            (3.0, false, [0.0; 3], 0),
            (2.0, true, [0.0; 3], 0),
            (1.0, false, [0.0; 3], 1),
        ]);
        let groups = answer(&record, 0.0, &Budget::new(100_000)).unwrap();
        assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [24, 12]);
        for t in &mut record.triangles {
            t.swap(0, 2);
        }
        assert_eq!(answer(&record, 0.0, &Budget::new(100_000)).unwrap(), groups);
        assert_eq!(
            answer(&record, 1.0, &Budget::new(100_000)).unwrap().len(),
            1
        );
    }
    #[test]
    fn crossing_distinct_solids_and_face_contact_are_not_self_intersections() {
        for displacement in [1.0, 2.0] {
            let record = boxes(&[
                (1.0, false, [0.0; 3], 0),
                (1.0, false, [displacement, 0.0, 0.0], 1),
            ]);
            assert_eq!(
                answer(&record, 0.0, &Budget::new(100_000)).unwrap().len(),
                1
            );
        }
        let record = boxes(&[
            (3.0, false, [0.0; 3], 0),
            (2.0, true, [0.0; 3], 0),
            (1.0, false, [3.0, 0.0, 0.0], 1),
        ]);
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000)).unwrap().len(),
            1
        );
    }

    #[test]
    fn primitive_premises_nested_independent_solids_are_not_cavities() {
        for inverted in [false, true] {
            let record = boxes(&[(3.0, false, [0.0; 3], 0), (1.0, inverted, [0.0; 3], 1)]);
            assert_eq!(
                answer(&record, 0.0, &Budget::new(100_000)).unwrap().len(),
                1
            );
        }
    }

    #[test]
    fn primitive_premises_complementary_open_sources_cannot_close_each_other() {
        let mut record = boxes(&[(1.0, false, [0.0; 3], 0)]);
        record.primitives.push(super::super::Primitive {
            name: "other-open-half".into(),
            vertex_start: 0,
            vertex_count: 8,
        });
        for owner in &mut record.triangle_primitives[6..] {
            *owner = 1;
        }
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000))
                .unwrap_err()
                .kind,
            BackendErrorKind::InvalidInput
        );
    }

    #[test]
    fn point_edge_and_face_contact_join_closed_material_bodies() {
        for translation in [[2.0, 2.0, 2.0], [2.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
            let record = boxes(&[(1.0, false, [0.0; 3], 0), (1.0, false, translation, 1)]);
            assert_eq!(
                answer(&record, 0.0, &Budget::new(100_000)).unwrap().len(),
                1
            );
        }
    }

    #[test]
    fn crossing_subtractive_root_ambiguity_refuses_instead_of_unioning_shells() {
        let record = boxes(&[(3.0, false, [0.0; 3], 0), (2.0, true, [3.0, 0.0, 0.0], 0)]);
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000))
                .unwrap_err()
                .kind,
            BackendErrorKind::Unsupported
        );
        let record = boxes(&[(1.0, false, [0.0; 3], 0), (1.0, true, [1.0, 0.0, 0.0], 1)]);
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000)).unwrap().len(),
            1
        );
    }

    #[test]
    fn invalid_material_premise_cannot_pass_through_negation() {
        use crate::{codec::Json, registry::Capability, result::Polarity};
        let mut record = boxes(&[(1.0, false, [0.0; 3], 0)]);
        record.triangles.pop();
        record.triangle_primitives.pop();
        let error = answer(&record, 0.0, &Budget::new(100_000)).unwrap_err();
        for polarity in [Polarity::Positive, Polarity::Negative] {
            let result = crate::result::finish(
                "invalid",
                Capability::ToHaveConnectedComponents,
                polarity,
                crate::subject::backend_refusal(error.clone()),
            )
            .unwrap();
            let Json::Object(fields) = result else {
                panic!("canonical result object")
            };
            assert_eq!(
                fields
                    .iter()
                    .find(|(key, _)| key == "status")
                    .map(|(_, value)| value),
                Some(&Json::string("refused"))
            );
        }
    }
    #[test]
    fn exact_euclidean_proximity_does_not_use_overlapping_envelopes() {
        let record = boxes(&[(1.0, false, [0.0; 3], 0), (1.0, false, [3.0, 3.0, 0.0], 1)]);
        assert_eq!(
            answer(&record, 1.0, &Budget::new(100_000)).unwrap().len(),
            2
        );
        assert_eq!(
            answer(&record, 1.5, &Budget::new(100_000)).unwrap().len(),
            1
        );
    }
    #[test]
    fn incomplete_invalid_and_unaffordable_material_premises_refuse() {
        let mut record = boxes(&[(1.0, false, [0.0; 3], 0)]);
        assert!(matches!(
            answer(&record, 0.0, &Budget::new(0)).unwrap_err().kind,
            BackendErrorKind::BudgetExceeded { .. }
        ));
        let limit = analysis_limit(clusters(&record, 0.0, &Budget::new(100_000), 1).unwrap_err());
        assert_eq!((limit.limit, limit.mesh), (1, Some((8, 12))));
        assert!(limit.required > 1);
        record.triangles[0].swap(0, 2);
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000))
                .unwrap_err()
                .kind,
            BackendErrorKind::InvalidInput
        );
        let mut record = boxes(&[(1.0, false, [0.0; 3], 0)]);
        record.positions[7] = [-2.0, -2.0, -2.0];
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000))
                .unwrap_err()
                .kind,
            BackendErrorKind::InvalidInput
        );
        record.triangles[0].swap(0, 2);
        record.triangles.pop();
        record.triangle_primitives.pop();
        assert_eq!(
            answer(&record, 0.0, &Budget::new(100_000))
                .unwrap_err()
                .kind,
            BackendErrorKind::InvalidInput
        );
    }
    #[test]
    fn cold_repeated_work_has_the_exact_same_refusal_boundary() {
        let record = boxes(&[(1.0, false, [0.0; 3], 0), (1.0, false, [3.0, 0.0, 0.0], 1)]);
        let budget = Budget::new(100_000);
        let first = answer(&record, 1.0, &budget).unwrap();
        let units = budget.used();
        let exact = Budget::new(units);
        assert_eq!(answer(&record, 1.0, &exact).unwrap(), first);
        assert_eq!(exact.used(), units);
        assert_eq!(
            answer(&record, 1.0, &Budget::new(units - 1))
                .unwrap_err()
                .kind,
            BackendErrorKind::BudgetExceeded {
                limit: units - 1,
                used: units
            }
        );
    }
    #[test]
    fn exact_distance_includes_skew_edges_and_triangle_piercing() {
        let (d, _, _) =
            segment_segment(&p(-1, 0, 0), &p(1, 0, 0), &p(0, -1, 1), &p(0, 1, 1)).unwrap();
        assert_eq!(d, unit());
        let a = [p(-2, -2, 0), p(2, -2, 0), p(0, 2, 0)];
        let b = [p(0, 0, -1), p(0, 0, 1), p(3, 0, 1)];
        assert!(triangle_pair(&a, &b).unwrap().0.is_zero());
    }
    /// A closed cube surface whose faces are `n` by `n` grids, with binary32
    /// coordinates widened to binary64 as a glTF intake delivers them.
    pub(super) fn grid_box(
        record: &mut MeshAnalysisRecord,
        n: u32,
        origin: [f32; 3],
        step: f32,
        primitive: u32,
    ) {
        let mut index = std::collections::HashMap::<[u32; 3], u32>::new();
        let mut vertex = |record: &mut MeshAnalysisRecord, p: [u32; 3]| -> u32 {
            *index.entry(p).or_insert_with(|| {
                record.positions.push(std::array::from_fn(|axis| {
                    f64::from(origin[axis] + p[axis] as f32 * step)
                }));
                record.positions.len() as u32 - 1
            })
        };
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for side in [0, n] {
                for i in 0..n {
                    for j in 0..n {
                        let corner = |di: u32, dj: u32| {
                            let mut p = [0; 3];
                            p[axis] = side;
                            p[u] = i + di;
                            p[v] = j + dj;
                            p
                        };
                        let quad = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)];
                        let quad = quad.map(|p| vertex(record, p));
                        // e_u x e_v = e_axis, outward only on the far side.
                        let [a, b, c, d] = if side == n {
                            quad
                        } else {
                            [quad[0], quad[3], quad[2], quad[1]]
                        };
                        record.triangles.extend([[a, b, c], [a, c, d]]);
                        record.triangle_primitives.extend([primitive; 2]);
                    }
                }
            }
        }
    }
    pub(super) fn grid_boxes(n: u32, count: u32) -> MeshAnalysisRecord {
        let mut record = MeshAnalysisRecord {
            positions: Vec::new(),
            triangles: Vec::new(),
            triangle_primitives: Vec::new(),
            primitives: Vec::new(),
        };
        for primitive in 0..count {
            let origin = [12.5 + 40.0 * primitive as f32, -7.25, 3.1];
            grid_box(&mut record, n, origin, 0.1, primitive);
            record.primitives.push(super::super::Primitive {
                name: format!("grid#{primitive}"),
                vertex_start: 0,
                vertex_count: 0,
            });
        }
        record
    }
    use super::reserved;
    /// An analysis-capacity refusal, never an evidence gap.
    fn analysis_limit(error: BackendError) -> crate::backend::CapacityLimit {
        match error.kind {
            BackendErrorKind::LimitExceeded(
                limit @ crate::backend::CapacityLimit {
                    class: crate::backend::LimitClass::Analysis,
                    ..
                },
            ) => limit,
            kind => panic!("expected an analysis limit, got {kind:?}"),
        }
    }
    fn earlier_reservation(record: &MeshAnalysisRecord) -> u64 {
        (record.positions.len() as u64) * 6 * (2 * 136 + 64)
            + (record.triangles.len() as u64) * 2048
            + 640 * 1024
    }
    const ENTRY_MESH_BYTES: u64 = 256 * 1024 * 1024;
    /// Exactly `2^exponent`, normal or subnormal, independent of `powi`.
    fn power(exponent: i32) -> f64 {
        if exponent >= -1022 {
            f64::from_bits(((exponent + 1023) as u64) << 52)
        } else {
            f64::from_bits(1 << (exponent + 1074))
        }
    }

    #[test]
    fn coordinate_limbs_bound_every_exact_coordinate_that_is_built_or_cloned() {
        let mut values = vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            3.0,
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE / 3.0,
            5e-324,
            f64::MAX,
            -f64::MAX,
            f64::EPSILON,
            1e300,
            1e-300,
            123.456,
            f64::from(0.1f32),
            f64::from(12.5f32 + 0.1f32 * 37.0),
            power(63),
            power(64),
            power(127),
            power(-63),
            power(-64),
            power(-1022),
        ];
        // Every exponent with a dense and a sparse mantissa.
        for field in 0..2047u64 {
            for fraction in [0, 1, 0x000f_ffff_ffff_ffff, 0x0008_0000_0000_0000] {
                values.push(f64::from_bits(field << 52 | fraction));
            }
        }
        for value in values {
            // Retained bytes are reserved per coordinate; construction
            // temporaries belong to the fixed predicate working set.
            let ((built, retained), peak) = crate::analysis::test_alloc::peak_bytes(|| {
                crate::analysis::test_alloc::retained_bytes(|| {
                    Box::new(arithmetic::rational(value).unwrap())
                })
            });
            assert!(
                retained <= scratch::built_coordinate_bytes(value),
                "{value:e}: built {retained} > {}",
                scratch::built_coordinate_bytes(value)
            );
            assert!(peak <= 4096, "{value:e}: construction peak {peak}");
            let (_, retained) =
                crate::analysis::test_alloc::retained_bytes(|| Box::new((*built).clone()));
            assert!(
                retained <= scratch::cloned_coordinate_bytes(value),
                "{value:e}: clone {retained} > {}",
                scratch::cloned_coordinate_bytes(value)
            );
        }
    }

    #[test]
    fn measured_peak_heap_stays_within_the_reservation() {
        let mut outward = boxes(&[
            (3.0, false, [0.0; 3], 0),
            (2.0, true, [0.0; 3], 0),
            (1.0, false, [0.0; 3], 1),
            (1.0, false, [9.0, 0.0, 0.0], 2),
        ]);
        let huge = boxes(&[(power(-1000), false, [0.0; 3], 0)]);
        let mut cases = vec![
            (grid_boxes(2, 2), 2, 48, 0.0),
            (huge, 1, 12, 0.0),
            (outward.clone(), 4, 12, 0.0),
            (outward.clone(), 4, 12, 1.0),
        ];
        for t in &mut outward.triangles {
            t.swap(0, 2);
        }
        cases.push((outward, 4, 12, 1.0));
        for (record, shells, largest, tolerance) in cases {
            let budget = Budget::new(u64::MAX / 4);
            let (value, peak) = crate::analysis::test_alloc::peak_bytes(|| {
                classify(
                    &record,
                    tolerance,
                    &budget,
                    u64::MAX,
                    false,
                    &mut ChargeTrace::disabled(),
                )
            });
            assert!(value.is_ok(), "{:?}", value.err());
            let bound = reserved(&record, shells, largest);
            assert!(peak <= bound, "peak {peak} > reservation {bound}");
        }
        // A refusal after the topology containers exist stays inside the
        // first-stage reservation that admitted them.
        let mut open = grid_boxes(2, 1);
        open.triangles.pop();
        open.triangle_primitives.pop();
        let (value, peak) = crate::analysis::test_alloc::peak_bytes(|| {
            clusters(&open, 0.0, &Budget::new(u64::MAX / 4), u64::MAX)
        });
        assert_eq!(value.unwrap_err().kind, BackendErrorKind::InvalidInput);
        assert!(peak <= scratch::admission(&open).bytes);
    }

    #[test]
    fn geode_sized_binary32_mesh_fits_the_entry_mesh_limit() {
        // Two closed grid cubes: 158 704 vertices and 317 400 triangles with
        // binary32 coordinates, the size of a 484 515-edge voxel geode.
        let record = grid_boxes(115, 2);
        assert_eq!(record.positions.len(), 158_704);
        assert_eq!(record.triangles.len(), 317_400);
        let earlier = earlier_reservation(&record);
        let now = reserved(&record, 2, 158_700);
        println!(
            "geode-sized reservation: earlier {earlier} B, now {now} B, limit {ENTRY_MESH_BYTES} B"
        );
        assert!(earlier > ENTRY_MESH_BYTES);
        assert!(now < ENTRY_MESH_BYTES);
        // One shell of every triangle is the largest per-shell stage.
        assert!(reserved(&record, 1, 317_400) < ENTRY_MESH_BYTES);
    }

    #[test]
    fn worst_case_exponents_refuse_a_mesh_that_binary32_coordinates_admit() {
        let record = grid_boxes(130, 2);
        let largest = record.triangles.len() as u64 / 2;
        assert!(reserved(&record, 2, largest) < ENTRY_MESH_BYTES);
        // The same structure scaled near the subnormal range carries
        // 1075-bit denominators, and the coordinate limbs alone refuse it.
        let mut subnormal = record;
        for p in &mut subnormal.positions {
            for c in p.iter_mut() {
                *c *= power(-1060);
            }
        }
        assert!(scratch::admission(&subnormal).bytes > ENTRY_MESH_BYTES);
        let limit = analysis_limit(
            clusters(
                &subnormal,
                0.0,
                &Budget::new(u64::MAX / 4),
                ENTRY_MESH_BYTES,
            )
            .unwrap_err(),
        );
        assert_eq!(limit.limit, ENTRY_MESH_BYTES);
        assert_eq!(limit.required, scratch::admission(&subnormal).bytes);
    }

    #[test]
    fn scratch_refusal_fires_at_each_stage_boundary() {
        let record = grid_boxes(2, 2);
        let admission = scratch::staged_admission(&record);
        let shells = scratch::staged_shells(admission, 96, 2, 48, 2);
        // The two boxes are apart: no candidate shell pair.
        let full = shells + scratch::staged_pairs(2, 0, 96);
        assert!(admission < full);
        let run = |limit| clusters(&record, 0.0, &Budget::new(u64::MAX / 4), limit);
        // Each stage names what it needs, the limit and the mesh it sized.
        for (limit, required) in [
            (admission - 1, admission),
            (admission, shells),
            (full - 1, full),
        ] {
            let error = run(limit).unwrap_err();
            let message = error.message.clone();
            let refusal = analysis_limit(error);
            assert_eq!(
                (refusal.limit, refusal.required, refusal.mesh),
                (limit, required, Some((52, 96))),
                "{message}"
            );
        }
        let error = run(admission - 1).unwrap_err();
        assert!(
            error.message.starts_with("Material exact scratch needs "),
            "{}",
            error.message
        );
        assert!(error.message.contains(&format!("({admission} bytes)")));
        let error = run(full - 1).unwrap_err();
        assert!(
            error.message.starts_with("Material pair scratch needs "),
            "{}",
            error.message
        );
        assert_eq!(run(full).unwrap().len(), 2);
        // A single large shell refuses a limit the earlier small one passed.
        assert!(scratch::staged_admission(&grid_boxes(8, 1)) > full);
        analysis_limit(
            clusters(&grid_boxes(8, 1), 0.0, &Budget::new(u64::MAX / 4), full).unwrap_err(),
        );
    }
    #[test]
    fn exact_diagonal_distance_is_not_an_axis_gap() {
        let (d, _) = point_segment(&p(0, 0, 0), &p(1, 1, 0), &p(1, 1, 1)).unwrap();
        assert_eq!(d, Q::from_integer(2.into()));
    }
}
