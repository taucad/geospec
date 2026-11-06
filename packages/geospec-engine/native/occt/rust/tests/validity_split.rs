//! P8: the per-solid validity split under a grant. The validity facts through
//! the granted connector are the serial connector's bytes at every grant this
//! host can hold, the grant of one never reports parallel use, and the split
//! reports parallel use only for a shape it actually splits. Widths above the
//! host's CPUs and the cap at four run in the bridge controls
//! (`tests/qualification.rs`, `p8_*`), on a private pool.
#![cfg(not(target_os = "emscripten"))]

use geospec_engine_native_occt::{
    configure_thread_pool_width, BrepConnector, Document, OcctConnector, ParallelOcctConnector,
};

/// Each fixture, and whether the split runs on it: a loop over two or more
/// independent solids, none holding more than a quarter of the faces (two
/// equal cubes stay on the caller; seven run the split).
const FIXTURES: &[(&str, bool, &[u8])] = &[
    (
        "regular-solid-controls",
        false,
        include_bytes!("fixtures/regular-solid-controls.step"),
    ),
    (
        "seven-cube-shared-curves",
        true,
        include_bytes!("fixtures/seven-cube-shared-curves.step"),
    ),
    (
        "two-cube-assembly",
        false,
        include_bytes!("fixtures/two-cube-assembly.step"),
    ),
    (
        "two-cube-reversed-face",
        false,
        include_bytes!("fixtures/two-cube-reversed-face.step"),
    ),
    (
        "two-cube-solid-and-faces",
        false,
        include_bytes!("fixtures/two-cube-solid-and-faces.step"),
    ),
    (
        "shared-shell-placements",
        false,
        include_bytes!("fixtures/shared-shell-placements.step"),
    ),
    (
        "bounds-free-edges",
        false,
        include_bytes!("fixtures/bounds-free-edges.step"),
    ),
    (
        "inch-cube",
        false,
        include_bytes!("fixtures/inch-cube.step"),
    ),
];

fn serial_bytes(bytes: &[u8]) -> String {
    let document = OcctConnector.open_step(bytes).unwrap();
    format!("{:?}", document.validity().unwrap())
}

#[test]
fn granted_validity_is_the_serial_bytes_and_reports_parallel_use_honestly() {
    let host = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let widths: Vec<u32> = [1, 2, 4, 8].into_iter().filter(|w| *w <= host).collect();
    let cap = *widths.last().unwrap();
    // One pool per process: fix it at the widest grant this host can hold.
    assert_eq!(configure_thread_pool_width(cap).unwrap(), cap);
    for (name, splits, bytes) in FIXTURES {
        let serial = serial_bytes(bytes);
        for &width in &widths {
            // SAFETY: this test binary owns the private OCCT closure and runs
            // one granted query at a time on this thread.
            let connector = unsafe { ParallelOcctConnector::with_grant(width, cap) }.unwrap();
            for _ in 0..2 {
                let document = connector.open_step(bytes).unwrap();
                assert_eq!(
                    format!("{:?}", document.validity().unwrap()),
                    serial,
                    "{name}: validity at grant {width} is the serial bytes"
                );
            }
            let direct = Document::from_step(bytes).unwrap();
            // SAFETY: as above; the pool was initialized at `cap >= width`.
            let (facts, used_parallel) = unsafe { direct.validity_dedicated(width) }.unwrap();
            assert_eq!(format!("{facts:?}"), serial, "{name}: direct grant {width}");
            assert_eq!(
                used_parallel,
                width >= 2 && *splits,
                "{name}: parallel use at grant {width} is reported exactly when the split ran"
            );
            // The proof is now memoized on the document: a second call runs
            // nothing, so it reports no parallel use and the same bytes.
            let (again, reused) = unsafe { direct.validity_dedicated(width) }.unwrap();
            assert_eq!(format!("{again:?}"), serial);
            assert!(!reused, "{name}: a memoized proof reports no parallel use");
        }
    }
}
