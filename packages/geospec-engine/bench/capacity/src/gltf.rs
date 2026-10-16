//! A minimal glTF 2.0 writer for procedural capacity fixtures.
//!
//! Buffers become external `.bin` resources, except buffer 0 of a GLB, which
//! is the embedded BIN chunk. Every buffer view starts on a four-byte boundary.
use serde_json::{json, Value};
use std::{fs, io, path::Path};

/// Closed triangle soup with shared vertices, in millimetres.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// Appends `other`, offsetting its indices.
    pub fn append(&mut self, other: &Mesh) {
        let base = u32::try_from(self.positions.len()).expect("vertex index fits u32");
        self.positions.extend_from_slice(&other.positions);
        self.triangles.extend(
            other
                .triangles
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }

    /// Signed enclosed volume from the f32 positions, accumulated in f64.
    pub fn volume(&self) -> f64 {
        let p = |i: u32| self.positions[i as usize].map(f64::from);
        self.triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    /// Axis-aligned bounds of the f32 positions.
    pub fn bounds(&self) -> [[f32; 3]; 2] {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        [min, max]
    }
}

/// Container shape of the primary file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Container {
    /// `.glb` with buffer 0 embedded and any further buffers external.
    Glb,
    /// `.gltf` JSON with every buffer external.
    Gltf,
}

/// Accumulates glTF objects and binary buffers.
#[derive(Default)]
pub struct Builder {
    buffers: Vec<(String, Vec<u8>)>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    roots: Vec<usize>,
}

fn pad4(bytes: &mut Vec<u8>) {
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
}

impl Builder {
    /// Adds a buffer; `name` is its resource file name.
    pub fn buffer(&mut self, name: &str) -> usize {
        self.buffers.push((name.to_owned(), Vec::new()));
        self.buffers.len() - 1
    }

    fn view(&mut self, buffer: usize, data: &[u8]) -> usize {
        let bytes = &mut self.buffers[buffer].1;
        pad4(bytes);
        let offset = bytes.len();
        bytes.extend_from_slice(data);
        self.views
            .push(json!({"buffer": buffer, "byteOffset": offset, "byteLength": data.len()}));
        self.views.len() - 1
    }

    /// Adds a float attribute accessor; positions also declare min and max.
    pub fn floats<const N: usize>(&mut self, buffer: usize, values: &[[f32; N]]) -> usize {
        let data: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view = self.view(buffer, &data);
        let kind = match N {
            2 => "VEC2",
            3 => "VEC3",
            4 => "VEC4",
            _ => panic!("unsupported attribute width"),
        };
        let mut accessor =
            json!({"bufferView": view, "componentType": 5126, "count": values.len(), "type": kind});
        if N == 3 {
            let mut min = [f32::INFINITY; N];
            let mut max = [f32::NEG_INFINITY; N];
            for value in values {
                for axis in 0..N {
                    min[axis] = min[axis].min(value[axis]);
                    max[axis] = max[axis].max(value[axis]);
                }
            }
            accessor["min"] = json!(min.to_vec());
            accessor["max"] = json!(max.to_vec());
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// Adds an unsigned 32-bit triangle index accessor.
    pub fn indices(&mut self, buffer: usize, triangles: &[[u32; 3]]) -> usize {
        let data: Vec<u8> = triangles
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view = self.view(buffer, &data);
        self.accessors.push(json!({
            "bufferView": view, "componentType": 5125, "count": triangles.len() * 3, "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }

    /// Adds a one-primitive mesh from accessor indices and extra attributes.
    pub fn mesh(
        &mut self,
        name: &str,
        position: usize,
        indices: usize,
        extra: &[(&str, usize)],
    ) -> usize {
        let mut attributes = json!({"POSITION": position});
        for (key, accessor) in extra {
            attributes[*key] = json!(accessor);
        }
        self.meshes.push(json!({"name": name, "primitives": [{"attributes": attributes, "indices": indices, "mode": 4}]}));
        self.meshes.len() - 1
    }

    /// Adds a mesh with one primitive per (position, indices) accessor pair.
    pub fn mesh_primitives(&mut self, name: &str, primitives: &[(usize, usize)]) -> usize {
        let primitives: Vec<Value> = primitives
            .iter()
            .map(|(position, indices)| json!({"attributes": {"POSITION": position}, "indices": indices, "mode": 4}))
            .collect();
        self.meshes
            .push(json!({"name": name, "primitives": primitives}));
        self.meshes.len() - 1
    }

    /// Writes `mesh` into `buffer` (positions then indices) and returns a mesh index.
    pub fn simple_mesh(&mut self, name: &str, buffer: usize, mesh: &Mesh) -> usize {
        let position = self.floats(buffer, &mesh.positions);
        let indices = self.indices(buffer, &mesh.triangles);
        self.mesh(name, position, indices, &[])
    }

    /// Adds a scene-root node instancing `mesh` at `translation`.
    pub fn root(&mut self, name: &str, mesh: usize, translation: [f32; 3]) -> usize {
        let mut node = json!({"name": name, "mesh": mesh});
        if translation != [0.0; 3] {
            node["translation"] = json!(translation);
        }
        self.nodes.push(node);
        self.roots.push(self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// Writes the primary file and its resources into `dir`.
    /// Returns the primary file name and the ordered resource names.
    pub fn write(
        mut self,
        dir: &Path,
        stem: &str,
        container: Container,
    ) -> io::Result<(String, Vec<String>)> {
        for (_, bytes) in &mut self.buffers {
            pad4(bytes);
        }
        let mut resources = Vec::new();
        let buffers: Vec<Value> = self
            .buffers
            .iter()
            .enumerate()
            .map(|(index, (name, bytes))| {
                if container == Container::Glb && index == 0 {
                    json!({"byteLength": bytes.len()})
                } else {
                    resources.push(name.clone());
                    json!({"uri": name, "byteLength": bytes.len()})
                }
            })
            .collect();
        let document = json!({
            "asset": {"version": "2.0", "generator": "geospec-capacity-bench"},
            "scene": 0,
            "scenes": [{"nodes": self.roots}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": buffers,
        });
        let mut text = serde_json::to_vec(&document).expect("document encodes");
        let mut external = self.buffers.into_iter().enumerate();
        let primary = match container {
            Container::Gltf => {
                let name = format!("{stem}.gltf");
                fs::write(dir.join(&name), &text)?;
                name
            }
            Container::Glb => {
                let (_, (_, bin)) = external.next().expect("GLB has buffer 0");
                while text.len() % 4 != 0 {
                    text.push(b' ');
                }
                let total = 12 + 8 + text.len() + 8 + bin.len();
                let mut out = Vec::with_capacity(total);
                out.extend_from_slice(b"glTF");
                out.extend_from_slice(&2_u32.to_le_bytes());
                out.extend_from_slice(
                    &(u32::try_from(total).expect("GLB under 4 GiB")).to_le_bytes(),
                );
                out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                out.extend_from_slice(b"JSON");
                out.extend_from_slice(&text);
                out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
                out.extend_from_slice(b"BIN\0");
                out.extend_from_slice(&bin);
                let name = format!("{stem}.glb");
                fs::write(dir.join(&name), out)?;
                name
            }
        };
        for (_, (name, bytes)) in external {
            fs::write(dir.join(&name), bytes)?;
        }
        Ok((primary, resources))
    }
}
