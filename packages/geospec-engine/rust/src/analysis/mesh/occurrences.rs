//! glTF node occurrences in the STEP occurrence shape: one per node whose
//! subtree carries triangles, named by its path from a root node (`.`
//! between levels, `[k]` on repeated sibling names) and bounded by the
//! vertices its subtree's triangles reference.

use std::collections::HashMap;

use crate::backend::brep::{Bounds, OccurrenceFacts};

use super::{gltf::MeshNode, MeshAnalysisRecord};

/// Deeper node trees refuse rather than recurse without bound.
pub const MAX_DEPTH: usize = 64;

pub fn node_occurrences(
    nodes: &[MeshNode],
    record: &MeshAnalysisRecord,
) -> Option<Vec<OccurrenceFacts>> {
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
    for (triangle, &primitive) in record.triangles.iter().zip(&record.triangle_primitives) {
        let node = owner[primitive as usize];
        for &vertex in triangle {
            let box_ = &mut bounds[node];
            for (axis, value) in record.positions[vertex as usize].into_iter().enumerate() {
                box_.min[axis] = box_.min[axis].min(value);
                box_.max[axis] = box_.max[axis].max(value);
            }
        }
    }
    // A subtree box is its own box joined with its children's, deepest
    // first. The world-matrix pass already refused cycles.
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
            return None;
        }
    }
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(depth[index]));
    for index in order {
        let Some(parent) = nodes[index].parent else {
            continue;
        };
        let own = bounds[index];
        let parent = parent as usize;
        for axis in 0..3 {
            bounds[parent].min[axis] = bounds[parent].min[axis].min(own.min[axis]);
            bounds[parent].max[axis] = bounds[parent].max[axis].max(own.max[axis]);
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
    let mut output = Vec::new();
    let level = Level {
        nodes,
        bounds: &bounds,
        children: &children,
    };
    level.append(&roots, None, "", &[], &mut output);
    Some(output)
}

struct Level<'a> {
    nodes: &'a [MeshNode],
    bounds: &'a [Bounds],
    children: &'a [Vec<usize>],
}

impl Level<'_> {
    /// Pre-order, siblings in document order, as STEP components are listed.
    fn append(
        &self,
        siblings: &[usize],
        parent: Option<u32>,
        prefix: &str,
        ordinals: &[u32],
        output: &mut Vec<OccurrenceFacts>,
    ) {
        let mut totals = HashMap::new();
        for &index in siblings {
            *totals
                .entry(self.nodes[index].name.as_str())
                .or_insert(0_usize) += 1;
        }
        let mut seen = HashMap::new();
        for (position, &index) in siblings.iter().enumerate() {
            let name = self.nodes[index].name.as_str();
            let mut segment = name.to_owned();
            if totals[name] > 1 {
                let ordinal = seen.entry(name).or_insert(0_usize);
                *ordinal += 1;
                segment = format!("{segment}[{ordinal}]");
            }
            if !self.bounds[index].min[0].is_finite() {
                continue;
            }
            let path = if prefix.is_empty() {
                segment
            } else {
                format!("{prefix}.{segment}")
            };
            let mut ordinal_path = ordinals.to_vec();
            ordinal_path.push(position as u32 + 1);
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
