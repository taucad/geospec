//! The heap bytes `MeshAnalysis::watertight` allocates, each requirement
//! checked against the analysis byte limit before the allocation it counts.
//!
//! The weld and the edge table grow with the mesh, so a `Meter` checks every
//! growth of their tables, lists and maps before it happens: the bytes then
//! live, the block being replaced and the new block. Everything after the
//! edge table has an upper bound, `completion`, from the counts of open and
//! non-manifold edges, checked before any of it is allocated. The largest
//! requirement checked is the analysis's reservation: the least limit under
//! which it decides. Growth rules:
//! - a list the meter grows doubles, to at least four slots, by an exact
//!   reservation, so its old and new blocks are both live while it moves;
//! - a vector grown by `push` to `len` holds at most `3 * len + 4` slots at
//!   any instant, counting both blocks of a moving reallocation;
//! - a std `HashMap` or `HashSet` of capacity `c` holds `c + 1` buckets below
//!   eight and `c / 7 * 8` from eight up (hashbrown's 7/8 load factor), grows
//!   only when an insert finds it full, then into twice the buckets (sixteen
//!   from empty), and holds the old table while it moves. One built from `n`
//!   inserts has at most `buckets(n + 1)` buckets.
//!
//! The counting allocator tests below measure the peak live heap of whole
//! analyses against the reservation.

use std::mem::size_of;

use super::{
    CellKey, Edge, IrregularEdgeCluster, IrregularEdgeSample, MeshAnalysisRecord, Watertight,
    WatertightPrimitiveBreakdown,
};
use crate::backend::{BackendError, CapacityLimit, LimitUnit};

const WORD: u64 = size_of::<usize>() as u64;

/// The widest SIMD control group std's hash table aligns to.
const GROUP: u64 = 16;

/// Entry bytes of the weld's representative map and column set and of the
/// edge key map.
pub(super) const REPRESENTATIVE: usize = size_of::<(CellKey<3>, u32)>();
pub(super) const COLUMN: usize = size_of::<CellKey<2>>();
pub(super) const EDGE_KEY: usize = size_of::<((u32, u32, u32), usize)>();

/// Buckets a std `HashMap` allocates to hold `capacity` entries: hashbrown's
/// `capacity_to_buckets`, at most sixteen below fifteen entries.
fn buckets(capacity: u64) -> u64 {
    if capacity < 15 {
        16
    } else {
        (capacity.saturating_mul(8) / 7)
            .checked_next_power_of_two()
            .unwrap_or(u64::MAX)
    }
}

/// One table allocation of `buckets` slots of `entry` bytes and their control
/// bytes.
fn table(buckets: u64, entry: usize) -> u64 {
    if buckets == 0 {
        return 0;
    }
    buckets
        .saturating_mul(entry as u64)
        .div_ceil(GROUP)
        .saturating_mul(GROUP)
        .saturating_add(buckets)
        .saturating_add(GROUP)
}

/// Buckets behind a std hash table's reported `capacity`.
fn buckets_of(capacity: usize) -> u64 {
    let capacity = capacity as u64;
    match capacity {
        0 => 0,
        1..8 => capacity + 1,
        _ => capacity / 7 * 8,
    }
}

/// Bytes of a std hash table of reported `capacity`.
pub(super) fn table_bytes(capacity: usize, entry: usize) -> u64 {
    table(buckets_of(capacity), entry)
}

/// Bytes of the table a full std hash table of `capacity` grows into.
pub(super) fn grown_table_bytes(capacity: usize, entry: usize) -> u64 {
    let buckets = buckets_of(capacity);
    table(if buckets == 0 { 16 } else { buckets * 2 }, entry)
}

/// Peak bytes of a std `HashMap` with `entry`-byte entries that is only
/// inserted into, up to `entries` keys: its last table and the one it grew
/// out of.
fn hash_map(entries: u64, entry: usize) -> u64 {
    if entries == 0 {
        return 0;
    }
    let last = buckets(entries.saturating_add(1));
    table(last, entry).saturating_add(table(last / 2, entry))
}

/// The capacity a full metered list of `len` slots doubles to.
pub(super) fn doubled(len: usize) -> usize {
    len.saturating_mul(2).max(4)
}

/// Slots a pushed vector of final length `len` can hold at once.
fn grown(len: u64) -> u64 {
    len.saturating_mul(3).saturating_add(4)
}

/// Bytes of one `Rc<T>` allocation: two counters and the value.
fn rc<T>() -> u64 {
    2 * WORD + size_of::<T>() as u64
}

/// The canonical vertex table, retained in its `Rc` once built.
pub(super) fn canonical(positions: usize) -> u64 {
    (positions as u64)
        .saturating_mul(size_of::<u32>() as u64)
        .saturating_add(rc::<Vec<u32>>())
}

/// The label map and component ids `edges` holds while it runs.
pub(super) fn labels(primitives: usize) -> u64 {
    let primitives = primitives as u64;
    hash_map(primitives, size_of::<(&str, u32)>())
        .saturating_add(primitives.saturating_mul(size_of::<u32>() as u64))
}

/// The largest requirement one analysis checked, refusing the first that
/// exceeds the limit.
pub(super) struct Meter {
    limit: u64,
    mesh: (u64, u64),
    peak: u64,
}

impl Meter {
    pub(super) fn new(record: &MeshAnalysisRecord, limit: u64) -> Self {
        Self {
            limit,
            mesh: (record.positions.len() as u64, record.triangles.len() as u64),
            peak: 0,
        }
    }

    /// "Watertight scratch needs …; the analysis limit is …." when `bytes`
    /// exceeds the limit; otherwise `bytes` may be allocated.
    pub(super) fn require(&mut self, bytes: u64) -> Result<(), BackendError> {
        if bytes > self.limit {
            return Err(refusal(self.mesh, bytes, self.limit));
        }
        self.peak = self.peak.max(bytes);
        Ok(())
    }

    /// A meter that admits every requirement, for tests.
    #[cfg(test)]
    pub(super) fn unlimited() -> Self {
        Self {
            limit: u64::MAX,
            mesh: (0, 0),
            peak: 0,
        }
    }

    pub(super) fn peak(&self) -> u64 {
        self.peak
    }
}

/// The analysis-limit refusal for `required` of `limit` bytes.
pub(super) fn refusal(mesh: (u64, u64), required: u64, limit: u64) -> BackendError {
    BackendError::limit(
        "Watertight scratch",
        CapacityLimit::analysis(LimitUnit::Bytes, required, limit).with_mesh(mesh.0, mesh.1),
    )
}

/// What one irregular-edge pass needs for its `edges` selected edges.
#[derive(Clone, Copy, Debug, Default)]
struct Selection {
    edges: u64,
    /// Primitive entries over the selected edges, each edge listing each of
    /// its primitives once: an upper bound on the entries its samples copy.
    primitives: u64,
    /// Name bytes of those entries.
    names: u64,
    /// The most primitives one selected edge lists.
    widest: u64,
}

impl Selection {
    fn add(&mut self, record: &MeshAnalysisRecord, edge: &Edge) {
        let count = 1 + edge.later_primitives.len() as u64;
        self.edges += 1;
        self.primitives = self.primitives.saturating_add(count);
        self.widest = self.widest.max(count);
        for primitive in edge.primitives() {
            self.names = self
                .names
                .saturating_add(record.primitives[primitive as usize].name.len() as u64);
        }
    }

    /// Working set of `irregular_clusters`, freed when it returns: union-find
    /// parents, the endpoint and root maps, the pushed group lists (at most
    /// one group and one slot of each per edge), one group's stable-sort
    /// buffer and one sample's primitive ids.
    fn transient(&self) -> u64 {
        let edges = self.edges;
        if edges == 0 {
            return 0;
        }
        edges
            .saturating_mul(WORD)
            .saturating_add(hash_map(edges.saturating_mul(2), size_of::<(u32, usize)>()))
            .saturating_add(hash_map(edges, size_of::<(usize, usize)>()))
            .saturating_add(grown(edges).saturating_mul(size_of::<Vec<usize>>() as u64))
            .saturating_add(edges.saturating_mul(5 * WORD))
            .saturating_add(edges.saturating_add(48).saturating_mul(WORD))
            .saturating_add(
                self.widest
                    .saturating_add(4)
                    .saturating_mul(size_of::<u32>() as u64),
            )
    }

    /// The clusters it returns: at most one cluster and one sample per edge,
    /// and each sample's copied primitive names.
    fn retained(&self) -> u64 {
        self.edges
            .saturating_mul(
                (size_of::<IrregularEdgeCluster>() + size_of::<IrregularEdgeSample>()) as u64,
            )
            .saturating_add(self.primitives.saturating_mul(size_of::<String>() as u64))
            .saturating_add(self.names)
    }
}

/// The bound checked once `edges` exists and before anything else of the
/// analysis is built: the retained canonical table, the edge table and its
/// later-primitive lists as allocated, the pushed open and non-manifold
/// index lists, the per-primitive sums and breakdown with copied names, both
/// cluster passes (each pass's working set, or the extend that joins their
/// results) beside both results, and the result's `Rc`.
pub(super) fn completion(record: &MeshAnalysisRecord, edges: &Vec<Edge>) -> u64 {
    let mut open = Selection::default();
    let mut non_manifold = Selection::default();
    let mut lists = (edges.capacity() as u64).saturating_mul(size_of::<Edge>() as u64);
    for edge in edges {
        lists = lists.saturating_add(
            (edge.later_primitives.capacity() as u64).saturating_mul(size_of::<u32>() as u64),
        );
        match edge.incident_triangle_count {
            1 => open.add(record, edge),
            count if count > 2 => non_manifold.add(record, edge),
            _ => {}
        }
    }
    let primitives = record.primitives.len() as u64;
    let names = record.primitives.iter().fold(0u64, |sum, primitive| {
        sum.saturating_add(primitive.name.len() as u64)
    });
    let breakdown = primitives
        .saturating_mul(
            (size_of::<(usize, [f64; 3])>() + size_of::<WatertightPrimitiveBreakdown>()) as u64,
        )
        .saturating_add(names);
    let indices = grown(open.edges)
        .saturating_add(grown(non_manifold.edges))
        .saturating_mul(WORD);
    let extend = open
        .edges
        .saturating_add(non_manifold.edges)
        .saturating_mul(size_of::<IrregularEdgeCluster>() as u64);
    canonical(record.positions.len())
        .saturating_add(lists)
        .saturating_add(indices)
        .saturating_add(breakdown)
        .saturating_add(open.retained())
        .saturating_add(non_manifold.retained())
        .saturating_add(open.transient().max(non_manifold.transient()).max(extend))
        .saturating_add(rc::<Watertight>())
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::super::{analyze, tests::box_record, MeshAnalysis, Primitive};
    use super::*;
    use crate::backend::BackendErrorKind;

    fn primitive(name: &str, vertex_count: usize) -> Primitive {
        Primitive {
            name: name.into(),
            vertex_start: 0,
            vertex_count: vertex_count as u32,
        }
    }

    /// A closed box of `n` by `n` quads a side, its triangles dealt round
    /// `primitives` primitives that share one base label.
    fn grid(n: u32, primitives: usize) -> MeshAnalysisRecord {
        let mut positions = Vec::new();
        let mut index = std::collections::HashMap::<[u32; 3], u32>::new();
        let mut vertex = |p: [u32; 3]| -> u32 {
            *index.entry(p).or_insert_with(|| {
                positions.push(p.map(|value| 0.25 + f64::from(value) * 0.5));
                positions.len() as u32 - 1
            })
        };
        let mut triangles = Vec::new();
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
                        let [a, b, c, d] = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)]
                            .map(&mut vertex);
                        triangles.extend([[a, b, c], [a, c, d]]);
                    }
                }
            }
        }
        let count = positions.len();
        MeshAnalysisRecord {
            triangle_primitives: (0..triangles.len())
                .map(|index| (index % primitives) as u32)
                .collect(),
            positions,
            triangles,
            primitives: (0..primitives)
                .map(|index| primitive(&format!("part#{index}"), count))
                .collect(),
        }
    }

    /// An unwelded soup: every triangle its own open cluster.
    fn soup(count: u32) -> MeshAnalysisRecord {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for index in 0..count {
            let x = f64::from(index) * 3.0;
            let base = positions.len() as u32;
            positions.extend([[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]]);
            triangles.push([base, base + 1, base + 2]);
        }
        let primitives = (0..count)
            .map(|index| primitive(&format!("piece-{index}"), positions.len()))
            .collect();
        MeshAnalysisRecord {
            triangle_primitives: (0..count).collect(),
            positions,
            triangles,
            primitives,
        }
    }

    /// Fins on a box edge: the edge is non-manifold and the fins' other
    /// edges are open, across two primitives that share a base label.
    fn finned() -> MeshAnalysisRecord {
        let mut record = box_record(0.0);
        record.primitives = vec![primitive("box#0", 8), primitive("box#1", 8)];
        for index in 0..3u32 {
            let apex = record.positions.len() as u32;
            record
                .positions
                .push([0.5, -0.5 * f64::from(index), -1.0 - f64::from(index)]);
            record.triangles.extend([[0, 1, apex], [0, 3, apex]]);
            record.triangle_primitives.extend([1, index % 2]);
        }
        record
    }

    fn cases() -> Vec<(&'static str, MeshAnalysisRecord)> {
        let mut open = grid(6, 1);
        open.triangles.truncate(open.triangles.len() - 7);
        open.triangle_primitives.truncate(open.triangles.len());
        let mut long_names = grid(4, 3);
        for (index, primitive) in long_names.primitives.iter_mut().enumerate() {
            primitive.name = format!("{}#{index}", "assembly/".repeat(40));
        }
        long_names
            .triangles
            .truncate(long_names.triangles.len() - 5);
        long_names
            .triangle_primitives
            .truncate(long_names.triangles.len());
        vec![
            ("closed box", box_record(2.0)),
            ("empty", soup(0)),
            ("closed grid", grid(12, 1)),
            ("larger closed grid", grid(60, 1)),
            ("open grid", open),
            ("non-manifold fins", finned()),
            ("soup", soup(300)),
            ("many primitives", grid(10, 97)),
            ("long shared names", long_names),
        ]
    }

    /// Heap bytes formatting one refusal message may hold.
    const MESSAGE_BYTES: u64 = 1024;

    /// One fresh analysis under `limit`, with its peak live heap.
    fn measure(
        record: &Rc<MeshAnalysisRecord>,
        limit: u64,
    ) -> (Result<(Rc<Watertight>, u64), BackendError>, u64) {
        let analysis = analyze(record);
        crate::analysis::test_alloc::peak_bytes(|| analysis.watertight_within(limit))
    }

    #[test]
    fn measured_peak_heap_stays_within_the_reservation() {
        for (name, record) in cases() {
            let (value, peak) = measure(&Rc::new(record), u64::MAX);
            let (_, reservation) = value.expect("an unlimited analysis decides");
            println!("{name}: peak {peak} B, reservation {reservation} B");
            assert!(
                peak <= reservation,
                "{name}: peak {peak} > reservation {reservation}"
            );
        }
    }

    #[test]
    fn a_refused_analysis_never_allocates_past_the_limit() {
        for (name, record) in cases() {
            let record = Rc::new(record);
            let (value, _) = measure(&record, u64::MAX);
            let (_, reservation) = value.unwrap();
            for limit in [1, reservation / 8, reservation / 3, reservation / 2] {
                let (value, peak) = measure(&record, limit);
                let refusal = value.unwrap_err();
                assert!(
                    matches!(refusal.kind, BackendErrorKind::LimitExceeded(capacity)
                        if capacity.limit == limit && capacity.required > limit),
                    "{name}: {refusal:?}"
                );
                // The refusal's own message is the only allocation past the limit.
                assert!(
                    peak <= limit + MESSAGE_BYTES,
                    "{name}: peak {peak} > limit {limit}"
                );
            }
        }
    }

    #[test]
    fn one_byte_below_the_reservation_refuses_and_the_reservation_decides() {
        for (name, record) in cases() {
            let record = Rc::new(record);
            let (value, _) = measure(&record, u64::MAX);
            let (expected, bytes) = value.unwrap();
            let (value, _) = measure(&record, bytes - 1);
            let refusal = value.unwrap_err();
            assert!(
                matches!(refusal.kind, BackendErrorKind::LimitExceeded(limit)
                    if limit.required == bytes && limit.limit == bytes - 1),
                "{name}: {refusal:?}"
            );
            assert_eq!(
                refusal.message,
                format!(
                    "Watertight scratch needs {}; the analysis limit is {}.",
                    crate::backend::quantity(LimitUnit::Bytes, bytes),
                    crate::backend::quantity(LimitUnit::Bytes, bytes - 1)
                ),
                "{name}"
            );
            let (value, _) = measure(&record, bytes);
            let (decided, reservation) = value.expect(name);
            assert_eq!(reservation, bytes, "{name}");
            assert_eq!(decided, expected, "{name}");
            // A retained result refuses a smaller limit with its reservation.
            let warm: MeshAnalysis = analyze(&record);
            warm.watertight(bytes).unwrap();
            assert_eq!(warm.watertight(bytes - 1).unwrap_err(), refusal, "{name}");
            assert_eq!(warm.watertight(bytes).unwrap(), expected, "{name}");
        }
    }

    #[test]
    fn a_one_byte_limit_refuses_before_the_weld() {
        let analysis = analyze(&Rc::new(grid(8, 1)));
        let (value, peak) = crate::analysis::test_alloc::peak_bytes(|| analysis.watertight(1));
        let refusal = value.unwrap_err();
        assert!(
            refusal.message.ends_with("; the analysis limit is 1 byte."),
            "{}",
            refusal.message
        );
        assert!(peak <= 1 + MESSAGE_BYTES, "peak {peak}");
    }
}
