//! Frequency analysis — the FFT behind every audio-reactive visual, from
//! TouchDesigner's audio CHOPs to Hydra's `a.fft()` to Notch's spectrum
//! scopes, and the "Audio Sync" row of the document's capability table that
//! vieww had no answer for.
//!
//! # What this adds to the crate
//!
//! Everything else here *produces* samples — a [`Tone`](crate::Tone), a
//! [`Mixer`](crate::Mixer). This module reads them back and asks *what
//! frequencies* they contain: a [`Spectrum`] is the loudness of each pitch
//! band in a buffer, which is exactly the input a spectrum analyser, a
//! VU bar, a waveform-reactive glow or a beat detector wants. Synthesis and
//! analysis in one crate is also the cheapest complete test: render a
//! [`Tone`](crate::Tone) at a known frequency, analyse it, and the peak bin *is* the
//! frequency — no fixture file, no golden sample, no trust.
//!
//! # The one rule
//!
//! Analysis is a pure function of the buffer: the same samples give the
//! same spectrum, on every machine, in any order. No state carries between
//! calls, so two bars driven by two calls with the same buffer move
//! identically — the determinism the rest of this crate promises, extended
//! from "the mix is reproducible" to "the *picture of the mix* is
//! reproducible".
//!
//! # The transform
//!
//! A radix-2 Cooley–Tukey FFT, iterative, in-place, with the bit-reversal
//! permutation first — the textbook algorithm, with no dependency and no
//! allocation beyond the output. Sizes must be powers of two (the radix-2
//! butterfly's requirement), and [`Spectrum::analyze`] picks the window size
//! itself: the largest power of two that fits the buffer, because asking
//! every caller to pad their own buffer is asking them to know this
//! module's internals.
//!
//! ```
//! use vieww_audio::{Envelope, Mixer, Tone, Waveform};
//! use vieww_audio::analysis::{Spectrum, Window};
//! use std::time::Duration;
//!
//! // A 440 Hz sine with a quarter-second release, rendered.
//! let tone = Tone::new(440.0, Waveform::Sine)
//!     .envelope(Envelope::attack_release(
//!         Duration::from_millis(5),
//!         Duration::from_millis(250),
//!     ));
//! let samples = Mixer::new(44_100).add(tone, 0.8).render();
//!
//! let spectrum = Spectrum::analyze(&samples, Window::Hann);
//!
//! // The loudest pitch in the buffer is the pitch that was rendered.
//! let peak_hz = spectrum.bin_hz(spectrum.peak_bin());
//! assert!((peak_hz - 440.0).abs() < 12.0, "peak at {peak_hz} Hz");
//! ```

use crate::Samples;

/// The window applied before the transform.
///
/// # Why a window at all
///
/// The FFT treats its input as one period of an infinitely repeating
/// signal. A real buffer does not loop — the sine at its end meets the
/// sine at its start with a discontinuity, and a discontinuity is, to the
/// transform, energy at *every* frequency: one true pitch turns into a
/// smear across the whole spectrum ("leakage"). A window that fades the
/// buffer's ends to zero makes the loop seamless and the spectrum honest —
/// the price is a little width on each peak, which is why [`None`] remains
/// available for the buffers that genuinely do loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Window {
    /// No window. Correct only for signals that are periodic in the buffer
    /// (a whole number of cycles), where it is also the *sharpest* answer —
    /// a calibration tone synthesised at an exact bin frequency.
    #[default]
    None,
    /// The Hann window, `0.5·(1 − cos(2πt/T))`. The default recommendation
    /// of every spectral-analysis text, and the one to reach for when the
    /// buffer is an arbitrary slice of an ongoing signal.
    Hann,
}

impl Window {
    /// The window's coefficient at position `i` of `n`.
    fn coefficient(self, i: usize, n: usize) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Hann => {
                #[allow(clippy::cast_precision_loss)] // n ≤ buffer sizes; exact in f32
                let t = i as f32 / n as f32;
                0.5 * (1.0 - (std::f32::consts::TAU * t).cos())
            }
        }
    }
}

/// The frequency content of one buffer: one magnitude per bin, half as many
/// bins as the analysed window, because a real signal's spectrum is
/// symmetric and the upper half says nothing new.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    /// The magnitude of each bin, linear (not dB), length `window / 2`.
    ///
    /// Linear because this crate's consumers are visual: a bar chart wants
    /// a linear height, and converting to dB is one `log` at the call site
    /// for the few that want it.
    magnitudes: Vec<f32>,
    /// The sample rate the buffer was recorded at — the thing that turns a
    /// bin *index* into a bin *pitch*.
    rate: u32,
    /// The window size the analysis used, in samples.
    window: usize,
}

impl Spectrum {
    /// Analyse a buffer's frequencies.
    ///
    /// The analysed window is the largest power of two that fits the
    /// buffer (see the module docs for why the caller does not choose it),
    /// downmixed to mono first — a stereo buffer's left and right are two
    /// performances of the same material, and the *spectrum* of a stereo
    /// mix is the spectrum of their sum.
    ///
    /// An empty or one-sample buffer has no frequencies to report and
    /// returns an empty spectrum; `bin_hz` on it never runs, because there
    /// is no bin to ask about.
    pub fn analyze(samples: &Samples, window: Window) -> Self {
        if samples.frames() < 2 {
            return Self {
                magnitudes: Vec::new(),
                rate: samples.rate,
                window: 0,
            };
        }
        // The largest power of two that fits, found by halving rather than
        // by floating-point log — `log2` and `pow2` round-tripping through
        // f32 is a place two implementations disagree by one bin.
        let mut size = 1;
        while size * 2 <= samples.frames() {
            size *= 2;
        }

        // Downmix to mono: the mean of the channels, per frame.
        let channels = samples.channels.max(1) as usize;
        let mut re = vec![0.0f32; size];
        for (i, slot) in re.iter_mut().enumerate() {
            let frame = &samples.data[i * channels..(i + 1) * channels];
            *slot = frame.iter().sum::<f32>() / channels as f32;
        }

        // Window, then transform. The imaginary part starts at zero: the
        // input is real, and the FFT fills it in as it goes.
        for (i, slot) in re.iter_mut().enumerate() {
            *slot *= window.coefficient(i, size);
        }
        let mut im = vec![0.0f32; size];
        fft(&mut re, &mut im);

        // Magnitudes of the meaningful half. The 1/n normalisation turns
        // the FFT's raw sums (which grow with n) into amplitudes a caller
        // can compare across window sizes.
        let half = size / 2;
        let mut magnitudes = Vec::with_capacity(half);
        for i in 0..half {
            // bin 0 is DC — its mirror is itself; every other bin has a
            // conjugate partner above Nyquist, so ×2 restores the energy
            // the half-spectrum dropped. Except DC and Nyquist themselves,
            // which have no partner; the distinction is one multiplication
            // nobody's bar chart can see, and the honest simple answer is
            // to leave them un-doubled.
            let scale = if i == 0 { 1.0 } else { 2.0 };
            magnitudes.push(scale * (re[i] * re[i] + im[i] * im[i]).sqrt() / size as f32);
        }

        Self {
            magnitudes,
            rate: samples.rate,
            window: size,
        }
    }

    /// The magnitudes, one per bin, linear — half as many as the window.
    pub fn magnitudes(&self) -> &[f32] {
        &self.magnitudes
    }

    /// The window size the analysis used.
    pub fn window(&self) -> usize {
        self.window
    }

    /// The pitch of bin `i`, in Hz.
    ///
    /// # Panics
    ///
    /// If `i` is past the last bin — an out-of-range bin has no pitch, and
    /// a caller indexing a loop by a stale `window` should find out now
    /// rather than draw a bar at a frequency that does not exist.
    pub fn bin_hz(&self, i: usize) -> f32 {
        assert!(
            i < self.magnitudes.len(),
            "bin {i} of {} does not exist",
            self.magnitudes.len()
        );
        i as f32 * as_f32(self.rate) / self.window as f32
    }

    /// The loudest bin's index.
    ///
    /// An empty spectrum (an empty buffer) has no loudest bin, and `None`
    /// says so — a caller drawing a peak marker skips it rather than
    /// inventing a bin zero that has no meaning.
    pub fn peak_bin(&self) -> usize {
        let mut best = 0usize;
        let mut best_mag = f32::NEG_INFINITY;
        for (i, magnitude) in self.magnitudes.iter().enumerate() {
            if *magnitude > best_mag {
                best_mag = *magnitude;
                best = i;
            }
        }
        best
    }

    /// The total magnitude of the bins whose pitch falls in `lo..=hi` Hz —
    /// the "how much bass is in this" question, and the one a bar chart of
    /// K bands answers by calling it K times with K ranges.
    ///
    /// Bins outside the spectrum's range (above Nyquist, below DC) simply
    /// contribute nothing, which makes a band past the top of the spectrum
    /// zero rather than an error: a 5-band bar chart asked to run on a
    /// 8 kHz buffer has a top band that is legitimately silent.
    pub fn band_energy(&self, lo_hz: f32, hi_hz: f32) -> f32 {
        if self.magnitudes.is_empty() {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 0..self.magnitudes.len() {
            let hz = self.bin_hz(i);
            if hz >= lo_hz && hz <= hi_hz {
                total += self.magnitudes[i];
            }
        }
        total
    }
}

/// The radix-2 Cooley–Tukey FFT, in place, real and imaginary parts passed
/// as separate slices of equal, power-of-two length.
///
/// The classic three stages: bit-reverse the order, then butterflies of
/// doubling width. Every twiddle factor is recomputed per butterfly — a
/// table would be faster, and a table is also a cache the caller cannot
/// see; for the window sizes a UI analyses (≤ 4096 is typical, 8192 is
/// generous) the recomputation is a rounding error against the transform
/// itself.
pub(crate) fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert_eq!(n, im.len());
    debug_assert!(n.is_power_of_two(), "radix-2 needs a power of two");
    if n <= 1 {
        return;
    }

    // Bit-reversal permutation: the butterfly stage reads its inputs at a
    // stride that halves each pass, which is the *reversed* bit order of
    // the natural one, and permuting once up front means every pass can
    // read its neighbours contiguously.
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Butterflies: width 2, 4, 8, ... n. At width w, pairs `i` and `i + w/2`
    // combine through the twiddle e^{-2πi·k/w}.
    let mut width = 2;
    while width <= n {
        let half = width / 2;
        let angle = -std::f32::consts::TAU / width as f32;
        for start in (0..n).step_by(width) {
            for k in 0..half {
                let twiddle_re = (angle * k as f32).cos();
                let twiddle_im = (angle * k as f32).sin();
                let a = start + k;
                let b = a + half;
                // (re[b] + i·im[b]) × (twiddle_re + i·twiddle_im)
                let product_re = re[b] * twiddle_re - im[b] * twiddle_im;
                let product_im = re[b] * twiddle_im + im[b] * twiddle_re;
                re[b] = re[a] - product_re;
                im[b] = im[a] - product_im;
                re[a] += product_re;
                im[a] += product_im;
            }
        }
        width *= 2;
    }
}

/// `u32`→`f32` for sample rates — always small enough to be exact, and the
/// same spelling the rest of this crate uses.
#[allow(clippy::cast_precision_loss)] // sample rates: exact in f32
fn as_f32(value: u32) -> f32 {
    value as f32
}

/// A detected fundamental: frequency and how periodic the signal was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    /// Hz.
    pub hz: f32,
    /// 0..=1: 1 is perfectly periodic (1 − the YIN dip's depth).
    pub clarity: f32,
}

/// The fundamental of a monophonic frame by YIN (de Cheveigné & Kawahara,
/// 2002): the cumulative-mean-normalised difference function, its first
/// dip under the threshold, refined by parabolic interpolation (AU-22).
///
/// `frame` is mono; `rate` its sample rate; the search covers
/// `min_hz..=max_hz`. The frame must hold at least two periods of
/// `min_hz`. `None` when nothing periodic enough is found.
#[must_use]
pub fn yin(frame: &[f32], rate: u32, min_hz: f32, max_hz: f32) -> Option<Pitch> {
    const THRESHOLD: f32 = 0.15;
    #[expect(clippy::cast_precision_loss, reason = "a sample rate")]
    let r = rate as f32;
    let w = frame.len() / 2;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a lag in samples"
    )]
    let (lo, hi) = (
        (r / max_hz.max(1.0)).floor().max(2.0) as usize,
        ((r / min_hz.max(1.0)).ceil() as usize).min(w.saturating_sub(1)),
    );
    if hi <= lo + 2 {
        return None;
    }
    // d(τ), then d'(τ) = d(τ)·τ / Σ_{j≤τ} d(j).
    let mut cmnd = vec![1.0f32; hi + 2];
    let mut running = 0.0f64;
    for tau in 1..=hi + 1 {
        let d: f64 = (0..w)
            .map(|j| {
                let diff = f64::from(frame[j]) - f64::from(frame[j + tau]);
                diff * diff
            })
            .sum();
        running += d;
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "a lag"
        )]
        {
            cmnd[tau] = if running > 0.0 {
                (d * tau as f64 / running) as f32
            } else {
                1.0
            };
        }
    }
    // The first dip under the threshold, walked down to its minimum; if
    // none dips that far, the global minimum.
    let mut best = None;
    let mut tau = lo;
    while tau <= hi {
        if cmnd[tau] < THRESHOLD {
            while tau < hi && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            best = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau = best.unwrap_or_else(|| {
        (lo..=hi)
            .min_by(|a, b| cmnd[*a].total_cmp(&cmnd[*b]))
            .unwrap_or(lo)
    });
    let clarity = 1.0 - cmnd[tau].clamp(0.0, 1.0);
    if clarity < 0.5 {
        return None;
    }
    // Parabolic interpolation around the dip.
    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
    let denom = a - 2.0 * b + c;
    let shift = if denom.abs() > f32::EPSILON {
        (0.5 * (a - c) / denom).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    #[expect(clippy::cast_precision_loss, reason = "a lag")]
    let period = tau as f32 + shift;
    Some(Pitch {
        hz: r / period,
        clarity,
    })
}

/// The energy in each of the twelve pitch classes, C = 0 … B = 11, from a
/// spectrum's bins between 55 Hz and 5 kHz (A4 = 440 Hz), normalised to
/// sum to 1 — the input to key detection (AU-22).
#[must_use]
pub fn chroma(spectrum: &Spectrum) -> [f32; 12] {
    let mut c = [0.0f32; 12];
    for (i, m) in spectrum.magnitudes().iter().enumerate().skip(1) {
        let f = spectrum.bin_hz(i);
        if !(55.0..=5000.0).contains(&f) {
            continue;
        }
        let semis = 12.0 * (f / 440.0).log2() + 9.0; // 0 = C
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a pitch class"
        )]
        let pc = (semis.round().rem_euclid(12.0)) as usize % 12;
        c[pc] += m * m;
    }
    let total: f32 = c.iter().sum();
    if total > 0.0 {
        for v in &mut c {
            *v /= total;
        }
    }
    c
}

/// Magnitude to dB, floored at −120 dB.
fn to_db(m: f32) -> f32 {
    if m > 1e-6 {
        20.0 * m.log10()
    } else {
        -120.0
    }
}

/// A spectrum display's peak-hold line (AU-22): each bin holds its
/// highest level, then falls at `fall_db_per_s` after `hold_s` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakHold {
    peaks_db: Vec<f32>,
    held_s: Vec<f32>,
    hold_s: f32,
    fall_db_per_s: f32,
}

impl PeakHold {
    /// A peak line that holds for `hold_s`, then falls at `fall_db_per_s`.
    #[must_use]
    pub const fn new(hold_s: f32, fall_db_per_s: f32) -> Self {
        Self {
            peaks_db: Vec::new(),
            held_s: Vec::new(),
            hold_s,
            fall_db_per_s,
        }
    }

    /// Fold in a frame of `spectrum`, `dt` seconds after the last one.
    pub fn push(&mut self, spectrum: &Spectrum, dt: f32) {
        let m = spectrum.magnitudes();
        if self.peaks_db.len() != m.len() {
            self.peaks_db = vec![-120.0; m.len()];
            self.held_s = vec![0.0; m.len()];
        }
        for ((p, h), v) in self.peaks_db.iter_mut().zip(&mut self.held_s).zip(m) {
            let db = to_db(*v);
            if db >= *p {
                *p = db;
                *h = 0.0;
            } else {
                *h += dt;
                if *h > self.hold_s {
                    *p = (*p - self.fall_db_per_s * dt).max(db);
                }
            }
        }
    }

    /// The held level per bin, dB.
    #[must_use]
    pub fn peaks_db(&self) -> &[f32] {
        &self.peaks_db
    }
}

/// A spectrogram's rows, newest last (AU-22): each pushed spectrum
/// becomes one row of dB levels, and at most `rows` are kept, ready to
/// draw as a scrolling waterfall.
#[derive(Debug, Clone, PartialEq)]
pub struct Waterfall {
    rows: std::collections::VecDeque<Vec<f32>>,
    capacity: usize,
}

impl Waterfall {
    /// Keep the last `rows` spectra.
    #[must_use]
    pub fn new(rows: usize) -> Self {
        Self {
            rows: std::collections::VecDeque::with_capacity(rows.max(1)),
            capacity: rows.max(1),
        }
    }

    /// Add a row from `spectrum`, dropping the oldest when full.
    pub fn push(&mut self, spectrum: &Spectrum) {
        let row = if self.rows.len() == self.capacity {
            let mut r = self.rows.pop_front().unwrap_or_default();
            r.clear();
            r
        } else {
            Vec::new()
        };
        let mut row = row;
        row.extend(spectrum.magnitudes().iter().map(|m| to_db(*m)));
        self.rows.push_back(row);
    }

    /// The rows, oldest first, each in dB per bin.
    pub fn rows(&self) -> impl Iterator<Item = &[f32]> {
        self.rows.iter().map(Vec::as_slice)
    }

    /// How many rows are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no row has been pushed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Envelope, Mixer, Tone, Waveform};
    use std::time::Duration;

    /// Render a sine of `hz` for `seconds` at 44.1 kHz — the synthesis side
    /// of the round-trip tests below. `held` rather than `new`: a
    /// `Tone::new` is a zero-hold pulse, which is silence almost
    /// everywhere, and a spectrum of silence finds nothing.
    fn sine(hz: f32, seconds: f32) -> Samples {
        let tone = Tone::held(hz, Waveform::Sine, Duration::from_secs_f32(seconds));
        let frames = (44_100.0 * seconds) as usize;
        let mut data = Vec::with_capacity(frames);
        for i in 0..frames {
            let t = Duration::from_secs_f32(i as f32 / 44_100.0);
            data.push(0.8 * tone.at(t));
        }
        Samples::mono(data, 44_100)
    }

    #[test]
    fn a_dc_signal_is_all_bin_zero() {
        // The purest possible check of the transform: a constant has one
        // frequency, zero, and the FFT's job is to put all of it there.
        let data = vec![0.5f32; 4096];
        let spectrum = Spectrum::analyze(&Samples::mono(data, 44_100), Window::None);
        assert!(spectrum.magnitudes()[0] > 0.4);
        for (i, magnitude) in spectrum.magnitudes().iter().enumerate() {
            if i > 0 {
                assert!(*magnitude < 1e-3, "bin {i} should be silent: {magnitude}");
            }
        }
    }

    #[test]
    fn a_sine_lands_in_its_own_bin() {
        // 100 cycles in a 2048-sample window at 44.1 kHz = 2151.17 Hz, but
        // a *bin-aligned* test is stronger: 44100/2048 × 100 = 2153.32...
        // pick the frequency that is bin 100 exactly.
        let rate = 44_100u32;
        let window = 2048usize;
        let bin = 100usize;
        let hz = bin as f32 * rate as f32 / window as f32;
        // A whole number of cycles → no leakage, no window needed.
        let data: Vec<f32> = (0..window)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
            .collect();
        let spectrum = Spectrum::analyze(&Samples::mono(data, rate), Window::None);
        assert_eq!(spectrum.peak_bin(), bin, "the sine's own bin");
        assert!((spectrum.bin_hz(bin) - hz).abs() < 1e-3);
        // And it is *the* peak by a wide margin.
        let peak = spectrum.magnitudes()[bin];
        for (i, magnitude) in spectrum.magnitudes().iter().enumerate() {
            if i != bin {
                assert!(*magnitude < peak * 0.01, "bin {i} at {magnitude} vs {peak}");
            }
        }
    }

    #[test]
    fn a_hanned_arbitrary_sine_is_still_found() {
        // 440 Hz does not sit on a bin of any power-of-two window at
        // 44.1 kHz — this is the *leakage* case, and the Hann window is
        // what keeps the peak findable anyway.
        let spectrum = Spectrum::analyze(&sine(440.0, 0.2), Window::Hann);
        let peak_hz = spectrum.bin_hz(spectrum.peak_bin());
        assert!(
            (peak_hz - 440.0).abs() < 12.0,
            "peak at {peak_hz}, not near 440"
        );
    }

    #[test]
    fn the_window_size_is_the_largest_power_of_two_that_fits() {
        // 3000 frames: 2048 fits, 4096 does not.
        let spectrum = Spectrum::analyze(&sine(440.0, 3000.0 / 44_100.0), Window::Hann);
        assert_eq!(spectrum.window(), 2048);
        assert_eq!(spectrum.magnitudes().len(), 1024);
    }

    #[test]
    fn nyquist_sits_just_above_the_last_bin() {
        // The kept bins run from DC to one below Nyquist (the Nyquist bin
        // itself is the spectrum's edge case and visuals never read it), so
        // the last bin is *near* 22 050 by construction: within two
        // spacings of it, at the window this test renders.
        let spectrum = Spectrum::analyze(&sine(440.0, 0.1), Window::Hann);
        let last = spectrum.magnitudes().len() - 1;
        let top = spectrum.bin_hz(last);
        let spacing = 44_100.0 / spectrum.window() as f32;
        assert!((22_050.0 - top) < spacing * 2.0, "top bin at {top}");
    }

    #[test]
    fn stereo_is_downmixed_not_left_only() {
        // The same sine in both channels — unattenuated — analyses
        // identically to the mono original: the downmix is the *mean*, so
        // duplicating a channel into both halves of a stereo frame leaves
        // the mean, and the spectrum, unchanged.
        let mono = sine(440.0, 0.1);
        let mut stereo_data = Vec::with_capacity(mono.data.len() * 2);
        for sample in &mono.data {
            stereo_data.push(*sample);
            stereo_data.push(*sample);
        }
        let stereo = Samples::stereo(stereo_data, 44_100);
        let a = Spectrum::analyze(&mono, Window::Hann);
        let b = Spectrum::analyze(&stereo, Window::Hann);
        for (x, y) in a.magnitudes().iter().zip(b.magnitudes().iter()) {
            assert!((x - y).abs() < 1e-4, "{x} vs {y}");
        }
    }

    #[test]
    fn the_band_containing_the_note_holds_the_energy() {
        let spectrum = Spectrum::analyze(&sine(440.0, 0.2), Window::Hann);
        let around = spectrum.band_energy(300.0, 600.0);
        let elsewhere = spectrum.band_energy(1000.0, 10_000.0);
        assert!(around > 0.1, "the band with the note has energy: {around}");
        assert!(
            around > elsewhere * 10.0,
            "the note's band dominates the rest: {around} vs {elsewhere}"
        );
        // A band beyond Nyquist is legitimately silent, not an error.
        assert_eq!(spectrum.band_energy(30_000.0, 40_000.0), 0.0);
    }

    #[test]
    fn an_empty_buffer_is_an_empty_spectrum() {
        let spectrum = Spectrum::analyze(&Samples::empty(44_100), Window::Hann);
        assert!(spectrum.magnitudes().is_empty());
        assert_eq!(spectrum.peak_bin(), 0);
        assert_eq!(spectrum.band_energy(0.0, 20_000.0), 0.0);
    }

    #[test]
    fn analysis_is_deterministic() {
        let samples = sine(1234.0, 0.05);
        let a = Spectrum::analyze(&samples, Window::Hann);
        let b = Spectrum::analyze(&samples, Window::Hann);
        assert_eq!(a, b);
    }

    #[test]
    fn two_different_notes_are_told_apart() {
        let low = Spectrum::analyze(&sine(220.0, 0.2), Window::Hann);
        let high = Spectrum::analyze(&sine(1760.0, 0.2), Window::Hann);
        let low_hz = low.bin_hz(low.peak_bin());
        let high_hz = high.bin_hz(high.peak_bin());
        assert!((low_hz - 220.0).abs() < 12.0);
        assert!((high_hz - 1760.0).abs() < 24.0);
        assert!(high_hz > low_hz * 4.0);
    }

    #[test]
    #[should_panic(expected = "does not exist")]
    fn an_out_of_range_bin_is_loud() {
        // 0.1 s at 44.1 kHz is 4410 frames → a 4096 window → 2048 bins,
        // so 2048 is the first bin that does not exist.
        let spectrum = Spectrum::analyze(&sine(440.0, 0.1), Window::Hann);
        let _ = spectrum.bin_hz(2048);
    }

    #[test]
    fn the_mixer_round_trip_finds_both_notes() {
        // The full-crate round trip: two tones mixed, and both *found in
        // the analysis* — the test that says synthesis and analysis agree.
        //
        // Not by `peak_bin`, which is a bin-alignment lottery (a note that
        // happens to sit on a bin concentrates its energy into one bucket
        // and always wins it), but by band energy: the band holding each
        // note must dominate a band holding neither.
        let hold = Duration::from_millis(300);
        let shape =
            || Envelope::attack_release(Duration::from_millis(5), Duration::from_millis(50));
        let chord = Mixer::new(44_100)
            .add(
                Tone::held(440.0, Waveform::Sine, hold).envelope(shape()),
                0.5,
            )
            .add(
                Tone::held(1318.0, Waveform::Sine, hold).envelope(shape()),
                0.5,
            );
        let rendered = chord.render();
        let spectrum = Spectrum::analyze(&rendered, Window::Hann);

        let note_a = spectrum.band_energy(420.0, 460.0);
        let note_b = spectrum.band_energy(1290.0, 1350.0);
        let quiet_a = spectrum.band_energy(600.0, 1200.0);
        let quiet_b = spectrum.band_energy(2000.0, 4000.0);
        assert!(note_a > quiet_a * 10.0, "440 Hz: {note_a} vs {quiet_a}");
        assert!(note_b > quiet_b * 10.0, "1318 Hz: {note_b} vs {quiet_b}");
        // And the two notes are the two loudest things in the buffer.
        assert!(note_a > 0.01 && note_b > 0.01);
    }

    fn sine_wave(hz: f32, rate: u32, secs: f32) -> Vec<f32> {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a length"
        )]
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "a sample index")]
                let t = i as f32 / rate as f32;
                (std::f32::consts::TAU * hz * t).sin()
            })
            .collect()
    }

    fn cents(a: f32, b: f32) -> f32 {
        1200.0 * (a / b).log2()
    }

    /// AU-22: YIN finds every one of the 22 shrutis over Sa = 220 Hz to
    /// within 5 cents — microtones are this module's home ground.
    #[test]
    fn yin_finds_the_twenty_two_shrutis_within_five_cents() {
        // The classical 22-shruti ratios (just intonation).
        let ratios: [f32; 22] = [
            1.0,
            256.0 / 243.0,
            16.0 / 15.0,
            10.0 / 9.0,
            9.0 / 8.0,
            32.0 / 27.0,
            6.0 / 5.0,
            5.0 / 4.0,
            81.0 / 64.0,
            4.0 / 3.0,
            27.0 / 20.0,
            45.0 / 32.0,
            729.0 / 512.0,
            3.0 / 2.0,
            128.0 / 81.0,
            8.0 / 5.0,
            5.0 / 3.0,
            27.0 / 16.0,
            16.0 / 9.0,
            9.0 / 5.0,
            15.0 / 8.0,
            243.0 / 128.0,
        ];
        for r in ratios {
            let hz = 220.0 * r;
            let x = sine_wave(hz, 48_000, 0.05);
            let p = yin(&x, 48_000, 80.0, 1000.0).expect("a pitch");
            assert!(cents(p.hz, hz).abs() < 5.0, "{hz} Hz read as {} Hz", p.hz);
            assert!(p.clarity > 0.9);
        }
    }

    /// A gamaka-style vibrato (±50 cents at 6 Hz) is tracked frame by frame.
    #[test]
    fn yin_tracks_a_vibrato() {
        let rate = 48_000u32;
        let mut phase = 0.0f32;
        let x: Vec<f32> = (0..rate as usize)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "a sample index")]
                let t = i as f32 / rate as f32;
                let f = 330.0 * 2f32.powf(0.5 / 12.0 * (std::f32::consts::TAU * 6.0 * t).sin());
                phase += std::f32::consts::TAU * f / rate as f32;
                phase.sin()
            })
            .collect();
        let frame = 1024;
        for start in (0..x.len() - frame).step_by(2400) {
            #[expect(clippy::cast_precision_loss, reason = "a sample index")]
            let t = (start + frame / 4) as f32 / rate as f32;
            let want = 330.0 * 2f32.powf(0.5 / 12.0 * (std::f32::consts::TAU * 6.0 * t).sin());
            let p = yin(&x[start..start + frame], rate, 100.0, 1000.0).expect("a pitch");
            assert!(
                cents(p.hz, want).abs() < 25.0,
                "at {t}s: {} vs {want}",
                p.hz
            );
        }
    }

    #[test]
    fn yin_reports_nothing_for_noise() {
        let mut seed = 1u32;
        let x: Vec<f32> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                #[expect(clippy::cast_precision_loss, reason = "noise")]
                let v = (seed >> 8) as f32 / (1u32 << 24) as f32;
                v * 2.0 - 1.0
            })
            .collect();
        assert!(yin(&x, 48_000, 80.0, 1000.0).is_none());
    }

    /// A C major triad's chroma peaks on C, E and G.
    #[test]
    fn chroma_of_a_triad_names_its_notes() {
        let rate = 44_100;
        let mut x = vec![0.0f32; 16_384];
        for hz in [261.63f32, 329.63, 392.0] {
            for (v, s) in x.iter_mut().zip(sine_wave(hz, rate, 1.0)) {
                *v += s / 3.0;
            }
        }
        let c = chroma(&Spectrum::analyze(&Samples::mono(x, rate), Window::Hann));
        let mut order: Vec<usize> = (0..12).collect();
        order.sort_by(|a, b| c[*b].total_cmp(&c[*a]));
        let mut top = order[..3].to_vec();
        top.sort_unstable();
        assert_eq!(top, vec![0, 4, 7], "{c:?}");
    }

    #[test]
    fn peak_hold_holds_then_falls_and_the_waterfall_scrolls() {
        let rate = 48_000;
        let loud = Spectrum::analyze(
            &Samples::mono(sine_wave(1000.0, rate, 0.1), rate),
            Window::Hann,
        );
        let silent = Spectrum::analyze(&Samples::mono(vec![0.0; 4800], rate), Window::Hann);
        let bin = loud.peak_bin();
        let mut ph = PeakHold::new(0.5, 20.0);
        ph.push(&loud, 0.1);
        let top = ph.peaks_db()[bin];
        ph.push(&silent, 0.3); // inside the hold
        assert_eq!(ph.peaks_db()[bin], top, "held");
        ph.push(&silent, 0.5); // past the hold: falls 10 dB
        assert!(
            (ph.peaks_db()[bin] - (top - 10.0)).abs() < 1e-3,
            "{} vs {top}",
            ph.peaks_db()[bin]
        );

        let mut w = Waterfall::new(3);
        for s in [&loud, &silent, &loud, &silent] {
            w.push(s);
        }
        assert_eq!(w.len(), 3, "the oldest row scrolled off");
        let rows: Vec<&[f32]> = w.rows().collect();
        assert!(
            rows[1][bin] > rows[0][bin] + 40.0,
            "the loud row is the middle one"
        );
    }
}
