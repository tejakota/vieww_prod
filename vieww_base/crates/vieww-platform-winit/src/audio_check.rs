//! The on-device audio check (AU-02, AU-03, AU-04), run by the Device Farm
//! suites.
//!
//! The demo app starts it when asked (Android: `debug.vieww.audio=1`; iOS:
//! `VIEWW_AUDIO=1`). On its own thread it plays a looping tone through
//! [`MediaPlayer`], with lock-screen metadata, and checks on the phone what
//! the unit tests can only model:
//!
//! * the session bridge is registered and focus is granted;
//! * **Android:** a competing transient focus request (made from this
//!   process with a different focus client, so the loss travels through the
//!   platform's focus stack to vieww's listener) pauses playback and freezes
//!   the position; giving it back resumes. A "may duck" request ducks and
//!   its release unducks;
//! * **iOS:** the session category is `.playback` and the lock screen has a
//!   now-playing entry. The interruption notification iOS posts for a call
//!   pauses playback, and its end with "should resume" resumes. The
//!   notification is posted by the check, since a device cloud cannot
//!   place a call.
//!
//! Every line starts with `__VIEWW_AUDIO__`:
//!
//! ```text
//! __VIEWW_AUDIO__ PASS <check> <detail>
//! __VIEWW_AUDIO__ FAIL <check> <detail>
//! __VIEWW_AUDIO__ result PASS|FAIL
//! __VIEWW_AUDIO__ status t_ms=… callbacks=… rate=… position_ms=… playing=… suspended=… duck=… holding=…
//! ```
//!
//! After `result`, it keeps playing, applies lock-screen and headset
//! commands (`MediaPlayer::poll_remote`), and writes a `status` line every
//! second. The runner drives the rest from outside: pause and play through
//! the system media session, and the screen off for a minute, checked
//! against `callbacks`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vieww_audio::engine_player::{Engine, MediaPlayer, MediaState, Source};
use vieww_audio::session::AudioSession;
use vieww_audio::Samples;

fn say(line: &str) {
    log::info!("__VIEWW_AUDIO__ {line}");
    println!("__VIEWW_AUDIO__ {line}");
}

struct Checks {
    failed: bool,
}

impl Checks {
    fn check(&mut self, name: &str, ok: bool, detail: &str) {
        say(&format!(
            "{} {name} {detail}",
            if ok { "PASS" } else { "FAIL" }
        ));
        self.failed |= !ok;
    }
}

/// A 30-second 440 Hz tone at -14 dBFS, mono.
fn tone() -> Samples {
    let rate = 48_000u32;
    let data = (0..rate * 30)
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "a sample index")]
            let t = i as f32 / rate as f32;
            0.2 * (std::f32::consts::TAU * 440.0 * t).sin()
        })
        .collect();
    Samples {
        data,
        channels: 1,
        rate,
    }
}

/// Position advance over `wait`, in ms (looping wraps at 30 s; the
/// shorter way round is taken).
fn advance(player: &mut MediaPlayer, wait: Duration) -> i64 {
    let a = player.position();
    std::thread::sleep(wait);
    let b = player.position();
    let d = i64::try_from(b.as_millis()).unwrap_or(0) - i64::try_from(a.as_millis()).unwrap_or(0);
    if d < -15_000 {
        d + 30_000
    } else {
        d
    }
}

fn status(started: Instant, engine: &Engine, player: &mut MediaPlayer, session: &AudioSession) {
    let playing = player.state() == MediaState::Playing && !session.suspended();
    say(&format!(
        "status t_ms={} callbacks={} rate={} position_ms={} playing={} suspended={} duck={:.3} holding={}",
        started.elapsed().as_millis(),
        engine.stats().frames,
        engine.rate(),
        player.position().as_millis(),
        playing,
        session.suspended(),
        session.duck_gain(),
        session.holding(),
    ));
}

fn tail(started: Instant, engine: &Arc<Engine>, mut player: MediaPlayer) -> ! {
    let session = AudioSession::shared();
    let mut last = Instant::now();
    loop {
        for c in player.poll_remote() {
            say(&format!("remote {c:?} (not handled by the player)"));
        }
        if last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            status(started, engine, &mut player, session);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start() -> Option<(Arc<Engine>, MediaPlayer, Checks)> {
    let mut checks = Checks { failed: false };
    let session = AudioSession::shared();
    checks.check(
        "session_bridge_registered",
        session.bridge_name().is_some(),
        session.bridge_name().unwrap_or("none"),
    );
    let engine = match Engine::open_default() {
        Ok(e) => Arc::new(e),
        Err(e) => {
            checks.check("output_opens", false, &e.to_string());
            say("result FAIL");
            return None;
        }
    };
    checks.check(
        "output_opens",
        true,
        &format!("{} at {} Hz", engine.backend(), engine.rate()),
    );
    let mut player = MediaPlayer::new(Arc::clone(&engine));
    if let Err(e) = player.load(Source::Samples(tone())) {
        checks.check("tone_loads", false, &e.to_string());
        say("result FAIL");
        return None;
    }
    player.set_looping(true);
    player.set_metadata("vieww audio check", "vieww", "Device Farm");
    let played = player.play();
    checks.check(
        "focus_granted",
        played.is_ok(),
        &played
            .err()
            .map_or_else(|| "play() ok".to_owned(), |e| e.to_string()),
    );
    let d = advance(&mut player, Duration::from_secs(2));
    checks.check("plays", d >= 1_500, &format!("advanced {d} ms in 2 s"));
    Some((engine, player, checks))
}

/// Android: run the check on its own thread.
#[cfg(target_os = "android")]
pub fn spawn(app: &crate::AndroidApp) {
    use crate::audio_android::CompetingFocus;
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("vieww-audio-check".into())
        .spawn(move || {
            let started = Instant::now();
            // Let the activity come up and the session bridge register.
            std::thread::sleep(Duration::from_secs(2));
            let Some((engine, mut player, mut checks)) = start() else {
                return;
            };
            let session = AudioSession::shared();

            // A transient loss: a navigation prompt, a call.
            match CompetingFocus::claim(&app, 2) {
                Some(claim) => {
                    std::thread::sleep(Duration::from_millis(800));
                    checks.check(
                        "transient_loss_pauses",
                        session.suspended() && player.interrupted(),
                        &format!("suspended={}", session.suspended()),
                    );
                    let d = advance(&mut player, Duration::from_secs(1));
                    checks.check(
                        "nothing_advances_while_paused",
                        d.abs() <= 30,
                        &format!("moved {d} ms in 1 s"),
                    );
                    claim.release();
                    std::thread::sleep(Duration::from_millis(800));
                    let d = advance(&mut player, Duration::from_secs(1));
                    checks.check(
                        "focus_return_resumes",
                        !session.suspended() && d >= 700,
                        &format!("suspended={} advanced {d} ms in 1 s", session.suspended()),
                    );
                }
                None => checks.check(
                    "transient_loss_pauses",
                    false,
                    "the competing request was refused",
                ),
            }

            // A "may duck" loss: a notification sound.
            match CompetingFocus::claim(&app, 3) {
                Some(claim) => {
                    std::thread::sleep(Duration::from_millis(800));
                    let duck = session.duck_gain();
                    checks.check(
                        "may_duck_loss_ducks",
                        duck < 0.5 && !session.suspended(),
                        &format!("duck gain {duck:.3}, suspended={}", session.suspended()),
                    );
                    claim.release();
                    std::thread::sleep(Duration::from_millis(800));
                    let duck = session.duck_gain();
                    checks.check(
                        "focus_return_unducks",
                        (duck - 1.0).abs() < 1e-6,
                        &format!("duck gain {duck:.3}"),
                    );
                }
                None => checks.check(
                    "may_duck_loss_ducks",
                    false,
                    "the competing request was refused",
                ),
            }

            say(&format!(
                "events {:?}",
                session.log().iter().rev().take(16).collect::<Vec<_>>()
            ));
            say(&format!(
                "result {}",
                if checks.failed { "FAIL" } else { "PASS" }
            ));
            tail(started, &engine, player);
        });
}

/// iOS: run the check on its own thread.
#[cfg(target_os = "ios")]
pub fn spawn() {
    let _ = std::thread::Builder::new()
        .name("vieww-audio-check".into())
        .spawn(move || {
            let started = Instant::now();
            std::thread::sleep(Duration::from_secs(2));
            let Some((engine, mut player, mut checks)) = start() else {
                return;
            };
            let session = AudioSession::shared();
            checks.check(
                "session_category_playback",
                crate::audio_ios::category_is_playback(),
                "AVAudioSession.category",
            );
            checks.check(
                "now_playing_published",
                crate::audio_ios::now_playing_published(),
                "MPNowPlayingInfoCenter.nowPlayingInfo",
            );

            crate::audio_ios::post_interruption(true, false);
            std::thread::sleep(Duration::from_millis(500));
            checks.check(
                "interruption_pauses",
                session.suspended() && player.interrupted(),
                &format!("suspended={}", session.suspended()),
            );
            let d = advance(&mut player, Duration::from_secs(1));
            checks.check(
                "nothing_advances_while_interrupted",
                d.abs() <= 30,
                &format!("moved {d} ms in 1 s"),
            );
            crate::audio_ios::post_interruption(false, true);
            std::thread::sleep(Duration::from_millis(800));
            let d = advance(&mut player, Duration::from_secs(1));
            checks.check(
                "interruption_end_resumes",
                !session.suspended() && d >= 700,
                &format!("suspended={} advanced {d} ms in 1 s", session.suspended()),
            );

            say(&format!(
                "result {}",
                if checks.failed { "FAIL" } else { "PASS" }
            ));
            tail(started, &engine, player);
        });
}
