//! Meshes every face of a STEP file on its own and prints the time and
//! triangle count per face, to find faces Truck's mesher cannot bound.
//! Usage: mesh_faces <file.step> <tolerance-mm>

use std::time::Instant;
use truck_geotrait::BoundedCurve;
use truck_meshalgo::tessellation::RobustMeshableShape;
use truck_polymesh::PolylineCurve;
use truck_stepio::r#in::Table;
use truck_topology::compress::CompressedShell;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let tol: f64 = args[2].parse().unwrap();
    let t = Instant::now();
    let table = Table::from_step(&text).unwrap();
    eprintln!(
        "table {:.1} ms, {} shells",
        t.elapsed().as_secs_f64() * 1e3,
        table.shell.len()
    );
    let mut ids: Vec<_> = table.shell.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let t = Instant::now();
        let shell = table.to_compressed_shell(&table.shell[&id]).unwrap();
        eprintln!(
            "shell {id} compressed {:.1} ms, {} faces",
            t.elapsed().as_secs_f64() * 1e3,
            shell.faces.len()
        );
        if std::env::var("EDGES").is_ok() {
            for (i, edge) in shell.edges.iter().enumerate() {
                let kind = surface_name(&edge.curve);
                eprintln!("edge {i} {kind} start");
                let t = Instant::now();
                let line = PolylineCurve::from_curve(&edge.curve, edge.curve.range_tuple(), tol);
                eprintln!(
                    "edge {i} {kind} {:.1} ms {} points",
                    t.elapsed().as_secs_f64() * 1e3,
                    line.len()
                );
            }
            continue;
        }
        for (i, face) in shell.faces.iter().enumerate() {
            let single = CompressedShell {
                vertices: shell.vertices.clone(),
                edges: shell.edges.clone(),
                faces: vec![face.clone()],
            };
            let kind = surface_name(&face.surface);
            if std::env::var("SKIP").is_ok_and(|v| v.split(',').any(|x| x == i.to_string())) {
                eprintln!("shell {id} face {i} {kind} skipped");
                continue;
            }
            eprintln!("shell {id} face {i} {kind} start");
            let t = Instant::now();
            let meshed = single.robust_triangulation(tol);
            let tris: usize = meshed
                .faces
                .iter()
                .map(|f| {
                    f.surface
                        .as_ref()
                        .map_or(0, |m| m.faces().triangle_iter().count())
                })
                .sum();
            eprintln!(
                "shell {id} face {i} {kind} {:.1} ms {tris} triangles",
                t.elapsed().as_secs_f64() * 1e3
            );
        }
    }
}

fn surface_name<T: std::fmt::Debug>(surface: &T) -> String {
    let text = format!("{surface:?}");
    text.split(['(', ' ', '{'])
        .take(2)
        .collect::<Vec<_>>()
        .join("/")
}
