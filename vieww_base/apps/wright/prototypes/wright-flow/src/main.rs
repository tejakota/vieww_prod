//! wright-flow — the desktop entry point.
//!
//! Deliberately thin. Everything this does is in `src/lib.rs`, because Android
//! and iOS cannot reach a `main` and have to call into the library instead —
//! and three entry points that each set up their own window would drift apart
//! within a week.

fn main() {
    wright_flow::run();
}
