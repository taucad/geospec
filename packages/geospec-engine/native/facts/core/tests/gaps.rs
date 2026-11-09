//! The box of a body given for the box alone ([`BoxBody`]): a joint table
//! that does not fit the body refuses.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use geospec_facts::{
    BodyDefinition, BoxBody, BoxJoints, Curve2, FaceDefinition, Joint, Material, Orientation,
    RefusalCode, Surface, TrimPiece, Vec3,
};

fn line(a: [f64; 2], b: [f64; 2]) -> TrimPiece {
    TrimPiece {
        curve: Curve2::Line {
            origin: a,
            direction: [b[0] - a[0], b[1] - a[1]],
        },
        range: [0.0, 1.0],
        reversed: false,
    }
}

fn plane() -> Surface {
    Surface::Plane {
        origin: Vec3::ZERO,
        u_axis: Vec3::new(1.0, 0.0, 0.0),
        v_axis: Vec3::new(0.0, 1.0, 0.0),
    }
}

fn face(surface: Surface, trims: Vec<TrimPiece>) -> FaceDefinition {
    let loops = vec![trims.len()];
    FaceDefinition {
        surface,
        trims,
        loops,
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

fn body(faces: Vec<FaceDefinition>, size: f64) -> BodyDefinition {
    BodyDefinition {
        centre: Vec3::ZERO,
        size,
        bounds: None,
        faces,
        file_tolerance: None,
        closed: None,
    }
}

fn square() -> FaceDefinition {
    face(
        plane(),
        vec![
            line([0.0, 0.0], [1.0, 0.0]),
            line([1.0, 0.0], [1.0, 1.0]),
            line([1.0, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
    )
}

fn joint(x: f64, y: f64, tolerance: f64) -> Option<Joint> {
    Some(Joint {
        vertex: Vec3::new(x, y, 0.0),
        tolerance,
    })
}

#[test]
fn a_table_that_does_not_fit_refuses() {
    let d = body(vec![square()], 1.0);
    let ends = vec![joint(1.0, 0.0, 1e-3); 4];
    let table = |bridges: Vec<usize>, ends: Vec<Option<Joint>>| vec![BoxJoints { bridges, ends }];
    let cases = [
        (Vec::new(), Some(1e-3)),
        (table(vec![4], ends.clone()), Some(1e-3)),
        (table(vec![2, 1], ends.clone()), Some(1e-3)),
        (table(vec![1, 1], ends.clone()), Some(1e-3)),
        (table(Vec::new(), ends[..3].to_vec()), Some(1e-3)),
        (
            table(Vec::new(), vec![joint(f64::NAN, 0.0, 1e-3); 4]),
            Some(1e-3),
        ),
        (
            table(Vec::new(), vec![joint(0.0, 0.0, -1.0); 4]),
            Some(1e-3),
        ),
        (table(Vec::new(), ends.clone()), Some(f64::INFINITY)),
        (table(Vec::new(), ends.clone()), Some(-1e-3)),
    ];
    for (k, (joints, tolerance)) in cases.into_iter().enumerate() {
        let r = BoxBody::new(d.clone(), joints, tolerance);
        assert_eq!(
            r.map(|_| ()).unwrap_err().code,
            RefusalCode::InvalidDefinition,
            "case {k}"
        );
    }
    // A bridge must be a line.
    let mut curved = square();
    curved.trims[1].curve = Curve2::Nurbs(geospec_facts::NurbsCurve2 {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![[1.0, 0.0], [1.0, 1.0]],
        weights: None,
    });
    let r = BoxBody::new(body(vec![curved], 1.0), table(vec![1], ends.clone()), None);
    assert_eq!(
        r.map(|_| ()).unwrap_err().code,
        RefusalCode::InvalidDefinition
    );
    let ok = BoxBody::new(d, table(vec![1], ends), Some(1e-3)).unwrap();
    assert!(ok.bridged());
}
