//! A media player (AU-21): `MediaPlayer` behind a transport — play, pause,
//! skip, normalization — with the state it reports shown live.
//!
//! `cargo run -p feature-media-player [-- FILE]`
//!
//! With a file (WAV, AIFF, FLAC, MP3, AAC or Ogg Vorbis) it plays that,
//! streamed from disk if it is long; without one, a generated chord
//! progression. With no output device (CI, a headless box) it runs on a
//! detached engine: the transport still works, nothing is heard.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use vieww_audio::engine_player::{Engine, MediaPlayer, MediaState, Normalization, Source};
use vieww_audio::{Samples, DEFAULT_TARGET_LUFS};
use vieww_element::Signal;
use vieww_foundation::{Color, Size};
use vieww_widget::prelude::*;

/// Four bars of I–vi–IV–V, a second each.
fn progression() -> Samples {
    let rate = 44_100u32;
    let chords: [[f32; 3]; 4] = [
        [261.6, 329.6, 392.0],
        [220.0, 261.6, 329.6],
        [174.6, 220.0, 261.6],
        [196.0, 246.9, 293.7],
    ];
    let mut data = Vec::with_capacity(rate as usize * 4);
    for chord in chords {
        for i in 0..rate {
            let t = i as f32 / rate as f32;
            let env = (t * 40.0).min(1.0) * (1.0 - t).max(0.0);
            let s: f32 = chord
                .iter()
                .map(|f| (std::f32::consts::TAU * f * t).sin())
                .sum();
            data.push(0.15 * env * s);
        }
    }
    Samples {
        data,
        channels: 1,
        rate,
    }
}

fn describe(p: &mut MediaPlayer) -> String {
    let state = match p.state() {
        MediaState::Empty => "empty",
        MediaState::Paused => "paused",
        MediaState::Playing => "playing",
        MediaState::Ended => "ended",
    };
    format!(
        "{state} · {:.1} s of {:.1} s · {} · normalization {:+.1} dB",
        p.position().as_secs_f64(),
        p.duration().as_secs_f64(),
        if p.is_streaming() {
            "streamed from disk"
        } else {
            "in memory"
        },
        p.normalization_gain_db()
    )
}

#[derive(Debug)]
struct Status {
    text: Signal<String>,
}

impl Widget for Status {
    fn debug_name(&self) -> &'static str {
        "Status"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        Text::new(self.text.get())
            .size(14.0)
            .color(Color::rgb(40, 44, 52))
            .into()
    }
}

widget_node_from!(Status);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Arc::new(Engine::open_default().unwrap_or_else(|_| Engine::detached(48_000, 2)));
    let player = Rc::new(RefCell::new(MediaPlayer::new(Arc::clone(&engine))));
    {
        let mut p = player.borrow_mut();
        p.set_normalization(Normalization::Track {
            target_lufs: DEFAULT_TARGET_LUFS,
        });
        let source = std::env::args().nth(1).map_or_else(
            || Source::Samples(progression()),
            |f| Source::File(f.into()),
        );
        p.load(source)?;
        p.set_looping(true);
        p.set_metadata("vieww media player", "vieww", "Feature examples");
    }
    feature_harness::launch("92 — media player", Size::new(680.0, 200.0), move |d| {
        let text = d
            .elements()
            .runtime()
            .signal(describe(&mut player.borrow_mut()));
        let button = |label: &str, act: fn(&mut MediaPlayer)| {
            let (player, text) = (Rc::clone(&player), text.clone());
            Button::new(label).on_pressed(move || {
                let mut p = player.borrow_mut();
                act(&mut p);
                text.set(describe(&mut p));
            })
        };
        feature_harness::set_page(
            d,
            Container::new()
                .color(Color::WHITE)
                .padding(EdgeInsets::all(24.0))
                .child(
                    Flex::column()
                        .spacing(16.0)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(children![
                            Text::new("Media player").size(20.0),
                            Flex::row().spacing(12.0).children(children![
                                button("Play", |p| {
                                    let _ = p.play();
                                }),
                                button("Pause", MediaPlayer::pause),
                                button("Back 1 s", |p| {
                                    let at = p.position().saturating_sub(Duration::from_secs(1));
                                    p.seek(at);
                                }),
                                button("Stop", MediaPlayer::stop),
                            ]),
                            Status { text },
                        ]),
                ),
        );
    })?;
    Ok(())
}
