//! A trim that leaves a net's domain within the tolerance: the box holds
//! the surface OCCT extrapolates there (`Geom_BSplineSurface::Value` past
//! the knots continues the end span's polynomial), and a net whose
//! extrapolated weights may vanish refuses.
//!
//! The nets are the independent review's counterexamples (R2 review 1, C1):
//! degree 3 by 1 with x-poles `0, 1, 1, 1, 1`, y the v pole row, trimmed to
//! `u = -0.99e-6` (inside the relative 1e-6 tolerance), and a degree 3 by 3
//! net trimmed past a corner. The `OCCT_*` points are `Geom_BSplineSurface::Value`
//! at the noted parameters, from the static OCCT prefix with
//! `-ffp-contract=off`, printed with 17 significant digits and written
//! here as the shortest literal of the same value (clippy's precision lint
//! does not always accept that one).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    clippy::excessive_precision
)]

use geospec_facts::{
    face_box, Curve2, FaceDefinition, Material, NurbsCurve2, NurbsSurface, Orientation,
    RefusalCode, Sides, Surface, TrimPiece, Vec3,
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

fn rectangle(u0: f64, u1: f64, v0: f64, v1: f64) -> FaceDefinition {
    FaceDefinition {
        surface: Surface::Plane {
            origin: Vec3::ZERO,
            u_axis: Vec3::new(1.0, 0.0, 0.0),
            v_axis: Vec3::new(0.0, 1.0, 0.0),
        },
        trims: vec![
            line([u0, v0], [u1, v0]),
            line([u1, v0], [u1, v1]),
            line([u1, v1], [u0, v1]),
            line([u0, v1], [u0, v0]),
        ],
        loops: vec![4],
        orientation: Orientation::Forward,
        material: Material::Neither,
    }
}

const X: [f64; 5] = [0.0, 1.0, 1.0, 1.0, 1.0];
/// The trims' excursion below the domain's low end, relative to its width 1.
const BEYOND: f64 = -0.99e-6;

/// Degree 3 by 1: pole `(i, j)` at `(X[i], j, 0)`, the first column's
/// weight `w0`, the u knots `0^4, a, 1^4`.
fn strip_net(a: f64, w0: Option<f64>) -> NurbsSurface {
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for (i, x) in X.iter().enumerate() {
        for j in 0..2 {
            poles.push(Vec3::new(*x, j as f64, 0.0));
            weights.push(if i == 0 { w0.unwrap_or(1.0) } else { 1.0 });
        }
    }
    NurbsSurface {
        degree_u: 3,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles,
        weights: w0.map(|_| weights),
        count_u: 5,
        count_v: 2,
    }
}

/// Degree 3 by 3, knots `0^4, a, 1^4` both ways. `bump`: pole `(i, j)` at
/// `(X[i], X[j], 0)` with z 1 at the corner pole only (non-rational);
/// otherwise at `(X[i], X[j], X[i] X[j])` with the corner pole's weight 2e-3.
fn corner_net(a: f64, bump: bool) -> NurbsSurface {
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let corner = i == 0 && j == 0;
            let z = if bump {
                f64::from(u8::from(corner))
            } else {
                X[i] * X[j]
            };
            poles.push(Vec3::new(X[i], X[j], z));
            weights.push(if corner { 2e-3 } else { 1.0 });
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0];
    NurbsSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles,
        weights: (!bump).then_some(weights),
        count_u: 5,
        count_v: 5,
    }
}

fn net_face(net: NurbsSurface, v0: f64) -> FaceDefinition {
    let mut f = rectangle(BEYOND, 1.0, v0, 1.0);
    f.surface = Surface::Nurbs(net);
    f
}

/// The trims' excursion above the domain's high end.
const ABOVE: f64 = 1.0 + 0.99e-6;

/// [`strip_net`] mirrored to the high end: x-poles `1, 1, 1, 1, 0`, the
/// last column's weight `w4`, the u knots `0^4, a, 1^4`.
fn high_strip_net(a: f64, w4: Option<f64>) -> NurbsSurface {
    let mut n = strip_net(a, None);
    let mut weights = Vec::new();
    for (i, x) in X.iter().rev().enumerate() {
        for j in 0..2 {
            n.poles[i * 2 + j] = Vec3::new(*x, j as f64, 0.0);
            weights.push(if i == 4 { w4.unwrap_or(1.0) } else { 1.0 });
        }
    }
    n.weights = w4.map(|_| weights);
    n
}

/// [`corner_net`] (rational) mirrored to the high-high corner, knots
/// `0^4, 0.99, 1^4` both ways.
fn high_corner_net() -> NurbsSurface {
    let mut n = corner_net(0.99, false);
    let mut weights = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (a, b) = (X[4 - i], X[4 - j]);
            n.poles[i * 5 + j] = Vec3::new(a, b, a * b);
            weights.push(if i == 4 && j == 4 { 2e-3 } else { 1.0 });
        }
    }
    n.weights = Some(weights);
    n
}

fn high_face(net: NurbsSurface, v1: f64) -> FaceDefinition {
    let mut f = rectangle(0.0, ABOVE, 0.0, v1);
    f.surface = Surface::Nurbs(net);
    f
}

/// Every point inside the outer box, and the box's sides ordered.
fn holds(what: &str, s: &Sides, points: &[[f64; 3]]) {
    for p in points {
        let v = [-p[0], -p[1], -p[2], p[0], p[1], p[2]];
        for k in 0..6 {
            assert!(
                s.outer[k].is_finite() && s.inner[k].is_finite(),
                "{what} side {k}: not finite: [{}, {}]",
                s.inner[k],
                s.outer[k]
            );
            assert!(
                v[k] <= s.outer[k],
                "{what}: OCCT point {p:?} beyond side {k}: {} > outer {}",
                v[k],
                s.outer[k]
            );
            assert!(s.inner[k] <= s.outer[k], "{what} side {k}");
        }
    }
}

const OCCT_A: [[f64; 3]; 19] = [
    [-1294.029000000001, 0.0, 0.0], // (u, v) = (-9.9e-07, 0.0)
    [-1294.029000000001, 0.5, 0.0], // (u, v) = (-9.9e-07, 0.5)
    [-1294.029000000001, 1.0, 0.0], // (u, v) = (-9.9e-07, 1.0)
    [-613.125, 0.0, 0.0],           // (u, v) = (-7.5e-07, 0.0)
    [-613.125, 0.5, 0.0],           // (u, v) = (-7.5e-07, 0.5)
    [-613.125, 1.0, 0.0],           // (u, v) = (-7.5e-07, 1.0)
    [-215.0, 0.0, 0.0],             // (u, v) = (-5e-07, 0.0)
    [-215.0, 0.5, 0.0],             // (u, v) = (-5e-07, 0.5)
    [-215.0, 1.0, 0.0],             // (u, v) = (-5e-07, 1.0)
    [-41.875, 0.0, 0.0],            // (u, v) = (-2.5e-07, 0.0)
    [-41.875, 0.5, 0.0],            // (u, v) = (-2.5e-07, 0.5)
    [-41.875, 1.0, 0.0],            // (u, v) = (-2.5e-07, 1.0)
    [0.0, 0.0, 0.0],                // (u, v) = (0.0, 0.0)
    [0.0, 0.5, 0.0],                // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                // (u, v) = (0.0, 1.0)
    [1.0, 0.0, 0.0],                // (u, v) = (0.001, 0.0)
    [1.0, 0.0, 0.0],                // (u, v) = (0.5, 0.0)
    [1.0, 0.5, 0.0],                // (u, v) = (0.5, 0.5)
    [1.0, 0.0, 0.0],                // (u, v) = (1.0, 0.0)
];
const OCCT_B_5E4: [[f64; 3]; 19] = [
    [-1.4623410022779761, 0.0, 0.0], // (u, v) = (-9.9e-07, 0.0)
    [-1.4623410022779761, 0.5, 0.0], // (u, v) = (-9.9e-07, 0.5)
    [-1.4623410022779761, 1.0, 0.0], // (u, v) = (-9.9e-07, 1.0)
    [-0.8179587329515317, 0.0, 0.0], // (u, v) = (-7.5e-07, 0.0)
    [-0.8179587329515317, 0.5, 0.0], // (u, v) = (-7.5e-07, 0.5)
    [-0.8179587329515317, 1.0, 0.0], // (u, v) = (-7.5e-07, 1.0)
    [-0.4285102118079395, 0.0, 0.0], // (u, v) = (-5e-07, 0.0)
    [-0.4285102118079395, 0.5, 0.0], // (u, v) = (-5e-07, 0.5)
    [-0.4285102118079395, 1.0, 0.0], // (u, v) = (-5e-07, 1.0)
    [-0.176460208136068, 0.0, 0.0],  // (u, v) = (-2.5e-07, 0.0)
    [-0.176460208136068, 0.5, 0.0],  // (u, v) = (-2.5e-07, 0.5)
    [-0.176460208136068, 1.0, 0.0],  // (u, v) = (-2.5e-07, 1.0)
    [0.0, 0.0, 0.0],                 // (u, v) = (0.0, 0.0)
    [0.0, 0.5, 0.0],                 // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                 // (u, v) = (0.0, 1.0)
    [0.9986567881944765, 0.0, 0.0],  // (u, v) = (0.001, 0.0)
    [1.0, 0.0, 0.0],                 // (u, v) = (0.5, 0.0)
    [1.0, 0.5, 0.0],                 // (u, v) = (0.5, 0.5)
    [1.0, 0.0, 0.0],                 // (u, v) = (1.0, 0.0)
];
const OCCT_B_4E4: [[f64; 3]; 19] = [
    [-2.881279564336016, 0.0, 0.0],   // (u, v) = (-9.9e-07, 0.0)
    [-2.881279564336016, 0.5, 0.0],   // (u, v) = (-9.9e-07, 0.5)
    [-2.881279564336016, 1.0, 0.0],   // (u, v) = (-9.9e-07, 1.0)
    [-1.2852736094623887, 0.0, 0.0],  // (u, v) = (-7.5e-07, 0.0)
    [-1.2852736094623887, 0.5, 0.0],  // (u, v) = (-7.5e-07, 0.5)
    [-1.2852736094623887, 1.0, 0.0],  // (u, v) = (-7.5e-07, 1.0)
    [-0.5999040137601198, 0.0, 0.0],  // (u, v) = (-5e-07, 0.0)
    [-0.5999040137601198, 0.5, 0.0],  // (u, v) = (-5e-07, 0.5)
    [-0.5999040137601198, 1.0, 0.0],  // (u, v) = (-5e-07, 1.0)
    [-0.23075503034067496, 0.0, 0.0], // (u, v) = (-2.5e-07, 0.0)
    [-0.23075503034067496, 0.5, 0.0], // (u, v) = (-2.5e-07, 0.5)
    [-0.23075503034067496, 1.0, 0.0], // (u, v) = (-2.5e-07, 1.0)
    [0.0, 0.0, 0.0],                  // (u, v) = (0.0, 0.0)
    [0.0, 0.5, 0.0],                  // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                  // (u, v) = (0.0, 1.0)
    [0.9989251418031373, 0.0, 0.0],   // (u, v) = (0.001, 0.0)
    [1.0, 0.0, 0.0],                  // (u, v) = (0.5, 0.0)
    [1.0, 0.5, 0.0],                  // (u, v) = (0.5, 0.5)
    [1.0, 0.0, 0.0],                  // (u, v) = (1.0, 0.0)
];
const OCCT_B_1E3: [[f64; 3]; 19] = [
    [-0.42235614612074857, 0.0, 0.0], // (u, v) = (-9.9e-07, 0.0)
    [-0.42235614612074857, 0.5, 0.0], // (u, v) = (-9.9e-07, 0.5)
    [-0.42235614612074857, 1.0, 0.0], // (u, v) = (-9.9e-07, 1.0)
    [-0.29026639864764847, 0.0, 0.0], // (u, v) = (-7.5e-07, 0.0)
    [-0.29026639864764847, 0.5, 0.0], // (u, v) = (-7.5e-07, 0.5)
    [-0.29026639864764847, 1.0, 0.0], // (u, v) = (-7.5e-07, 1.0)
    [-0.17644982908633886, 0.0, 0.0], // (u, v) = (-5e-07, 0.0)
    [-0.17644982908633886, 0.5, 0.0], // (u, v) = (-5e-07, 0.5)
    [-0.17644982908633886, 1.0, 0.0], // (u, v) = (-5e-07, 1.0)
    [-0.0810766985201119, 0.0, 0.0],  // (u, v) = (-2.5e-07, 0.0)
    [-0.0810766985201119, 0.5, 0.0],  // (u, v) = (-2.5e-07, 0.5)
    [-0.0810766985201119, 1.0, 0.0],  // (u, v) = (-2.5e-07, 1.0)
    [0.0, 0.0, 0.0],                  // (u, v) = (0.0, 0.0)
    [0.0, 0.5, 0.0],                  // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                  // (u, v) = (0.0, 1.0)
    [0.9973171799844699, 0.0, 0.0],   // (u, v) = (0.001, 0.0)
    [1.0, 0.0, 0.0],                  // (u, v) = (0.5, 0.0)
    [1.0, 0.5, 0.0],                  // (u, v) = (0.5, 0.5)
    [1.0, 0.0, 0.0],                  // (u, v) = (1.0, 0.0)
];
const OCCT_C: [[f64; 3]; 28] = [
    [
        -0.21110212790855634,
        -0.21110212790855634,
        6.27035392294448e-05,
    ], // (u, v) = (-9.9e-07, -9.9e-07)
    [
        -0.19116165259274442,
        -0.09654155865381225,
        2.867568162526183e-05,
    ], // (u, v) = (-9.9e-07, -5e-07)
    [-0.1743575761132918, 0.0, 0.0], // (u, v) = (-9.9e-07, 0.0)
    [
        -0.0010910495609268828,
        0.9954382531138257,
        -0.00029567443101118497,
    ], // (u, v) = (-9.9e-07, 0.001)
    [-0.00029702940396984654, 1.0, -0.00029702940396984654], // (u, v) = (-9.9e-07, 0.5)
    [-0.00029702940396984654, 1.0, -0.00029702940396984654], // (u, v) = (-9.9e-07, 1.0)
    [
        -0.09654155865381225,
        -0.19116165259274442,
        2.867568162526183e-05,
    ], // (u, v) = (-5e-07, -9.9e-07)
    [
        -0.08821051489120639,
        -0.08821051489120639,
        1.3232238823596885e-05,
    ], // (u, v) = (-5e-07, -5e-07)
    [-0.08107231636045177, 0.0, 0.0], // (u, v) = (-5e-07, 0.0)
    [
        -0.0005507917247713459,
        0.995047296224113,
        -0.00014926455741303457,
    ], // (u, v) = (-5e-07, 0.001)
    [-0.00015000750012531658, 1.0, -0.00015000750012531658], // (u, v) = (-5e-07, 0.5)
    [-0.00015000750012531658, 1.0, -0.00015000750012531658], // (u, v) = (-5e-07, 1.0)
    [0.0, -0.1743575761132918, 0.0], // (u, v) = (0.0, -9.9e-07)
    [0.0, -0.08107231636045177, 0.0], // (u, v) = (0.0, -5e-07)
    [0.0, 0.0, 0.0],                 // (u, v) = (0.0, 0.0)
    [0.0, 0.9946487164994239, 0.0],  // (u, v) = (0.0, 0.001)
    [0.0, 1.0, 0.0],                 // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                 // (u, v) = (0.0, 1.0)
    [
        0.9954382531138257,
        -0.0010910495609268828,
        -0.0002956744310111849,
    ], // (u, v) = (0.001, -9.9e-07)
    [
        0.9950472962241133,
        -0.000550791724771346,
        -0.0001492645574130346,
    ], // (u, v) = (0.001, -5e-07)
    [0.9946487164994239, 0.0, 0.0],  // (u, v) = (0.001, 0.0)
    [1.0, -0.00029702940396984654, -0.00029702940396984654], // (u, v) = (0.5, -9.9e-07)
    [1.0, -0.00015000750012531658, -0.00015000750012531658], // (u, v) = (0.5, -5e-07)
    [1.0, 0.0, 0.0],                 // (u, v) = (0.5, 0.0)
    [1.0, 1.0, 1.0],                 // (u, v) = (0.5, 0.5)
    [1.0, -0.00029702940396984654, -0.00029702940396984654], // (u, v) = (1.0, -9.9e-07)
    [1.0, -0.00015000750012531658, -0.00015000750012531658], // (u, v) = (1.0, -5e-07)
    [1.0, 0.0, 0.0],                 // (u, v) = (1.0, 0.0)
];
const OCCT_D: [[f64; 3]; 28] = [
    [-1294.029000000175, -1294.029000000001, 1677100.1108410018], // (u, v) = (-9.9e-07, -9.9e-07)
    [-1294.0290000000205, -215.0, 279726.26400000014],            // (u, v) = (-9.9e-07, -5e-07)
    [-1294.029000000001, 0.0, 1295.0290000000007],                // (u, v) = (-9.9e-07, 0.0)
    [-1294.029000000001, 1.0, 0.0],                               // (u, v) = (-9.9e-07, 0.001)
    [-1294.029000000001, 1.0, 0.0],                               // (u, v) = (-9.9e-07, 0.5)
    [-1294.029000000001, 1.0, 0.0],                               // (u, v) = (-9.9e-07, 1.0)
    [-215.0, -1294.029000000001, 279726.26400000014],             // (u, v) = (-5e-07, -9.9e-07)
    [-215.0, -215.0, 46656.0],                                    // (u, v) = (-5e-07, -5e-07)
    [-215.0, 0.0, 216.0],                                         // (u, v) = (-5e-07, 0.0)
    [-215.0, 1.0, 0.0],                                           // (u, v) = (-5e-07, 0.001)
    [-215.0, 1.0, 0.0],                                           // (u, v) = (-5e-07, 0.5)
    [-215.0, 1.0, 0.0],                                           // (u, v) = (-5e-07, 1.0)
    [0.0, -1294.029000000001, 1295.0290000000007],                // (u, v) = (0.0, -9.9e-07)
    [0.0, -215.0, 216.0],                                         // (u, v) = (0.0, -5e-07)
    [0.0, 0.0, 1.0],                                              // (u, v) = (0.0, 0.0)
    [0.0, 1.0, 0.0],                                              // (u, v) = (0.0, 0.001)
    [0.0, 1.0, 0.0],                                              // (u, v) = (0.0, 0.5)
    [0.0, 1.0, 0.0],                                              // (u, v) = (0.0, 1.0)
    [1.0, -1294.029000000001, 0.0],                               // (u, v) = (0.001, -9.9e-07)
    [1.0, -215.0, 0.0],                                           // (u, v) = (0.001, -5e-07)
    [1.0, 0.0, 0.0],                                              // (u, v) = (0.001, 0.0)
    [1.0, -1294.029000000001, 0.0],                               // (u, v) = (0.5, -9.9e-07)
    [1.0, -215.0, 0.0],                                           // (u, v) = (0.5, -5e-07)
    [1.0, 0.0, 0.0],                                              // (u, v) = (0.5, 0.0)
    [1.0, 1.0, 0.0],                                              // (u, v) = (0.5, 0.5)
    [1.0, -1294.029000000001, 0.0],                               // (u, v) = (1.0, -9.9e-07)
    [1.0, -215.0, 0.0],                                           // (u, v) = (1.0, -5e-07)
    [1.0, 0.0, 0.0],                                              // (u, v) = (1.0, 0.0)
];

const OCCT_HIGH_A: [[f64; 3]; 19] = [
    [-1294.0290017807256, 0.0, 0.0], // (u, v) = (1.00000099, 0.0)
    [-1294.0290017807256, 0.5, 0.0], // (u, v) = (1.00000099, 0.5)
    [-1294.0290017807256, 1.0, 0.0], // (u, v) = (1.00000099, 1.0)
    [-613.1250010828837, 0.0, 0.0],  // (u, v) = (1.00000075, 0.0)
    [-613.1250010828837, 0.5, 0.0],  // (u, v) = (1.00000075, 0.5)
    [-613.1250010828837, 1.0, 0.0],  // (u, v) = (1.00000075, 1.0)
    [-215.00000035971226, 0.0, 0.0], // (u, v) = (1.0000005, 0.0)
    [-215.00000035971226, 0.5, 0.0], // (u, v) = (1.0000005, 0.5)
    [-215.00000035971226, 1.0, 0.0], // (u, v) = (1.0000005, 1.0)
    [-41.87500006120104, 0.0, 0.0],  // (u, v) = (1.00000025, 0.0)
    [-41.87500006120104, 0.5, 0.0],  // (u, v) = (1.00000025, 0.5)
    [-41.87500006120104, 1.0, 0.0],  // (u, v) = (1.00000025, 1.0)
    [0.0, 0.0, 0.0],                 // (u, v) = (1.0, 0.0)
    [0.0, 0.5, 0.0],                 // (u, v) = (1.0, 0.5)
    [0.0, 1.0, 0.0],                 // (u, v) = (1.0, 1.0)
    [1.0, 1.0, 0.0],                 // (u, v) = (0.999, 1.0)
    [1.0, 0.5, 0.0],                 // (u, v) = (0.5, 0.5)
    [1.0, 1.0, 0.0],                 // (u, v) = (0.5, 1.0)
    [1.0, 1.0, 0.0],                 // (u, v) = (0.0, 1.0)
];
const OCCT_HIGH_B: [[f64; 3]; 19] = [
    [-1.4623410022053027, 0.0, 0.0],  // (u, v) = (1.00000099, 0.0)
    [-1.4623410022053027, 0.5, 0.0],  // (u, v) = (1.00000099, 0.5)
    [-1.4623410022053027, 1.0, 0.0],  // (u, v) = (1.00000099, 1.0)
    [-0.8179587331584145, 0.0, 0.0],  // (u, v) = (1.00000075, 0.0)
    [-0.8179587331584145, 0.5, 0.0],  // (u, v) = (1.00000075, 0.5)
    [-0.8179587331584145, 1.0, 0.0],  // (u, v) = (1.00000075, 1.0)
    [-0.4285102118922011, 0.0, 0.0],  // (u, v) = (1.0000005, 0.0)
    [-0.4285102118922011, 0.5, 0.0],  // (u, v) = (1.0000005, 0.5)
    [-0.4285102118922011, 1.0, 0.0],  // (u, v) = (1.0000005, 1.0)
    [-0.17646020816556782, 0.0, 0.0], // (u, v) = (1.00000025, 0.0)
    [-0.17646020816556782, 0.5, 0.0], // (u, v) = (1.00000025, 0.5)
    [-0.17646020816556782, 1.0, 0.0], // (u, v) = (1.00000025, 1.0)
    [0.0, 0.0, 0.0],                  // (u, v) = (1.0, 0.0)
    [0.0, 0.5, 0.0],                  // (u, v) = (1.0, 0.5)
    [0.0, 1.0, 0.0],                  // (u, v) = (1.0, 1.0)
    [0.9986567881944765, 1.0, 0.0],   // (u, v) = (0.999, 1.0)
    [1.0, 0.5, 0.0],                  // (u, v) = (0.5, 0.5)
    [1.0, 1.0, 0.0],                  // (u, v) = (0.5, 1.0)
    [1.0, 1.0, 0.0],                  // (u, v) = (0.0, 1.0)
];
const OCCT_HIGH_C: [[f64; 3]; 28] = [
    [
        -0.21110212790249824,
        -0.21110212790249824,
        6.270353922637945e-05,
    ], // (u, v) = (1.00000099, 1.00000099)
    [
        -0.1911616525906853,
        -0.09654155866801262,
        2.867568162890083e-05,
    ], // (u, v) = (1.00000099, 1.0000005)
    [-0.17435757610915928, 0.0, 0.0], // (u, v) = (1.00000099, 1.0)
    [
        -0.001091049560904837,
        0.9954382531138098,
        -0.00029567443100521085,
    ], // (u, v) = (1.00000099, 0.999)
    [-0.00029702940396384987, 1.0, -0.00029702940396384987], // (u, v) = (1.00000099, 0.5)
    [-0.00029702940396384987, 1.0, -0.00029702940396384987], // (u, v) = (1.00000099, 0.0)
    [
        -0.0965415586680126,
        -0.19116165259068524,
        2.867568162890083e-05,
    ], // (u, v) = (1.0000005, 1.00000099)
    [
        -0.08821051490548923,
        -0.08821051490548923,
        1.3232238827561143e-05,
    ], // (u, v) = (1.0000005, 1.0000005)
    [-0.08107231637251637, 0.0, 0.0], // (u, v) = (1.0000005, 1.0)
    [
        -0.0005507917248472051,
        0.9950472962241679,
        -0.00014926455743359258,
    ], // (u, v) = (1.0000005, 0.999)
    [-0.00015000750014596866, 1.0, -0.00015000750014596866], // (u, v) = (1.0000005, 0.5)
    [-0.00015000750014596866, 1.0, -0.00015000750014596866], // (u, v) = (1.0000005, 0.0)
    [0.0, -0.17435757610915928, 0.0], // (u, v) = (1.0, 1.00000099)
    [0.0, -0.08107231637251637, 0.0], // (u, v) = (1.0, 1.0000005)
    [0.0, 0.0, 0.0],                  // (u, v) = (1.0, 1.0)
    [0.0, 0.9946487164994237, 0.0],   // (u, v) = (1.0, 0.999)
    [0.0, 1.0, 0.0],                  // (u, v) = (1.0, 0.5)
    [0.0, 1.0, 0.0],                  // (u, v) = (1.0, 0.0)
    [
        0.9954382531138097,
        -0.0010910495609048369,
        -0.00029567443100521085,
    ], // (u, v) = (0.999, 1.00000099)
    [
        0.995047296224168,
        -0.0005507917248472052,
        -0.00014926455743359258,
    ], // (u, v) = (0.999, 1.0000005)
    [0.9946487164994237, 0.0, 0.0],   // (u, v) = (0.999, 1.0)
    [1.0, -0.00029702940396384987, -0.00029702940396384987], // (u, v) = (0.5, 1.00000099)
    [1.0, -0.00015000750014596866, -0.00015000750014596866], // (u, v) = (0.5, 1.0000005)
    [1.0, 0.0, 0.0],                  // (u, v) = (0.5, 1.0)
    [1.0, 1.0, 1.0],                  // (u, v) = (0.5, 0.5)
    [1.0, -0.00029702940396384987, -0.00029702940396384987], // (u, v) = (0.0, 1.00000099)
    [1.0, -0.00015000750014596866, -0.00015000750014596866], // (u, v) = (0.0, 1.0000005)
    [1.0, 0.0, 0.0],                  // (u, v) = (0.0, 1.0)
];

/// Case A: a narrow end span (1e-7), so the trims reach 9.9 end spans past
/// it; the extrapolated cubic reaches x = -1294.
#[test]
fn a_narrow_end_span_holds_the_extrapolated_cubic() {
    let b = face_box(&net_face(strip_net(1e-7, None), 0.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("A", &b.sides, &OCCT_A);
}

/// Case B: a rational net whose first weight is small, so the extrapolated
/// weight falls below every pole's (to 0.41 of the smallest).
#[test]
fn a_falling_weight_holds_the_extrapolated_surface() {
    let b = face_box(&net_face(strip_net(0.01, Some(5e-4)), 0.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("B w0 = 5e-4", &b.sides, &OCCT_B_5E4);
}

/// Case B with w0 = 4e-4: the extrapolated weight falls to 0.26 of the
/// smallest pole weight.
#[test]
fn a_steeper_falling_weight_holds_the_extrapolated_surface() {
    let b = face_box(&net_face(strip_net(0.01, Some(4e-4)), 0.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("B w0 = 4e-4", &b.sides, &OCCT_B_4E4);
}

/// Case B with w0 = 1e-3: the extrapolated weight falls to 0.70 of the
/// smallest pole weight, where the second-order bound's old growth factor
/// assumed no lower than 0.997 of it.
#[test]
fn a_falling_weight_takes_no_second_order_bound_beyond_the_domain() {
    let b = face_box(&net_face(strip_net(0.01, Some(1e-3)), 0.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("B w0 = 1e-3", &b.sides, &OCCT_B_1E3);
}

/// A corner: the trims leave the domain below both u and v, and the corner
/// pole's weight is small, so the surface there moves in x, y and z.
#[test]
fn a_corner_excursion_holds_the_extrapolated_corner() {
    let b = face_box(&net_face(corner_net(0.01, false), BEYOND), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("corner", &b.sides, &OCCT_C);
}

/// A corner of two narrow end spans: the corner pole's basis function grows
/// 1295-fold past each, so z reaches 1.68e6 at the corner while it stays at
/// 1295 where only one parameter leaves the domain. The u strip's bound must
/// take the v extension (the two factors multiply), not only v's domain.
#[test]
fn a_corner_of_two_narrow_end_spans_holds_the_product() {
    let b = face_box(&net_face(corner_net(1e-7, true), BEYOND), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("narrow corner", &b.sides, &OCCT_D);
}

/// With w0 = 2e-4 the extrapolated weight crosses zero inside the strip
/// (OCCT's surface there is at x = +3.06, through infinity): the box cannot
/// bound it and refuses.
#[test]
fn an_extrapolated_weight_that_may_vanish_refuses() {
    let r = face_box(&net_face(strip_net(0.01, Some(2e-4)), 0.0), 1e-7).unwrap_err();
    assert_eq!(r.code, RefusalCode::TrimOutsideDomain, "{r:?}");
    // Inside the domain the same net boxes.
    let mut f = rectangle(0.0, 1.0, 0.0, 1.0);
    f.surface = Surface::Nurbs(strip_net(0.01, Some(2e-4)));
    assert!(!face_box(&f, 1e-7).unwrap().extrapolated);
}

/// Case A at the high end: the extrapolated cubic reaches x = -1294 past
/// u = 1, so the bound must take the high end span.
#[test]
fn a_narrow_high_end_span_holds_the_extrapolated_cubic() {
    let b = face_box(&high_face(high_strip_net(1.0 - 1e-7, None), 1.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("high A", &b.sides, &OCCT_HIGH_A);
}

/// Case B at the high end: the last column's weight 5e-4 falls past u = 1.
#[test]
fn a_falling_final_weight_holds_the_extrapolated_surface() {
    let b = face_box(&high_face(high_strip_net(0.99, Some(5e-4)), 1.0), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("high B", &b.sides, &OCCT_HIGH_B);
}

/// The rational corner at the high end of both u and v.
#[test]
fn a_high_corner_excursion_holds_the_extrapolated_corner() {
    let b = face_box(&high_face(high_corner_net(), ABOVE), 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("high corner", &b.sides, &OCCT_HIGH_C);
}

/// Subnormal weights (R2 review of A7, F1): their rounding is absolute, so
/// the extrapolated weights could be "shown" positive while the exact one
/// crosses zero in the strip. The definition refuses them, as OCCT does.
#[test]
fn subnormal_weights_refuse() {
    let (w0, w1, w2) = (9.4e-323, 6.291526e-318, 5.89346e-318);
    let mut n = strip_net(0.1, Some(1.0));
    let ws = [w0, w1, w2, w2, w2];
    n.weights = Some((0..10).map(|k| ws[k / 2]).collect());
    let mut f = rectangle(-5e-7, 1.0, 0.0, 1.0);
    f.surface = Surface::Nurbs(n);
    let r = face_box(&f, 1e-7).unwrap_err();
    assert_eq!(r.code, RefusalCode::InvalidDefinition, "{r:?}");
}

/// A trim whose computed start rounds into the domain (R2 review of A7,
/// F4): the degree-1 trim from u = -1 to 1 starts at `0.5 - 2^-60`, so its
/// exact start is u = -2^-59 but computes to 0. On case A's net OCCT gives
/// x = -5.2e-11 there; the trims' rounding must reach the excursion.
#[test]
fn a_trim_whose_start_rounds_into_the_domain_widens_the_box() {
    // `Geom_BSplineSurface::Value` at (-2^-59, 0.5).
    const OCCT_START: [f64; 3] = [-5.204170428020699e-11, 0.5, 0.0];
    // 2^-60.
    let b0 = 0.5 - 8.673617379884035e-19;
    let start = [-(1.0 - b0) + b0, 0.5];
    let curve = Curve2::Nurbs(NurbsCurve2 {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        poles: vec![[-1.0, 0.5], [1.0, 0.5]],
        weights: None,
    });
    let mut f = rectangle(0.0, 1.0, 0.0, 1.0);
    f.surface = Surface::Nurbs(strip_net(1e-7, None));
    f.trims = vec![
        TrimPiece {
            curve,
            range: [b0, 0.75],
            reversed: false,
        },
        line([0.5, 0.5], [0.5, 0.75]),
        line([0.5, 0.75], start),
    ];
    f.loops = vec![3];
    let b = face_box(&f, 1e-7).unwrap();
    assert!(b.extrapolated, "{b:?}");
    holds("rounded start", &b.sides, &[OCCT_START]);
}
