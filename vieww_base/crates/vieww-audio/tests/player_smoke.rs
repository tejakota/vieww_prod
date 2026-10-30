//! The app-facing player on a live output (AU-01): `EnginePlayer` — what
//! every platform registers as `dyn AudioPlayer` — opens the default device
//! on first play, plays 20 tones, and the stream reports running callbacks
//! and no render that overran its block.
//!
//! Needs a sound server or device, so ignored by default; the
//! audio-backends workflow (PulseAudio null sink) and the self-hosted
//! hardware scripts run it:
//!
//! `cargo test --release -p vieww-audio --test player_smoke -- --ignored --nocapture`

use std::time::Duration;

use vieww_audio::{AudioPlayer, EnginePlayer, Envelope, Sound, Tone, Waveform};

#[test]
#[ignore = "needs an audio output (a sound server or device)"]
fn twenty_tones_through_the_registered_player() {
    let player = EnginePlayer::new();
    let engine = match player.engine() {
        Ok(e) => e,
        Err(e) => panic!("no output opened: {e}"),
    };
    let gain: f32 = std::env::var("VIEWW_LIVE_GAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    // Unattended runs on real speakers play silence; every frame is still
    // rendered and delivered.
    engine.set_master_gain(gain);
    for k in 0..20u8 {
        let tone = Tone::new(220.0 * 2f32.powf(f32::from(k) / 12.0), Waveform::Sine)
            .envelope(Envelope::attack_release(
                Duration::from_millis(5),
                Duration::from_millis(40),
            ))
            .hold(Duration::from_millis(60));
        player
            .play(&Sound::tone(tone))
            .expect("the player takes a tone");
        std::thread::sleep(Duration::from_millis(120));
    }
    std::thread::sleep(Duration::from_millis(300));
    let st = engine.stream_stats().expect("stream statistics");
    eprintln!("player_smoke on {}: {st:?}", engine.backend());
    assert!(st.callbacks > 0, "no audio callbacks ran");
    assert_eq!(st.late, 0, "renders overran their block");
    assert!(!st.ended, "the stream ended");
}
