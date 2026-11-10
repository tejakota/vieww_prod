//! wright-flow — a vieww application.
//!
//! One library, four entry points: [`run`] for desktop, `android_main` for an
//! Android activity, `vieww_ios_main` for `ios/App/main.m`, and [`start`] for
//! the browser (`web/index.html`). Everything they share is in [`app`] and
//! [`mount`], so the window cannot end up configured differently depending on
//! where it was started from — and neither can the canvas.

use vieww::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use vieww_platform_winit::App;

pub mod screens;

/// The window's size at startup. Phone-shaped, because the screen in
/// `screens/home.rs` is laid out for one — change both together.
///
/// Ignored on a phone, where the window is whatever the device is.
const WINDOW: Size = Size {
    width: 1280.0,
    height: 800.0,
};

/// The theme this application draws in.
///
/// One value. `App::theme` sets the window's background from it *and*
/// publishes it above the widget tree, so the two cannot disagree — change
/// this line and the window and everything in it move together.
///
/// `light()` because that is what vieww Studio previews with by default, so
/// what you see there and what you get here are the same screen. `dark()` is
/// the other one; `ThemeData::adaptive(platform, dark)` follows the platform.
#[must_use]
pub fn theme() -> ThemeData {
    ThemeData::dark()
}

/// The application, configured but not started.
///
/// Winit's, everywhere the process owns its own event loop. The browser is the
/// one host that does not, so there [`start`] mounts the web platform's
/// equivalent instead — same tree, same theme, no `App`.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn app() -> App {
    App::new().title("Wright · Flow").size(WINDOW).theme(theme())
}

/// Mount the first screen.
///
/// `screen()` returns `impl Widget` rather than a concrete type — that is the
/// preview pipeline's contract (`screens/mod.rs`) — so there is no
/// `Into<WidgetNode>` for `set_root` to find without naming the conversion,
/// which only a widget's own concrete type carries. `WidgetNode::new` is the
/// generic form of exactly that conversion.
pub fn mount(driver: &mut FrameDriver) {
    driver.set_root(WidgetNode::new(screens::home::screen()));
}

/// Start on a desktop, where the process owns its own event loop.
///
/// Gated off wasm32 for the same reason `app` is: there is no event loop to
/// run, only a page that has already started. [`start`] is that side's
/// entry point.
#[cfg(not(target_arch = "wasm32"))]
pub fn run() {
    match app().run(mount) {
        Ok(report) => println!("{report}"),
        Err(error) => eprintln!("wright-flow failed to start: {error}"),
    }
}

/// The browser's entry point, on the `<canvas id="vieww-canvas">` that
/// `web/index.html` carries.
///
/// `wasm_bindgen(start)` is called by the shim `wasm-bindgen` generates, the
/// moment the module is instantiated — no `main`, no attribute to forget to
/// call. It mounts the same tree on the same theme the desktop does, keeps the
/// handle alive for the life of the page (dropping it would stop the frame
/// loop), and surfaces a failed start in the page's boot line rather than as a
/// blank canvas.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use vieww_platform_web::{WebApp, WebSurface};

    let surface = WebSurface::by_id("vieww-canvas")
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    WebApp::new(surface)
        // The theme's surface, which is what `App::theme` paints the desktop
        // window with — the browser and the desktop clear to the same colour,
        // so the same screen does not sit on two different grounds depending
        // on which one opened it.
        .background(theme().colors.surface)
        .mount_with(mount)
        .map(std::mem::forget)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

/// Android's entry point.
///
/// The activity loads `libwright_flow.so` and calls this; there is no `main`. The
/// `AndroidApp` it hands over carries the looper, the asset manager and the
/// surface, none of which Rust can create for itself — which is why this takes
/// an argument and [`run`] does not.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: vieww_platform_winit::AndroidApp) {
    if let Err(error) = app().run_android(android, mount) {
        // No stdout on a device. `android_logger` is initialised by the
        // platform crate, so this reaches `adb logcat`.
        log::error!("wright-flow failed to start: {error}");
    }
}

/// iOS's entry point, called from `ios/App/main.m`.
///
/// # Why iOS needs a symbol and the others do not
///
/// `UIApplicationMain` must own the process before any window exists and never
/// returns, so the `main` that runs on iOS is Objective-C's. This crate is
/// linked in as a static library and this is the one symbol that file calls.
///
/// `no_mangle` and `extern "C"` are both load-bearing: the linker looks for
/// exactly `vieww_ios_main`, and a mangled or Rust-ABI symbol produces an
/// "undefined symbol" at link time that names something unrecognisable.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn vieww_ios_main() {
    // `run` never returns on iOS — winit hands control to UIKit — so anything
    // written after this line would be unreachable rather than a cleanup.
    run();
}
