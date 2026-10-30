//! `LiveScore::render` allocates nothing (AU-18): every demo piece played
//! live for 2,000 uneven device blocks, with a counting allocator armed on
//! this (the rendering) thread.
//!
//! Run as its own process (`harness = false`) and with a static std
//! (`RUSTFLAGS=-Cprefer-dynamic=no`): with a shared libstd, Vec growth
//! runs in libstd's own code and never reaches this allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use vieww_music::live::LiveScore;
use vieww_music::render::Settings;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

fn armed() -> bool {
    ARMED.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if armed() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if armed() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: from `System` via `alloc`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn main() {
    let settings = Settings {
        rate: 24_000,
        polyphony: 8,
        tail: 0.5,
        ..Settings::default()
    };
    for score in [
        vieww_music::pieces::rock(),
        vieww_music::pieces::chorale(true),
        vieww_music::pieces::carnatic(1).0,
    ] {
        let title = score.title.clone();
        let mut live = LiveScore::new(score, settings);
        live.play();
        let mut out = vec![0.0f32; 2 * 256];
        live.render(&mut out); // warm-up
        ALLOCS.store(0, Ordering::SeqCst);
        ARMED.with(|a| a.set(true));
        let mut loud = false;
        for i in 0..2_000 {
            // Uneven device blocks, as a real callback hands them.
            let frames = [256usize, 480, 128, 441][i % 4];
            live.render(&mut out[..frames.min(256) * 2]);
            loud |= out.iter().any(|v| v.abs() > 1e-4);
        }
        ARMED.with(|a| a.set(false));
        let n = ALLOCS.load(Ordering::SeqCst);
        assert_eq!(n, 0, "{title}: LiveScore::render allocated {n} time(s)");
        assert!(loud, "{title}: and it played");
    }
    println!("realtime_no_alloc: LiveScore::render allocates nothing across every demo piece");
}
