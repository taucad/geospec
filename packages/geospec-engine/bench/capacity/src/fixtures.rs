//! Procedural closed, watertight capacity fixtures.
//!
//! Every fixture is generated at run time into a caller-owned directory and is
//! described by `fixture.json`: its files, the facts the generator knows
//! independently of the engine (counts, bytes, components, enclosed volume and
//! bounds) and the claims to run with their expected truth.
use crate::gltf::{Builder, Container, Mesh};
use serde_json::{json, Value};
use std::{fs, io, path::Path};

/// Torus grid with `nu` x `nv` unique vertices, outward oriented, genus one.
pub fn torus(nu: u32, nv: u32, major: f32, minor: f32, center: [f32; 3]) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.positions.reserve((nu * nv) as usize);
    for i in 0..nu {
        let u = i as f64 / nu as f64 * std::f64::consts::TAU;
        for j in 0..nv {
            let v = j as f64 / nv as f64 * std::f64::consts::TAU;
            let ring = f64::from(major) + f64::from(minor) * v.cos();
            mesh.positions.push([
                center[0] + (ring * u.cos()) as f32,
                center[1] + (ring * u.sin()) as f32,
                center[2] + (f64::from(minor) * v.sin()) as f32,
            ]);
        }
    }
    mesh.triangles.reserve((2 * nu * nv) as usize);
    let at = |i: u32, j: u32| (i % nu) * nv + (j % nv);
    for i in 0..nu {
        for j in 0..nv {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            mesh.triangles.push([a, b, c]);
            mesh.triangles.push([a, c, d]);
        }
    }
    mesh
}

/// Axis-aligned closed box from `min` with edge `size`.
pub fn cube(min: [f32; 3], size: f32) -> Mesh {
    let [x, y, z] = min;
    let s = size;
    Mesh {
        positions: vec![
            [x, y, z],
            [x + s, y, z],
            [x + s, y + s, z],
            [x, y + s, z],
            [x, y, z + s],
            [x + s, y, z + s],
            [x + s, y + s, z + s],
            [x, y + s, z + s],
        ],
        triangles: vec![
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
        ],
    }
}

/// An integer voxel-lattice point.
type Cell = (i32, i32, i32);

/// Boundary surface of a voxel set given by `inside(x, y, z)` over `[0, n)^3`,
/// with shared vertices. `inside` must be a digitized ball (or a union of
/// separated ones) so no two voxels meet only along an edge.
pub fn voxel_surface(n: i32, scale: f32, inside: impl Fn(i32, i32, i32) -> bool) -> (Mesh, u64) {
    use std::collections::HashMap;
    let mut mesh = Mesh::default();
    let mut index: HashMap<(i32, i32, i32), u32> = HashMap::new();
    let mut vertex = |p: (i32, i32, i32), mesh: &mut Mesh| {
        *index.entry(p).or_insert_with(|| {
            mesh.positions
                .push([p.0 as f32 * scale, p.1 as f32 * scale, p.2 as f32 * scale]);
            (mesh.positions.len() - 1) as u32
        })
    };
    let solid = |x: i32, y: i32, z: i32| {
        (0..n).contains(&x) && (0..n).contains(&y) && (0..n).contains(&z) && inside(x, y, z)
    };
    let mut voxels = 0_u64;
    // Faces as counter-clockwise corner offsets seen from outside.
    let faces: [(Cell, [Cell; 4]); 6] = [
        ((1, 0, 0), [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)]),
        ((-1, 0, 0), [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)]),
        ((0, 1, 0), [(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)]),
        ((0, -1, 0), [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)]),
        ((0, 0, 1), [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)]),
        ((0, 0, -1), [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)]),
    ];
    for x in 0..n {
        for y in 0..n {
            for z in 0..n {
                if !solid(x, y, z) {
                    continue;
                }
                voxels += 1;
                for (normal, corners) in &faces {
                    if solid(x + normal.0, y + normal.1, z + normal.2) {
                        continue;
                    }
                    let q = corners.map(|c| vertex((x + c.0, y + c.1, z + c.2), &mut mesh));
                    mesh.triangles.push([q[0], q[1], q[2]]);
                    mesh.triangles.push([q[0], q[2], q[3]]);
                }
            }
        }
    }
    (mesh, voxels)
}

/// One claim with the truth the generator knows independently.
fn claim(id: &str, capability: &str, payload: Value, truth: bool) -> Value {
    json!({"claimId": id, "capability": capability, "payload": payload, "expectedTruth": truth})
}

/// The four representative mesh claims, all true by construction.
fn standard_claims(bounds: [[f32; 3]; 2], components: u64, volume: f64) -> Vec<Value> {
    vec![
        claim(
            "bounding-box",
            "toHaveBoundingBox",
            json!({"kind": "boundingBox", "expected": {"min": bounds[0], "max": bounds[1], "tolerance": 0.001}}),
            true,
        ),
        claim(
            "watertight",
            "toBeWatertight",
            json!({"kind": "watertight", "expected": true}),
            true,
        ),
        claim(
            "connected-components",
            "toHaveConnectedComponents",
            json!({"kind": "connectedComponents", "expected": {"count": components}}),
            true,
        ),
        claim(
            "volume",
            "toHaveVolume",
            json!({"kind": "volume", "expected": {"value": volume, "tolerance": (volume.abs() * 1e-7).max(1e-6)}}),
            true,
        ),
    ]
}

fn translate(mesh: &Mesh, offset: [f32; 3]) -> Mesh {
    Mesh {
        positions: mesh
            .positions
            .iter()
            .map(|p| [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]])
            .collect(),
        triangles: mesh.triangles.clone(),
    }
}

/// Facts about one subject's files and geometry, written to `fixture.json`.
struct Subject {
    primary: String,
    resources: Vec<String>,
    format: &'static str,
    vertices: u64,
    triangles: u64,
    unique_vertices: u64,
    unique_triangles: u64,
    occurrences: u64,
    components: u64,
    volume: f64,
    bounds: [[f32; 3]; 2],
}

impl Subject {
    fn json(&self, dir: &Path) -> io::Result<Value> {
        let size = |name: &str| fs::metadata(dir.join(name)).map(|m| m.len());
        let primary_bytes = size(&self.primary)?;
        let resources = self
            .resources
            .iter()
            .map(|name| Ok(json!({"name": name, "byteLength": size(name)?})))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(json!({
            "format": self.format,
            "primary": self.primary,
            "primaryBytes": primary_bytes,
            "resources": resources,
            "facts": {
                "vertices": self.vertices,
                "triangles": self.triangles,
                "uniqueVertices": self.unique_vertices,
                "uniqueTriangles": self.unique_triangles,
                "occurrences": self.occurrences,
                "components": self.components,
                "volume": self.volume,
                "bounds": self.bounds,
            }
        }))
    }
}

/// One mesh instanced at each translation, written as glTF with one `.bin`.
fn instanced(
    dir: &Path,
    stem: &str,
    mesh: &Mesh,
    offsets: &[[f32; 3]],
    container: Container,
) -> io::Result<Subject> {
    let mut builder = Builder::default();
    let buffer = builder.buffer(&format!("{stem}.bin"));
    let index = builder.simple_mesh("part", buffer, mesh);
    let [min, max] = mesh.bounds();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for (n, offset) in offsets.iter().enumerate() {
        builder.root(&format!("part-{n}"), index, *offset);
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(min[axis] + offset[axis]);
            bounds[1][axis] = bounds[1][axis].max(max[axis] + offset[axis]);
        }
    }
    let (primary, resources) = builder.write(dir, stem, container)?;
    let copies = offsets.len() as u64;
    let volume: f64 = offsets
        .iter()
        .map(|offset| translate(mesh, *offset).volume())
        .sum();
    Ok(Subject {
        primary,
        resources,
        format: if container == Container::Glb {
            "glb"
        } else {
            "gltf"
        },
        vertices: mesh.positions.len() as u64 * copies,
        triangles: mesh.triangles.len() as u64 * copies,
        unique_vertices: mesh.positions.len() as u64,
        unique_triangles: mesh.triangles.len() as u64,
        occurrences: copies,
        components: copies,
        volume,
        bounds,
    })
}

/// One unique mesh as a single occurrence; positions and indices in separate
/// external resources when `split`, otherwise one buffer.
fn single(
    dir: &Path,
    stem: &str,
    mesh: &Mesh,
    components: u64,
    container: Container,
    split: bool,
    extra_attributes: bool,
) -> io::Result<Subject> {
    let mut builder = Builder::default();
    let positions_buffer = builder.buffer(&format!("{stem}-positions.bin"));
    let index_buffer = if split {
        builder.buffer(&format!("{stem}-indices.bin"))
    } else {
        positions_buffer
    };
    let position = builder.floats(positions_buffer, &mesh.positions);
    let indices = builder.indices(index_buffer, &mesh.triangles);
    let mut extra = Vec::new();
    if extra_attributes {
        // Real per-vertex attributes in their own resources: unit normals of
        // the position direction, an opaque colour and planar texcoords.
        let normals: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|p| {
                let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2])
                    .sqrt()
                    .max(f32::MIN_POSITIVE);
                [p[0] / length, p[1] / length, p[2] / length]
            })
            .collect();
        let normal_buffer = builder.buffer(&format!("{stem}-normals.bin"));
        extra.push(("NORMAL", builder.floats(normal_buffer, &normals)));
        drop(normals);
        let colours = vec![[0.6_f32, 0.6, 0.62, 1.0]; mesh.positions.len()];
        let colour_buffer = builder.buffer(&format!("{stem}-colors.bin"));
        extra.push(("COLOR_0", builder.floats(colour_buffer, &colours)));
        drop(colours);
        let texcoords: Vec<[f32; 2]> = mesh.positions.iter().map(|p| [p[0], p[1]]).collect();
        let texcoord_buffer = builder.buffer(&format!("{stem}-texcoords.bin"));
        extra.push(("TEXCOORD_0", builder.floats(texcoord_buffer, &texcoords)));
    }
    let index = builder.mesh("part", position, indices, &extra);
    builder.root("part", index, [0.0; 3]);
    let (primary, resources) = builder.write(dir, stem, container)?;
    Ok(Subject {
        primary,
        resources,
        format: if container == Container::Glb {
            "glb"
        } else {
            "gltf"
        },
        vertices: mesh.positions.len() as u64,
        triangles: mesh.triangles.len() as u64,
        unique_vertices: mesh.positions.len() as u64,
        unique_triangles: mesh.triangles.len() as u64,
        occurrences: 1,
        components,
        volume: mesh.volume(),
        bounds: mesh.bounds(),
    })
}

/// Splits a `torus(nu, nv, ..)` grid into two halves along `u`, each with its
/// own copy of the shared boundary rings.
fn halves(mesh: &Mesh, nu: u32, nv: u32) -> (Mesh, Mesh) {
    let half = |from: u32, to: u32| {
        let mut out = Mesh::default();
        let rows = to - from + 1;
        for i in from..=to {
            for j in 0..nv {
                out.positions
                    .push(mesh.positions[((i % nu) * nv + j) as usize]);
            }
        }
        for i in 0..rows - 1 {
            for j in 0..nv {
                let at = |i: u32, j: u32| i * nv + (j % nv);
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                out.triangles.push([a, b, c]);
                out.triangles.push([a, c, d]);
            }
        }
        out
    };
    (half(0, nu / 2), half(nu / 2, nu))
}

fn grid_offsets(count: usize, pitch: f32) -> Vec<[f32; 3]> {
    let side = (count as f64).cbrt().ceil() as usize;
    (0..count)
        .map(|n| {
            [
                (n % side) as f32 * pitch,
                ((n / side) % side) as f32 * pitch,
                (n / (side * side)) as f32 * pitch,
            ]
        })
        .collect()
}

/// Every fixture id the generator knows, in matrix order.
pub const FIXTURES: &[&str] = &[
    "small-torus",
    "unique-2m",
    "instanced-2m",
    "primary-64mib",
    "closure-128mib",
    "occurrences-65536",
    "occurrences-65537",
    "occurrences-70000",
    "material-256mib",
    "voxel-geode-323k",
    "subjects-40",
    "subjects-300",
    "partition-components",
    "partition-components-mono",
    "partition-interference",
    "partition-interference-mono",
];

/// Generates fixture `id` into `dir` and writes `dir/fixture.json`.
pub fn generate(id: &str, dir: &Path) -> io::Result<Value> {
    fs::create_dir_all(dir)?;
    let mut subjects = Vec::new();
    let mut claims = None;
    let mut plan = "per-claim";
    match id {
        // Small control fixture used by the injected-boundary sweep.
        "small-torus" => subjects.push(single(
            dir,
            "small",
            &torus(12, 10, 20.0, 5.0, [0.0; 3]),
            1,
            Container::Gltf,
            true,
            false,
        )?),
        // (a) Unique geometry above 2,000,000 vertices and 4,000,000 triangles;
        // positions and indices are separate resources under 64 MiB each and
        // the closure is under 128 MiB, so only the count limits are crossed.
        "unique-2m" => subjects.push(single(
            dir,
            "unique",
            &torus(1500, 1400, 200.0, 40.0, [0.0; 3]),
            1,
            Container::Gltf,
            true,
            false,
        )?),
        // (b) The same counts from 100 instances of one 21,000-vertex torus.
        "instanced-2m" => subjects.push(instanced(
            dir,
            "instanced",
            &torus(150, 140, 20.0, 5.0, [0.0; 3]),
            &grid_offsets(100, 60.0),
            Container::Gltf,
        )?),
        // (c1) A GLB whose embedded BIN exceeds 64 MiB while the vertex and
        // triangle counts stay below their limits.
        "primary-64mib" => subjects.push(single(
            dir,
            "primary",
            &torus(1380, 1380, 200.0, 40.0, [0.0; 3]),
            1,
            Container::Glb,
            false,
            false,
        )?),
        // (c2) A closure above 128 MiB from real per-vertex attributes in five
        // resources, each under 64 MiB, with counts below their limits.
        "closure-128mib" => subjects.push(single(
            dir,
            "closure",
            &torus(1380, 1380, 200.0, 40.0, [0.0; 3]),
            1,
            Container::Gltf,
            true,
            true,
        )?),
        // (d) Occurrence counts at and above 65,536 from one instanced cube.
        "occurrences-65536" => subjects.push(instanced(
            dir,
            "occurrences",
            &cube([0.0; 3], 1.0),
            &grid_offsets(65_536, 2.0),
            Container::Gltf,
        )?),
        "occurrences-65537" => subjects.push(instanced(
            dir,
            "occurrences",
            &cube([0.0; 3], 1.0),
            &grid_offsets(65_537, 2.0),
            Container::Gltf,
        )?),
        "occurrences-70000" => subjects.push(instanced(
            dir,
            "occurrences",
            &cube([0.0; 3], 1.0),
            &grid_offsets(70_000, 2.0),
            Container::Gltf,
        )?),
        // (e) A mesh inside every admission limit whose exact material
        // analysis demands more than the 256 MiB analysis retention limit.
        "material-256mib" => subjects.push(single(
            dir,
            "material",
            &torus(600, 500, 200.0, 40.0, [0.0; 3]),
            1,
            Container::Glb,
            false,
            false,
        )?),
        // (f) Two separated voxel balls, about 323k triangles: the voxel-like,
        // two-component closed mesh of the gyroid geode report.
        "voxel-geode-323k" => {
            let (n, r) = (134_i32, 31.5_f64);
            let (a, b) = ((33, 33, 33), (99, 99, 99));
            let ball = |c: (i32, i32, i32), x: i32, y: i32, z: i32| {
                let d = |p: i32, q: i32| f64::from(p - q);
                d(x, c.0).powi(2) + d(y, c.1).powi(2) + d(z, c.2).powi(2) <= r * r
            };
            let (mesh, voxels) =
                voxel_surface(n, 0.5, |x, y, z| ball(a, x, y, z) || ball(b, x, y, z));
            let mut subject = single(dir, "geode", &mesh, 2, Container::Glb, false, false)?;
            // The enclosed volume is exact: voxel count times the voxel volume.
            subject.volume = voxels as f64 * 0.125;
            subjects.push(subject);
        }
        // (g) More subjects than the 32 retained-subject and 256 mesh-entry
        // limits; one plan demands every subject's mesh at once.
        "subjects-40" | "subjects-300" => {
            let count = if id == "subjects-40" { 40 } else { 300 };
            for n in 0..count {
                let mesh = cube([n as f32 * 3.0, 0.0, 0.0], 1.0 + n as f32 / 1024.0);
                subjects.push(single(
                    dir,
                    &format!("subject-{n}"),
                    &mesh,
                    1,
                    Container::Glb,
                    false,
                    false,
                )?);
            }
            plan = "combined";
        }
        // Partition analogues: the decisive feature lives in the last resource
        // (partitioned) or inside one buffer (monolithic). Both forms must agree.
        "partition-components" | "partition-components-mono" => {
            // 64 disjoint tori. Partitioned: one resource per torus and the
            // last torus split into two half primitives in two resources that
            // meet only through equal boundary positions. Monolithic: one
            // buffer, every torus one primitive. Truth: 64 closed components.
            let split = id == "partition-components";
            let mut builder = Builder::default();
            let mut whole = Mesh::default();
            let first = builder.buffer("part-0.bin");
            for n in 0..64 {
                let part = torus(48, 24, 10.0, 3.0, [n as f32 * 30.0, 0.0, 0.0]);
                let buffer = if split && n > 0 {
                    builder.buffer(&format!("part-{n}.bin"))
                } else {
                    first
                };
                let mesh = if split && n == 63 {
                    let (left, right) = halves(&part, 48, 24);
                    let other = builder.buffer("part-63b.bin");
                    let a = builder.floats(buffer, &left.positions);
                    let ai = builder.indices(buffer, &left.triangles);
                    let b = builder.floats(other, &right.positions);
                    let bi = builder.indices(other, &right.triangles);
                    builder.mesh_primitives(&format!("part-{n}"), &[(a, ai), (b, bi)])
                } else {
                    builder.simple_mesh(&format!("part-{n}"), buffer, &part)
                };
                builder.root(&format!("part-{n}"), mesh, [0.0; 3]);
                whole.append(&part);
            }
            let (primary, resources) = builder.write(dir, "partition", Container::Gltf)?;
            subjects.push(Subject {
                primary,
                resources,
                format: "gltf",
                vertices: whole.positions.len() as u64,
                triangles: whole.triangles.len() as u64,
                unique_vertices: whole.positions.len() as u64,
                unique_triangles: whole.triangles.len() as u64,
                occurrences: 64,
                components: 64,
                volume: whole.volume(),
                bounds: whole.bounds(),
            });
        }
        "partition-interference" | "partition-interference-mono" => {
            // 32 separated cubes; only the last pair overlaps.
            let split = id == "partition-interference";
            let mut builder = Builder::default();
            let mut whole = Mesh::default();
            let first = builder.buffer("cube-0.bin");
            for n in 0..32 {
                let x = if n == 31 {
                    30.0 * 3.0 + 0.5
                } else {
                    n as f32 * 3.0
                };
                let part = cube([x, 0.0, 0.0], 1.0);
                let buffer = if split && n > 0 {
                    builder.buffer(&format!("cube-{n}.bin"))
                } else {
                    first
                };
                let mesh = builder.simple_mesh(&format!("cube-{n}"), buffer, &part);
                builder.root(&format!("cube-{n}"), mesh, [0.0; 3]);
                whole.append(&part);
            }
            let (primary, resources) = builder.write(dir, "interference", Container::Gltf)?;
            subjects.push(Subject {
                primary,
                resources,
                format: "gltf",
                vertices: whole.positions.len() as u64,
                triangles: whole.triangles.len() as u64,
                unique_vertices: whole.positions.len() as u64,
                unique_triangles: whole.triangles.len() as u64,
                occurrences: 32,
                components: 31,
                volume: whole.volume(),
                bounds: whole.bounds(),
            });
            claims = Some(vec![
                claim(
                    "no-interference",
                    "toHaveNoComponentInterference",
                    json!({"kind": "componentInterference", "expected": {}}),
                    false,
                ),
                // The overlapping pair is one spatially connected component.
                claim(
                    "connected-components",
                    "toHaveConnectedComponents",
                    json!({"kind": "connectedComponents", "expected": {"count": 31}}),
                    true,
                ),
            ]);
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown fixture {id}"),
            ))
        }
    }
    let claims = claims.unwrap_or_else(|| {
        let s = &subjects[0];
        standard_claims(s.bounds, s.components, s.volume)
    });
    let subjects = subjects
        .iter()
        .map(|s| s.json(dir))
        .collect::<io::Result<Vec<_>>>()?;
    let manifest = json!({"schema": "geospec-capacity-fixture-v1", "id": id, "plan": plan, "subjects": subjects, "claims": claims});
    fs::write(
        dir.join("fixture.json"),
        serde_json::to_vec_pretty(&manifest).expect("manifest encodes"),
    )?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Every directed edge appears once and its reverse once: closed and consistently oriented.
    fn closed(mesh: &Mesh) -> bool {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &mesh.triangles {
            for i in 0..3 {
                *edges.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
            }
        }
        edges
            .iter()
            .all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }

    #[test]
    fn generated_meshes_are_closed_and_outward() {
        let ring = torus(24, 12, 10.0, 3.0, [0.0; 3]);
        assert!(closed(&ring));
        let exact = 2.0 * std::f64::consts::PI.powi(2) * 10.0 * 9.0;
        assert!(ring.volume() > 0.9 * exact && ring.volume() < exact);
        let unit = cube([1.0, 2.0, 3.0], 2.0);
        assert!(closed(&unit));
        assert!((unit.volume() - 8.0).abs() < 1e-12);
        let (balls, voxels) = voxel_surface(20, 0.5, |x, y, z| {
            let d = |c: i32, p: i32| f64::from(p - c).powi(2);
            d(5, x) + d(5, y) + d(5, z) <= 16.0 || d(14, x) + d(14, y) + d(14, z) <= 16.0
        });
        assert!(closed(&balls));
        assert!((balls.volume() - voxels as f64 * 0.125).abs() < 1e-9);
        let (left, right) = halves(&ring, 24, 12);
        let mut both = left.clone();
        both.append(&right);
        assert_eq!(both.triangles.len(), ring.triangles.len());
        assert!((both.volume() - ring.volume()).abs() < 1e-9);
    }

    #[test]
    fn manifests_record_files_and_facts() {
        let dir =
            std::env::temp_dir().join(format!("geospec-capacity-test-{}", std::process::id()));
        let manifest = generate("partition-interference", &dir).unwrap();
        let subject = &manifest["subjects"][0];
        assert_eq!(subject["resources"].as_array().unwrap().len(), 32);
        assert_eq!(subject["facts"]["components"], 31);
        let primary = fs::read(dir.join(subject["primary"].as_str().unwrap())).unwrap();
        assert_eq!(
            primary.len() as u64,
            subject["primaryBytes"].as_u64().unwrap()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
