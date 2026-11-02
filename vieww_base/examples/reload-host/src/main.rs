//! The half that keeps running while the other half is rebuilt.
//!
//! ```console
//! # once, so there is something to load
//! cargo build -p reload-guest
//!
//! # then, in one terminal, rebuild the guest on every edit
//! cargo watch -w examples/reload-guest -x 'build -p reload-guest'
//!
//! # and in another, the window
//! cargo run -p reload-host
//! ```
//!
//! Without `cargo watch`, `cargo build -p reload-guest` by hand does the same
//! thing — the host watches the file, not the tool that wrote it.
//!
//! **The window updates on its own.** It does not need to be focused, clicked
//! or moved: a watcher thread asks the event loop for a frame when the library
//! changes, which is the only reason an idle window notices at all.
//!
//! # What to look at
//!
//! Tap the button a few times, then edit `BAND` in the guest and rebuild. The
//! colour changes and **the count stays**. That is a new library, loaded into a
//! running process, with the element tree intact.
//!
//! Then add a field to the guest's `Taps` and rebuild. The count resets, and the
//! host says why. See `docs/HOT-RELOAD.md`.

use std::path::PathBuf;
use std::time::Duration;

use vieww_devtools::{ReloadLedger, ReloadOutcome};
use vieww_foundation::task::FrameWaker;
use vieww_foundation::{Color, Size};
use vieww_platform_winit::App;
use vieww_reload::{Reloaded, Reloader};

const SURFACE: Size = Size {
    width: 720.0,
    height: 480.0,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guest = guest_path();
    println!("host: watching {}", guest.display());

    let mut reloader = Reloader::new(&guest)
        .map_err(|error| format!("{error}\n\nBuild it first:\n    cargo build -p reload-guest"))?;

    // The *first* build panicking is fatal, unlike every reload after it:
    // there is no previous screen to fall back to, and a window showing
    // nothing at all is not a more useful answer than a message.
    let first = reloader
        .root()
        .map_err(|error| format!("{error}\n\nThe first build has to succeed."))?;

    let app = App::new()
        .title("vieww — hot reload")
        .size(SURFACE)
        .background(Color::rgb(18, 18, 22));

    // **Without this the window would only notice a rebuild the next time it
    // drew for some other reason** — a mouse move, a resize — because the swap
    // rides `before_frame` and an idle window draws nothing. The watcher thread
    // does no loading; it only asks for the frame in which `poll` below runs.
    let waker = app.waker();
    reloader.wake_on_change(Duration::from_millis(250), move || waker.wake());

    // The session's score, in the shape the devtools overlay prints it: how
    // many reloads this run accepted, and the share that kept the tree. The
    // HUD form of the same numbers is `ReloadReport::hud_lines`, for a host
    // that draws one.
    let mut ledger = ReloadLedger::new();

    let report = app
        // The whole integration. One hook, called between frames.
        .before_frame(move |driver| match reloader.poll() {
            Reloaded::Nothing => {}
            Reloaded::Root(root) => {
                ledger.record(ReloadOutcome::TreeKept);
                println!(
                    "host: reloaded #{} — state kept ({})",
                    reloader.generation(),
                    ledger.report().summary_line()
                );
                driver.set_root(root);
            }
            Reloaded::Restart { root, changed } => {
                // The guest's state changed shape. Reusing the old allocations
                // would be undefined behaviour, so the tree goes — and the
                // diff says which type paths moved, so the developer knows
                // which edit cost them the tree.
                ledger.record(ReloadOutcome::TreeRebuilt);
                println!(
                    "host: reloaded #{} — state CHANGED SHAPE ({}), tree rebuilt ({})",
                    reloader.generation(),
                    changed,
                    ledger.report().summary_line()
                );
                driver.remount(root);
            }
            Reloaded::Failed(error) => {
                // A compile error in the guest. The window carries on running
                // whatever was loaded last, which is what a developer wants
                // mid-edit. Counted, but not against the preservation rate: a
                // build that never loaded preserved everything it accepted.
                ledger.record(ReloadOutcome::BuildFailed);
                eprintln!("host: keeping the running build — {error}");
            }
        })
        .run(move |driver| driver.set_root(first))?;

    // The session's score on the way out, alongside the runner's own report.
    println!("{} · {}", report, ledger.report().summary_line());
    Ok(())
}

/// Where cargo puts the guest, for this platform's library naming.
///
/// Derived from this binary's own location rather than assumed to be
/// `target/debug`, so `--release` and a custom `CARGO_TARGET_DIR` both work
/// without the path being passed in.
fn guest_path() -> PathBuf {
    if let Some(explicit) = std::env::args().nth(1) {
        return PathBuf::from(explicit);
    }
    let name = if cfg!(target_os = "windows") {
        "reload_guest.dll"
    } else if cfg!(target_os = "macos") {
        "libreload_guest.dylib"
    } else {
        "libreload_guest.so"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}
