//! glTF node occurrences in the STEP occurrence shape: one per node whose
//! subtree carries triangles, named by its path from a root node (`.`
//! between levels, `[k]` on repeated sibling names) and bounded by the
//! vertices its subtree's triangles reference.

use std::{collections::HashMap, mem::size_of};

use crate::backend::brep::{Bounds, OccurrenceFacts};

use super::{gltf::MeshNode, MeshAnalysisRecord};

/// Deeper node trees refuse rather than recurse without bound.
pub const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OccurrenceRefusal {
    Depth,
    /// A triangle references a non-finite position, so no box is measured.
    NonFinite,
    Count(u64),
    Bytes(u64),
}

/// Checked before any path or name is copied.
#[derive(Clone, Copy, Debug)]
pub struct OccurrenceLimits {
    pub max_occurrences: u64,
    pub max_bytes: u64,
}

pub fn node_occurrences(
    nodes: &[MeshNode],
    record: &MeshAnalysisRecord,
    limits: OccurrenceLimits,
) -> Result<Vec<OccurrenceFacts>, OccurrenceRefusal> {
    let mut owner = vec![usize::MAX; record.primitives.len()];
    for (index, node) in nodes.iter().enumerate() {
        for primitive in node.primitives.clone() {
            owner[primitive as usize] = index;
        }
    }
    let empty = Bounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };
    let mut bounds = vec![empty; nodes.len()];
    let mut carries = vec![false; nodes.len()];
    for (triangle, &primitive) in record.triangles.iter().zip(&record.triangle_primitives) {
        let node = owner[primitive as usize];
        carries[node] = true;
        let box_ = &mut bounds[node];
        for &vertex in triangle {
            for (axis, value) in record.positions[vertex as usize].into_iter().enumerate() {
                if !value.is_finite() {
                    return Err(OccurrenceRefusal::NonFinite);
                }
                box_.min[axis] = box_.min[axis].min(value);
                box_.max[axis] = box_.max[axis].max(value);
            }
        }
    }
    // The world-matrix pass already refused cycles.
    let mut depth = vec![usize::MAX; nodes.len()];
    for start in 0..nodes.len() {
        let mut chain = Vec::new();
        let mut current = start;
        while depth[current] == usize::MAX {
            chain.push(current);
            match nodes[current].parent {
                Some(parent) => current = parent as usize,
                None => break,
            }
        }
        if depth[current] == usize::MAX {
            // `current` is an unvisited root, the last link of the chain.
            chain.pop();
            depth[current] = 0;
        }
        let mut level = depth[current] + 1;
        while let Some(index) = chain.pop() {
            depth[index] = level;
            level += 1;
        }
        if depth[start] > MAX_DEPTH {
            return Err(OccurrenceRefusal::Depth);
        }
    }
    // A subtree box is its own box joined with its children's, deepest first.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(depth[index]));
    for &index in &order {
        let Some(parent) = nodes[index].parent else {
            continue;
        };
        let parent = parent as usize;
        if carries[index] {
            carries[parent] = true;
            let own = bounds[index];
            for axis in 0..3 {
                bounds[parent].min[axis] = bounds[parent].min[axis].min(own.min[axis]);
                bounds[parent].max[axis] = bounds[parent].max[axis].max(own.max[axis]);
            }
        }
    }
    let mut children = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match node.parent {
            Some(parent) => children[parent as usize].push(index),
            None => roots.push(index),
        }
    }
    // Sibling segments (`[k]` on a repeated name) and each path's length,
    // shallowest first, so the limits hold before any string is built.
    let mut ordinals = vec![0_usize; nodes.len()];
    let mut positions = vec![0_u32; nodes.len()];
    for siblings in std::iter::once(&roots).chain(&children) {
        let mut totals = HashMap::new();
        for &index in siblings {
            *totals.entry(nodes[index].name.as_str()).or_insert(0_usize) += 1;
        }
        let mut seen = HashMap::new();
        for (position, &index) in siblings.iter().enumerate() {
            let name = nodes[index].name.as_str();
            positions[index] = position as u32 + 1;
            if totals[name] > 1 {
                let ordinal = seen.entry(name).or_insert(0_usize);
                *ordinal += 1;
                ordinals[index] = *ordinal;
            }
        }
    }
    let segment_len = |index: usize| {
        let ordinal = ordinals[index];
        nodes[index].name.len()
            + if ordinal == 0 {
                0
            } else {
                ordinal.to_string().len() + 2
            }
    };
    let mut path_len = vec![0_u64; nodes.len()];
    // The traversal keeps each ancestor's path, segment and ordinal buffer
    // (grown by one push, so at most twice its length) alive while it
    // descends; the deepest such chain is the transient peak.
    let mut live = vec![0_u64; nodes.len()];
    let (mut count, mut bytes, mut peak) = (0_u64, 0_u64, 0_u64);
    for &index in order.iter().rev() {
        let segment = segment_len(index) as u64;
        let ordinal_bytes = 4 * (depth[index] as u64 + 1);
        let (parent_path, parent_live) = nodes[index].parent.map_or((0, 0), |parent| {
            (path_len[parent as usize] + 1, live[parent as usize])
        });
        path_len[index] = parent_path.saturating_add(segment);
        live[index] = parent_live
            .saturating_add(path_len[index])
            .saturating_add(segment)
            .saturating_add(2 * ordinal_bytes);
        if carries[index] {
            count += 1;
            peak = peak.max(live[index]);
            bytes = bytes
                .saturating_add(size_of::<OccurrenceFacts>() as u64)
                .saturating_add(path_len[index])
                .saturating_add(3 * nodes[index].name.len() as u64)
                .saturating_add(ordinal_bytes);
        }
    }
    let bytes = bytes.saturating_add(peak);
    if count > limits.max_occurrences {
        return Err(OccurrenceRefusal::Count(count));
    }
    if bytes > limits.max_bytes {
        return Err(OccurrenceRefusal::Bytes(bytes));
    }
    let mut output = Vec::with_capacity(count as usize);
    let tree = Tree {
        nodes,
        bounds: &bounds,
        carries: &carries,
        children: &children,
        ordinals: &ordinals,
        positions: &positions,
    };
    tree.append(&roots, None, "", &[], &mut output);
    Ok(output)
}

struct Tree<'a> {
    nodes: &'a [MeshNode],
    bounds: &'a [Bounds],
    carries: &'a [bool],
    children: &'a [Vec<usize>],
    ordinals: &'a [usize],
    positions: &'a [u32],
}

impl Tree<'_> {
    /// Pre-order, siblings in document order, as STEP components are listed.
    fn append(
        &self,
        siblings: &[usize],
        parent: Option<u32>,
        prefix: &str,
        ordinals: &[u32],
        output: &mut Vec<OccurrenceFacts>,
    ) {
        for &index in siblings {
            if !self.carries[index] {
                continue;
            }
            let name = self.nodes[index].name.as_str();
            let segment = match self.ordinals[index] {
                0 => name.to_owned(),
                ordinal => format!("{name}[{ordinal}]"),
            };
            let path = if prefix.is_empty() {
                segment
            } else {
                format!("{prefix}.{segment}")
            };
            let mut ordinal_path = ordinals.to_vec();
            ordinal_path.push(self.positions[index]);
            let id = output.len() as u32;
            output.push(OccurrenceFacts {
                name: name.to_owned(),
                placement: self.nodes[index].placement,
                bounds: self.bounds[index],
                path: path.clone(),
                parent,
                product: id,
                product_name: name.to_owned(),
                instance_name: Some(name.to_owned()),
                ordinal_path: ordinal_path.clone(),
                face_count: 0,
            });
            self.append(
                &self.children[index],
                Some(id),
                &path,
                &ordinal_path,
                output,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::mesh::Primitive;

    const OPEN: OccurrenceLimits = OccurrenceLimits {
        max_occurrences: u64::MAX,
        max_bytes: u64::MAX,
    };

    fn node(name: &str, parent: Option<u32>, primitives: std::ops::Range<u32>) -> MeshNode {
        MeshNode {
            name: name.into(),
            parent,
            placement: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            primitives,
        }
    }

    fn record(corner: f64) -> MeshAnalysisRecord {
        MeshAnalysisRecord {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, corner, 0.0]],
            triangles: vec![[0, 1, 2]],
            triangle_primitives: vec![0],
            primitives: vec![Primitive {
                name: "part".into(),
                vertex_start: 0,
                vertex_count: 3,
            }],
        }
    }

    #[test]
    fn should_refuse_a_non_finite_triangle_position() {
        let nodes = [node("part", None, 0..1)];
        assert_eq!(
            node_occurrences(&nodes, &record(f64::NAN), OPEN),
            Err(OccurrenceRefusal::NonFinite)
        );
    }

    #[test]
    fn should_refuse_count_and_bytes_before_building_paths() {
        let nodes = [node("assembly", None, 0..0), node("part", Some(0), 0..1)];
        let facts = node_occurrences(&nodes, &record(1.0), OPEN).unwrap();
        assert_eq!(
            facts
                .iter()
                .map(|fact| fact.path.as_str())
                .collect::<Vec<_>>(),
            ["assembly", "assembly.part"]
        );
        let counted = OccurrenceLimits {
            max_occurrences: 1,
            ..OPEN
        };
        assert_eq!(
            node_occurrences(&nodes, &record(1.0), counted),
            Err(OccurrenceRefusal::Count(2))
        );
        let sized = OccurrenceLimits {
            max_bytes: 64,
            ..OPEN
        };
        assert!(matches!(
            node_occurrences(&nodes, &record(1.0), sized),
            Err(OccurrenceRefusal::Bytes(bytes)) if bytes > 64
        ));
    }

    #[test]
    fn should_count_the_live_ancestor_paths_of_a_deep_chain() {
        let long = "a".repeat(1000);
        let mut nodes = vec![node(&long, None, 0..0)];
        for level in 0..10 {
            nodes.push(node(&long, Some(level), 0..0));
        }
        nodes.last_mut().unwrap().primitives = 0..1;
        let Err(OccurrenceRefusal::Bytes(required)) = node_occurrences(
            &nodes,
            &record(1.0),
            OccurrenceLimits {
                max_bytes: 0,
                ..OPEN
            },
        ) else {
            panic!("expected a byte refusal");
        };
        // Eleven retained paths of 1000..=11010 bytes, plus the same paths
        // held live along the traversal.
        let retained: u64 = (1..=11).map(|level| 1001 * level - 1).sum();
        assert!(required >= 2 * retained);
        let fits = OccurrenceLimits {
            max_bytes: required,
            ..OPEN
        };
        assert_eq!(
            node_occurrences(&nodes, &record(1.0), fits).unwrap().len(),
            11
        );
        let short = OccurrenceLimits {
            max_bytes: required - 1,
            ..OPEN
        };
        assert_eq!(
            node_occurrences(&nodes, &record(1.0), short),
            Err(OccurrenceRefusal::Bytes(required))
        );
    }

    #[test]
    fn should_refuse_a_node_tree_deeper_than_the_limit() {
        let mut nodes = vec![node("n", None, 0..0)];
        for level in 0..=MAX_DEPTH as u32 {
            nodes.push(node("n", Some(level), 0..0));
        }
        nodes.last_mut().unwrap().primitives = 0..1;
        assert_eq!(
            node_occurrences(&nodes, &record(1.0), OPEN),
            Err(OccurrenceRefusal::Depth)
        );
    }
}
