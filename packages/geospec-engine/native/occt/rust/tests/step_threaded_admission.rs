//! Native STEP admission threads by default (charter D30): independent manifold
//! solids are completed on worker threads and the reader is torn down in the
//! background. Each threaded admission returns the serial bytes. A solid that
//! shares curves with a solid still being completed is completed in place, and a
//! threaded transfer that diverges is read again in place. A lookup of anything
//! a pending solid's translation bound waits for the pending completions.
use geospec_engine_native_occt::{BrepEntity, BrepSubject, Document, TessellationProfile};
use std::{path::PathBuf, sync::Mutex};

extern "C" {
    fn geospec_occt_qualify_step_admission_mode(mode: i32, force_divergence: bool);
    fn geospec_occt_qualify_last_step_admission(
        threaded: *mut i32,
        deferred_fixes: *mut i32,
        shared_fixes: *mut i32,
        reshaped_fixes: *mut i32,
        serial_retries: *mut i32,
    );
}

// The mode override is process-wide: one admission test at a time.
static MODE: Mutex<()> = Mutex::new(());

const SERIAL: i32 = 0;
const THREADED: i32 = 1;

#[derive(Debug, PartialEq, Eq)]
struct Admission {
    threaded: bool,
    deferred_fixes: i32,
    shared_fixes: i32,
    reshaped_fixes: i32,
    serial_retries: i32,
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("retained fixture must be readable")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

// Admission, structure, source numerics and a fine mesh of the whole shape.
// Debug text distinguishes every finite f64, including signed zero.
fn admitted(document: &Document) -> String {
    let mesh = BrepSubject::tessellate(
        document,
        BrepEntity::Whole,
        TessellationProfile {
            linear_deflection_mm: 0.01,
            angular_deflection_rad: 0.1,
        },
    )
    .map(|mesh| {
        let mut bytes = Vec::new();
        for point in &mesh.positions {
            for value in point {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        for triangle in &mesh.triangles {
            for value in triangle {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        fnv1a64(&bytes)
    });
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:x?}",
        document.admission_facts(),
        document.step_subject_metadata(),
        document.faces(),
        document.document_rows(),
        document.source_occurrences(),
        mesh
    )
}

fn admit(name: &str, mode: i32, force_divergence: bool) -> (String, Admission) {
    let _guard = MODE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: plain value setters; the MODE guard keeps them to one admission.
    unsafe { geospec_occt_qualify_step_admission_mode(mode, force_divergence) };
    let document = Document::from_step(&fixture(name));
    let (mut threaded, mut deferred_fixes, mut shared_fixes) = (0, 0, 0);
    let (mut reshaped_fixes, mut serial_retries) = (0, 0);
    // SAFETY: five valid out-pointers to this frame's integers.
    unsafe {
        geospec_occt_qualify_last_step_admission(
            &mut threaded,
            &mut deferred_fixes,
            &mut shared_fixes,
            &mut reshaped_fixes,
            &mut serial_retries,
        );
        geospec_occt_qualify_step_admission_mode(-1, false);
    }
    (
        match document {
            Ok(document) => admitted(&document),
            Err(refusal) => format!("refused: {refusal:?}"),
        },
        Admission {
            threaded: threaded == 1,
            deferred_fixes,
            shared_fixes,
            reshaped_fixes,
            serial_retries,
        },
    )
}

// Admits `name` serially once, then threaded `reps` times; each threaded
// admission returns the serial bytes. Returns what the last threaded one did.
fn threaded_admissions_match_serial(name: &str, reps: usize) -> Admission {
    let (serial, serial_admission) = admit(name, SERIAL, false);
    assert!(!serial.starts_with("refused"), "{name}: {serial}");
    assert_eq!(
        serial_admission,
        Admission {
            threaded: false,
            deferred_fixes: 0,
            shared_fixes: 0,
            reshaped_fixes: 0,
            serial_retries: 0
        }
    );
    let mut last = None;
    for _ in 0..reps {
        let (threaded, admission) = admit(name, THREADED, false);
        assert_eq!(threaded, serial, "{name}");
        if let Some(previous) = &last {
            assert_eq!(&admission, previous, "{name}");
        }
        last = Some(admission);
    }
    last.unwrap()
}

#[test]
fn independent_solids_are_completed_on_workers_with_the_serial_bytes() {
    // Each completion replaces results the transfer bound, so the merge path runs.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-assembly.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 2,
            shared_fixes: 0,
            reshaped_fixes: 2,
            serial_retries: 0
        }
    );
}

#[test]
fn a_face_authored_reversed_is_completed_with_the_serial_bytes() {
    // Cube B's face #2110 is authored with same_sense .F.; its completion replaces bound results.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-reversed-face.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 2,
            shared_fixes: 0,
            reshaped_fixes: 2,
            serial_retries: 0
        }
    );
}

#[test]
fn a_solid_sharing_curves_with_a_pending_solid_is_completed_in_place() {
    // Cube B's edges use cube A's LINE entities, so the transfer process hands
    // both solids the same curves; cube B waits for cube A's completion.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-shared-curves.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 1,
            shared_fixes: 1,
            reshaped_fixes: 1,
            serial_retries: 0
        }
    );
    // The shared curves change nothing measured: each cube is the other's copy.
    let (serial, _) = admit("two-cube-shared-curves.step", SERIAL, false);
    let (independent, _) = admit("two-cube-assembly.step", SERIAL, false);
    let facts = |text: &str| text.lines().next().unwrap().to_owned();
    assert_eq!(facts(&serial), facts(&independent));
}

#[test]
fn a_solid_whose_void_is_a_pending_solid_shell_is_completed_in_place() {
    // Cube A is a BREP_WITH_VOIDS whose void is cube B's closed shell.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-outside-void.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 1,
            shared_fixes: 1,
            reshaped_fixes: 1,
            serial_retries: 0
        }
    );
}

#[test]
fn a_faceted_brep_with_voids_is_completed_in_place() {
    // The voids of a FACETED_BREP_AND_BREP_WITH_VOIDS are not walked: cube A
    // is completed in place, and cube B, whose shell is that void, after it.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-faceted-void.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 1,
            shared_fixes: 0,
            reshaped_fixes: 1,
            serial_retries: 0
        }
    );
}

#[test]
fn faces_of_a_pending_solid_in_a_shape_aspect_are_read_after_its_completion() {
    // SHAPE_REPRESENTATION #9004 lists three faces of cube A, already bound by
    // cube A's translation while its completion may still run.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-face-aspect.step", 16),
        Admission {
            threaded: true,
            deferred_fixes: 2,
            shared_fixes: 0,
            reshaped_fixes: 2,
            serial_retries: 0
        }
    );
}

#[test]
fn a_pending_solid_and_its_own_faces_in_one_representation_match_serial() {
    // Cube A's representation lists the solid and then two of its faces.
    assert_eq!(
        threaded_admissions_match_serial("two-cube-solid-and-faces.step", 16),
        Admission {
            threaded: true,
            deferred_fixes: 2,
            shared_fixes: 0,
            reshaped_fixes: 2,
            serial_retries: 0
        }
    );
}

#[test]
fn a_stand_in_that_cannot_hold_its_completion_is_read_again_in_place() {
    // Every stand-in check in the actor fails: the actor reports the transfer
    // as diverged, stops deferring, and the bridge reads it again in place.
    let (serial, _) = admit("two-cube-assembly.step", SERIAL, false);
    for _ in 0..4 {
        let (retried, admission) = admit("two-cube-assembly.step", THREADED, true);
        assert_eq!(retried, serial);
        assert!(admission.threaded);
        assert!((1..=2).contains(&admission.deferred_fixes), "{admission:?}");
        assert_eq!(admission.serial_retries, 1);
    }
}

#[test]
fn a_diverged_transfer_awaits_its_pending_completions_before_completing_in_place() {
    // Cubes A to F are independent and cube G shares curves of cubes D and F.
    // At most two completions per worker stay pending, so cube E's submission at
    // the latest waits for cube A's completion. When its stand-in check fails, the
    // transfer stops deferring while later completions, cube D's among them, are
    // still pending, and all of them finish before a solid is completed in place.
    assert_eq!(
        threaded_admissions_match_serial("seven-cube-shared-curves.step", 8),
        Admission {
            threaded: true,
            deferred_fixes: 6,
            shared_fixes: 1,
            reshaped_fixes: 6,
            serial_retries: 0
        }
    );
    let (serial, _) = admit("seven-cube-shared-curves.step", SERIAL, false);
    for _ in 0..16 {
        let (retried, admission) = admit("seven-cube-shared-curves.step", THREADED, true);
        assert_eq!(retried, serial);
        assert!(admission.threaded);
        assert!((1..=4).contains(&admission.deferred_fixes), "{admission:?}");
        assert_eq!(admission.serial_retries, 1);
    }
}

#[test]
fn every_retained_step_fixture_admits_the_serial_bytes_threaded() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut directories = vec![root.clone()];
    let mut names = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "step") {
                let name = path.strip_prefix(&root).unwrap().to_string_lossy();
                names.push(name.into_owned());
            }
        }
    }
    names.sort();
    assert!(names.len() >= 20, "{names:?}");
    for name in &names {
        let (serial, _) = admit(name, SERIAL, false);
        let (threaded, _) = admit(name, THREADED, false);
        assert_eq!(threaded, serial, "{name}");
    }
}
