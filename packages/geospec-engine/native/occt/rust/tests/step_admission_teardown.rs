//! The background reader teardown under load. Admission is serialized from the
//! gate through the teardown, and the thread that starts a teardown publishes
//! its handle before the gate can be released. The fork tests live in
//! step_admission_fork, a binary of their own.
use geospec_engine_native_occt::Document;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("retained fixture must be readable")
}

#[test]
fn many_threads_admit_valid_and_invalid_bytes() {
    let valid = fixture("two-cube-assembly.step");
    let invalid = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=NOT_AN_ENTITY(;\nENDSEC;\n".to_vec();
    std::thread::scope(|scope| {
        for thread in 0..8 {
            let (valid, invalid) = (&valid, &invalid);
            scope.spawn(move || {
                for admission in 0..24 {
                    if (thread + admission) % 3 == 0 {
                        assert!(Document::from_step(invalid).is_err());
                    } else {
                        drop(Document::from_step(valid).expect("valid admission"));
                    }
                }
            });
        }
    });
}
