//! A score played live (AU-17): the same part streams and mixer as
//! [`render`](crate::render::render), pulled block by block from an audio
//! callback, with a transport.
//!
//! Live playback is offline rendering run incrementally. The unmastered
//! mix it produces is the one
//! [`render_unmastered`](crate::render::render_unmastered) returns, sample
//! for sample, whatever block size the device asks for, which a test pins.
//! Mastering is the one stage that cannot run live: it measures the
//! loudness of the whole piece before it sets a gain. A live player
//! applies a fixed gain instead ([`LiveScore::set_gain_db`]), for example
//! the one a previous offline render settled on.
//!
//! [`LiveScore::render`] fills an interleaved stereo buffer at the
//! score's rate. That is the shape of an
//! [`Engine`](vieww_audio::Engine) or [`Stream`](vieww_audio::stream)
//! render callback. Parts render into scratch buffers sized once, and up to
//! 32 parts are mixed without allocating.

use crate::render::{mixer_for, part_stream, total_frames, PartStream, Settings};
use crate::score::Score;
use vieww_audio::mixer::Mixer;

/// The largest block rendered in one pass; longer requests are split.
const BLOCK: usize = 1024;

/// A score with a playhead.
pub struct LiveScore {
    score: Score,
    settings: Settings,
    streams: Vec<Box<dyn PartStream>>,
    mixer: Mixer,
    scratch: Vec<Vec<f32>>,
    frame: usize,
    end: usize,
    playing: bool,
    gain: f32,
}

impl std::fmt::Debug for LiveScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveScore")
            .field("title", &self.score.title)
            .field("frame", &self.frame)
            .field("end", &self.end)
            .field("playing", &self.playing)
            .finish_non_exhaustive()
    }
}

impl LiveScore {
    /// A score ready to play from the start, paused.
    #[must_use]
    pub fn new(score: Score, settings: Settings) -> Self {
        let streams = score
            .parts
            .iter()
            .map(|p| part_stream(p, &score, settings))
            .collect();
        let mixer = mixer_for(&score, settings);
        let scratch = vec![vec![0.0; BLOCK * 2]; score.parts.len()];
        let end = total_frames(&score, settings);
        Self {
            score,
            settings,
            streams,
            mixer,
            scratch,
            frame: 0,
            end,
            playing: false,
            gain: 1.0,
        }
    }

    /// Start (or continue) playing.
    pub fn play(&mut self) {
        self.playing = true;
    }

    /// Stop where it is; [`Self::render`] then writes silence.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Whether it is playing.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// A fixed output gain in dB (live playback is not mastered).
    pub fn set_gain_db(&mut self, db: f32) {
        self.gain = 10f32.powf(db / 20.0);
    }

    /// The playhead, in beats.
    #[must_use]
    pub fn position_beats(&self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "a frame count")]
        let secs = self.frame as f64 / f64::from(self.settings.rate);
        secs / self.score.seconds_per_beat()
    }

    /// Whether the playhead has passed the end (tail included).
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.frame >= self.end
    }

    /// Move the playhead to `beat`. The streams are rebuilt and run
    /// silently up to it, so what follows is exactly what playing through
    /// would have produced (reverb tails and sustained notes included).
    /// Not real-time: call it from the UI thread, not the callback.
    pub fn seek_beat(&mut self, beat: f64) {
        let playing = self.playing;
        let gain = self.gain;
        *self = Self::new(
            std::mem::replace(&mut self.score, Score::new("", 120.0)),
            self.settings,
        );
        self.playing = playing;
        self.gain = gain;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame index"
        )]
        let target = ((beat.max(0.0) * self.score.seconds_per_beat())
            * f64::from(self.settings.rate))
        .round() as usize;
        let mut sink = vec![0.0f32; BLOCK * 2];
        while self.frame < target.min(self.end) {
            let n = (target - self.frame).min(BLOCK);
            self.advance(&mut sink[..n * 2]);
        }
    }

    /// Fill `out` (interleaved stereo) with the next frames: the mix while
    /// playing, silence when paused or finished.
    #[vieww_audio::realtime]
    pub fn render(&mut self, out: &mut [f32]) {
        if !self.playing {
            out.fill(0.0);
            return;
        }
        for chunk in out.chunks_mut(BLOCK * 2) {
            if self.finished() {
                chunk.fill(0.0);
                continue;
            }
            self.advance(chunk);
            if self.gain != 1.0 {
                for v in chunk.iter_mut() {
                    *v *= self.gain;
                }
            }
        }
    }

    /// Render the next `out.len() / 2` (≤ BLOCK) frames of the mix.
    // More than 32 parts take a Vec of input slices: the one allocation,
    // named here; the no-alloc harness plays 32 or fewer.
    #[vieww_audio::realtime(allow(collect))]
    fn advance(&mut self, out: &mut [f32]) {
        let len = out.len();
        for (stream, buf) in self.streams.iter_mut().zip(self.scratch.iter_mut()) {
            stream.render(&mut buf[..len]);
        }
        // Up to 32 parts without touching the allocator; more take a Vec.
        const FIXED: usize = 32;
        if self.scratch.len() <= FIXED {
            let mut refs: [&[f32]; FIXED] = [&[]; FIXED];
            for (r, b) in refs.iter_mut().zip(&self.scratch) {
                *r = &b[..len];
            }
            self.mixer.process(&refs[..self.scratch.len()], out);
        } else {
            let inputs: Vec<&[f32]> = self.scratch.iter().map(|b| &b[..len]).collect();
            self.mixer.process(&inputs, out);
        }
        self.frame += len / 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render_unmastered;

    fn settings() -> Settings {
        Settings {
            rate: 24_000,
            polyphony: 8,
            tail: 0.5,
            ..Settings::default()
        }
    }

    /// The flagship check: every demo piece played live, in uneven device
    /// blocks, is the offline mix — a null deeper than 60 dB (in practice
    /// it is exact).
    #[test]
    fn live_playback_is_the_offline_mix() {
        for score in [
            crate::pieces::rock(),
            crate::pieces::chorale(true),
            crate::pieces::carnatic(3).0,
        ] {
            let s = settings();
            let offline = render_unmastered(&score, s).data;
            let mut live = LiveScore::new(score.clone(), s);
            live.play();
            let mut got = Vec::with_capacity(offline.len());
            let blocks = [128usize, 441, 1024, 2048, 37];
            let mut k = 0;
            while got.len() < offline.len() {
                let n = blocks[k % blocks.len()].min((offline.len() - got.len()) / 2);
                let mut b = vec![0.0f32; n * 2];
                live.render(&mut b);
                got.extend_from_slice(&b);
                k += 1;
            }
            let err: f64 = offline
                .iter()
                .zip(&got)
                .map(|(a, b)| f64::from(a - b).powi(2))
                .sum();
            let sig: f64 = offline.iter().map(|a| f64::from(*a).powi(2)).sum();
            let null_db = 10.0 * (sig / err.max(1e-30)).log10();
            assert!(
                null_db > 60.0,
                "{}: live vs offline null only {null_db:.1} dB",
                score.title
            );
            assert!(live.finished());
        }
    }

    #[test]
    fn pause_seek_and_gain() {
        let score = crate::pieces::rock();
        let s = settings();
        let offline = render_unmastered(&score, s).data;
        let mut live = LiveScore::new(score.clone(), s);
        let mut b = vec![1.0f32; 512];
        live.render(&mut b);
        assert!(b.iter().all(|v| *v == 0.0), "paused is silent");
        assert_eq!(live.position_beats(), 0.0);

        // Seek to beat 4 and play: the samples are the offline mix there.
        live.seek_beat(4.0);
        live.play();
        let at = (4.0 * score.seconds_per_beat() * f64::from(s.rate)).round() as usize;
        assert!((live.position_beats() - 4.0).abs() < 1e-3);
        let mut b = vec![0.0f32; 2048];
        live.render(&mut b);
        let want = &offline[at * 2..at * 2 + 2048];
        let max_err = b
            .iter()
            .zip(want)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_err < 1e-4,
            "after a seek the mix continues exactly ({max_err})"
        );

        live.set_gain_db(-6.0206);
        let mut c = vec![0.0f32; 512];
        live.render(&mut c);
        let want = &offline[at * 2 + 2048..at * 2 + 2048 + 512];
        for (x, y) in c.iter().zip(want) {
            assert!((x - y * 0.5).abs() < 1e-4);
        }
    }
}
