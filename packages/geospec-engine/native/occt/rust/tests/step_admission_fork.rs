//! A forked child can admit STEP bytes: a fork waits for the admission and
//! teardown in flight, holds the gate's mutex across the fork and gives the
//! child a free gate. This binary has one test, so no other test's threads hold
//! OCCT or allocator locks when it forks.
#![cfg(target_os = "linux")]

use geospec_engine_native_occt::Document;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("retained fixture must be readable")
}

extern "C" {
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn _exit(status: i32) -> !;
}

const WNOHANG: i32 = 1;
const SIGKILL: i32 = 9;

// Forks a child that admits `bytes` and exits; fails unless it does so
// within 60 s.
fn child_admits(bytes: &[u8]) {
    // SAFETY: the child only admits STEP bytes and leaves through _exit.
    let pid = unsafe { fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        let admitted = Document::from_step(bytes).is_ok();
        // SAFETY: ends the child without running the parent's test harness.
        unsafe { _exit(if admitted { 0 } else { 1 }) };
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut status = 0;
    loop {
        // SAFETY: waits on this test's own child with a valid status pointer.
        let reaped = unsafe { waitpid(pid, &mut status, WNOHANG) };
        if reaped == pid {
            break;
        }
        assert_eq!(reaped, 0, "waitpid failed");
        if Instant::now() > deadline {
            // SAFETY: kills this test's own child.
            unsafe { kill(pid, SIGKILL) };
            panic!("the forked child did not finish its admission within 60 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Normal exit with status 0: WIFEXITED and WEXITSTATUS on Linux.
    assert_eq!(status & 0x7f, 0, "child was killed by signal {}", status & 0x7f);
    assert_eq!((status >> 8) & 0xff, 0, "child admission failed");
}

// One test, so the harness runs no other test beside a fork: the only other
// thread is the one this test starts to keep an admission in flight.
#[test]
fn a_forked_child_admits_again() {
    let bytes = fixture("two-cube-assembly.step");

    // After a threaded admission: the reader is released on a background
    // teardown thread, which may still be running when the fork happens.
    drop(Document::from_step(&bytes).expect("parent admission"));
    child_admits(&bytes);
    drop(Document::from_step(&bytes).expect("parent admission after fork"));

    // While another thread admits, or waits for the gate.
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        // Stops the background admissions also when a child fails.
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                drop(Document::from_step(&bytes).expect("background admission"));
            }
        });
        let _stop = Stop(&stop);
        for _ in 0..16 {
            child_admits(&bytes);
        }
    });
}
