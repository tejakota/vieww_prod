//! The web's `AudioPlayer` end to end (AU-01), for
//! `web-test/audio_smoke.py`: `services()` registers the engine, a tone
//! plays through Web Audio, and the page counts the blocks the worklet
//! pulled.
//!
//! Built as a cdylib for wasm32:
//! `cargo build --release --target wasm32-unknown-unknown -p vieww-platform-web --example audio_smoke`
//!
//! On any other target it is empty: the player it drives is the web's.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use vieww_audio::{AudioPlayer, Envelope, Sound, Tone, Waveform};
use vieww_foundation::service::Services;

thread_local! {
    static SERVICES: RefCell<Option<Services>> = const { RefCell::new(None) };
}

fn player() -> Option<Rc<dyn AudioPlayer>> {
    SERVICES.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(vieww_platform_web::services)
            .get::<dyn AudioPlayer>()
    })
}

/// 1 when the platform registered an `AudioPlayer`.
#[unsafe(no_mangle)]
pub extern "C" fn smoke_registered() -> u32 {
    u32::from(player().is_some())
}

/// Play a two-second tone; 1 when the player accepted it.
#[unsafe(no_mangle)]
pub extern "C" fn smoke_play() -> u32 {
    let Some(p) = player() else { return 0 };
    let tone = Tone::new(440.0, Waveform::Sine)
        .envelope(Envelope::attack_release(
            Duration::from_millis(5),
            Duration::from_millis(50),
        ))
        .hold(Duration::from_secs(2));
    u32::from(p.play(&Sound::tone(tone)).is_ok())
}
