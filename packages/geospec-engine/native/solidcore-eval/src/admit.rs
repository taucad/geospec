//! Fail-closed admission and fact refusal: GeoSpec's refusal taxonomy over
//! solidcore's import and build.
//!
//! A file is admitted only when the importer found nothing an admission profile
//! refuses on (`step_import::admission`: Part 21 and header defects, entity
//! parameters OCCT's reader refuses, dangling and external references, units the
//! file does not state, assembly placements the importer does not apply) and it is
//! within the engine's subject size limit. An admitted body's facts are refused when
//! they cannot describe a solid: a box at solidcore's infinity marker or not finite,
//! a volume that is not finite and positive or exceeds the body's own box, a centre
//! of mass outside that box, or an edge or vertex tolerance raised above a claim's
//! tolerance. A refusal is a typed reason; nothing is repaired.

use solidcore::math::INFINITE_PARAMETER;
use step_import::admission::Finding;

/// The engine's default subject size limit (`max_subject_bytes`, 64 MiB): a larger
/// file is refused before it is read.
pub const MAX_SUBJECT_BYTES: usize = 64 * 1024 * 1024;

/// The zone tolerance solidcore builds at (`SM_ZONE_TOL_3D`).
pub const ZONE_TOLERANCE: f64 = solidcore::geom::tol::ZONE_TOL_3D;

/// Whether an edge or vertex tolerance `t` was raised by the build above the zone
/// tolerance (beyond a relative 1e-6, the round-off of the tolerance arithmetic).
pub fn raised(t: f64) -> bool {
    t.is_nan() || t > ZONE_TOLERANCE * (1.0 + 1e-6)
}

/// Why an admission was refused: the class of the first finding and every finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// The refusal class (`size`, or a [`step_import::admission::Class`] name).
    pub class: &'static str,
    /// Every reason, `class: message`, in the order found (at most [`MAX_REASONS`]).
    pub reasons: Vec<String>,
    /// How many findings there were in all.
    pub count: usize,
}

/// The most reasons a refusal lists.
pub const MAX_REASONS: usize = 8;

/// The admission refusal for a file of `bytes` bytes whose import found `findings`,
/// or `None` when it is admitted.
pub fn admission(bytes: usize, findings: &[Finding]) -> Option<Refusal> {
    if bytes > MAX_SUBJECT_BYTES {
        return Some(Refusal {
            class: "size",
            reasons: vec![format!(
                "size: {bytes} bytes; the subject limit is {MAX_SUBJECT_BYTES} bytes"
            )],
            count: 1,
        });
    }
    let first = findings.first()?;
    Some(Refusal {
        class: first.class.as_str(),
        reasons: findings
            .iter()
            .take(MAX_REASONS)
            .map(|f| match f.step_id {
                Some(id) => format!("{}: #{id}: {}", f.class.as_str(), f.message),
                None => format!("{}: {}", f.class.as_str(), f.message),
            })
            .collect(),
        count: findings.len(),
    })
}

/// One body's facts, as the harness computed them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    /// Whether the body is a manifold solid (its volume and centroid are facts).
    pub manifold: bool,
    pub volume: Option<f64>,
    pub area: Option<f64>,
    pub centroid: Option<[f64; 3]>,
    /// `[min x, min y, min z, max x, max y, max z]`.
    pub bounds: Option<[f64; 6]>,
    /// The largest edge or vertex tolerance of the built B-rep.
    pub max_tolerance: Option<f64>,
}

/// The relative slack of the box containment tests (of the box volume, and of its
/// diagonal for the centre of mass), as the Mac fail-open scan used.
const SLACK: f64 = 1e-6;

/// Why a body's facts are refused, or `None` when they stand. `claim_tolerance` is
/// the finest tolerance a claim on them asks for, when one does.
pub fn facts(f: &Facts, claim_tolerance: Option<f64>) -> Option<String> {
    if let Some(b) = f.bounds {
        if b.iter().any(|x| !x.is_finite()) {
            return Some("extent: not finite".into());
        }
        if b.iter().any(|x| x.abs() >= INFINITE_PARAMETER) {
            return Some(format!(
                "extent: at solidcore's infinity marker ({INFINITE_PARAMETER})"
            ));
        }
    }
    if let Some(a) = f.area {
        if !a.is_finite() || a < 0.0 {
            return Some(format!("area: {a} is not finite and non-negative"));
        }
    }
    if let (Some(t), Some(claim)) = (f.max_tolerance, claim_tolerance) {
        if t > claim {
            return Some(format!(
                "tolerance: raised to {t} mm, coarser than the claim's {claim} mm"
            ));
        }
    }
    if !f.manifold {
        return None;
    }
    let Some(v) = f.volume else { return None };
    if !v.is_finite() || v <= 0.0 {
        return Some(format!("volume: {v} is not finite and positive"));
    }
    let Some(b) = f.bounds else { return None };
    let size = [b[3] - b[0], b[4] - b[1], b[5] - b[2]];
    let box_volume = size[0] * size[1] * size[2];
    if v > box_volume * (1.0 + SLACK) {
        return Some(format!("volume: {v} exceeds its box's {box_volume}"));
    }
    if let Some(c) = f.centroid {
        let diagonal = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt();
        let slack = SLACK * diagonal;
        let outside = (0..3).any(|i| match (c.get(i), b.get(i), b.get(i + 3)) {
            (Some(&x), Some(&lo), Some(&hi)) => !x.is_finite() || x < lo - slack || x > hi + slack,
            _ => true,
        });
        if outside {
            return Some(format!("centroid: {c:?} outside its box"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use step_import::admission::Class;

    fn cube() -> Facts {
        Facts {
            manifold: true,
            volume: Some(1000.0),
            area: Some(600.0),
            centroid: Some([5.0, 5.0, 5.0]),
            bounds: Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
            max_tolerance: Some(1e-5),
        }
    }

    #[test]
    fn a_clean_file_is_admitted() {
        assert_eq!(admission(100, &[]), None);
    }

    #[test]
    fn findings_refuse_with_the_first_class() {
        let findings = [
            Finding {
                class: Class::UnitsAssumed,
                step_id: Some(7),
                message: "no unit".into(),
            },
            Finding {
                class: Class::PlacementsNotApplied,
                step_id: None,
                message: "2 instances".into(),
            },
        ];
        assert_eq!(
            admission(100, &findings),
            Some(Refusal {
                class: "units-assumed",
                reasons: vec![
                    "units-assumed: #7: no unit".to_string(),
                    "placements-not-applied: 2 instances".to_string(),
                ],
                count: 2,
            })
        );
    }

    #[test]
    fn an_oversized_subject_is_refused() {
        assert!(admission(MAX_SUBJECT_BYTES + 1, &[]).is_some_and(|r| r.class == "size"));
        assert_eq!(admission(MAX_SUBJECT_BYTES, &[]), None);
    }

    #[test]
    fn a_sound_body_stands() {
        assert_eq!(facts(&cube(), None), None);
        assert_eq!(facts(&cube(), Some(1e-3)), None);
    }

    #[test]
    fn sentinel_and_non_finite_extents_refuse() {
        let mut f = cube();
        f.bounds = Some([0.0, 0.0, -INFINITE_PARAMETER, 10.0, 10.0, 10.0]);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("extent: at")));
        f.bounds = Some([0.0, 0.0, f64::NAN, 10.0, 10.0, 10.0]);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("extent: not finite")));
        // a sheet (no volume) at the marker refuses too
        f.manifold = false;
        f.volume = None;
        f.bounds = Some([0.0, 0.0, 0.0, 10.0, 10.0, INFINITE_PARAMETER]);
        assert!(facts(&f, None).is_some());
    }

    #[test]
    fn invariant_violations_refuse() {
        let mut f = cube();
        f.volume = Some(0.0);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("volume:")));
        f.volume = Some(-3.0);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("volume:")));
        f.volume = Some(1001.0);
        assert!(facts(&f, None).is_some_and(|r| r.contains("exceeds its box")));
        f = cube();
        f.centroid = Some([5.0, 5.0, 11.0]);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("centroid:")));
        f = cube();
        f.area = Some(f64::INFINITY);
        assert!(facts(&f, None).is_some_and(|r| r.starts_with("area:")));
        // a body that is not a manifold solid has no volume to test
        f = cube();
        f.manifold = false;
        f.volume = Some(-1.0);
        assert_eq!(facts(&f, None), None);
    }

    #[test]
    fn a_tolerance_is_raised_beyond_round_off() {
        assert!(!raised(ZONE_TOLERANCE));
        assert!(!raised(1.000_000_000_2e-5));
        assert!(raised(1.000_002e-5));
        assert!(raised(f64::NAN));
    }

    #[test]
    fn a_raised_tolerance_refuses_finer_claims_only() {
        let mut f = cube();
        f.max_tolerance = Some(2e-3);
        assert_eq!(facts(&f, None), None);
        assert_eq!(facts(&f, Some(1e-2)), None);
        assert!(facts(&f, Some(1e-3)).is_some_and(|r| r.starts_with("tolerance:")));
    }
}
