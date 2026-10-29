//! A release build that panics, for `ci/check/crash-symbols.sh` (FW-12):
//! built with `--profile dist-symbols`, split and stripped by
//! `ci/release/symbols.sh`, run, and its crash report symbolised offline by
//! `ci/release/symbolize.py`. The script checks the report names
//! `outer_frame` and `inner_frame` with this file's lines, which a stripped
//! binary's own backtrace cannot.

use vieww_foundation::crash::{Crashes, FileSink};

#[inline(never)]
fn inner_frame(depth: u32) -> u32 {
    assert!(depth < 2, "a crash for the symbols check, at depth {depth}");
    depth
}

#[inline(never)]
fn outer_frame(depth: u32) -> u32 {
    inner_frame(depth + 1).wrapping_mul(3)
}

fn main() {
    let dir = std::env::var("VIEWW_CRASH_DIR").unwrap_or_else(|_| "crashes".into());
    Crashes::new().with_sink(FileSink::new(dir)).install();
    let depth = u32::try_from(std::env::args().count()).unwrap_or(1);
    println!("{}", outer_frame(depth));
}
