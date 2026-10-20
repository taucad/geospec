//! Straight-segment void witness for materials outside the box and cylinder
//! domains.
//!
//! A waypoint outside every material by more than the margin lies in the
//! void. A segment that meets no material face and stays clear by more than
//! the margin joins its two waypoints inside the convex region, so the whole
//! path is one void body. A waypoint inside material by more than the margin
//! decides the claim false. Anything else refuses: a segment that grazes
//! material does not show that the void is broken.

use super::Claim;
use crate::{
    backend::brep::{VoidWitness, VoidWitnessState},
    codec::Json,
};

pub(crate) const PROFILE: &str = "geospec-void-segment-witness-v1";
pub(crate) const MAX_WAYPOINTS: usize = 32;
pub(crate) const MAX_MATERIALS: usize = 16;
/// The absolute part of the margin, above OCCT's extrema and classifier
/// precision for a millimetre model.
const MARGIN_FLOOR_MM: f64 = 1e-6;
/// Below π by more than its binary64 rounding, so a disc area never
/// overstates a section.
const PI_LOWER: f64 = std::f64::consts::PI * (1.0 - 1e-9);

pub(crate) enum Decision {
    Positive {
        evidence: Json,
    },
    Material {
        index: usize,
        evidence: Json,
    },
    Refused {
        message: String,
        suggestion: &'static str,
        evidence: Json,
    },
}

/// Twice the materials' largest shape tolerance plus a fixed floor.
pub(crate) fn margin(tolerance: f64) -> f64 {
    2.0 * tolerance + MARGIN_FLOOR_MM
}

/// The claim shapes this route decides before any geometry is read.
pub(crate) fn admit(claim: &Claim) -> Result<(), &'static str> {
    if !claim.isolated_from.is_empty() {
        return Err("The segment witness proves only that the path is connected; isolation needs a box or cylinder material.");
    }
    if !(2..=MAX_WAYPOINTS).contains(&claim.waypoints.len()) {
        return Err("The segment witness requires 2 to 32 waypoints.");
    }
    Ok(())
}

pub(crate) fn decide(claim: &Claim, witness: &VoidWitness) -> Decision {
    let margin = margin(witness.tolerance);
    let point_rows = witness
        .points
        .iter()
        .zip(&claim.waypoints)
        .enumerate()
        .map(|(index, (row, point))| {
            let (state, occurrence) = match row.state {
                VoidWitnessState::Outside => ("void", None),
                VoidWitnessState::Inside { occurrence } => ("material", Some(occurrence)),
                VoidWitnessState::Undecided => ("undecided", None),
            };
            let mut fields = vec![
                ("index".to_string(), Json::Number(index as f64)),
                ("point".to_string(), point_json(*point)),
                ("state".to_string(), Json::string(state)),
                ("clearance".to_string(), Json::Number(row.clearance)),
            ];
            if let Some(occurrence) = occurrence {
                fields.push((
                    "occurrence".to_string(),
                    Json::Number(f64::from(occurrence)),
                ));
            }
            Json::Object(fields)
        })
        .collect::<Vec<_>>();
    let segment_rows = witness
        .segments
        .iter()
        .enumerate()
        .map(|(index, row)| {
            Json::object([
                ("index", Json::Number(index as f64)),
                ("clearance", Json::Number(row.clearance)),
                ("crossesMaterial", Json::Bool(row.crosses_material)),
                ("witness", point_json(row.witness)),
            ])
        })
        .collect::<Vec<_>>();
    let evidence = |section: Option<Json>| {
        let mut fields = vec![
            ("profile".to_string(), Json::string(PROFILE)),
            ("tolerance".to_string(), Json::Number(witness.tolerance)),
            ("margin".to_string(), Json::Number(margin)),
            ("points".to_string(), Json::Array(point_rows.clone())),
            ("segments".to_string(), Json::Array(segment_rows.clone())),
        ];
        if let Some(section) = section {
            fields.push(("section".to_string(), section));
        }
        Json::Object(fields)
    };
    if witness.points.len() != claim.waypoints.len()
        || witness.segments.len() + 1 != claim.waypoints.len()
    {
        return Decision::Refused {
            message: "The void witness does not match the claim's waypoints.".into(),
            suggestion: "Report this subject; the connector returned inconsistent rows.",
            evidence: evidence(None),
        };
    }
    if let Some(index) = witness.points.iter().position(|row| {
        matches!(row.state, VoidWitnessState::Inside { .. }) && row.clearance > margin
    }) {
        return Decision::Material {
            index,
            evidence: evidence(None),
        };
    }
    if let Some(index) = witness
        .points
        .iter()
        .position(|row| !matches!(row.state, VoidWitnessState::Outside) || row.clearance <= margin)
    {
        return Decision::Refused {
            message: format!(
                "Void waypoint {index} lies within {margin} mm of a material face, so the witness cannot place it."
            ),
            suggestion: "Move the waypoint clear of the material surface.",
            evidence: evidence(None),
        };
    }
    if let Some(index) = witness
        .segments
        .iter()
        .position(|row| row.crosses_material || row.clearance <= margin)
    {
        return Decision::Refused {
            message: format!(
                "The straight segment from waypoint {index} to waypoint {} meets or grazes material, so the witness cannot follow it; that does not show the void is broken.",
                index + 1
            ),
            suggestion: "Add waypoints so that each straight segment stays clear of the material.",
            evidence: evidence(None),
        };
    }
    let Some(minimum) = claim.min_cross_section else {
        return Decision::Positive {
            evidence: evidence(None),
        };
    };
    match section_lower_bound(claim, witness, margin) {
        Ok((area, radius, axis)) => {
            let section = Json::object([
                ("axis", Json::Number(axis as f64)),
                ("radius", Json::Number(radius)),
                ("lowerBound", Json::Number(area)),
            ]);
            if area >= minimum {
                Decision::Positive {
                    evidence: evidence(Some(section)),
                }
            } else {
                Decision::Refused {
                    message: format!(
                        "The witness shows a cross-section of at least {area} mm², below the declared {minimum} mm²; it cannot show a narrower channel."
                    ),
                    suggestion: "Lower `minCrossSection` to what the clear disc shows, or use a box or cylinder material.",
                    evidence: evidence(Some(section)),
                }
            }
        }
        Err(message) => Decision::Refused {
            message: message.into(),
            suggestion: "Use collinear waypoints along one Cartesian axis for `minCrossSection`.",
            evidence: evidence(None),
        },
    }
}

/// A disc around the path, perpendicular to its Cartesian axis, clear of the
/// material and inside the region at every station: its area bounds every
/// section from below. Returns the area, the disc radius and the axis.
fn section_lower_bound(
    claim: &Claim,
    witness: &VoidWitness,
    margin: f64,
) -> Result<(f64, f64, usize), &'static str> {
    let first = claim.waypoints[0];
    let last = claim.waypoints[claim.waypoints.len() - 1];
    let axes = (0..3)
        .filter(|&axis| first[axis] != last[axis])
        .collect::<Vec<_>>();
    let [axis] = axes.as_slice() else {
        return Err("A witness cross-section requires a path along one Cartesian axis.");
    };
    let axis = *axis;
    let transverse = (0..3).filter(|&other| other != axis).collect::<Vec<_>>();
    let ascending = last[axis] > first[axis];
    for pair in claim.waypoints.windows(2) {
        let ordered = if ascending {
            pair[0][axis] < pair[1][axis]
        } else {
            pair[0][axis] > pair[1][axis]
        };
        if !ordered
            || transverse
                .iter()
                .any(|&other| pair[1][other] != first[other])
        {
            return Err(
                "A witness cross-section requires collinear, strictly monotone Cartesian waypoints.",
            );
        }
    }
    let mut radius = witness
        .segments
        .iter()
        .map(|row| row.clearance - margin)
        .fold(f64::INFINITY, f64::min);
    for &other in &transverse {
        radius = radius
            .min(first[other] - claim.region.min[other])
            .min(claim.region.max[other] - first[other]);
    }
    let radius = radius.max(0.0);
    Ok((PI_LOWER * radius * radius, radius, axis))
}

fn point_json(point: [f64; 3]) -> Json {
    Json::Array(point.into_iter().map(Json::Number).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::brep::{Bounds, VoidWitnessPoint, VoidWitnessSegment};

    fn claim(waypoints: Vec<[f64; 3]>, min_cross_section: Option<f64>) -> Claim {
        Claim {
            waypoints,
            materials: vec![0],
            material_paths: vec!["part".into()],
            region: Bounds {
                min: [-4.0, -4.0, 0.0],
                max: [4.0, 4.0, 20.0],
            },
            isolated_from: Vec::new(),
            min_cross_section,
        }
    }

    fn point(state: VoidWitnessState, clearance: f64) -> VoidWitnessPoint {
        VoidWitnessPoint {
            state,
            clearance,
            nearest: [0.0; 3],
        }
    }

    fn segment(crosses_material: bool, clearance: f64) -> VoidWitnessSegment {
        VoidWitnessSegment {
            crosses_material,
            clearance,
            witness: [0.0; 3],
        }
    }

    fn witness(points: Vec<VoidWitnessPoint>, segments: Vec<VoidWitnessSegment>) -> VoidWitness {
        VoidWitness {
            tolerance: 1e-7,
            points,
            segments,
        }
    }

    const AXIS: [[f64; 3]; 2] = [[0.0, 0.0, 1.0], [0.0, 0.0, 19.0]];

    #[test]
    fn clear_segments_prove_the_path() {
        let decision = decide(
            &claim(AXIS.to_vec(), None),
            &witness(
                vec![point(VoidWitnessState::Outside, 6.0); 2],
                vec![segment(false, 6.0)],
            ),
        );
        assert!(matches!(decision, Decision::Positive { .. }));
    }

    #[test]
    fn a_deep_material_waypoint_fails_the_claim() {
        let decision = decide(
            &claim(AXIS.to_vec(), None),
            &witness(
                vec![
                    point(VoidWitnessState::Outside, 6.0),
                    point(VoidWitnessState::Inside { occurrence: 0 }, 1.0),
                ],
                vec![segment(true, 0.0)],
            ),
        );
        assert!(matches!(decision, Decision::Material { index: 1, .. }));
    }

    #[test]
    fn grazing_and_shallow_cases_refuse() {
        let crossing = decide(
            &claim(AXIS.to_vec(), None),
            &witness(
                vec![point(VoidWitnessState::Outside, 6.0); 2],
                vec![segment(true, 0.0)],
            ),
        );
        assert!(matches!(crossing, Decision::Refused { .. }));
        let shallow = decide(
            &claim(AXIS.to_vec(), None),
            &witness(
                vec![
                    point(VoidWitnessState::Outside, 6.0),
                    point(VoidWitnessState::Inside { occurrence: 0 }, 1e-7),
                ],
                vec![segment(false, 1e-7)],
            ),
        );
        assert!(matches!(shallow, Decision::Refused { .. }));
        let undecided = decide(
            &claim(AXIS.to_vec(), None),
            &witness(
                vec![
                    point(VoidWitnessState::Outside, 6.0),
                    point(VoidWitnessState::Undecided, 3.0),
                ],
                vec![segment(false, 3.0)],
            ),
        );
        assert!(matches!(undecided, Decision::Refused { .. }));
    }

    #[test]
    fn the_section_bound_is_a_clear_disc_inside_the_region() {
        let rows = || {
            witness(
                vec![point(VoidWitnessState::Outside, 6.0); 2],
                vec![segment(false, 6.0)],
            )
        };
        // The region's 4 mm half-width caps the 6 mm clearance.
        let Decision::Positive { .. } = decide(&claim(AXIS.to_vec(), Some(50.0)), &rows()) else {
            panic!("a 4 mm disc bounds 50 mm² from below");
        };
        let Decision::Refused { .. } = decide(&claim(AXIS.to_vec(), Some(51.0)), &rows()) else {
            panic!("a lower bound below the minimum refuses, never fails");
        };
        let Decision::Refused { .. } = decide(
            &claim(vec![[0.0, 0.0, 1.0], [1.0, 0.0, 19.0]], Some(1.0)),
            &rows(),
        ) else {
            panic!("an oblique path has no witness section");
        };
    }

    #[test]
    fn isolation_is_not_admitted() {
        let mut isolated = claim(AXIS.to_vec(), None);
        isolated.isolated_from.push([0.0, 0.0, 10.0]);
        assert!(admit(&isolated).is_err());
        assert!(admit(&claim(AXIS.to_vec(), None)).is_ok());
    }
}
