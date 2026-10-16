//! Staged material classification: topology first, exact geometry only
//! where it decides the verdict.
//!
//! The rational path in `material.rs` evaluates every predicate with exact
//! rationals. This path returns the same classification, refusal for refusal,
//! and spends exact work only on the steps that need it:
//! - Exact vertex identity is binary64 equality with zero's sign ignored,
//!   which is exact rational equality, so canonical vertices, edge uses,
//!   shells and vertex links are integer topology.
//! - Nondegeneracy, the folded-edge test and each shell's volume sign are
//!   signs of integer polynomials (`integer`), exact with no rationals.
//! - Self-intersection and shell proximity visit only pairs whose boxes the
//!   broad phase reports. A pair whose triangles an exact separating
//!   direction proves apart (or meeting only at their shared vertex) is
//!   settled there; every other pair runs the rational predicate unchanged.
//! - Shell pairs come from a sweep over shell boxes. Ray parity finds the
//!   directions a shell's planes forbid once per shell, as integer roots,
//!   and walks the broad-phase tree along the ray, so only triangles whose
//!   boxes the ray meets run the rational ray step.
//!
//! The rational predicates refuse when an intermediate value exceeds the
//! 8192-bit arithmetic guard. This path skips some of them, so it runs only
//! when no skipped predicate could have refused: every coordinate is an
//! integer multiple of `2^low` below `2^high` in magnitude with
//! `high - low <= MAX_SPAN` (`low <= 0 <= high`). The largest rational
//! intermediate, the squared distance from a ray hit to a triangle edge, has
//! a numerator and denominator of degree at most sixteen in those
//! coordinates, so with the ray parameter's at most 36-bit `k` and every
//! constant factor no guarded operand exceeds about 5000 bits. Other records
//! take the rational path.
//!
//! Work is charged before it runs and depends only on the record: one unit a
//! coordinate, triangle and vertex-link use, one per narrow-phase pair and
//! per rational ray step, one per triangle when a shell's forbidden ray
//! directions are found, and one per `BOX_TESTS_PER_UNIT` box tests of tree
//! construction, broad-phase queries, the shell sweep and ray walks.

use std::{cell::Cell, collections::BTreeMap};

use num_traits::Zero;

use super::{
    candidate, charge, depth, exact, find, integer, invalid, mul, proximity, ray_step, scratch,
    scratch_limit, self_pair, unit, unresolved, BoxTree, ChargeStage, ChargeTrace, Classification,
    MeshAnalysisRecord, Point, Triangle, Work, Q,
};
use crate::{
    analysis::{
        continuous::exact as arithmetic,
        mesh::{empty_aabb, exact::BOX_TESTS_PER_UNIT, expand, Aabb},
    },
    backend::{BackendError, BackendErrorKind},
    budget::Budget,
};

type Result<T> = std::result::Result<T, BackendError>;

/// The widest binary span, in bits, of coordinates this path accepts.
pub(super) const MAX_SPAN: i32 = 256;

/// Every coordinate is finite and the record's binary span is at most
/// `MAX_SPAN` bits, including the binary point.
pub(super) fn eligible(record: &MeshAnalysisRecord) -> bool {
    let (mut low, mut high) = (0i32, 0i32);
    for &value in record.positions.iter().flatten() {
        if !value.is_finite() {
            return false;
        }
        if let Some((mantissa, exponent)) = integer::decompose(value) {
            let width = 64 - mantissa.unsigned_abs().leading_zeros() as i32;
            low = low.min(exponent);
            high = high.max(exponent + width);
        }
    }
    high - low <= MAX_SPAN
}

/// One closed shell: its primitive, ascending triangles, orientation sign
/// and binary64 box, whose extrema are exact coordinates.
pub(super) struct Shell {
    primitive: u32,
    triangles: Vec<usize>,
    sign: bool,
    aabb: Aabb,
}

/// The decided relations of one candidate shell pair `a < b`.
#[derive(Clone, Copy)]
pub(super) struct Pair {
    a: usize,
    b: usize,
    contains_ab: bool,
    contains_ba: bool,
    near: bool,
    contact: bool,
}

/// Broad-phase box tests, one unit per `BOX_TESTS_PER_UNIT` before each
/// batch runs.
struct Meter(Cell<u64>);
impl Meter {
    fn tick(&self, work: &Work<'_>) -> Result<()> {
        let done = self.0.get();
        if done % BOX_TESTS_PER_UNIT == 0 {
            charge(work)?;
        }
        self.0.set(done + 1);
        Ok(())
    }
}

fn coordinates(record: &MeshAnalysisRecord, index: usize) -> [[f64; 3]; 3] {
    record.triangles[index].map(|vertex| record.positions[vertex as usize])
}

fn rational_point(p: [f64; 3]) -> Result<Point> {
    Ok([
        arithmetic::rational(p[0]).map_err(exact)?,
        arithmetic::rational(p[1]).map_err(exact)?,
        arithmetic::rational(p[2]).map_err(exact)?,
    ])
}

fn rational_triangle(record: &MeshAnalysisRecord, index: usize) -> Result<Triangle> {
    let [a, b, c] = coordinates(record, index);
    Ok([rational_point(a)?, rational_point(b)?, rational_point(c)?])
}

/// Exact box contact of two binary64 boxes: comparisons of exact values.
fn touching(a: Aabb, b: Aabb) -> bool {
    (0..3).all(|axis| a.min[axis] <= b.max[axis] && b.min[axis] <= a.max[axis])
}

/// Items whose boxes are candidates for `query` within `tolerance` and that
/// `keep` accepts, ascending.
fn overlapping(
    tree: &BoxTree,
    bounds: &[Aabb],
    query: Aabb,
    tolerance: f64,
    meter: &Meter,
    work: &Work<'_>,
    keep: impl Fn(usize) -> bool,
) -> Result<Vec<usize>> {
    let mut result = Vec::new();
    if tree.nodes.is_empty() {
        return Ok(result);
    }
    let mut stack = vec![0];
    while let Some(index) = stack.pop() {
        meter.tick(work)?;
        let node = &tree.nodes[index];
        if !candidate(query, node.bounds, tolerance) {
            continue;
        }
        if let Some(children) = node.children {
            stack.extend(children);
        } else {
            for &other in &tree.items[node.items.clone()] {
                meter.tick(work)?;
                if keep(other) && candidate(query, bounds[other], tolerance) {
                    result.push(other);
                }
            }
        }
    }
    result.sort_unstable();
    Ok(result)
}

fn exceeded(message: &'static str) -> impl Fn(crate::budget::BudgetExceeded) -> BackendError {
    move |error| BackendError {
        kind: BackendErrorKind::BudgetExceeded {
            limit: error.limit,
            used: error.used,
        },
        message: message.into(),
    }
}

pub(super) fn classify(
    record: &MeshAnalysisRecord,
    tolerance: f64,
    budget: &Budget,
    byte_limit: u64,
    regular_operand: bool,
    trace: &mut ChargeTrace,
) -> Result<Classification> {
    let work = Work {
        budget,
        trace: std::cell::RefCell::new(trace),
        stage: Cell::new(ChargeStage::BodySetup),
    };
    let budget = &work;
    let meter = Meter(Cell::new(0));
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(invalid(
            "Material tolerance must be finite and nonnegative.",
        ));
    }
    record.validate().map_err(invalid)?;
    let reservation = scratch::staged_admission(record);
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
        .map_err(exceeded(
            "Material coordinate work exceeds the declared budget.",
        ))?;
    // Equal binary64 coordinates, zero's sign ignored, are equal rationals.
    let key = |index: usize| record.positions[index].map(|c| (c + 0.0).to_bits());
    let mut order: Vec<usize> = (0..record.positions.len()).collect();
    order.sort_unstable_by(|&a, &b| key(a).cmp(&key(b)).then(a.cmp(&b)));
    let mut vertices: Vec<usize> = (0..record.positions.len()).collect();
    for run in order.chunk_by(|&a, &b| key(a) == key(b)) {
        for &index in run {
            vertices[index] = run[0];
        }
    }
    drop(order);
    let canonical = |index: usize| record.triangles[index].map(|v| vertices[v as usize]);
    let mut edges: Vec<scratch::EdgeUse> =
        Vec::with_capacity(record.triangles.len().saturating_mul(3));
    let mut parent: Vec<usize> = (0..record.triangles.len()).collect();
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
    let m = record.triangles.len() as u64;
    budget
        .charge(
            m.saturating_mul(u64::from(record.triangles.len().max(1).ilog2()) + 1)
                .div_ceil(BOX_TESTS_PER_UNIT),
        )
        .map_err(exceeded(
            "Material broad-phase construction exceeds the declared budget.",
        ))?;
    let tree = BoxTree::new(&triangle_bounds);
    budget.stage.set(ChargeStage::BodySetup);
    for (index, row) in record.triangles.iter().enumerate() {
        charge(budget)?;
        if integer::degenerate(&coordinates(record, index)) {
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
    let mut memberships: Vec<(usize, usize)> = Vec::with_capacity(record.triangles.len());
    for index in 0..record.triangles.len() {
        memberships.push((find(&mut parent, index), index));
    }
    memberships.sort_unstable();
    let runs = || memberships.chunk_by(|a, b| a.0 == b.0);
    let shell_count = runs().count();
    let shell_scratch = scratch::staged_shells(
        reservation,
        m,
        shell_count as u64,
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
    let mut shells: Vec<Shell> = Vec::with_capacity(shell_count);
    for members in runs() {
        let members: Vec<usize> = members.iter().map(|&(_, index)| index).collect();
        budget.stage.set(ChargeStage::BodySetup);
        // Every vertex link in a closed two-manifold shell is connected. Two
        // incident triangles are linked when they share another vertex, so
        // the link is connected exactly when the graph of the edges opposite
        // the vertex is: each use joins its two far vertices.
        let mut links: Vec<(usize, usize)> = Vec::with_capacity(members.len() * 3);
        for &index in &members {
            for v in canonical(index) {
                links.push((v, index));
            }
        }
        links.sort_unstable();
        for incident in links.chunk_by(|a, b| a.0 == b.0) {
            let vertex = incident[0].0;
            let mut far: Vec<(usize, usize)> = Vec::with_capacity(incident.len() * 2);
            for (use_index, &(_, index)) in incident.iter().enumerate() {
                charge(budget)?;
                for v in canonical(index) {
                    if v != vertex {
                        far.push((v, use_index));
                    }
                }
            }
            far.sort_unstable();
            let mut linked: Vec<usize> = (0..incident.len()).collect();
            for shared in far.chunk_by(|a, b| a.0 == b.0) {
                for pair in shared.windows(2) {
                    let left = find(&mut linked, pair[0].1);
                    let right = find(&mut linked, pair[1].1);
                    linked[left] = right;
                }
            }
            if (0..linked.len()).any(|index| find(&mut linked, index) != find(&mut linked, 0)) {
                return Err(invalid("A material shell has a disconnected vertex link."));
            }
        }
        drop(links);
        let origin = record.positions[record.triangles[members[0]][0] as usize];
        let least = integer::least(
            members
                .iter()
                .flat_map(|&index| record.triangles[index])
                .flat_map(|vertex| record.positions[vertex as usize].iter()),
        );
        let volume = integer::volume(
            || members.iter().map(|&index| coordinates(record, index)),
            &origin,
            least,
        );
        if volume == std::cmp::Ordering::Equal {
            return Err(invalid("Material shells require nonzero oriented volume."));
        }
        budget.stage.set(ChargeStage::FaceDistance);
        for &a in &members {
            let root = find(&mut parent, a);
            let near = overlapping(
                &tree,
                &triangle_bounds,
                triangle_bounds[a],
                0.0,
                &meter,
                budget,
                |b| b > a,
            )?;
            for b in near {
                if find(&mut parent, b) != root {
                    continue;
                }
                charge(budget)?;
                if !touching(triangle_bounds[a], triangle_bounds[b]) {
                    continue;
                }
                let (ca, cb) = (canonical(a), canonical(b));
                let mut shared = None;
                let mut count = 0;
                for (i, v) in ca.iter().enumerate() {
                    if let Some(j) = cb.iter().position(|w| w == v) {
                        shared = Some((i, j));
                        count += 1;
                    }
                }
                let (pa, pb) = (coordinates(record, a), coordinates(record, b));
                if count == 2 {
                    if integer::folded(&pa, &pb) {
                        return Err(invalid(
                            "Adjacent material triangles overlap in their plane.",
                        ));
                    }
                    continue;
                }
                if count <= 1 && integer::apart(&pa, &pb, shared) {
                    continue;
                }
                self_pair(
                    &rational_triangle(record, a)?,
                    &rational_triangle(record, b)?,
                )?;
            }
        }
        let mut aabb = empty_aabb();
        for &index in &members {
            expand(&mut aabb, triangle_bounds[index].min);
            expand(&mut aabb, triangle_bounds[index].max);
        }
        shells.push(Shell {
            primitive: record.triangle_primitives[members[0]],
            triangles: members,
            sign: volume == std::cmp::Ordering::Greater,
            aabb,
        });
    }
    // Shells are final: each triangle's parent now names its shell.
    for (shell_index, shell) in shells.iter().enumerate() {
        for &index in &shell.triangles {
            parent[index] = shell_index;
        }
    }
    let shell_of = parent;
    let count = shells.len();
    budget.stage.set(ChargeStage::FaceDistance);
    // Shell pairs whose boxes are candidates, by a sweep along x: sorted by
    // minimum x, a shell meets later shells only until their minimum passes
    // its widened maximum.
    let mut sweep: Vec<usize> = (0..count).collect();
    sweep.sort_unstable_by(|&a, &b| {
        shells[a].aabb.min[0]
            .total_cmp(&shells[b].aabb.min[0])
            .then(a.cmp(&b))
    });
    let visit = |emit: &mut dyn FnMut(usize, usize)| -> Result<()> {
        for (position, &a) in sweep.iter().enumerate() {
            let reach = (shells[a].aabb.max[0] + tolerance).next_up();
            for &b in &sweep[position + 1..] {
                meter.tick(budget)?;
                if shells[b].aabb.min[0] > reach {
                    break;
                }
                if candidate(shells[a].aabb, shells[b].aabb, tolerance) {
                    emit(a.min(b), a.max(b));
                }
            }
        }
        Ok(())
    };
    let mut candidates = 0u64;
    visit(&mut |_, _| candidates += 1)?;
    let pair_scratch = scratch::staged_pairs(count as u64, candidates, m);
    if reservation.saturating_add(pair_scratch) > byte_limit {
        return Err(scratch_limit(
            "Material pair scratch",
            record,
            reservation.saturating_add(pair_scratch),
            byte_limit,
        ));
    }
    budget.trace.borrow_mut().limit_to_bytes(
        byte_limit
            .saturating_sub(reservation)
            .saturating_sub(pair_scratch),
    );
    let mut pairs: Vec<Pair> = Vec::with_capacity(candidates as usize);
    visit(&mut |a, b| {
        pairs.push(Pair {
            a,
            b,
            contains_ab: false,
            contains_ba: false,
            near: false,
            contact: false,
        })
    })?;
    drop(sweep);
    pairs.sort_unstable_by_key(|pair| (pair.a, pair.b));
    let threshold = arithmetic::rational(tolerance).map_err(exact)?;
    let threshold = mul(&threshold, &threshold)?;
    let mut rays = Rays {
        record,
        tree: &tree,
        bounds: &triangle_bounds,
        shell_of: &shell_of,
        meter: &meter,
        parallel: (0..count).map(|_| None).collect(),
    };
    for pair in &mut pairs {
        let (a, b) = (pair.a, pair.b);
        budget.stage.set(ChargeStage::FaceDistance);
        charge(budget)?;
        let corners = |aabb: Aabb| -> Result<Triangle> {
            let low = rational_point(aabb.min)?;
            let high = rational_point(aabb.max)?;
            Ok([low, high.clone(), high])
        };
        if super::box_distance(&corners(shells[a].aabb)?, &corners(shells[b].aabb)?)? > threshold {
            continue;
        }
        let mut boundary_contact = false;
        'shell: for &i in &shells[a].triangles {
            let near = overlapping(
                &tree,
                &triangle_bounds,
                triangle_bounds[i],
                tolerance,
                &meter,
                budget,
                |j| shell_of[j] == b,
            )?;
            for j in near {
                charge(budget)?;
                let (pi, pj) = (coordinates(record, i), coordinates(record, j));
                if threshold.is_zero() {
                    let (ci, cj) = (canonical(i), canonical(j));
                    if !ci.iter().any(|v| cj.contains(v)) && integer::apart(&pi, &pj, None) {
                        // Apart closed triangles are at a positive distance.
                        continue;
                    }
                }
                let Some(squared) = proximity(
                    &rational_triangle(record, i)?,
                    &rational_triangle(record, j)?,
                    &threshold,
                )?
                else {
                    continue;
                };
                boundary_contact |= squared.is_zero();
                pair.near |= squared <= threshold;
                if boundary_contact {
                    // Contact is also nearness; nothing later changes either.
                    break 'shell;
                }
            }
        }
        if !boundary_contact {
            let first =
                |shell: &Shell| record.positions[record.triangles[shell.triangles[0]][0] as usize];
            pair.contains_ab = inside(first(&shells[b]), a, &shells[a], &mut rays, budget)?;
            pair.contains_ba = inside(first(&shells[a]), b, &shells[b], &mut rays, budget)?;
            if pair.contains_ab && pair.contains_ba {
                return Err(invalid("Material shell nesting is ambiguous."));
            }
        }
        pair.contact = boundary_contact;
    }
    let contains = |container: usize, child: usize| -> bool {
        let key = (container.min(child), container.max(child));
        pairs
            .binary_search_by_key(&key, |pair| (pair.a, pair.b))
            .is_ok_and(|found| {
                let pair = &pairs[found];
                if container < child {
                    pair.contains_ab
                } else {
                    pair.contains_ba
                }
            })
    };
    // (child, container) for every containment, children ascending and each
    // child's containers ascending.
    let mut containment: Vec<(usize, usize)> = Vec::with_capacity(pairs.len() * 2);
    for pair in &pairs {
        if pair.contains_ab {
            containment.push((pair.b, pair.a));
        }
        if pair.contains_ba {
            containment.push((pair.a, pair.b));
        }
    }
    containment.sort_unstable();
    let containers = |child: usize| -> &[(usize, usize)] {
        let start = containment.partition_point(|&(c, _)| c < child);
        let end = containment.partition_point(|&(c, _)| c <= child);
        &containment[start..end]
    };
    let mut nesting = vec![None; count];
    for (child, slot) in nesting.iter_mut().enumerate() {
        let candidates: Vec<_> = containers(child)
            .iter()
            .map(|&(_, a)| a)
            .filter(|&a| shells[a].primitive == shells[child].primitive)
            .collect();
        let nearest: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|&a| !candidates.iter().any(|&b| a != b && contains(a, b)))
            .collect();
        if nearest.len() > 1 {
            return Err(unresolved(
                "Crossing containers require complete material-cell classification.",
            ));
        }
        *slot = nearest.first().copied();
    }
    let mut parent: Vec<_> = (0..count).collect();
    for pair in &pairs {
        let (a, b) = (pair.a, pair.b);
        if pair.contact
            && shells[a].primitive == shells[b].primitive
            && nesting[a].is_none()
            && nesting[b].is_none()
            && shells[a].sign != shells[b].sign
        {
            return Err(unresolved("Intersecting opposite-oriented root shells require complete material-cell classification."));
        }
    }
    for child in 0..count {
        if let Some(container) = nesting[child] {
            if shells[child].sign != shells[container].sign {
                let depth = depth(child, &nesting)?;
                if depth % 2 == 1 {
                    parent[child] = container;
                }
            } else {
                return Err(unresolved("Nested shells with inconsistent relative orientation require explicit material-cell classification."));
            }
        }
    }
    if regular_operand && pairs.iter().any(|pair| pair.contact) {
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
    for child in 0..count {
        if depth(child, &nesting)? % 2 == 1 {
            continue;
        }
        let mut deepest = BTreeMap::<u32, (usize, usize)>::new();
        for &(_, container) in containers(child) {
            charge(budget)?;
            if shells[container].primitive == shells[child].primitive {
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
    for pair in &pairs {
        if pair.near {
            let left = find(&mut parent, pair.a);
            let right = find(&mut parent, pair.b);
            parent[left] = right;
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

/// The broad phase and shell map a ray-parity pass reads.
struct Rays<'a> {
    record: &'a MeshAnalysisRecord,
    tree: &'a BoxTree,
    bounds: &'a [Aabb],
    shell_of: &'a [usize],
    meter: &'a Meter,
    /// Per shell, once computed: every `k` whose direction is parallel to
    /// one of its triangles' planes, ascending.
    parallel: Vec<Option<Vec<u64>>>,
}

/// Ray parity of `p` against shell `index`, as the rational path decides
/// it. The rational path refuses direction `k` when any triangle's plane is
/// parallel to it; those `k` are the integer roots of one quadratic per
/// triangle, found once per shell. Of the remaining triangles only those
/// whose boxes the ray meets can be crossed, so the ray walks the broad-phase
/// tree, skipping every node box it exactly misses, and each triangle it
/// reaches runs the rational ray step.
fn inside(
    p: [f64; 3],
    index: usize,
    shell: &Shell,
    rays: &mut Rays<'_>,
    budget: &Work<'_>,
) -> Result<bool> {
    budget.stage.set(ChargeStage::NestedClassify);
    let attempts = shell.triangles.len().saturating_mul(8).saturating_add(1);
    if rays.parallel[index].is_none() {
        let mut parallel = Vec::with_capacity(shell.triangles.len() * 2);
        for &triangle in &shell.triangles {
            charge(budget)?;
            parallel.extend(integer::parallel_directions(
                &coordinates(rays.record, triangle),
                attempts as u64,
            ));
        }
        parallel.sort_unstable();
        parallel.dedup();
        rays.parallel[index] = Some(parallel);
    }
    let point = rational_point(p)?;
    for k in 1..=attempts {
        if rays.parallel[index]
            .as_ref()
            .is_some_and(|parallel| parallel.binary_search(&(k as u64)).is_ok())
        {
            continue;
        }
        let kq = Q::from_integer(k.into());
        let direction = [unit(), kq.clone(), mul(&kq, &kq)?];
        let mut crossings = 0;
        let mut ambiguous = false;
        let mut stack = vec![0];
        'walk: while let Some(node) = stack.pop() {
            rays.meter.tick(budget)?;
            let node = &rays.tree.nodes[node];
            if integer::misses(&p, &node.bounds.min, &node.bounds.max, k as i64) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
                continue;
            }
            for &triangle in &rays.tree.items[node.items.clone()] {
                rays.meter.tick(budget)?;
                let bounds = rays.bounds[triangle];
                if rays.shell_of[triangle] != index
                    || integer::misses(&p, &bounds.min, &bounds.max, k as i64)
                {
                    continue;
                }
                charge(budget)?;
                match ray_step(
                    &point,
                    &direction,
                    &rational_triangle(rays.record, triangle)?,
                )? {
                    None => {
                        ambiguous = true;
                        break 'walk;
                    }
                    Some(true) => crossings += 1,
                    Some(false) => {}
                }
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

#[cfg(test)]
pub(super) mod tests {
    use super::super::classify_rational;
    use super::*;
    use crate::analysis::mesh::Primitive;

    /// A deterministic generator: xorshift64*.
    pub(crate) struct Rng(pub(crate) u64);
    impl Rng {
        pub(crate) fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        pub(crate) fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        /// A multiple of `1 / 8` in `[-n, n]`.
        fn grid(&mut self, n: i64) -> f64 {
            (self.below((16 * n + 1) as u64) as i64 - 8 * n) as f64 / 8.0
        }
    }

    fn empty() -> MeshAnalysisRecord {
        MeshAnalysisRecord {
            positions: Vec::new(),
            triangles: Vec::new(),
            triangle_primitives: Vec::new(),
            primitives: Vec::new(),
        }
    }

    /// Appends a closed shell from local vertices and outward faces.
    fn shell(
        record: &mut MeshAnalysisRecord,
        points: &[[f64; 3]],
        faces: &[[u32; 3]],
        inward: bool,
        primitive: u32,
    ) {
        let start = record.positions.len() as u32;
        record.positions.extend_from_slice(points);
        for face in faces {
            let [a, b, c] = face.map(|v| v + start);
            record
                .triangles
                .push(if inward { [c, b, a] } else { [a, b, c] });
            record.triangle_primitives.push(primitive);
        }
        while record.primitives.len() <= primitive as usize {
            record.primitives.push(Primitive {
                name: "random".into(),
                vertex_start: 0,
                vertex_count: 0,
            });
        }
    }

    const CUBE: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    const OCTAHEDRON: [[u32; 3]; 8] = [
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];

    fn cuboid(low: [f64; 3], high: [f64; 3]) -> Vec<[f64; 3]> {
        (0..8)
            .map(|i| {
                let x = if (i & 1) ^ ((i >> 1) & 1) == 1 {
                    high[0]
                } else {
                    low[0]
                };
                let y = if i & 2 == 2 { high[1] } else { low[1] };
                let z = if i & 4 == 4 { high[2] } else { low[2] };
                [x, y, z]
            })
            .collect()
    }

    /// Two to five random shells: boxes on a coarse grid (so faces touch,
    /// coincide, cross and nest), jittered octahedra with oblique faces and
    /// subdivided boxes, each outward or inward in a random primitive.
    pub(crate) fn random_record(rng: &mut Rng) -> MeshAnalysisRecord {
        let mut record = empty();
        let shells = 2 + rng.below(4);
        let primitives = 1 + rng.below(3) as u32;
        for _ in 0..shells {
            let inward = rng.below(4) == 0;
            let primitive = rng.below(u64::from(primitives)) as u32;
            match rng.below(3) {
                0 => {
                    let low = [0, 1, 2].map(|_| rng.below(5) as f64 - 2.0);
                    let high = low.map(|v| v + 1.0 + rng.below(3) as f64);
                    shell(&mut record, &cuboid(low, high), &CUBE, inward, primitive);
                }
                1 => {
                    let centre = [0, 1, 2].map(|_| rng.grid(2));
                    let radius = 0.5 + rng.below(4) as f64 * 0.5;
                    let mut points = Vec::new();
                    for axis in 0..3 {
                        for sign in [1.0, -1.0] {
                            let mut p = centre;
                            p[axis] += sign * radius;
                            for c in &mut p {
                                *c += rng.grid(1) / 4.0;
                            }
                            points.push(p);
                        }
                    }
                    let [a, b, c, d, e, f] = [0, 1, 2, 3, 4, 5].map(|i| points[i]);
                    // +x, -x, +y, -y, +z, -z in OCTAHEDRON's vertex order.
                    shell(
                        &mut record,
                        &[a, b, c, d, e, f],
                        &OCTAHEDRON,
                        inward,
                        primitive,
                    );
                }
                _ => {
                    let mut sub = empty();
                    let origin = [0, 1, 2].map(|_| rng.below(5) as f32 - 2.0);
                    super::super::tests::grid_box(&mut sub, 2, origin, 0.5, 0);
                    let faces: Vec<[u32; 3]> = sub.triangles.clone();
                    shell(&mut record, &sub.positions, &faces, inward, primitive);
                }
            }
        }
        record
    }

    /// One random defect, or none: an open edge, a flipped or duplicated
    /// triangle, a moved vertex or a collapsed triangle.
    pub(crate) fn mutate(rng: &mut Rng, record: &mut MeshAnalysisRecord) {
        match rng.below(8) {
            0 => {
                let index = rng.below(record.triangles.len() as u64) as usize;
                record.triangles.remove(index);
                record.triangle_primitives.remove(index);
            }
            1 => {
                let index = rng.below(record.triangles.len() as u64) as usize;
                record.triangles[index].swap(0, 1);
            }
            2 => {
                let index = rng.below(record.positions.len() as u64) as usize;
                for c in &mut record.positions[index] {
                    *c += rng.grid(2);
                }
            }
            3 => {
                let index = rng.below(record.triangles.len() as u64) as usize;
                let [a, b, _] = record.triangles[index];
                record.positions[b as usize] = record.positions[a as usize];
            }
            _ => {}
        }
    }

    fn compare(
        record: &MeshAnalysisRecord,
        tolerance: f64,
        regular: bool,
    ) -> Result<Classification> {
        let run = |staged: bool| {
            let budget = Budget::new(u64::MAX / 4);
            let mut trace = ChargeTrace::disabled();
            if staged {
                assert!(eligible(record));
                classify(record, tolerance, &budget, u64::MAX, regular, &mut trace)
            } else {
                classify_rational(record, tolerance, &budget, u64::MAX, regular, &mut trace)
            }
        };
        let staged = run(true);
        assert_eq!(
            staged,
            run(false),
            "tolerance {tolerance}, regular {regular}: {record:?}"
        );
        staged
    }

    #[test]
    fn random_records_match_the_rational_reference() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut outcomes = BTreeMap::<String, usize>::new();
        // The rational reference is slow unoptimized; a release run covers
        // the full corpus.
        let cases = if cfg!(debug_assertions) { 6 } else { 400 };
        for case in 0..cases {
            let mut record = random_record(&mut rng);
            if case % 2 == 1 {
                mutate(&mut rng, &mut record);
            }
            let tolerance = [0.0, 0.0, 0.125, 1.0][rng.below(4) as usize];
            let regular = rng.below(5) == 0;
            let _ = compare(&record, tolerance, regular);
            let budget = Budget::new(u64::MAX / 4);
            let outcome = match classify(
                &record,
                tolerance,
                &budget,
                u64::MAX,
                regular,
                &mut ChargeTrace::disabled(),
            ) {
                Ok((groups, _, _)) => format!("{} groups", groups.len()),
                Err(error) => error.message,
            };
            *outcomes.entry(outcome).or_default() += 1;
        }
        println!("{outcomes:#?}");
        // The corpus reaches verdicts and several distinct refusals.
        if !cfg!(debug_assertions) {
            assert!(outcomes.keys().filter(|k| k.ends_with("groups")).count() >= 3);
            assert!(outcomes.len() >= 6, "{outcomes:#?}");
        }
    }

    /// Runs one classification on a fresh budget, with its wall time, work
    /// units and peak heap.
    fn measure(
        record: &MeshAnalysisRecord,
        units: u64,
        byte_limit: u64,
    ) -> (Result<usize>, std::time::Duration, u64, u64) {
        let budget = Budget::new(units);
        let start = std::time::Instant::now();
        let (value, peak) = crate::analysis::test_alloc::peak_bytes(|| {
            super::super::clusters(record, 0.0, &budget, byte_limit).map(|groups| groups.len())
        });
        (value, start.elapsed(), budget.used(), peak)
    }

    /// A hollow grid box with an island grid box in its cavity: two
    /// components, with shell pairs that need ray-parity classification.
    fn nested_voxels(n: u32) -> MeshAnalysisRecord {
        let mut record = empty();
        let mut add = |n: u32, origin: [f32; 3], step: f32, inward: bool, primitive: u32| {
            let mut sub = empty();
            super::super::tests::grid_box(&mut sub, n, origin, step, 0);
            shell(
                &mut record,
                &sub.positions,
                &sub.triangles,
                inward,
                primitive,
            );
        };
        add(n, [12.5, -7.25, 3.1], 0.1, false, 0);
        add(
            n / 2,
            [
                12.5 + 0.1 * (n / 4) as f32,
                -7.25 + 0.1 * (n / 4) as f32,
                3.1 + 0.1 * (n / 4) as f32,
            ],
            0.1,
            true,
            0,
        );
        add(
            n / 8,
            [
                12.5 + 0.1 * (n / 2 - n / 16) as f32,
                -7.25 + 0.1 * (n / 2 - n / 16) as f32,
                3.1 + 0.1 * (n / 2 - n / 16) as f32,
            ],
            0.1,
            false,
            1,
        );
        record
    }

    /// Acceptance: closed binary32 voxel meshes of about 323 000 triangles in
    /// two components fit the default 8 000 000 work units and the entry
    /// mesh byte limit. Run with `--release --ignored`.
    #[test]
    #[ignore = "release-mode capacity measurement"]
    fn voxel_meshes_of_323k_triangles_fit_the_default_budget() {
        const DEFAULT_UNITS: u64 = 8_000_000;
        const ENTRY_MESH_BYTES: u64 = 256 * 1024 * 1024;
        for (name, record) in [
            ("two grid boxes", super::super::tests::grid_boxes(116, 2)),
            ("box, cavity and island", nested_voxels(146)),
        ] {
            let (value, elapsed, units, peak) = measure(&record, DEFAULT_UNITS, ENTRY_MESH_BYTES);
            println!(
                "{name}: {} vertices, {} triangles, {:?}, {elapsed:?}, {units} units, peak {peak} B",
                record.positions.len(),
                record.triangles.len(),
                value
            );
            assert_eq!(value.unwrap(), 2);
            assert!(units <= DEFAULT_UNITS);
        }
    }

    /// Acceptance: about two million triangles under a raised host limit.
    #[test]
    #[ignore = "release-mode capacity measurement"]
    fn two_million_triangles_classify_under_a_raised_limit() {
        let record = super::super::tests::grid_boxes(289, 2);
        let (value, elapsed, units, peak) = measure(&record, u64::MAX / 4, u64::MAX);
        println!(
            "two grid boxes: {} vertices, {} triangles, {:?}, {elapsed:?}, {units} units, peak {peak} B, reservation {} B",
            record.positions.len(),
            record.triangles.len(),
            value,
            scratch::staged_shells(
                scratch::staged_admission(&record),
                record.triangles.len() as u64,
                2,
                record.triangles.len() as u64 / 2,
                2
            ) + scratch::staged_pairs(2, 0, record.triangles.len() as u64)
        );
        assert_eq!(value.unwrap(), 2);
    }

    const TETRAHEDRON: [[u32; 3]; 4] = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];

    fn tetrahedron(offset: [f64; 3]) -> Vec<[f64; 3]> {
        [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            .map(|p| std::array::from_fn(|axis| p[axis] + offset[axis]))
            .to_vec()
    }

    /// Named structures for each refusal the random corpus rarely reaches,
    /// and coordinates near the edge of the staged span.
    fn structured_records() -> Vec<(&'static str, MeshAnalysisRecord)> {
        let mut cases = Vec::new();
        let mut bowtie = empty();
        shell(&mut bowtie, &tetrahedron([0.0; 3]), &TETRAHEDRON, false, 0);
        shell(
            &mut bowtie,
            &tetrahedron([-1.0, 0.0, 0.0]),
            &TETRAHEDRON,
            false,
            0,
        );
        cases.push(("vertex-only contact in one primitive", bowtie.clone()));
        bowtie.triangle_primitives[4..].fill(1);
        bowtie.primitives.push(Primitive {
            name: "other".into(),
            vertex_start: 0,
            vertex_count: 0,
        });
        cases.push(("vertex-only contact across primitives", bowtie));
        let mut flat = empty();
        shell(
            &mut flat,
            &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            &[[0, 1, 2], [0, 2, 1]],
            false,
            0,
        );
        cases.push(("two-sided flat shell", flat));
        let mut folded = empty();
        let mut points = cuboid([0.0; 3], [2.0; 3]);
        points[7] = [2.0, 0.0, 2.0];
        points.push([0.0; 3]);
        shell(&mut folded, &points[..8], &CUBE, false, 0);
        cases.push(("top face folded over its diagonal", folded));
        let mut nested = empty();
        shell(&mut nested, &cuboid([0.0; 3], [6.0; 3]), &CUBE, false, 0);
        shell(&mut nested, &cuboid([1.0; 3], [5.0; 3]), &CUBE, true, 0);
        shell(&mut nested, &cuboid([2.0; 3], [3.0; 3]), &CUBE, false, 0);
        shell(&mut nested, &cuboid([3.5; 3], [4.5; 3]), &CUBE, false, 1);
        cases.push(("box, cavity and islands", nested.clone()));
        let mut inverted = nested;
        inverted.triangles[12..24]
            .iter_mut()
            .for_each(|t| t.swap(0, 2));
        cases.push(("cavity with an inconsistent orientation", inverted));
        let mut crossing = empty();
        shell(&mut crossing, &cuboid([0.0; 3], [6.0; 3]), &CUBE, false, 0);
        shell(&mut crossing, &cuboid([1.0; 3], [4.0; 3]), &CUBE, true, 0);
        shell(&mut crossing, &cuboid([2.0; 3], [5.0; 3]), &CUBE, true, 0);
        shell(&mut crossing, &cuboid([2.5; 3], [3.5; 3]), &CUBE, false, 0);
        cases.push(("island inside two crossing cavities", crossing));
        let mut grids = empty();
        for (index, origin) in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 0.5, 2.25]]
            .into_iter()
            .enumerate()
        {
            let mut sub = empty();
            super::super::tests::grid_box(&mut sub, 2, origin, 0.5, 0);
            shell(
                &mut grids,
                &sub.positions,
                &sub.triangles,
                false,
                index as u32,
            );
        }
        cases.push(("face-sharing and offset grid boxes", grids));
        // A box near 2^61 around a box near 2^-150: a binary span of about
        // 215 bits, inside the staged span, with ray parity across it.
        let tiny = 2f64.powi(-150);
        let mut span = empty();
        shell(
            &mut span,
            &cuboid([-(2f64.powi(60)); 3], [2f64.powi(60); 3]),
            &CUBE,
            false,
            0,
        );
        shell(
            &mut span,
            &cuboid([tiny; 3], [3.0 * tiny; 3]),
            &CUBE,
            false,
            1,
        );
        cases.push(("wide binary span", span));
        cases
    }

    #[test]
    fn structured_records_match_the_rational_reference() {
        let mut outcomes = BTreeMap::new();
        for (name, record) in structured_records() {
            assert!(eligible(&record), "{name}");
            // Unoptimized runs check one configuration of each record.
            let tolerances: &[f64] = if cfg!(debug_assertions) {
                &[0.0]
            } else {
                &[0.0, 0.75]
            };
            let regulars: &[bool] = if cfg!(debug_assertions) {
                &[false]
            } else {
                &[false, true]
            };
            for &tolerance in tolerances {
                for &regular in regulars {
                    let outcome = match compare(&record, tolerance, regular) {
                        Ok((groups, _, _)) => format!("{} groups", groups.len()),
                        Err(error) => error.message,
                    };
                    outcomes.insert((name, tolerance.to_bits(), regular), outcome);
                }
            }
        }
        println!("{outcomes:#?}");
        let messages: std::collections::BTreeSet<_> = outcomes.values().collect();
        for expected in [
            "Material shells require nonzero oriented volume.",
            "Nested shells with inconsistent relative orientation require explicit material-cell classification.",
            "Crossing containers require complete material-cell classification.",
        ] {
            assert!(
                messages.iter().any(|m| m.as_str() == expected),
                "{expected}: {messages:#?}"
            );
        }
    }

    /// Relabels vertices and triangles with seeded permutations.
    fn permuted(record: &MeshAnalysisRecord, rng: &mut Rng) -> (MeshAnalysisRecord, Vec<usize>) {
        let shuffle = |n: usize, rng: &mut Rng| {
            let mut order: Vec<usize> = (0..n).collect();
            for i in (1..n).rev() {
                order.swap(i, rng.below(i as u64 + 1) as usize);
            }
            order
        };
        let vertices = shuffle(record.positions.len(), rng);
        let triangles = shuffle(record.triangles.len(), rng);
        let mut position = vec![0u32; vertices.len()];
        for (new, &old) in vertices.iter().enumerate() {
            position[old] = new as u32;
        }
        let mut out = empty();
        out.primitives = record.primitives.clone();
        out.positions = vertices.iter().map(|&old| record.positions[old]).collect();
        for &old in &triangles {
            let mut t = record.triangles[old].map(|v| position[v as usize]);
            t.rotate_left(rng.below(3) as usize);
            out.triangles.push(t);
            out.triangle_primitives
                .push(record.triangle_primitives[old]);
        }
        (out, triangles)
    }

    /// Groups as sets of original triangle indexes, independent of order.
    fn partition(groups: &[Vec<usize>], original: &[usize]) -> Vec<Vec<usize>> {
        let mut sets: Vec<Vec<usize>> = groups
            .iter()
            .map(|group| {
                let mut set: Vec<usize> = group.iter().map(|&t| original[t]).collect();
                set.sort_unstable();
                set
            })
            .collect();
        sets.sort();
        sets
    }

    /// Coverage item 4: connectivity across partition boundaries equals the
    /// monolithic equivalent. A chain of face-touching grid boxes, each its
    /// own primitive, spreads its contacts across broad-phase leaves and is
    /// one body exactly as one long box is; a gap splits both alike unless
    /// the tolerance spans it. Relabelling vertices and triangles, which
    /// moves every broad-phase partition, changes no component.
    #[test]
    fn connectivity_across_partitions_matches_the_monolithic_equivalent() {
        let chain = |links: u32, gap: f32| {
            let mut record = empty();
            for index in 0..links {
                let mut sub = empty();
                let x = index as f32 + if index >= links / 2 { gap } else { 0.0 };
                super::super::tests::grid_box(&mut sub, 2, [x, 0.0, 0.0], 0.5, 0);
                shell(&mut record, &sub.positions, &sub.triangles, false, index);
            }
            record
        };
        let monolith = |gap: f32| {
            let mut record = empty();
            let left = cuboid([0.0; 3], [4.0, 1.0, 1.0]);
            let right = cuboid(
                [4.0 + f64::from(gap), 0.0, 0.0],
                [8.0 + f64::from(gap), 1.0, 1.0],
            );
            shell(&mut record, &left, &CUBE, false, 0);
            shell(&mut record, &right, &CUBE, false, 1);
            record
        };
        let count = |record: &MeshAnalysisRecord, tolerance: f64| {
            super::super::clusters(record, tolerance, &Budget::new(u64::MAX / 4), u64::MAX)
                .unwrap()
                .len()
        };
        for (gap, tolerance, expected) in [(0.0, 0.0, 1), (0.25, 0.0, 2), (0.25, 0.25, 1)] {
            let partitioned = chain(8, gap);
            assert_eq!(count(&partitioned, tolerance), expected, "gap {gap}");
            assert_eq!(count(&monolith(gap), tolerance), expected, "gap {gap}");
            if !cfg!(debug_assertions) {
                let _ = compare(&partitioned, tolerance, false);
            }
        }
        let mut rng = Rng(0x5eed);
        for case in 0..if cfg!(debug_assertions) { 2 } else { 60 } {
            let record = if case % 2 == 0 {
                chain(6, 0.25)
            } else {
                random_record(&mut rng)
            };
            let Ok(base) =
                super::super::clusters(&record, 0.25, &Budget::new(u64::MAX / 4), u64::MAX)
            else {
                continue;
            };
            let (moved, original) = permuted(&record, &mut rng);
            let identity: Vec<usize> = (0..record.triangles.len()).collect();
            let moved_groups =
                super::super::clusters(&moved, 0.25, &Budget::new(u64::MAX / 4), u64::MAX).unwrap();
            assert_eq!(
                partition(&moved_groups, &original),
                partition(&base, &identity)
            );
        }
    }

    /// Coverage item 7: an enabled, disabled or undersized charge trace, and
    /// a retained or rebuilt batch result, give identical verdicts and work.
    #[test]
    fn trace_and_retention_leave_verdicts_and_work_unchanged() {
        let record = super::super::tests::grid_boxes(6, 3);
        let run = |trace: &mut ChargeTrace, byte_limit: u64| {
            let budget = Budget::new(u64::MAX / 4);
            let value =
                classify(&record, 0.0, &budget, byte_limit, false, trace).map(|value| value.0);
            (value, budget.used())
        };
        let (disabled, units) = run(&mut ChargeTrace::disabled(), u64::MAX);
        let mut full = ChargeTrace::default();
        assert_eq!(run(&mut full, u64::MAX), (disabled.clone(), units));
        assert!(full.complete);
        assert_eq!(full.stage_units.iter().sum::<u64>(), units);
        assert_eq!(
            crate::analysis::mesh::exact::atomic_steps(&full.steps)
                .map(|step| step.units)
                .sum::<u64>(),
            units
        );
        // Exactly the reservation leaves no trace storage: the trace is
        // abandoned, and nothing else changes.
        let reservation = scratch::staged_shells(
            scratch::staged_admission(&record),
            record.triangles.len() as u64,
            3,
            432,
            3,
        ) + scratch::staged_pairs(3, 0, record.triangles.len() as u64);
        let mut starved = ChargeTrace::default();
        assert_eq!(run(&mut starved, reservation), (disabled.clone(), units));
        assert!(!starved.complete);
        // The same work in a batch, cold, warm from the retained trace, and
        // rebuilt where the trace could not be retained.
        let analysis = crate::analysis::mesh::analyze(&std::rc::Rc::new(record.clone()));
        for byte_limit in [256 * 1024 * 1024, reservation] {
            let limits = crate::backend::AnalysisRetentionLimits {
                max_mesh_bytes: byte_limit,
                max_mesh_entries: 1,
                max_solid_entries: 0,
            };
            let batch = crate::analysis::batch::BatchAnalysis::new(
                [("voxels".into(), 0.0f64.to_bits())],
                limits,
            )
            .unwrap();
            let slot = std::cell::OnceCell::new();
            let mut seen = Vec::new();
            for _ in 0..3 {
                let budget = Budget::new(u64::MAX / 4);
                let value = batch
                    .connected_components("voxels", 0.0, &analysis, &budget, &slot)
                    .unwrap();
                seen.push((value.count, budget.used()));
            }
            assert_eq!(seen, vec![(3, units); 3], "byte limit {byte_limit}");
        }
    }
}
