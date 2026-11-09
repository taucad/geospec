//! A kernel's rounding at extrapolated points, in absolute coordinates.
//!
//! A net far from the origin whose end-span poles coincide has an almost
//! constant exact extension, but OCCT evaluates it in absolute coordinates,
//! where rounding grows with the end span's narrowness. The pinned values are
//! how far `Geom_BSplineSurface::Value` lands past the exact box at
//! `u = -0.9e-6` (OCCT built with `-ffp-contract=off`).
#![allow(clippy::unwrap_used)]

use geospec_facts::{
    face_box, Curve2, FaceDefinition, Material, NurbsSurface, Orientation, Surface, TrimPiece, Vec3,
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

fn far_net(x0: f64, a: f64) -> FaceDefinition {
    let xs = [x0, x0, x0, x0, x0 + 1.0, x0 + 1.0, x0 + 1.0];
    let zs = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let mut poles = Vec::new();
    for (x, z) in xs.iter().zip(zs) {
        for j in 0..2 {
            poles.push(Vec3::new(*x, x0 + f64::from(j), z));
        }
    }
    let surface = Surface::Nurbs(NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, a, 2.0 * a, 3.0 * a, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: None,
        count_u: 7,
        count_v: 2,
    });
    let u0 = -0.9e-6;
    let trims = vec![
        line([u0, 0.0], [0.5, 0.0]),
        line([0.5, 0.0], [0.5, 1.0]),
        line([0.5, 1.0], [u0, 1.0]),
        line([u0, 1.0], [u0, 0.0]),
    ];
    FaceDefinition {
        surface,
        trims,
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

#[test]
fn the_widening_holds_occt_rounding_far_from_the_origin() {
    // (origin offset, end span, OCCT's distance past the exact box)
    for (x0, a, occt) in [(1e5, 1e-8, 4.93e-6), (1e6, 1e-9, 9.62e-3)] {
        let b = face_box(&far_net(x0, a), 1e-9).unwrap();
        assert!(b.extrapolated, "X={x0:e} a={a:e}: not extrapolated");
        assert!(
            b.excursion >= occt,
            "X={x0:e} a={a:e}: widening {:e} below OCCT's {occt:e}",
            b.excursion
        );
    }
}
