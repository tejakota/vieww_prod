//! `#[realtime]` at run time (AU-18): under `CheckedAlloc`, a real-time
//! function whose callee allocates panics when its section ends; one that
//! does not allocate runs clean. Debug builds only (release compiles the
//! section away). Build with `-C prefer-dynamic=no`: with a shared libstd
//! the allocator never sees Vec growth (see `CheckedAlloc`), and the
//! allocating case would pass unnoticed.

#![cfg(debug_assertions)]

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

use vieww_audio::rt::{rt_violations_here, CheckedAlloc, RtSection};

thread_local! {
    static SEEN: Cell<usize> = const { Cell::new(0) };
}

/// `CheckedAlloc`, counting what reaches it on this thread, so a test can
/// tell whether this build's allocations reach the global allocator at all.
struct Probe;

unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = SEEN.try_with(|n| n.set(n.get() + 1));
        // SAFETY: forwarded unchanged.
        unsafe { CheckedAlloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { CheckedAlloc.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Probe = Probe;

/// Whether `Vec` growth reaches this binary's allocator. With a shared
/// libstd (the workspace's `prefer-dynamic`) it does not.
fn allocations_are_visible() -> bool {
    let before = SEEN.with(Cell::get);
    let v: Vec<Sample> = std::iter::repeat_n(Sample(0.0, 1.0), 32).collect();
    std::hint::black_box(&v);
    SEEN.with(Cell::get) > before
}

/// A type of this crate's own: `Vec<Sample>` is compiled here, so its
/// allocation reaches the global allocator even where the workspace links
/// std dynamically (a `Vec<u8>` can come pre-built from libstd.so, whose
/// copy calls std's allocator directly).
#[derive(Clone, Copy)]
struct Sample(f32, f32);

#[inline(never)]
fn helper_that_allocates(n: usize) -> usize {
    // The macro only sees the function it marks; this is what the
    // run-time layer is for.
    let v: Vec<Sample> = std::iter::repeat_n(Sample(0.0, 1.0), n).collect();
    v.len() + usize::from(v[0].1 > v[0].0)
}

#[vieww_audio::realtime]
fn clean(out: &mut [f32]) {
    for (i, o) in out.iter_mut().enumerate() {
        *o = i as f32;
    }
}

#[vieww_audio::realtime]
fn calls_an_allocating_helper(out: &mut [f32]) {
    out[0] = helper_that_allocates(out.len()) as f32;
}

#[test]
fn a_clean_function_runs() {
    let before = rt_violations_here();
    let mut out = [0.0f32; 64];
    clean(&mut out);
    assert_eq!(out[63], 63.0);
    assert!(!RtSection::active());
    assert_eq!(rt_violations_here(), before);
}

#[test]
fn an_allocating_callee_is_caught() {
    if !allocations_are_visible() {
        eprintln!(
            "skipped: this build links std dynamically, so its allocations never reach \
             the checking allocator — CI runs this with `-C prefer-dynamic=no`"
        );
        return;
    }
    let caught = std::panic::catch_unwind(|| {
        let mut out = [0.0f32; 64];
        calls_an_allocating_helper(&mut out);
    })
    .expect_err("the allocation is caught");
    let message = caught
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| caught.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default();
    assert!(
        message.contains("#[realtime] calls_an_allocating_helper: allocated"),
        "{message}"
    );
}
