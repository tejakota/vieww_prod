//! The desktop entry. Everything Android runs lives in the library; this only
//! opens the window.
//!
//! ```console
//! cargo run -p u --release
//! ```

fn main() {
    if let Err(error) = u::run() {
        eprintln!("U failed: {error}");
    }
}
