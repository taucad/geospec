//! A trim's crossing of a hard u knot line, found to the last bit (review
//! A7 F2).
//!
//! Across a hard knot line the outer integrand jumps. The crossing was
//! bisected only to 1e-12 relative, so the break could land a few 1e-12
//! short of the line, with the jump inside a panel where no node sees it.
//! The surface below is bent at `u = 1`: `|S_u x S_v|` is 1 before the line
//! and `sqrt 101` after it, so the area of a strip across it is known.
#![allow(clippy::unwrap_used)]

use geospec_facts::{
    face_mass, Curve2, FaceDefinition, Material, NurbsSurface, Orientation, Surface, TrimPiece,
    Vec3,
};

fn line(origin: [f64; 2], direction: [f64; 2], range: [f64; 2], reversed: bool) -> TrimPiece {
    TrimPiece {
        curve: Curve2::Line { origin, direction },
        range,
        reversed,
    }
}

#[test]
fn a_strip_across_a_bend_is_within_its_bound() {
    // x = u, z = 0 up to u = 1 and z = 10 (u - 1) after it; y = v.
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 10.0),
            Vec3::new(2.0, 1.0, 10.0),
        ],
        weights: None,
        count_u: 3,
        count_v: 2,
    });
    // The horizontal sides run over t in [0.25, 1.75] at u = t + 0.1, so that
    // the crossing (t = 0.9) falls between the search's samples.
    let d = 0.1;
    let (x0, x1) = (d + 0.25, d + 1.75);
    let face = FaceDefinition {
        surface,
        trims: vec![
            line([d, 0.0], [1.0, 0.0], [0.25, 1.75], false),
            line([x1, 0.0], [0.0, 1.0], [0.0, 1.0], false),
            line([d, 1.0], [1.0, 0.0], [0.25, 1.75], true),
            line([x0, 0.0], [0.0, 1.0], [0.0, 1.0], true),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    };
    let m = face_mass(&face, Vec3::new(1.0, 0.5, 0.5), 2.0, 1e-7).unwrap();
    let exact = (1.0 - x0) + (x1 - 1.0) * 101.0_f64.sqrt();
    let (area, bound) = (m.raw[0], m.bound[0]);
    assert!(
        (area - exact).abs() <= bound + 4.0 * f64::EPSILON * exact,
        "area {area:e} is {:e} from {exact:e}, bound {bound:e}",
        (area - exact).abs()
    );
}
