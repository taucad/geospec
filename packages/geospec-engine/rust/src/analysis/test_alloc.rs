//! The test binary's global allocator. It passes every request to `System`
//! and notes, per thread, the largest single request and the peak of live
//! requested bytes, so a test can compare a declared byte reservation with
//! what the code it guards actually allocated.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
    static LIVE: Cell<i128> = const { Cell::new(0) };
    static PEAK: Cell<i128> = const { Cell::new(0) };
}

fn grow(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
    let _ = LIVE.try_with(|live| {
        let value = live.get() + size as i128;
        live.set(value);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(value)));
    });
}

fn shrink(size: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get() - size as i128));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // A moving reallocation holds both blocks at once; count that peak.
        grow(size);
        let moved = System.realloc(pointer, layout, size);
        shrink(layout.size());
        moved
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        shrink(layout.size());
        System.dealloc(pointer, layout);
    }
}

/// Runs `run` and returns its value with this thread's largest single request.
pub(crate) fn largest_request<T>(run: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.with(|largest| largest.set(0));
    let value = run();
    (value, LARGEST.with(Cell::get))
}

/// Runs `run` and returns its value with this thread's peak live heap bytes
/// above the live bytes at entry, including blocks freed before it returns.
pub(crate) fn peak_bytes<T>(run: impl FnOnce() -> T) -> (T, u64) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let value = run();
    let peak = PEAK.with(Cell::get);
    (value, u64::try_from(peak - base).unwrap_or(0))
}

/// Runs `run` and returns its value with the heap bytes still live from it
/// when it returns: what the value retains.
pub(crate) fn retained_bytes<T>(run: impl FnOnce() -> T) -> (T, u64) {
    let base = LIVE.with(Cell::get);
    let value = run();
    let live = LIVE.with(Cell::get);
    (value, u64::try_from(live - base).unwrap_or(0))
}
