//! SnapSearch — find a photo by describing it, or by showing it one.
//!
//! Built entirely on `vieww` (the sibling `../vieww` crate) rather than any
//! HTML/CSS — see `README.md` for what's real, what's mocked, and the one
//! or two places this deliberately diverges from the earlier HTML prototype
//! because the real framework doesn't have an equivalent yet.

pub mod app;
pub mod concept;
pub mod embed;
pub mod library;
pub mod photo;
pub mod screens;
pub mod state;
pub mod theme;
pub mod widgets;
pub mod workers;

/// The Android entry point — `cargo apk run` looks for this symbol.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: vieww_platform_winit::AndroidApp) {
    // The cache lives in the app's private directory; `build_root` has no
    // `AndroidApp`, so the path is recorded here first.
    if let Some(dir) = android.internal_data_path() {
        workers::set_android_data_dir(dir);
    }
    let (app, slot) = app::configure(vieww_platform_winit::App::new());
    let result = app.run_android(android, move |driver| app::build_root(driver, &slot));
    if let Err(err) = result {
        eprintln!("SnapSearch failed to start: {err}");
    }
}
