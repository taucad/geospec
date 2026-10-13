//! Test-only S10 fault injection (cells R01 and R02), compiled only with the
//! `s10-fault-injection` feature, which no release build enables.
//!
//! `GEOSPEC_S10_FAULT` selects one cell per process and the selected panic
//! fires once. Unselected, every seam behaves exactly like the plain call.
use std::sync::atomic::{AtomicBool, Ordering};

static FIRED: AtomicBool = AtomicBool::new(false);

fn selected(cell: &str) -> bool {
    std::env::var("GEOSPEC_S10_FAULT").is_ok_and(|value| value == cell)
        && !FIRED.swap(true, Ordering::SeqCst)
}

/// R01: a Rust panic after admission and before the OCCT bridge call.
pub(crate) fn before_foreign() {
    if selected("R01") {
        panic!("geospec-s10-inject R01: Rust panic before the OCCT bridge call");
    }
}

/// R02: a Rust panic after the bridge returned and before the report.
pub(crate) fn after_foreign() {
    if selected("R02") {
        panic!("geospec-s10-inject R02: Rust panic after the OCCT bridge result");
    }
}

macro_rules! open_step {
    ($open:expr) => {{
        $crate::s10::before_foreign();
        let opened = $open;
        $crate::s10::after_foreign();
        opened
    }};
}
pub(crate) use open_step;
