//! A joint's end rounding taken from the span at the end, not the whole
//! curve (review A7 attempt 2, item 5).
//!
//! A rational piece with one pole weight below its own rounding has no
//! bound for the whole curve, so a joint at an interior parameter widened
//! the bound to infinity and the face refused `non-finite`. The span at the
//! piece's end does not reach that pole's weight in its sum, so its own
//! rounding is finite.
#![allow(clippy::unwrap_used)]

use geospec_facts::{
    face_mass, Curve2, FaceDefinition, Material, NurbsCurve2, Orientation, Surface, TrimPiece, Vec3,
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

#[test]
fn a_joint_inside_a_curve_with_a_tiny_weight_is_bounded() {
    // Along v = 0; the last pole's weight 1e-16 is below the weights'
    // rounding, 24 EPS.
    let bottom = Curve2::Nurbs(NurbsCurve2 {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        poles: vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [2.0, 0.0]],
        weights: Some(vec![1.0, 1.0, 1.0, 1e-16]),
    });
    let x = bottom.evaluate(0.5).unwrap().0[0];
    let face = FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            TrimPiece {
                curve: bottom,
                range: [0.0, 0.5],
                reversed: false,
            },
            line([x, 0.0], [x, 1.0]),
            line([x, 1.0], [0.0, 1.0]),
            line([0.0, 1.0], [0.0, 0.0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(0.5, 0.5, 0.0), 2.0, 1e-7).unwrap();
    let (area, bound) = (m.raw[0], m.bound[0]);
    assert!(bound.is_finite() && bound < 1e-12, "bound {bound:e}");
    assert!(
        (area - x).abs() <= bound,
        "area {area:e} x {x:e} bound {bound:e}"
    );
}
