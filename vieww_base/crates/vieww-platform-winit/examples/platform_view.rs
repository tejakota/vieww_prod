//! The first consumer of the `PlatformViews` capability: real X11 child
//! windows, created, moved and destroyed through the declared shape.
//!
//! ```console
//! cargo run -p vieww-platform-winit --example platform_view
//! ```
//!
//! # What this is for
//!
//! `vieww_foundation::capability` has declared the platform-view shape —
//! spec, handle, `create`/`set_bounds`/`send` — since the capability module
//! was written, and until FW-20 nothing had ever implemented it
//! (PENDING §6.2: *"no application has ever used one. Whether the existing
//! shape is the right one is unknown"*). This example is that first
//! consumer, at the level the capability lives at: no widget tree, no
//! rasterizer, no `App` — a host window, three child windows the X server
//! clears to their spec colours, and the lifecycle the trait promises,
//! driven end to end.
//!
//! What you should see on an X11 session: a white window with three coloured
//! rectangles in it. One slides right over ~2 s (`set_bounds`), the smallest
//! one disappears halfway through (dropping the handle destroys the view),
//! and the window closes when the example exits.
//!
//! # The honest limits, visible here on purpose
//!
//! - **The views are cleared, not drawn.** The X server paints each child
//!   with its `background` pixel and nothing else; vieww's rasterizer never
//!   sees them. That is what embedding *is* — the OS composites the child,
//!   the framework only positions it — and an example that drew into the
//!   child would be demonstrating a different capability.
//! - **No display, no example.** A session without X11 prints why and exits
//!   cleanly: `connect()` is fallible by design, and the documented fallback
//!   is refusing to embed, not failing to start. Run it under `Xvfb` if the
//!   machine is otherwise headless.
//! - **Logical == physical pixels.** Correct at scale factor 1.0, wrong
//!   under XWayland fractional scaling; declared open in
//!   `docs/architecture/platform-views.md` with the rest of the now-known
//!   shape.

use std::thread;
use std::time::Duration;

/// One step of the slide, in milliseconds — 50 steps at 40 ms is the ~2 s.
const STEP: Duration = Duration::from_millis(40);
const STEPS: usize = 50;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use vieww_foundation::capability::{PlatformViewSpec, PlatformViews};
    use vieww_foundation::Rect;
    use vieww_platform_winit::platform_views::{X11PlatformViews, KIND};

    // The whole service is one fallible call: a Wayland-only session has no
    // `libX11`, a headless one has no display, and both are reported rather
    // than fatal — the same philosophy as the rasterizer's own
    // `rasterizer_choice`: try the better path, fall back loudly.
    let views = match X11PlatformViews::connect() {
        Ok(views) => views,
        Err(reason) => {
            eprintln!("platform_view: {reason}");
            eprintln!("platform_view: embedding is refused, not the run — under Xvfb: Xvfb :99 & DISPLAY=:99 cargo run --example platform_view");
            return Ok(());
        }
    };

    // The anchor: the one wiring step the capability shape needed and the
    // trait did not carry. Whoever owns the host window hands it to the
    // service; here the example owns the window outright.
    let host = views.host_window(480, 320);
    views.anchor(&host)?;
    println!(
        "platform_view: host 0x{:x} anchored, kind {KIND:?}",
        host.xid()
    );

    // Three views at the same declared shape a map or a video surface would
    // use: a kind string plus string parameters.
    let spec = |colour: &str| PlatformViewSpec::new(KIND).with("background", colour);
    let wide = views.create(&spec("#2a7af6"), Rect::new(20.0, 20.0, 220.0, 120.0))?;
    let tall = views.create(&spec("#f6b02a"), Rect::new(260.0, 20.0, 200.0, 280.0))?;
    let small = views.create(&spec("#2af6a0"), Rect::new(20.0, 170.0, 60.0, 60.0))?;
    println!(
        "platform_view: three views created — {:?}, {:?}, {:?}",
        views,
        wide.id(),
        small.id()
    );

    // The scroll path: `set_bounds` every step, which is what a view inside
    // a scrolling list costs per frame. Halfway through, the smallest view
    // is dropped — the trait's disposal contract is that unmounting the
    // owner is enough. `Option` rather than a bare drop because a move
    // inside a loop body has to be expressible to every later iteration.
    let mut small = Some(small);
    for step in 1..=STEPS {
        let x = 20.0 + (160.0 * f32::from(step as u16) / STEPS as f32);
        views.set_bounds(wide.id(), Rect::new(x, 20.0, x + 200.0, 120.0))?;
        if step == STEPS / 2 {
            drop(small.take());
            println!("platform_view: the small view's handle dropped — the window is gone");
        }
        thread::sleep(STEP);
    }

    // Dropping the handles destroys the views; dropping the service would
    // too, through the dispose closures it minted them with.
    drop(wide);
    drop(tall);
    println!("platform_view: done — the host closes as this exits");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    // The seam this example demonstrates is X11 child windows; macOS,
    // Windows and Android each have their own (unwritten) equivalent, and a
    // stub that pretended to run would say less than this message does.
    eprintln!(
        "platform_view: the X11 platform-view seam is Linux-only; this is {}",
        std::env::consts::OS
    );
}
