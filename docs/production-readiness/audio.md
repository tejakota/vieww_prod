# vieww-audio + vieww-music — Production-Readiness Deep Research (Task 2-c)

Repo: `/home/z/my-project/vieww_artifacts` (`main`, HEAD `98367be`). Scope: `vieww_base/crates/vieww-audio` (54,569 lines), `vieww_base/crates/vieww-music` (5,432 lines), `vieww_base/crates/vieww-audio-plugin` (8,901 lines), `.github/workflows/{audio-ci,audio-backends,audio-plugins}.yml`, branch `origin/feature/main_fixes` (audio diff: **empty**).

## 0. Method & verification notes

- Static code reading only, no `cargo build/test` (per instructions). Every claim below carries a `path:line` reference into the checked-out tree; quotes are excerpts.
- Both branches diffed: `git diff main origin/feature/main_fixes -- vieww_base/crates/vieww-audio vieww_base/crates/vieww-music vieww_base/crates/vieww-audio-plugin .github/workflows/audio-*.yml` → **zero changes**. The audio stack is identical on both branches (§3).
- Workflow restoration history traced via `git log --all -- .github/workflows*`: `8fae3b4` ("the three workflows ride as data until a workflow-scoped token can place them") → `63ce0fa` (audio-ci rides in workflows-pending) → `c858840` (device loopback tests + CI on virtual devices + hardware audio step) → `82b89a8` (audio-ci placed in `.github/workflows/`) → `2b12449` (workflows-pending README updated) → `3d08785` (audio-backends refactored for a VM: the Azure kernel lacks sound modules, so ALSA loopback runs in a stock-kernel virtme-ng VM) → `c7e08db` ("Update audio-ci.yml" — an **empty commit**, no file changes).
- No live web access was available in this sandbox; leader URLs in §4/§8 are canonical, stable documentation/GitHub links, used as references for each leader's documented behavior, not as live-fetched quotes.
- Test-count verification: `rg -c "#\[test\]"` → vieww-audio 550, vieww-music 62, vieww-audio-plugin 24 (≈636 tests + doctests). Fuzz targets: **8** (`decode`, `decode_wav`, `decode_flac`, `decode_mp3`, `decode_vorbis`, `decode_aac_adts`, `decode_aac_mp4`, `decode_aiff`) plus the oracle test `fuzz/tests/oracle_on_fixtures.rs` (the task brief said 9; the 9th is the oracle, which is a `cargo test`, not a fuzz target).
- Receipts check: `receipts/2026-10-15-kabylake-*` contain GPU/fixtures only — **no audio receipt exists yet** even though `ci/hardware/linux.sh:254-276` and `ci/hardware/windows.ps1:325+` define the self-hosted audio step (real sound card, live backends under load, 600 s default soak, optional line-out→line-in loopback and MIDI cable).

## 1. Executive verdict

| # | Dimension | Score /10 | Justification (evidence) |
|---|---|---|---|
| 1 | RT-safety of the audio path | **9** | Counting-global-allocator proof that graph, mixer, multi-core voices, disk streaming and note-on paths allocate nothing (`tests/realtime_no_alloc.rs:1-22`, harness-less so it sees every thread); FTZ/DAZ guard around every callback (`backend/mod.rs:273`); NaN/∞ zeroed at the device boundary (`backend/mod.rs:312-316`); lock-free SPSC ring from atomics only (`stream.rs:36-45`); panic isolation that never unwinds into C (`backend/mod.rs:262-325`); RT promotion per OS (`rt.rs:19-30`). Deductions: promotion does a small allocation on the first callback (`backend/mod.rs:278-294`, documented); the proof is a test, not a compiler/lint guarantee (AU-18). |
| 2 | Backend coverage & resilience | **8** | 9 backends, one trait: PipeWire (native SPA POD, `pipewire.rs:1-14`), JACK (ports, xrun callback, latency, `jack.rs:205-214`), PulseAudio, ALSA (own ioctls, format/rate negotiation + sinc resample, `alsa.rs:1-15`, `device.rs`), CoreAudio (macOS HAL + iOS RemoteIO), WASAPI shared **and exclusive** (`wasapi.rs:1-23`, alignment dance at `wasapi.rs:648-680`), AAudio (dlopen, error callback), Web Audio (worklet + 3 transports incl. SharedArrayBuffer worker mode, `web.rs:1-30`). `ResilientStream` survives unplug/replug/default-change with the **same render closure** (effect tails continue) and refuses to resurrect a panicked closure (`resilient.rs:78-94, 207-211`). Deductions: AAudio always shared mode, no usage attributes (`aaudio.rs:225-226`); AAudio reports no latency; route changes detected by 1 s polling, not events. |
| 3 | Codec completeness & licensing | **7** | Hand-rolled decoders verified against ffmpeg reference decodes with SNR grading (`codec/mod.rs:243-274`): WAV (rw, 8/16/24/32/int + f32/f64 + EXTENSIBLE), AIFF (rw), FLAC (rw, MD5-verified), MP3 (MPEG-1/2/2.5 L3 complete incl. intensity stereo + LAME gapless, `mp3.rs:1-14`), Vorbis, AAC-LC (ADTS + MP4 with edit-list priming removal; TNS/PNS/intensity implemented; HE-AAC SBR/PS, Main prediction, LTP refused *by name*, `aac.rs:14-16`). Frame-level error concealment (`codec/mod.rs:82-96`). Table provenance file exists (`tools/THIRD_PARTY.md`). Deductions: no Opus, no ALAC/WavPack, no lossy encoders; no AAC patent-pool notice (AU-07). |
| 4 | Streaming/seeking/gapless (large media) | **4** | `codec::decode(&[u8])` decodes the **whole file** to `Vec<f32>`; `Loader` reads the entire file into RAM first (`loader.rs:246-247`) and only resamples on a worker thread. `DiskStream` (seek/loop/underrun-counted, stale-audio-proof seeks, `loader.rs:1-14, 320+`) is **WAV-only**. A 3 h stereo 44.1 kHz file ≈ 3.8 GB of f32 → OOM on any phone. MP3 gapless (LAME tag) and MP4 priming are correct, but there is no incremental decoder API, no HTTP source, no playlist/gapless-chaining player. |
| 5 | Platform lifecycle (focus/interruption/background/hot-plug/Bluetooth) | **2** | Zero audio-focus code (no `AudioManager`/`AudioFocusRequest` anywhere in the repo — grep across `vieww_base` finds none). Zero AVAudioSession category/interruption/route handling (only a doc note that iOS capture *needs* a record category, `coreaudio.rs:7-9`). No Android foreground service / MediaSession, no iOS `UIBackgroundModes: audio`, no Bluetooth-specific path. Desktop hot-plug is genuinely good (`resilient.rs` + per-backend `default_device_id`); mobile lifecycle is effectively absent. |
| 6 | Latency & underrun handling | **8** | Every backend but AAudio reports output latency in ns via `Counters::latency_ns` (WASAPI `GetStreamLatency`+buffer `wasapi.rs:776-780`; CoreAudio device latency `coreaudio.rs:580,782`; JACK port latency `jack.rs:205`; ALSA `alsa.rs:193`; Pulse `pa_simple_get_latency` `pulse.rs:60-78`; PipeWire `pipewire.rs:373`). Telemetry: callbacks, frames, xruns (JACK callback, WASAPI `AUDCLNT_BUFFERFLAGS_SILENT`-adjacent accounting, web from JS), render mean/max, peak load, late renders (`backend/mod.rs:129-158`). Underruns counted in ring + DiskStream. `quality::latency` measures stimulus→recording delay by cross-correlation with sub-sample interpolation (`quality.rs:20-21`). Deductions: no automatic underrun response policy (e.g. AAudio buffer-size bump), AAudio latency not queried. |
| 7 | Synthesis & effects quality | **9** | Synth: band-limited oscs, wavetable, additive, vector, FM with all 32 DX7 algorithms, subtractive, granular, physical models (bendable Karplus–Strong, bowed, clarinet, modal, tambura, mridangam), sampler with SFZ + SF2/SF3, MPE polyphony (`lib.rs:14`, `synth/*`). FX: SVF/ladder, parametric/graphic/dynamic/linear-phase EQ, compressor/expander/lookahead limiter/gate/transient/multiband with LR4 crossovers, Freeverb + Hadamard FDN with RT60-as-spec (verified by Schroeder backward integration, `fx/reverb.rs:8-13`) + partitioned-FFT convolution, tempo-synced/ping-pong delay, chorus/flanger/phaser/tremolo/vibrato/ring-mod, waveshaper/Chebyshev/fold/bitcrusher with oversampling, master bus (`fx/mod.rs`). Spatializer with ITD + head-shadow + Doppler (`spatial.rs:8-14`). Deduction: no published perceptual benchmarks vs commercial reverbs/limiters. |
| 8 | Analysis | **8** | Radix-2 FFT + `Spectrum` with windows and band energy (`analysis.rs`), spectral-flux `OnsetDetector` with adaptive threshold + `estimate_tempo` by onset autocorrelation (`dsp.rs:389-465`), THD/THD+N/SINAD/ENOB (IEEE 1057 sine fitting), SMPTE IMD, AES17 dynamic range, IEC 61672 A-weighting, Farina log-sweep + IR deconvolution with per-order distortion separation, fractional-octave band levels, null depth (`quality.rs:1-22`), EBU R128 loudness/true-peak/VU/PPM/correlation (`meter.rs`). Deduction: no polyphonic pitch/chroma detection (YIN/MPM) — only single-tone `estimate_frequency`. |
| 9 | MIDI & sequencing | **8** | Messages + running-status parser, SMF type 0/1 read/write with tempo maps, MIDI clock, MTC (`midi.rs:1-12`), system ports on Linux (rawmidi), Windows (WinMM, SysEx both ways), macOS/iOS (CoreMIDI) (`midi_io.rs:9-17`), MTS real-time tuning messages in/out (`mts.rs:6-13`), tempo maps with closed-form accelerando ramps + sample-accurate look-ahead scheduler with sub-sample remainders (`transport.rs:1-19`), piano-roll clips with piano-roll edits, step sequencer (probability, ratchets, polymeter), Ableton-style launcher, arrangement, arpeggiator (`sequencer.rs:1-26`). Deductions: no MIDI 2.0/UMP, no Android AMidi, no Web MIDI, rawmidi misses ALSA-sequencer virtual ports (documented `midi_io.rs:15`). |
| 10 | Music-theory depth | **9** | Carnatic: all 72 melakartas **generated by formula** (`melakarta.rs:1-15`), ragas as grammars (arohana/avarohana, vakra paths, phrase validation, `raga.rs:1-14`), gamakas as smooth C¹ pitch curves applied as exact per-channel MPE bends (`gamaka.rs`, `render.rs:71-73,163-174`), all 35 suladi talas + chapus by construction (`tala.rs:1-10`), mora/korvai arithmetic that provably lands on sam + konnakol sollus (`korvai.rs:1-12`), 10-raga reference library. Western: modes/chords/roman numerals built by stacking (melakartas harmonize too, `chord.rs:1-10`), minimal-motion voice leading, Euclidean rhythms + swing. Scala `.scl` in/out. Seeded raga-grammar walks and Markov generation. Three demo pieces with real structure. Deduction: janya library is a starter set (10), not a census. |
| 11 | Notation/engraving interchange | **4** | SMF export with MTS tuning + MPE bends (`score.rs:285-336`), Scala, tags. **No MusicXML, LilyPond, ABC, MEI, tab, or any engraved representation** — the score cannot round-trip into MuseScore/Sibelius/Dorico or web engravers (abcjs/VexFlow). |
| 12 | Plugin ABI | **7** | Own single-`Plugin` API exported to **CLAP and VST3** with host *and* plugin sides, no SDK dependency, sample-accurate parameter automation via block splitting (`lib.rs:1-25`), validated in CI against official C/C++ hosts, clap-validator and Steinberg's VST3 validator on a 4-OS matrix (`audio-plugins.yml`). Trademark notice present (`THIRD_PARTY.md`). Deductions: no AUv3 (blocks iOS App Store audio-unit distribution), no LV2 (Linux pro-audio standard), no AAX, **no GUI abstraction at all** (no IPlugView/CLAP-gui matches in the crate). |
| 13 | Fuzz & oracle testing | **8** | 8 libFuzzer targets over all decoders + generic `decode` (`fuzz/Cargo.toml`), a shape oracle (`fuzz_targets/common`) that is *itself tested against every known-good fixture* so a harness regression can't hide (`fuzz/tests/oracle_on_fixtures.rs:1-30`), a minimized hostile corpus with per-file "what it did before" provenance that runs on stable `cargo test` (`tests/hostile/README.md`), mutation-fuzzing history (2026-10-20) with concrete findings (runaway MP3 global gain, AAC fill-element underflow, FLAC GB allocations), SNR grading vs ffmpeg. Deductions: no fuzzing of DSP/stream/mixer/plugin paths, fuzz runs only manually for 300 s/target, no OSS-Fuzz. |
| 14 | CI matrix for audio | **6** | `audio-backends.yml` is unusually real: Linux with **running** JACK (dummy), PulseAudio (null sink) and PipeWire (WirePlumber + null sink) servers gated by `VIEWW_EXPECT_BACKENDS`; ALSA every-format + rate-matching both ways + MIDI through kernel `snd-aloop`/`snd-virmidi` **in a virtme-ng VM** (the Azure kernel has no sound); Windows WASAPI shared+exclusive through VB-CABLE; macOS BlackHole + CoreMIDI; `--cfg vieww_check_all` cross-lints every backend on Linux; web shim tested under Node. Loopback requires `VIEWW_REQUIRE_LOOPBACK=1` so a silently-lost device fails the job. **But every workflow is `workflow_dispatch`-only** (owner decision 2026-10-15), so nothing gates PRs; device-farm.yml has zero audio; no audio receipts from the self-hosted machines yet. |
| 15 | Docs & examples | **5** | The rustdoc is exceptional (honest crate-level manifest, failure-policy contract, per-module essays), CHANGELOG is precise, `docs/guide/` has **no audio chapter** (getting-started/state/widgets/theming/a11y/testing only), only 3 workspace feature examples (64-waveform, 65-audio-reactive, 90-midi-dsp-osc), 3 vieww-music examples, plugin examples; no "make your app beep on Android/iOS" app-facing guide, no music-player example, no device-selection UI example. |

**Overall:** the engine core is top-tier — a from-scratch, single-dependency, allocation-proven, fuzz-hardened audio engine with real backend verification that most Rust audio stacks (cpal/rodio/kira) and most framework ecosystems (Flutter, RN) simply do not have at the DSP/IO layer. The product surface is not there yet: **no framework wiring (nothing registers an `AudioPlayer`), no mobile lifecycle (focus/interruption/background), no large-media streaming, no Opus, no device-farm verification.** As it stands, vieww's audio story is a world-class engine bolted to nothing, while Flutter ships a weaker engine wired to everything.

**The 5 things that matter most:**
1. **AU-01** — wire the engine to the framework: a real cross-platform `AudioPlayer` implementation over `backend::open_output` + `Mixer`/`Stream`, registered by the platform crates. Until this exists, no vieww app can make noise on 5 of 6 platforms.
2. **AU-02/03/04** — the mobile lifecycle trio (audio focus, AVAudioSession, background modes). Every audio app shipped to stores needs these; every competitor framework has them.
3. **AU-05** — streaming playback architecture for compressed formats (incremental decoders over a `Read` source). Without it, "podcast/audiobook/music app" is not buildable.
4. **AU-06/07** — Opus decode + an explicit codec/patent posture document. Opus is the dominant voice/social format; its absence is the most visible codec gap.
5. **AU-14/15** — run the audio CI automatically and put audio on the device farm. The backend verification story is the stack's crown jewel and it currently only runs when a human clicks.

## 2. What exists today (verified inventory)

**Engine seam** (`src/lib.rs`): `Tone`/`Envelope`/`Mixer`/`Samples` (mono/stereo only, `lib.rs:171-184`), `AudioPlayer` service trait — `play(&Sound) -> PlaybackHandle`, `stop` only (`player.rs:106-126`), `Sound::{Tone,Buffer}` closed enum (`player.rs:57-65`), `NoAudio` + `RecordingPlayer` test double. `vieww` facade re-exports the seam (`vieww/src/lib.rs:100`).

**Backends** (`src/backend/`): trait `Backend` with `devices`/`open_output_on`/`default_device_id`/`open_input` (`mod.rs:445-506`); preference order Linux: PipeWire→JACK→Pulse→ALSA, macOS: CoreAudio→JACK, Windows: WASAPI→JACK, iOS: CoreAudio, Android: AAudio, web: WebAudio (`mod.rs:514-541`); `wasapi-exclusive` opt-in by name (`mod.rs:546-553`). All system libraries dlopen'd (`dl.rs`) — one dependency total (`Cargo.toml`: only `vieww-foundation`). `isolate_render`/`isolate_capture` wrap every callback: FTZ guard, one-time RT promotion, `catch_unwind`, render timing, NaN scrub, poisoned-slot silence (`mod.rs:262-349`). `Counters/StreamStats`: callbacks, frames, xruns, ended, priority class, render mean/max, peak load, late, latency (`mod.rs:129-242`).

**Resilience** (`resilient.rs`): supervisor thread at 100 ms poll / 1 s default-device poll; `Target::Default` and `Target::Named{fallback_after}`; closure lives in a slot, handed between device streams so effect tails survive a reopen; exponential backoff to 1 s; `reopens/failed_opens/panicked/following_default` metrics; tested against a fake unplug/replug backend (`resilient.rs:389-484`).

**Codecs** (`src/codec/`, `src/wav.rs`): table in `codec/mod.rs:3-20`; container sniffing (`mod.rs:194-215`); `Decoded::validated()` — rate range 1–768 kHz, channels 1–64, ±8.0 runaway bound, 1024-frame concealment (`mod.rs:48-96`); MP3 full Layer III from ISO 11172-3/13818-3 with gapless LAME handling (`mp3.rs:1-14`); AAC-LC from ISO 14496-3 with PNS/TNS/intensity and explicit refusal list (`aac.rs:7-16`); FLAC all subframes + MD5; Vorbis floor 1 + residues 0/1/2 + granule trimming; MP4 atom parser with edit lists; tags read ID3v2.2-2.4/ID3v1, Vorbis comments (incl. Opus files — audio decode for Opus does **not** exist), RIFF INFO, AIFF text, MP4 atoms; tag *write* for WAV/FLAC/AIFF (`tags.rs`).

**Engine** (`src/`): `graph.rs` (WebAudio-model node graph, 128-frame quantum, cycle-legal-through-delay, topological order at connect time, render allocates nothing), `mixer.rs` (tracks/buses/sends/sidechains/VCAs/solo-safe, PDC from per-effect `latency()`, insert-fault reset + counting, `mixer.rs:215-216, 638-643`), `parallel.rs` (multi-core rendering; mirrors caller FTZ mode so offline renders stay bit-identical across thread counts), `stream.rs` (atomic-only SPSC ring + `Stream` render thread), `loader.rs` (background decode pool, shared inflight loads, weak cache with sweep, cancel/progress, WAV `DiskStream` with seek generation/ack protocol), `resample.rs` (sinc, quality tiers), `channels.rs` (up to 7.1 remix rules), `simd.rs`, `dsp.rs` (Biquad/RBJ family, chains, `OnsetDetector`, `estimate_tempo`), `fft.rs`, `analysis.rs`, `meter.rs` (EBU R128), `quality.rs` (measurement suite incl. `latency`), `rt.rs`, `denormal.rs`, `preset.rs` (params/presets/banks/A-B/undo/morph), `modulation.rs`, `automation.rs`, `spatial.rs`, `sequencer.rs`, `transport.rs`, `midi.rs`, `midi_io.rs`, `midi_coremidi.rs`, `midi_winmm.rs`, `mts.rs`, `device.rs` (ALSA ioctls, `PcmFormat` negotiate/encode/decode with TPDF dither, `AlsaPlayer`), `wav.rs` (read/write incl. EXTENSIBLE, Dither), `preset.rs`.

**Tests** (in-crate): `realtime_no_alloc.rs` (counting allocator, no harness), `live_backends.rs` (soak under all-cores-spinning with scheduling-class/load/xrun assertions, needs servers), `device_loopback.rs` (997/1499 Hz prime-frequency dual-tone loopback, per-10 ms least-squares fit referenced to the capture clock — catches dropped/repeated frames by phase drift, `device_loopback.rs:23-31`), `hostile_inputs.rs` + `tests/hostile/` (10 minimized regressions), `nan_recovery.rs`, `audit_probes.rs`, `tuning.rs`, `tags.rs`, `fuzz/tests/oracle_on_fixtures.rs`.

**vieww-music** (`src/`): `pitch.rs` (exact ratios/cents), `scala.rs` (scl in/out), `carnatic/` (swara, melakarta, raga + library, gamaka, tala, korvai), `western/` (scale, chord, rhythm, voice_leading), `rock.rs`, `score.rs` (Score/Part/Event/Instrument, `to_clips` → sequencer, `to_smf` with MTS + MPE), `generate.rs` (seeded raga walks, Markov), `pieces.rs` (Carnatic Adi-tala piece, just-intoned chorale, 12-bar rock), `render.rs` (offline deterministic render: per-part `Poly` with MPE gamaka bends at 1.5 kHz control rate → DAW mixer with reverb bus → 20 kHz Butterworth → EBU R128 normalization → lookahead true-peak limiter with latency compensation → exact 4× oversampled true-peak trim; WAV+FLAC export with tags; `render.rs:1-24, 313-371`), examples `carnatic.rs`/`rock.rs`/`chorale.rs`.

**vieww-audio-plugin** (`src/`): `Plugin`/`PluginInfo`/`Driver` core; `clap/` (sys + wrapper + host; params/state/audio-ports/note-ports/latency/tail extensions), `vst3/` (sys + wrapper + host; factory/component/audio-processor/edit-controller, split-controller sync, host-side IHostApplication/IBStream/param queues/event lists), `export_clap!`/`export_vst3!`, examples `vieww_gain.rs`/`vieww_synth.rs`, C/C++ host + layout tools, `check-plugins.sh --full` (unit tests, own hosts, official-header hosts, clap-validator, Steinberg validator, bundling).

**CI**: §0 history + §4/AU-15 details. Self-hosted `hardware.yml` audio step: device list, live backends (10 s), default-output soak (default 600 s, `VIEWW_HW_AUDIO_GAIN=0` silent-but-streamed), optional wired loopback + MIDI cable (`ci/hardware/linux.sh:254-276`, `windows.ps1:325+`).

## 3. Branch delta: main → feature/main_fixes (audio touches?)

`git diff main origin/feature/main_fixes` over all audio/music/plugin/workflow paths: **empty**. The feature branch (`60adf6f`, "fmt: one long signature in mixed.rs") is GPU/scene work (35 files, +8,915/−1,035 — shaders, parity test harness, etc.) with **zero audio-stack changes**. Every gap and fix below applies equally to both branches; nothing is "[FIXED ON feature/main_fixes]".

Workflow restoration history (from `git log`, §0) shows the audio CI was authored with the loopback/hardware infrastructure in one arc (`c858840`), parked in `workflows-pending` for want of a workflow-scoped token (`63ce0fa`, `8fae3b4`), placed into `.github/workflows/` (`82b89a8`), then hardened (`3d08785` VM for ALSA because hosted runners' kernel lacks sound modules; `c7e08db` is an empty "Update audio-ci.yml" commit with no content). Root `TODO.md`'s "workflows-pending restore" item is therefore resolved for audio.

## 4. GAP CATALOG

Severity: P0 = blocks any real-world audio app release; P1 = blocks a class of apps or a platform; P2 = quality/verification debt; P3 = polish.

---

### AU-01 | P0 | The engine is an island: no platform registers an `AudioPlayer`, and the only playing implementation is a Linux-only blocking writer

**Evidence.** The service seam exists (`player.rs:106-126`) and the docs promise "a platform crate (`vieww-platform-winit` on desktop, the JNI bridge on Android) provides the implementation and registers it" (`lib.rs:100-102`) — but a repo-wide search finds **no** `impl AudioPlayer` outside vieww-audio, and `vieww-platform-winit` contains no `vieww_audio` usage at all. The only sound-producing implementation is `AlsaPlayer` (`device.rs:1000-1035`): opens card 0 device 0 at fixed 48 kHz, renders the whole `Sound` to an i16 buffer (`render_sound`), and streams it in 4,096-frame blocking writes on a detached thread — no volume, no backend selection, no PipeWire/Pulse (it bypasses the server and grabs `hw:0,0`), no Windows/macOS/Android/iOS/web equivalent. Everything else in the crate — `backend::open_output`, `Mixer`, `graph`, `Stream`, `ResilientStream`, `DiskStream`, sequencer — is reachable only by hand-writing a render closure.

**Blocks.** Any vieww app that wants sound on Windows, macOS, iOS, Android or web; the seam returns `None`/`NoAudio` there. Even on Linux, a "click" allocates the full tone buffer and races the sound server for the raw device.

**Leaders.** Flutter apps call `audioplayers`/`just_audio` and get sound on all platforms in one line (https://pub.dev/packages/just_audio); React Native has `expo-av`/`react-native-track-player` (https://github.com/react-native-kit/react-native-track-player); Unity/Godot embed a mixer wired to their scene APIs (https://docs.godotengine.org/en/stable/tutorials/audio/audio_buses.html). The pattern everywhere: *engine internals are an implementation detail; the framework hands apps a player*.

**Fix.** Add `src/player_engine.rs` to vieww-audio: an `EnginePlayer` implementing `AudioPlayer` (and the extended seam of AU-11) over `backend::open_output` + a `Mixer`-per-voice or ring-fed `Stream`, with `ResilientStream` wrapping, per-sound gain, and completion callbacks via a lock-free event queue drained by `poll()`. Then register it from each platform crate (`vieww-platform-winit` on desktop; the Android JNI bridge; the web shim) so `services.get::<dyn AudioPlayer>()` just works:

```rust
// src/player_engine.rs (sketch)
pub struct EnginePlayer { cmd: mpsc::Sender<Cmd>, }
enum Cmd { Play { id: u64, sound: Sound, gain: f32 }, Stop(u64), SetGain(u64, f32), Tick(Vec<Done>) }
// audio thread: backend::open_output(cfg, || render closure pulling from
// a Mixer bus per voice; ResilientStream::open(backend, Target::Default, …))
impl AudioPlayer for EnginePlayer { fn play(&self, s: &Sound) -> … { self.cmd.send(…) } }
```
`AlsaPlayer` stays as the zero-server fallback for embedded Linux.

**Tests.** GitHub CI: unit test with the `resilient.rs` Fake backend (play→stop→completion events, gain ramp). Self-hosted Linux: `live_backends` + a new `player_smoke` (play 20 tones, assert StreamStats.callbacks>0, zero late renders). Self-hosted Windows: same on WASAPI shared+exclusive. Device farm: UI-driven "tap → tone plays" with the loopback analyzer from `device_loopback.rs` capturing the result (see AU-14).

**Status: [OPEN]** (both branches).

---

### AU-02 | P0 | No Android audio focus: the app that talks over phone calls

**Evidence.** `aaudio.rs` sets only sharing mode + performance mode (`aaudio.rs:225-226`); there is no `AudioManager`, `AudioFocusRequest`, or focus listener anywhere in the repo (grep for `focus` across `vieww_base` returns only UI-focus files).

**Blocks.** Any Android app shipped to Play: starting playback during a call or over another media app is a policy violation and an instant uninstall; transient loss (notification) should duck, permanent loss should pause. AAudio itself does not do focus — the platform expects the app to (https://developer.android.com/media/optimize/audio-focus).

**Leaders.** Oboe's FAQ explicitly delegates focus to the app layer, and Google's own samples pair Oboe with `AudioFocusRequest`; Flutter's `audio_session` package wraps the whole dance (request on play, duck on transient-can-duck, pause on loss, resume on gain) for both Android and iOS (https://pub.dev/packages/audio_session). React Native Track Player does it internally (https://react-native-kit.github.io/react-native-track-player/docs/).

**Fix.** Add a `focus` module to vieww-audio (`src/focus.rs`) defining an OS-neutral contract — `FocusRequest { content: Content, gain: GainType, duck: bool }`, `FocusEvent::{Lost, LostTransient, LostCanDuck, Gained}` — and wire it in the Android JNI bridge: `AudioManager.requestAudioFocus(AudioFocusRequest.Builder(...).setAudioAttributes(usage=USAGE_GAME/MEDIA).setWillPauseWhenDucked(...))` mapped onto the player's transport. The engine side is pure state (duck gain is a `fader_db` on the master bus); the JNI side is ~100 lines.

**Tests.** Device farm (only place focus can be real): an instrumented test that plays, then fires a fake competitor focus request via `adb shell media dispatch` (`cmd media_session dispatch`), asserting duck→pause→resume transitions in the player's event log. GitHub CI: state-machine unit tests of the focus automaton.

**Status: [OPEN]**

---

### AU-03 | P0 | No AVAudioSession lifecycle on iOS: interruptions kill the stream and App Store background audio is impossible

**Evidence.** `coreaudio.rs:7-9` documents that capture "needs the app's `AVAudioSession` in a record category and the user's microphone permission" — but nothing sets one; there is no `setCategory`, no `AVAudioSession.interruptionNotification` observer, no `routeChangeNotification`, no `secondaryAudioShouldBeSilencedHint`, no `activate()` error handling (the classic "category not set → RemoteIO refuses on device, works in simulator" trap). On interruption the audio thread just ends; `ResilientStream` will reopen — but with the session still deactivated after a phone call, the reopen loop burns CPU retrying forever (backoff caps at 1 s).

**Blocks.** Any iOS audio app: answering a call mid-playback leaves silence + a busy retry loop; recording apps crash the first session; Apple review rejects background audio without `UIBackgroundModes` (AU-04) and correct category (https://developer.apple.com/documentation/avfaudio/avaudiosession — "Configuring your app's audio behavior").

**Leaders.** Apple's own AVAudioEngine samples set `.playback` + `mixWithOthers`, observe interruptions (`.began` → stop engine/`.ended` → `setActive(true)` with `options: .notifyOthersOnDeactivation`), and route changes (old device unavailable → rebuild). Flutter's `audio_session` does exactly this cross-platform; JUCE's iOS audio device does the session dance internally (`AudioIODevice` suspend/resume on interruption — https://juce.com/learn/tutorials/processors-interacting/audio-i-o/).

**Fix.** Add `src/session.rs` (neutral `AudioSession` API: `set_mode(Speech|Media|Game|Record)`, `handle_interruption`, events) + an iOS implementation in ObjC/C bridging `AVAudioSession` notifications into `StreamStats::ended` + a `ResilientStream` hint ("don't retry until session active"). Wire the JNI/Android side to the same trait via AU-02.

**Tests.** Device farm iOS (LambdaTest iPhone 13 Pro already in infra): play tone → simulate interruption via `XCUITest` + `SimulateAudioInterruption` (or simply place a real CallKit-less interruption with `AVAudioSession` test hook); assert pause → resume with ≤ 300 ms gap and no retry-storm (assert `failed_opens` bounded). GitHub CI: macOS unit test of the neutral state machine.

**Status: [OPEN]**

---

### AU-04 | P0 | No background playback story: no foreground service/MediaSession (Android), no `UIBackgroundModes` (iOS), no wake locks

**Evidence.** No `foregroundService`, `MediaSessionService`, `AudioService`, `MPNowPlayingInfoCenter`, or `UIBackgroundModes` anywhere in the repo (grep, §AU-02). `ci/mobile/device-suite.sh` runs UI suites; audio never runs while backgrounded.

**Blocks.** Music/podcast/audiobook/fitness apps: the moment the screen locks, Android freezes the process and audio stops; iOS suspends the app. This is the single most-requested feature of every mobile audio library.

**Leaders.** Flutter `audio_service` + `just_audio_background` (https://pub.dev/packages/audio_service, https://pub.dev/packages/just_audio_background) provide a MediaSession, notification controls, and background execution; RN Track Player ships a foreground service; Godot/Unity export templates contain the service declaration. Android requires a foreground service with `mediaPlayback` type (API 34+), a `MediaSession` for lock-screen controls, and `AudioAttributes` usage for correct routing (https://developer.android.com/media/progressive-media-playback/playing).

**Fix.** Platform-packaging work, mostly in the Android/iOS export templates + a thin `MediaSessionHandle` in the neutral player API (AU-11): Android — declare the service, implement `MediaSession` with play/pause/seek callbacks routed into the player's command channel; iOS — Info.plist `UIBackgroundModes: audio` + `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter` on the platform crate. The engine needs nothing (it is a library in-process).

**Tests.** Device farm: play 5-minute tone → `adb shell input keyevent KEYCODE_POWER` → assert `StreamStats.callbacks` still advancing after 60 s; lock-screen controls assert via `adb shell dumpsys media_session`. Self-hosted machines: not applicable. GitHub CI: template-lint that the export manifest/plist contains the required keys.

**Status: [OPEN]**

---

### AU-05 | P0 | Large-media playback: full-file decode in memory; only WAV disk-streams; no network source

**Evidence.** Every decoder takes the complete byte slice and returns a complete `Vec<f32>` (`codec/mod.rs:222-241`); `Loader` reads the whole file into a `Vec<u8>` with progress (`loader.rs:246-247`); `DiskStream::open` parses a WAV header and streams frames through the lock-free ring on a reader thread, with seek-generation/ack, looping and underrun accounting (`loader.rs:1-14, 320-…`). No `Read`-based incremental decode API exists; no HTTP/URL source; `Sound::Buffer(Samples)` is the only media type in the seam.

**Blocks.** The canonical scenario: a 3-hour 64 kbps AAC audiobook (~90 MB compressed) decodes to ~3.8 GB of f32 — OOM-kill on every phone; even a 10-minute FLAC set is ~200 MB per track. A podcast app, an audiobook app, a music-library app are all unbuildable. (WAV streaming exists but nobody distributes WAV.)

**Leaders.** `just_audio` streams over HTTP with a `LockCachingAudioSource`, arbitrary seeking, and gapless concatenation (`ConcatenatingAudioSource`); RN Track Player buffers network streams; symphonia-based rodio decodes incrementally from any `Read + Seek` (https://github.com/RustAudio/rodio, https://github.com/pdeljanov/Symphonia); ExoPlayer/Media3 (the platform gold standard) has a full loader/cache pipeline (https://developer.android.com/media/media3).

**Fix.** Two-stage:
1. **Incremental decoder API** — refactor each codec behind `PullDecoder { fn next_frames(&mut self, out: &mut [f32]) -> Result<usize> }` fed by `&mut dyn Read + Seek`. The container parsers (Ogg pages, ADTS frames, MP4 atom offsets, FLAC blocks) are already streaming-shaped internally; the frame decoders (`mp3::decode_frame`-level functions exist per-header) can be exposed one step at a time — FLAC and MP3 first (frame-synced formats), then Vorbis/AAC.
2. **`MediaStreamSource`** — a `Source` enum (`File(path)`, `Url(...)`, `Bytes`) behind the extended player (AU-11), with a reader thread feeding the existing `ring` exactly as `DiskStream` does, generalized from WAV to `PullDecoder`, plus a prebuffer policy (e.g. 3 s) and seek = `seek_gen` bump (protocol already proven in `DiskStream`).

```rust
// codec/mod.rs (sketch)
pub trait PullDecoder {
    fn stream_info(&mut self) -> Result<DecodedInfo, CodecError>;   // rate/channels only
    fn next_frames(&mut self, out: &mut [f32]) -> Result<usize, CodecError>; // interleaved
    fn seek(&mut self, frame: u64) -> Result<(), CodecError>;
}
```

**Tests.** GitHub CI: decode a 30-minute fixture (generate with ffmpeg in `make.sh`) via both paths, assert sample-equality with the whole-file decode, assert peak RSS < 100 MB (`/usr/bin/time -v` wrapper or a `jemalloc`-free RSS poll). Self-hosted Linux: 24 h soak of a 6 h WAV+FLAC loop through `DiskStream`+pull-FLAC with xrun assertions. Device farm: play a 3 h AAC/Opus file, assert process RSS stays under a budget (Android `dumpsys meminfo`) and playback position advances.

**Status: [OPEN]** (gapless for *files* already exists: LAME tag trimming in MP3, MP4 edit-list priming removal — `mp3.rs:12-14`, `aac.rs:3-5`; the gap is chaining playlists gaplessly at the player level, which folds into AU-11.)

---

### AU-06 | P1 | Codec coverage: no Opus (and tags-only support for .opus), no HE-AAC SBR/PS, no ALAC

**Evidence.** `sniff()` routes `.opus` (OggS) to `vorbis::decode`, which rejects it — while `tags.rs` happily reads Opus comments (fixture `tests/fixtures/tags.opus` is used only in `tests/tags.rs`). AAC refuses SBR/PS object types by name (`aac.rs:14-16`); MP4/ALAC (`alac` stsd) is not decoded. `codec/mod.rs:3-20` is the authoritative table.

**Blocks.** WhatsApp/Telegram voice notes, Discord, most YouTube audio, Zoom/Meet recordings, Spotify/Netflix delivery — all Opus or HE-AAC. An in-app voice-message feature or a video soundtrack (vieww-video is out of scope here) cannot play the formats users actually produce.

**Leaders.** Android `MediaCodec` and Apple `AudioConverter`/`AVAudioConverter` decode Opus natively (iOS 11+/macOS 10.13+); Flutter's `just_audio` reaches them via platform codecs with pure-Dart fallbacks; symphonia (pure Rust) *also* lacks production Opus and — notably — ships its AAC with an explicit patent warning, and rodio users pull platform decoders for the rest (https://github.com/pdeljanov/Symphonia#codecs--formats). **Opportunity:** a first-class pure-Rust Opus decoder with the same fuzz+oracle discipline as the existing six would be genuinely rare in the ecosystem.

**Fix.** Implement Opus decode (RFC 6716: SILK + CELT + mode coupling — a big but well-specified job; the existing `fft::Imdct`, `rng` (for the decoder's stable RNG usage), bits reader and table-generator infra all transfer). Interim: a `PlatformCodec` escape hatch on Android/iOS that shells to MediaCodec/CoreAudio for anything pure-Rust refuses. Add HE-AAC SBR (spectral band replication) as a follow-on — the core decoder already refuses it *by name*, i.e. the boundary is known.

**Tests.** GitHub CI: Opus fixtures from `libopus` reference encodes with the same SNR-oracle grading as the rest (`make.sh` extension); fuzz target `decode_opus`. Device farm: play real-world `.opus`/`.m4a` HE-AAC voice notes. Self-hosted: none special.

**Status: [OPEN]**

---

### AU-07 | P1 | AAC patent posture is undocumented: table provenance exists, patent-pool exposure does not

**Evidence.** `tools/THIRD_PARTY.md` credits minimp3 (CC0), vo-aacenc tables (Apache-2.0), stb_vorbis (PD/MIT) — clean. But there is no statement about **patents**: MP3's pool expired (2017 US; earlier elsewhere) and that is nowhere written down; AAC-LC decoding is still covered by the Via-LA AAC pool in some jurisdictions, and shipping an AAC decoder in a product has real legal-review cost. `codec/mod.rs` and the README are silent.

**Blocks.** Legal review for any commercial product; enterprise adoption; distribution in app stores (Apple/Google indemnify platform codecs, not in-app decoders).

**Leaders.** Symphonia's README carries an explicit patent caveat and points users at their own counsel; ffmpeg's `README` documents AAC's status; commercial stacks (JUCE) simply link platform codecs for AAC and let the OS vendor carry the license. Chrome/Firefox ship AAC only where the platform provides it.

**Fix.** Add `vieww_base/crates/vieww-audio/PATENTS.md`: (1) MP3 — Layer III patents expired (last US patents 2017), decoding unencumbered, cite the pool termination; (2) AAC — decoding is patent-pooled in *some* jurisdictions (Via Licensing International AAC pool), vieww ships source not binaries, downstream distributors must evaluate; offer a build feature `vieww-audio/aac` that is default-on with a documented opt-out for cautious distributors (and keep `CodecError::Unsupported` messaging consistent); (3) FLAC/Vorbis/Opus — Xiph/MPEG-era royalty-free commitments with links. One afternoon of writing; unblocks every downstream legal review.

**Tests.** None (documentation). CI: a docs-lint that `PATENTS.md` exists and is referenced from the crate README.

**Status: [OPEN]**

---

### AU-08 | P1 | AAudio: always shared mode, no usage/contentType/privacy attributes, no latency query, no underrun query

**Evidence.** `aaudio.rs:225-226`: `(set_sharing_mode)(b, AAUDIO_SHARING_MODE_SHARED)`, `(set_performance_mode)(b, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY)` and nothing else on the builder; `get_frames_per_burst` is read but `AAudioStream_setBufferSizeInFrames` is never called (no runtime latency tuning); `AAudioStream_getTimestamp`/`AAudioStream_getXRunCount` never called (no latency or underrun reporting — the only backend of nine that doesn't report latency, despite the CHANGELOG's "every backend reports latency"); the error callback collapses every error into `ended` (`aaudio.rs:157-161`) with no distinction for `AAUDIO_ERROR_DISCONNECTED` vs others.

**Blocks.** Pro-audio/low-latency Android apps: exclusive mode (or at least small bursts) is the difference between 8 ms and 40 ms; apps can't declare `USAGE_GAME`/`USAGE_VOICE`/`USAGE_NOTIFICATION` so Android routes and prioritizes them wrongly (a game's sounds can be treated as media, breaking focus expectations of AU-02); navigation/assistant use-cases need `CONTENT_TYPE_SPEECH`.

**Leaders.** Oboe defaults to exclusive mode with automatic fallback to shared, exposes usage/contentType/privacySensitive on the builder, tunes buffer size against `getFramesPerBurst`, and reports both `getTimestamp` latency and underrun counts (`AAudioStream_getXRunCount`) — https://developer.android.com/ndk/guides/audio/aaudio/aaudio, https://github.com/google/oboe.

**Fix.** Extend `StreamConfig` (or add `AndroidConfig`) with `usage`, `content_type`, `privacy_sensitive`, `exclusive: bool`; in `open()`: try `AAUDIO_SHARING_MODE_EXCLUSIVE` when asked and fall back on refusal (error text already propagates via `convert_result_to_text`); after open, `setBufferSizeInFrames(burst * k)` with k from config; add `AAudioStream_getTimestamp` → `Counters::set_latency_frames`; call `AAudioStream_getXRunCount` periodically from a stats sampler and fold into `xruns`. ~150 lines in one file.

**Tests.** Device farm (exclusive mode only works on real devices; emulators force shared): open exclusive, assert granted config `sharing` where introspectable; latency < 20 ms on the Redmi Note 7 Pro; play 60 s, assert zero xruns at k=2 bursts. GitHub CI: `vieww_check_all` lint keeps it compiling.

**Status: [OPEN]**

---

### AU-09 | P1 | Route changes and Bluetooth: 1-second polling instead of OS events; no Bluetooth-aware behavior at all

**Evidence.** `resilient.rs:34-37`: `POLL = 100 ms`, `DEFAULT_POLL = 1 s` — default-device changes are found by re-asking `Backend::default_device_id()`. No `IMMNotificationClient` (Windows), no `AudioObjectAddPropertyListener` for `kAudioHardwarePropertyDefaultOutputDevice` (macOS — `coreaudio.rs` reads the property, never subscribes), no `AudioDeviceCallback` (Android), no PipeWire registry events. Bluetooth specifics (A2DP connect/disconnect, the HFP profile switch that changes the sample rate to 8/16 kHz and drops to mono, the 200+ ms buffers) are nowhere handled or even documented; `ResilientStream` will happily reopen on a Bluetooth device's 300 ms latency and then under-count `late` renders because the block is huge.

**Blocks.** The canonical "music app dies on Bluetooth disconnect" — vieww survives it (stream ends → reopen), but: plugging headphones in takes up to 1 s to follow the default (audible gap when the OS itself doesn't migrate); on Bluetooth *connect* mid-playback on Windows the default endpoint changes under WASAPI and the old stream errors with a gap; no app-visible route-change event means UI can't show "playing on <device>".

**Leaders.** Apple: `AVAudioSession.routeChangeNotification` with reasons (old device unavailable, new device available, category change) — apps update UI and the engine follows (https://developer.apple.com/documentation/avfaudio/avaudiosession); Windows: `IMMNotificationClient::OnDefaultDeviceChanged` is the documented pattern (https://learn.microsoft.com/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immnotificationclient); Android: `AudioManager.registerAudioDeviceCallback` + Oboe's own reconnection recipe; JUCE wires all of these into `AudioDeviceManager` change broadcasts.

**Fix.** Add `Backend::watch(Box<dyn Fn(RouteEvent)>)` — default `None`; implement with `IMMNotificationClient` (WASAPI), `AudioObjectAddPropertyListener` (CoreAudio), `AudioDeviceCallback` via JNI (Android), PipeWire registry `GlobalParam`/node removed events. `ResilientStream` consumes events instead of polling (keep the poll as a fallback — it's a good safety net), and emits `RouteEvent::{DefaultChanged{to}, DeviceLost{id}, DeviceBack{id}}` on a channel for app UI. Document HFP: on Android, check `AudioManager.getProperty(PROPERTY_OUTPUT_SAMPLE_RATE)` before open, and re-run the rate negotiation that already exists in `alsa.rs`/`device.rs` for the new device class.

**Tests.** Self-hosted Linux: PipeWire null-sink swap while streaming (scripted `pw-cli` set-default) — assert reopen ≤ 100 ms and `reopens()==1`. Self-hosted Windows: VB-CABLE default flip; plus (if the owner has a BT dongle) a manual-infra Bluetooth pair/unpair soak. Device farm: Bluetooth headphones on the farm's Android device if available — pair/unpair 20× during playback, assert audio resumes and no retry-storm. GitHub CI: unit test of the event→resilient handoff with the fake backend.

**Status: [OPEN]**

---

### AU-10 | P1 | WASAPI shared mode: IAudioClient v1 with AUTOCONVERTPCM; no IAudioClient2/3 low-latency shared mode

**Evidence.** `wasapi.rs:100-102`: `STREAMFLAGS_EVENTCALLBACK | STREAMFLAGS_SRC_DEFAULT_QUALITY | STREAMFLAGS_AUTOCONVERTPCM` on `IAudioClient::Initialize` with a 3-block shared buffer (`wasapi.rs:732-733`); exclusive mode exists as the low-latency escape hatch (`wasapi.rs:15-23`, one-period buffer at `wasapi.rs:676-677`) but grabs the endpoint (refuses when in use, `E_DEVICE_IN_USE` at `wasapi.rs:97-99`) — useless for a normal app that must coexist with the mixer.

**Blocks.** Low-latency apps (games, soft synths, VoIP meters) on Windows choose between "10 ms+ engine latency" and "no other app can play"; the modern middle path — IAudioClient3 with the device's min period in shared mode (~3–6 ms) — is missing.

**Leaders.** Windows docs (https://learn.microsoft.com/windows/win32/coreaudio/low-latency-audio) prescribe `IAudioClient2/3` with `GetMinPeriod/GetDefaultPeriod` for low-latency shared; cpal's WASAPI backend exposes exclusive mode; PortAudio/Oboe-for-Windows follow the same guidance; games use XAudio2 (also not present, but IAudioClient3 is the WASAPI-native answer).

**Fix.** In `session()` (channels clamp at `wasapi.rs:644`): query `IAudioClient2::IsSupported`/`IAudioClient3::GetSharedModeEnginePeriod`, and when `config.block` ≤ default period and the OS supports it, initialize shared with the minimal period and no autoconvert (match rate exactly, else resample with the existing `Resampler` exactly as `alsa.rs` does). Keep v1 as fallback. ~120 lines; the vtable machinery is already transcribed.

**Tests.** Self-hosted Windows (real endpoint): open at min period, assert `StreamStats.latency` < 8 ms and zero late renders during the 600 s soak; loopback analyzer (`device_loopback.rs`) proves continuity. GitHub CI Windows job: build + refusal-cleanliness (runners have VB-CABLE already, so the full loopback path can assert too).

**Status: [OPEN]**

---

### AU-11 | P1 | The `AudioPlayer` seam is fire-and-forget: no volume, pause, position, seek, completion events, or media sources — you cannot build a player UI on it

**Evidence.** `player.rs:106-126`: the trait is `play` + `stop` and the docs defend this: "the trait's contract is fire-and-forget, and a caller that needs to know a sound failed is a caller with a sync problem this shape cannot express" (`player.rs:112-115`); `Sound` is closed (`Tone | Buffer`) with no file/URL/stream variant (`player.rs:53-65`); no position, no rate, no looping, no fade, no ducking, no playlist, no gapless chain.

**Blocks.** Every interactive use beyond a click: a media player, a game with per-channel volume/fade, a UI that wants "sound finished → animation continues". The docs are right that a UI-click seam should be small — but the framework offers *only* that seam; the rich engine underneath (transport with sample-accurate scheduling! sequencer! launcher!) has no framework-facing facade.

**Leaders.** `just_audio`'s `AudioPlayer` (setUrl, play/pause/seek, positionStream, volume/speed, gapless playlists, audio processing pipeline) is the benchmark shape (https://pub.dev/packages/just_audio); RN Track Player adds queue management and events; Godot's `AudioStreamPlayer` exposes volume_db, pitch_scale, finished signal, bus routing; kira gives tweened fades and clock-synced scheduling (https://github.com/tesselode/kira).

**Fix.** Keep `AudioPlayer` as the click seam; add a second, richer facade in vieww-audio (`src/transport_player.rs`): `MediaPlayer` with `load(Source)`, `play/pause/seek(Position)`, `set_volume/set_rate/set_pitch`, `position()/duration()`, `on_event(PlaybackEvent::{Ended, Underflow{count}, Route{..}, Focus{..}})` — implemented over the AU-05 `MediaStreamSource`, the engine's `Mixer` (volume/fades are `fader_db`), `Resampler` (rate), `ResilientStream` (events). The transport/sequencer already solves sample-accurate scheduling; this is glue, not DSP.

**Tests.** GitHub CI: golden-path unit tests (fake backend): pause/resume position error ≤ 1 block; fade curves; event ordering. Self-hosted both OSes: 10-minute file seek-storm test (100 random seeks, assert no stale audio — the `DiskStream` seek-generation test already proves the primitive). Device farm: user-visible position counter vs device clock.

**Status: [OPEN]**

---

### AU-12 | P1 | No lock-screen/now-playing/media-notification integration (MediaSession everywhere)

**Evidence.** No `MediaSession`, `MPNowPlayingInfoCenter`, or web `navigator.mediaSession` usage (repo-wide grep). Tied to AU-04 but separately necessary: even a foreground app benefits from hardware play/pause keys and OS media UI.

**Leaders.** Flutter `audio_service`; RN Track Player's events; the W3C Media Session API on web (https://developer.mozilla.org/en-US/docs/Web/API/MediaSession); every native platform ships one.

**Fix.** Part of the AU-04 platform work: a `MediaMetadata` + `MediaControls` struct in the neutral API; Android `MediaSession` + `MediaStyle` notification; iOS `MPRemoteCommandCenter`/`MPNowPlayingInfoCenter`; web `navigator.mediaSession.metadata` + action handlers. Engine exposes the transport state they bind to.

**Tests.** Device farm: `dumpsys media_session` assertions (Android), XCUITest remote-command injection (iOS); web: headless Chromium `navigator.mediaSession` presence test in the existing web test harness.

**Status: [OPEN]**

---

### AU-13 | P1 | Plugin formats: CLAP+VST3 only — no AUv3 (blocks iOS/macOS store distribution of audio units), no LV2 (Linux pro audio), no AAX, and no GUI story

**Evidence.** `lib.rs:13-19` lists exactly two wrappers; `THIRD_PARTY.md` handles the VST trademark correctly; `audio-plugins.yml` validates against clap-validator + Steinberg validator on 4 OSes — real, rare verification. But: no Audio Unit v3 (Apple's plugin API for iOS/OSX distribution; a `com.apple.AudioUnit` extension with a view controller), no LV2 (the Linux FOSS-pro-audio standard, used by Ardour — https://lv2plug.in), no AAX (Pro Tools). And there is no GUI abstraction at all: no `IPlugView`/CLAP `gui` extension code exists (grep). Parameter automation is sample-accurate (`lib.rs:23-25`) — the hard part is done.

**Blocks.** A vieww-built plugin can be sold on desktop (VST3/CLAP covers Bitwig/Reaper/Logic-in-VST3-mode) but cannot ship on iOS AUv3 hosts (AUM, GarageBand) at all; on Linux pro studios it won't load in Ardour; no plugin can show an editor — every commercial plugin needs one.

**Leaders.** JUCE compiles one C++ plugin to VST3/AU/AUv3/LV2/AAX/CLAP from one codebase and abstracts plugin GUIs into its component system (https://juce.com/learn/tutorials/plugins/create-audio-plugin/); nih-plug (Rust) does VST3+CLAP with a GUI via Vizia; this is table stakes for "pro audio" claims.

**Fix.** Priority order: (1) AUv3 wrapper (objective-C++ shim — the vieww audio-graph `process` shape maps directly; the viewcontroller can host a vieww widget tree — which would be the *first* framework-native plugin GUI story anywhere); (2) LV2 (pure C ABI, simpler than VST3; the Turtle manifest is data); (3) GUI extension for CLAP/VST3 that renders a vieww tree into a host-provided parent view (vieww already renders to native windows on desktop). AAX last (needs an NDA/SDK).

**Tests.** GitHub CI macOS: build the AUv3 extension, load it with `AUAudioUnit` in-process test; Linux: `lv2lint` on the bundle in the plugins workflow. Device farm iOS: load the AUv3 into a test host app on the iPhone.

**Status: [OPEN]**

---

### AU-14 | P1 | The device farm runs zero audio tests: AAudio has never run on a real device, iOS audio never on a real iPhone, Bluetooth never anywhere

**Evidence.** `grep -i audio .github/workflows/device-farm.yml` → no matches; the farm exercises UI/graphics suites (Redmi Note 7 Pro, iPhone 13 Pro via LambdaTest, per worklog facts). All Android/web audio coverage is `cargo clippy --target` type-checking (`audio-backends.yml` "targets" job); all loopback CI uses virtual cables (VB-CABLE/BlackHole/snd-aloop). The self-hosted audio step exists but has **no published receipts** (§0).

**Blocks.** The claims the stack most wants to make — real-device latency, underrun behavior under thermal throttling, Bluetooth recovery, focus/interruption correctness — are unverifiable. AAudio on a physical device behaves nothing like clippy.

**Leaders.** Google's own audio testing guidance uses real devices with `adb` metrics (underrun counts, latency measurement apps); Spotify/Chrome audio teams maintain device labs with loopback rigs; Mozilla's cubeb CI (which this repo already borrows VB-CABLE from) is the closest analog and still pairs hosted virtual devices with hardware runs.

**Fix.** Add an `audio` step to the device suite: an instrumented Rust binary (or the three-capture harness pattern from `apps/three`) that (a) opens AAudio via the backend, plays the 997/1499 Hz two-tone for 30 s while capturing via `AudioRecord` (mic permission granted in the test app), then runs the existing least-squares loopback analyzer locally on-device and writes a receipt JSON (latency estimate, xruns, scheduling class); (b) same for iOS via `AVAudioEngine` capture or the Lightning/USB audio interface if the farm has one; (c) farm automation optionally pairs a Bluetooth headphone (Android farm only) for pair/unpair recovery counts. Upload receipts to `receipts/device-farm/<date>-audio/` alongside the GPU ones.

**Tests.** Device farm (weekly schedule or per-release): the above; gate: zero xruns at 48 kHz/256 on the Redmi, latency receipt recorded (publish the number — see §6.4). GitHub CI: none needed (compiles under the existing targets job).

**Status: [OPEN]**

---

### AU-15 | P1 | All audio workflows are manual-only; the audio-ci.yml header comment still claims otherwise

**Evidence.** `audio-ci.yml` opens with "# Audio and music gate — the one workflow that runs without being asked… it runs only when audio or music code changes… on every push to main and every pull request that touches them" (`audio-ci.yml:1-12`) but the actual trigger is `on: workflow_dispatch` (`audio-ci.yml:26-27`, with the manual-only note at lines 23-25) — the header is stale (the owner made everything manual on 2026-10-15). Same `workflow_dispatch`-only for `audio-backends.yml`, `audio-plugins.yml`, and `framework-checks.yml`'s fuzz leg is report-only. So an audio regression can land on `main` un-noticed; the fuzz job exists but nobody schedules it.

**Blocks.** The core value proposition of the whole testing apparatus: proofs that don't run are marketing. The 550-test suite, the no-alloc proof, and the 8 fuzz targets gate nothing.

**Leaders.** Every comparable project gates PRs: RustAudio crates run CI on push; Oboe/JUCE run per-PR device + desktop matrices; Flutter plugins run on PRs by default. Nobody who can afford automation runs audio CI by hand.

**Fix.** Owner decision needed (worklog notes the manual-only choice is deliberate, pending a workflow-scoped token for the pending set — which has since been placed). Minimal automatic set: `audio-ci.yml` `test` job on `push` to main + PRs touching `vieww_base/crates/{vieww-audio,vieww-music,vieww-audio-plugin}/**` (paths filter — minutes, not hours, exactly as the header says); fuzz leg on a weekly `schedule` with a longer budget (1800 s/target) and artifact upload; fix or delete the stale header. `audio-backends.yml` and `audio-plugins.yml` can stay manual (they install system services / validators — slow), but a nightly schedule would catch server-drift.

**Tests.** GitHub CI: the workflow change itself; verify by a deliberate red PR (break a decoder assertion, assert the PR fails).

**Status: [OPEN]**

---

### AU-16 | P1 | No notation interchange: MusicXML out/in, LilyPond, ABC — the score cannot leave the ecosystem except as MIDI or audio

**Evidence.** `score.rs:285-336` exports SMF (with MTS tuning + MPE bends — genuinely better than most exporters), `render.rs` exports WAV/FLAC with tags, `scala.rs` exports `.scl`. Grep for MusicXML/LilyPond/ABC across crates: no hits (only base64 alphabets and unrelated "abc" test strings).

**Blocks.** A composition made with vieww-music cannot be opened in MuseScore/Dorico/Sibelius for engraving, printed, or handed to a human performer; education apps (the obvious Carnatic-app market) need staff notation; musicologists exchange MusicXML.

**Leaders.** MusicXML is the interchange standard every notation tool reads (https://www.w3.org/community/musicxml/ — now a W3C community spec); abcjs/VexFlow render on the web; Python's music21 reads/writes both; Dorico/Sibelius round-trip via it. No UI framework ships notation at all — which is exactly the opening.

**Fix.** `vieww-music/src/notation/musicxml.rs`: a `to_musicxml(&Score) -> String` writer first (partwise, divisions=PPQ, pitch as step+alter+octave with the score's tuning written as part of metadata; gamakas exported as `<pitch>`-bend annotations or grace slides with a documented convention — MusicXML has `<slide>`/`<bend>` elements that map to gamaka curves), then a reader that imports into `Score` (lossy on non-12-TET — document and preserve via `.scl` sidecar, which MuseScore can pair). ABC export (for session/folk users) is a weekend; LilyPond `.ly` export is a nicer PDF path than building an engraver. Rendering notation *inside* vieww (VexFlow-grade SVG) is a vieww-render feature, out of 2-c scope but noted for the framework report.

**Tests.** GitHub CI: round-trip fixtures — write MusicXML for each demo piece, parse back with a small self-written reader, assert note-level equality (start/length/pitch); validate against MuseScore's XSD if vendored; golden-file `.ly`/`.abc` outputs.

**Status: [OPEN]**

---

### AU-17 | P2 | vieww-music ↔ vieww-audio: no live playback path — the score renderer is offline-only

**Evidence.** `render.rs:23-24`: "Rendering is offline and deterministic". `Score::to_clips()` (`score.rs:10-11`) feeds the engine's sequencer — the building blocks exist — but nothing connects a Score to a running stream: no "play this Score through the AudioPlayer/EnginePlayer" function, no transport-linked scheduling of clips, no follow-the-playhead API. The demo pieces only ever become files.

**Blocks.** Any interactive composition UI (the studio, an education app): you can render a WAV and play it back as a blob, but you can't press play and hear the score with the transport, can't loop a bar, can't solo a part live.

**Leaders.** Ableton/Logic obviously; closer to home: Web MIDI + WebAudio sequencer apps; Sonic Pi's live-coding loop; kira's clock/scheduler. For a UI framework this is a "compose + immediately hear" story none of the competitors have at all.

**Fix.** `vieww-music/src/live.rs`: `LiveScore::new(score, settings) -> LiveScore` holding one `Poly` per part (the exact construction from `render_part`), scheduled through the engine's `Scheduler`+`Clip`s with transport control (`play/pause/seek_beat/loop_range`, `set_tempo`), mixed through the live `Mixer` (same faders/sends as offline) and playable through the AU-01/AU-11 player. Determinism note: live playback shares the offline DSP, so a live-recorded mixdown can be diffed against the offline render — a test no competitor can even express.

**Tests.** GitHub CI: for each demo piece, play it live into a WAV sink (render closure writing to a buffer) and assert ≥ 60 dB match against `render()` output modulo limiter lookahead (the `null_depth_db` helper in `quality.rs` exists for exactly this). Self-hosted: 10-minute live playback soak via the player.

**Status: [OPEN]**

---

### AU-18 | P2 | RT-safety is proven by test, not enforced by the compiler — no lint denying allocation/locking in the audio thread path

**Evidence.** The counting-allocator test (`realtime_no_alloc.rs`) is superb but is (a) opt-in per surface — it covers graph/mixer/parallel/diskstream/voices, not the plugin `Driver`, not `sequencer` event dispatch, not any *user* closure; (b) a runtime proof that runs on CI, so a regression lands only after review. `resilient.rs` deliberately uses a `Mutex<Option<RenderFn>>` slot — taken with `try_lock` in the callback (`resilient.rs:82`) — correct, but the pattern-safety lives in comments, not types.

**Blocks.** Future contributions can silently add a `Vec::push` to a render path; the test catches it only on Linux CI (which currently doesn't run automatically — AU-15).

**Leaders.** No mainstream framework enforces this at all (JUCE documents discipline; Realio/`rt_param`/`fermium` in Rust encode RT safety in types). **This is a leadership opportunity, not just debt** (§6.5).

**Fix.** Two layers: (1) a workspace clippy wrapper script (`ci/check/rt-lint.sh`) using `cargo clippy -- -W clippy::missing_panics_doc`-style plus a `#[vieww::rt]` proc-macro that (like `#[no_alloc]` experiments) inserts a counting-allocator assertion scoped to the function when `debug_assertions` — cheap and effective; (2) extend `realtime_no_alloc` to arm the allocator across a plugin `Driver::process` block, a sequencer+transport block, and a `LiveScore` (AU-17) block.

**Tests.** GitHub CI: the lint job (audio-ci `test`); the extended no-alloc harness on every push once AU-15 lands.

**Status: [OPEN]**

---

### AU-19 | P2 | MIDI ports: no Android (AMidi), no Web MIDI; Linux rawmidi misses ALSA-sequencer virtual ports (documented)

**Evidence.** `midi_io.rs:9-17`: ports on Linux (rawmidi `hw:C,D`), Windows (WinMM), macOS/iOS (CoreMIDI); "Elsewhere the lists are empty". The header honestly notes software ports (other apps' virtual ports) are invisible to rawmidi. `midi_coremidi.rs` (383 lines) and `midi_winmm.rs` (383) exist; no AMidi binding, no Web MIDI shim (the web backend ships a JS shim for audio — none for MIDI).

**Blocks.** Android MIDI controller apps (keyboards/drum pads over USB/BLE — AMidi is the supported path, https://developer.android.com/media/midi); web MIDI (Chrome/Edge support `navigator.requestMIDIAccess` — a vieww web music app can't see a controller); on Linux, a synth app can't receive from a DAW's virtual port without the sequencer API.

**Fix.** (1) AMidi: `midi_android.rs` behind the JNI bridge (the byte-level `Parser`/`MidiState` layer is already transport-agnostic); (2) Web MIDI: extend `web/vieww-audio.js` (rename conceptually to a vieww web-IO shim) with `requestMIDIAccess` bridging into the same `MidiInput::poll` byte path; (3) Linux: add an optional `alsa-seq` input backend (libasound `snd_seq_*` — dlopen like the rest), or document the limitation in a guide.

**Tests.** Device farm: Android USB MIDI adapter if the farm has one; else at minimum the byte-pipe unit tests. GitHub CI: web shim test in `web/test.mjs` style with a simulated `MIDIAccess`. Self-hosted Linux: `snd-virmidi` round trip already exists in CI (`device_loopback.rs` `midi_port_round_trip`) — reuse for the seq backend.

**Status: [OPEN]**

---

### AU-20 | P2 | Fuzz coverage extension: DSP/stream/mixer/plugin paths are not fuzzed; fuzzing is one-shot 300 s on demand

**Evidence.** All 8 targets are container/codec decoders (`fuzz/Cargo.toml`); `audio-ci.yml` fuzz job: `FUZZ_SECONDS: "300"` per target, manual dispatch only. The synth (SF2/SFZ *file* parsing is hostile-input-shaped!), the preset loader (binary presets!), the graph connect/validation, and the plugin state I/O (IBStream reads of plugin state!) are all attacker-shaped inputs with no fuzzing.

**Blocks.** A malicious SF2/SF3/preset file opened by an app crashes it (the codec discipline shows exactly this class of bug exists); a corrupt plugin state blob from a downloaded project kills a host.

**Leaders.** OSS-Fuzz covers ffmpeg, opus, and lame continuously; JUCE doesn't fuzz; Chrome's audio stack is heavily fuzzed. For a framework shipping decoders by hand, continuous fuzzing is the industry norm.

**Fix.** Add targets: `parse_sf2`, `parse_sfz`, `parse_midi_smf`, `load_preset` (binary), `plugin_state_roundtrip` (bytes → CLAP/VST3 state → params), `graph_random` (random connect/disconnect sequences asserting no panic + no alloc). Then: register the repo with OSS-Fuzz (a Rust project with an existing `fuzz/` crate is ~a day of integration) for continuous coverage; keep the CI job as the fast loop.

**Tests.** GitHub CI: new targets in the fuzz loop (once scheduled per AU-15). OSS-Fuzz: continuous. Hostile corpus growth: each minimized finding becomes a `tests/hostile/` row (the pipeline already exists).

**Status: [OPEN]**

---

### AU-21 | P2 | Docs: no audio/music chapter in the user guide; no app-facing "sound on each platform" walkthrough; examples are thin

**Evidence.** `docs/guide/` contains getting-started, state, widgets, theming, accessibility, testing — no audio page. Workspace examples touching audio: 3 visual demos + 3 vieww-music CLI examples + plugin examples; no example plays through a backend on a phone, no device-picker UI, no media player example. The crate rustdoc is excellent but speaks to engine users, not app builders.

**Blocks.** Discoverability: a Flutter developer evaluating vieww finds `just_audio`'s README in one search; vieww's equivalent story is buried in rustdoc of an unpublished crate. The music crate — the most differentiated thing in the repo — has no guide entry at all.

**Leaders.** Flutter's sound docs (https://docs.flutter.dev/cookbook/petworking? no — the audio packages each ship full READMEs with platform-setup tables); Godot's audio buses tutorial; Unity's audio manual chapters.

**Fix.** `docs/guide/audio.md`: (1) "A sound in three lines" per platform incl. the Info.plist/manifest bits from AU-04; (2) the player facade guide (AU-11) with a small music-player example app under `examples/features/`; (3) `docs/guide/music.md` walking the Carnatic piece and the chorale with the generated audio; (4) a backend-selection/latency tuning table; (5) wire both into the guide index + the studio's docs panel if applicable.

**Tests.** GitHub CI: doc-example builds (`cargo build --example media_player`) — the features examples already run in framework-checks.

**Status: [OPEN]**

---

### AU-22 | P2 | Analysis lacks polyphonic pitch/chroma; no key detection; `Spectrum` has no waterfall/peak-hold UI helpers

**Evidence.** `analysis.rs` exports `Spectrum` (analyze, magnitudes, bin_hz, peak_bin, band_energy) + `Window`; `dsp.rs` has `OnsetDetector` + `estimate_tempo`; `quality.rs` has single-tone `estimate_frequency` (least-squares). No YIN/MPM pitch tracker, no chroma vector, no key/scale estimation (the music crate has the theory to score candidate keys — a natural pairing).

**Blocks.** Tuner apps, transcription features, audio-reactive "musical" visuals (chord display), singing-instruction apps (the Carnatic audience again: a shruti/pitch-accuracy trainer needs a monophonic pitch tracker at minimum).

**Leaders.** Web Audio ecosystem (PitchDetect sample, ml5.js pitch); aubio is the C reference for onset/pitch/chroma; librosa for Python. No UI framework ships any of it.

**Fix.** `analysis::pitch::{yin, mpm}` (monophonic, well-specified autocorrelation-family algorithms; ~150 lines each + tests against synthesized tones *and* vibrato gamakas — the test material is uniquely available here), then `chroma(fft)` + `key_estimate(chroma)` scoring against `western::scale` candidates (major/minor + the 72 melakartas — a first).

**Tests.** GitHub CI: synthesized-tone ground truth (render known frequencies incl. the 22-shruti set, assert cents error < 5); `null_depth_db`-style regression corpus.

**Status: [OPEN]**

---

### AU-23 | P3 | `Samples` is stereo-max while the rest of the stack is multichannel — an inconsistent surface

**Evidence.** `lib.rs:168-184`: "`channels` 1 for mono, 2 for stereo… nothing above two is supported"; `codec::Decoded` carries up to 64 channels (`codec/mod.rs:50-54`); `channels.rs` implements up-to-7.1 layouts; WASAPI clamps to 18 (`wasapi.rs:644`); the graph/mixer are stereo. So a 5.1 FLAC decodes but cannot become `Samples`, play through the seam, or reach the device as 5.1 through the graph.

**Blocks.** Surround apps (video players, spatial installs); minor for most UI apps, but the docs' reasoning ("a type that pretended to carry it would be lying") is contradicted by the crate's own decode path.

**Fix.** Either (a) document the boundary as deliberate (UI-layer sounds are mono/stereo; multichannel media belongs to a future `vieww-video` audio path), or (b) widen `Samples`/graph/mixer to N channels with the existing `channels::Layout` doing the remixing. Given the seam's purpose, (a) plus a `Decoded::downmix_stereo()` helper (which likely exists as `to_samples` fallback — `to_samples` returns `None` for >2 today, `codec/mod.rs:104-112`) is honest and cheap; note it in the guide (AU-21).

**Tests.** GitHub CI: a 5.1 fixture decodes, `to_samples` documents the fold-down decision.

**Status: [OPEN]**

---

### AU-24 | P3 | Web: autoplay policy is a comment, not a mechanism; no Media Session on web; worker mode needs COOP/COEP documented per-app

**Evidence.** `web/vieww-audio.js:33-34`: "browsers require a user gesture before audio starts: open streams from an input handler, or the context waits suspended until one happens" — correct advice, but the shim already calls `await ctx.resume()` at line 247 in one path; there's no unified "ensure_resumed_on_gesture" helper, no surfaced `context state` event to the app, and `navigator.mediaSession` is untouched (AU-12's web leg). Worker mode's cross-origin-isolation requirement (`web.rs:17-21`) is crate doc only — no guide for app packagers (the site app needs COOP/COEP headers).

**Leaders.** Web Audio spec + MDN autoplay policy docs; every web audio app deals with this once and ships a helper.

**Fix.** Shim: `resume()` idempotent + a `statechange` callback exposed through the existing import/export seam; on first pointer/key event in the page (opt-in), auto-resume. Docs: a `docs/guide/audio.md` web section listing the exact headers and the three transport modes with their tradeoffs. MediaSession wiring rides AU-12.

**Tests.** GitHub CI: `web/test.mjs` gains a suspended-context scenario (fake `AudioContext` starting 'suspended') asserting the helper resumes on a synthetic gesture.

**Status: [OPEN]**

---

### AU-25 | P3 | No loudness normalization for media playback (ReplayGain/EBU R128 pre-analysis and album mode)

**Evidence.** `meter.rs` measures EBU R128 integrated loudness (the render.rs master uses `normalization_gain`, `render.rs:334-340`) — but the media path (`Loader`, future `MediaPlayer`) has no "analyze file → store gain → apply at playback" flow, no tag read/write for `REPLAYGAIN_*`/`R128_*` (tags.rs is generic key-value so storing works; nothing computes or applies it).

**Blocks.** A music app whose tracks play at wildly different levels — the #1 complaint of every player that skips normalization. Apple Sound Check / Android's loudness normalization have trained users to expect it.

**Leaders.** just_audio exposes a `volume` but the ecosystem pairs it with replaygain tags; ffmpeg `loudnorm`; Spotify's -14 LUFS normalization.

**Fix.** `Loader` option `analyze_loudness: bool` → per-file R128 integrated gain cached beside the decoded cache; `MediaPlayer::set_loudness_normalization(Album|Track|Off)` applying `fader_db` (the mixer already has the gain stage). ~100 lines over existing pieces.

**Tests.** GitHub CI: fixtures at ±10 LU offsets, assert playback gain compensates to ±0.5 LU.

**Status: [OPEN]**

---

**Gap counts: P0 = 5, P1 = 11, P2 = 6, P3 = 3 (total 25).** All OPEN on both branches (§3). Verified *non*-gaps worth stating: gapless MP3 (LAME tag) and MP4 priming are implemented; seeking in `DiskStream` has a generation/ack protocol that provably never lets stale audio through; underrun counting exists at ring, DiskStream and backend layers; latency is reported by 8 of 9 backends (AAudio the exception); device hot-plug recovery is real and tested; WASAPI exclusive exists; CoreAudio handles both macOS HAL and iOS RemoteIO; the plugin ABI is validated against official SDK validators.

## 5. Where vieww already leads

Each claim verified in code:

1. **A complete hand-rolled codec suite with fuzz + oracle + hostile-regression discipline** (`codec/` + `fuzz/` + `tests/hostile/`). Six formats decoded from spec, graded against ffmpeg reference decodes by SNR with lag search (`codec/mod.rs:243-274`), an oracle that is itself tested against every known-good fixture so a harness bug can't fake coverage, and a minimized corpus where each file documents the crash it once caused (`tests/hostile/README.md`). No UI framework ships its own codecs (Flutter/RN delegate entirely); among Rust audio, only symphonia approaches this rigor, and symphonia does not carry the per-file regression provenance or the concealment policy (`Decoded::validated`, `codec/mod.rs:68-96`).
2. **RT-safety as a proven property, not a style guide.** The counting-allocator harness with `harness = false` so it observes *every* thread including worker pools and the disk reader (`realtime_no_alloc.rs:1-22`, with the documented reason why libtest itself broke it); FTZ/DAZ scoping per callback with the caller-mode mirroring that keeps offline renders bit-identical across thread counts (`denormal.rs:17-25`); panic isolation that poisons rather than re-panics (`backend/mod.rs:252-254`). JUCE documents discipline; vieww *tests* it. cpal/rodio/kira make no such claim.
3. **Device-loss resilience with closure continuity.** `ResilientStream` hands the *same render closure* to each new device stream (`resilient.rs:78-94`) — effect tails and sequencer position survive an unplug/replug, which even most native apps get wrong (they restart playback). Tested against a fake hot-plug backend including the poisoned-closure never-run-again case.
4. **Carnatic music theory as a first-class, computed library.** All 72 melakartas generated by rule (no table), ragas as validated grammars with vakra paths, gamakas as C¹ pitch curves applied as exact MPE bends at 1.5 kHz control rate (`render.rs:71-73, 163-174`), 35 suladi talas by construction, mora/korvai arithmetic that *provably* lands on sam (`korvai.rs:1-12`), konnakol sollus, a physical mridangam and tambura model in the synth, and MTS so the tuning travels over MIDI. **No framework, no audio library, no leader has any of this.** The nearest neighbors are academic (raga recognition papers) and a handful of niche iOS apps.
5. **Deterministic mastering-grade offline rendering.** Score → per-part MPE polys → DAW mixer with sends/PDC → 20 kHz Butterworth → EBU R128 normalization → lookahead true-peak limiter with latency compensation → exact 4× oversampled true-peak trim (`render.rs:313-371`), asserted by tests that measure the LUFS and dBTP of the output (`render.rs:481-511`), and bit-for-bit deterministic (`render.rs:551-565`).
6. **Backend verification on real OS audio stacks in CI** — including the lengths gone to when hosted runners lack hardware: booting a stock kernel in a virtme-ng VM for `snd-aloop` (`audio-backends.yml` linux-alsa-loopback job), VB-CABLE/BlackHole installs mirroring Mozilla cubeb's CI, `VIEWW_REQUIRE_LOOPBACK=1` so a vanished device fails rather than skips. Most projects test backends by compiling them.
7. **A plugin ABI transcribed from specs and validated against the formats themselves** — official CLAP headers in C hosts, Steinberg's own validator, clap-validator, on a 4-OS matrix (`audio-plugins.yml`), with sample-accurate automation splitting (`lib.rs:23-25`). nih-plug is the only comparable Rust effort, and it doesn't ship hosts.
8. **Measurement as a product feature**: the `quality.rs` suite (IEEE 1057 sine fitting, SMPTE IMD, AES17 DR, Farina sweeps with per-order distortion separation, cross-correlation latency with sub-sample precision, IEC 61672 A-weighting, fractional-octave bands) is pro-audio-analyzer territory; the loopback analyzer that catches dropped frames by phase drift per 10 ms window (`device_loopback.rs:23-31`) is what audio-device companies write internally.

## 6. Novel methodologies NO leader has today (PoC candidates)

### 6.1 Golden-WAV CI: full-song bit-exact receipts for the whole audio stack
**What:** render the three demo pieces (and a stress score: all instruments, all gamaka types, all FX) to WAV offline in CI on every platform GitHub hosts (linux x64+arm, macos, windows, wasm via the Node shim) and pin SHA-256 of the *samples* (not the file — tags/version headers excluded) as a receipt; any platform that diverges by one bit fails.
**Why it's feasible here and nowhere else:** the stack is built on the rule "every type is a function of time; nothing reads a clock" (`lib.rs:59-67`), `render()` is asserted deterministic (`render.rs:551-565`), FTZ mode is mirrored across threads so denormals can't differ (`denormal.rs:23-25`), and there is one dependency. Flutter/RN/Unity cannot do this — their audio goes through platform float paths and vendor codecs. JUCE could in principle but has no such harness.
**Risks to kill first (verify in PoC):** cross-platform f32 differences in `sinf`/`powf` (glibc vs macOS libm vs MSVC RT) — the PoC should hash each instrument *and* the mix to bisect any divergence; likely fix: crate-local polynomial approximations for the few transcendentals in DSP (or accept per-platform goldens, which is still a unique artifact: "the same song, four OSes, four hashes, one lineage").
**PoC sketch:** CI job renders `pieces::carnatic(15)` to `Samples`, hashes `data` via the crate's md5 (`codec/md5.rs` — already in-tree for FLAC!), compares against `receipts/audio/golden/<platform>.md5`. **Proves:** the determinism claim end-to-end; catches libm drift, optimizer-induced FMA reordering, and FTZ bugs the day they appear.

### 6.2 Audio parity farm: sample-exact synthesis diff across real devices
**What:** the same short deterministic score played *live* on the device farm (Android phone, iPhone, both self-hosted PCs), captured through each device's loopback or USB audio interface, time-aligned and correlated against the offline golden — publishing per-device correlation, latency and xrun counts as a compatibility database in `receipts/`.
**Why no leader has it:** nobody has (a) deterministic synthesis worth comparing, (b) a farm, and (c) a loopback analyzer that references the capture's own clock (vieww has all three: `device_loopback.rs`'s least-squares fitter is exactly the tool). Google publishes Android audio latency numbers per device (https://developer.android.com/ndk/guides/audio/samplerates-note); a *framework* publishing signed latency/xrun/parity receipts for its own engine per device would be first.
**Feasibility:** moderate — needs the AU-14 farm step plus, ideally, a cheap USB interface on each farm device; the Android mic-capture path alone (in-app AudioRecord) suffices for a v1 correlation metric.
**Proves:** "your app sounds identical and arrives within N ms on device X" — the claim every audio framework wishes it could make.

### 6.3 Gamaka-aware microtonal UI primitives
**What:** widget-level `GamakaSlider`/`RagaField` — a pitch control whose *motion* is constrained by `Raga::check` and whose value is a pitch number in the current `Tuning`; drag gestures produce smooth gamaka curves (kampita/jaru) via the same C¹ curve library the renderer uses, rendered as a pitch-ribbon widget; a tala-widget that lays out the 35 suladi talas as beat grids with nadai.
**Why no leader:** no framework has a raga grammar, a gamaka curve type, or a tuning-propagating score to constrain the widget with — these are exactly the `vieww-music` types (`raga.rs`, `gamaka.rs`, `tala.rs`). Flutter/RN would need all of vieww-music underneath before writing the widget.
**Feasibility:** high — pure vieww-render work over existing data structures; the music crate's `generate::raga_walk` already proves the grammar can drive interactive sequences.
**Proves:** the framework's differentiator is real at the UI layer, not just a library — "compose in Mayamalavagowla with correct gamakas, hear it immediately" (pairs with AU-17's live playback).

### 6.4 Latency-budget receipts as a published, regressed database
**What:** every CI run that touches a device (hosted loopback jobs, self-hosted audio step, farm) emits a structured receipt (backend, device, granted config, `StreamStats.latency`, peak load, xruns, scheduling class). A script regresses new receipts against the database and *fails* when a platform regresses latency by >10%, with a per-device public dashboard page in the repo.
**Why no leader:** Apple/Google publish one-time guidance tables; nobody regresses their own engine's latency per device per commit. Oboe's test suite measures but doesn't publish a lineage.
**Feasibility:** trivial mechanically (the data is already in `StreamStats`; receipts infrastructure exists for GPU) — the work is governance (choosing budgets).
**Proves:** latency is a contract, not a benchmark screenshot — the thing pro-audio customers actually buy.

### 6.5 RT-safety as a CI type-check (the `#[rt]` boundary)
**What:** a proc-macro attribute `#[vieww_audio::rt]` marking render-path functions; in debug builds it installs the counting allocator around a call and asserts zero allocations; in CI a clippy-driver pass denies `Vec::new`, `Box::new`, `.lock()`, `format!` etc. inside `#[rt]` functions (allow-listed escape hatch `#[rt_alloc]`). The plugin `Driver`, graph render, mixer process, and backend forwarders all get marked.
**Why no leader:** JUCE enforces by review; Rust's real-time ecosystem (realio, rt_param, fermium) types *parameters*, not whole render paths; nobody ships the allocator-assert + lint combo as a reusable attribute on their framework's public API so *app code* written against the framework gets the same protection.
**Feasibility:** high — the allocator trick is proven in-tree; the lint is a clippy-driver wrapper over `rustc_ast`; the attribute makes it available to users, which is the novel part (Flutter/RN app audio code has zero such guardrails).
**Proves:** "the audio thread cannot allocate" becomes a property the compiler checks for user code, not a library-internal test — a genuine framework-level first.

### 6.6 (Bonus) The oracle-of-oracles: mutation-scored decoders
**What:** CI job that *injects* known bit-flips into each fixture (the same technique that found the 2026-10-20 hostile corpus, now automated) and measures the decoder suite's mutation score — the fraction of mutants killed by (refuse | decode-correctly | conceal) vs. (panic | hang | garbage).
**Why no leader:** coverage numbers exist everywhere; *mutation scores for decoders* are a research-lab practice. The in-repo history ("mutation fuzzing found… by the hundred") shows the method works manually.
**Feasibility:** high; the hostile-input pipeline (find → minimize → pin) already exists; this automates its measurement.
**Proves:** the fuzz+oracle harness is quantitatively good, not just present — and regresses like any other metric.

## 7. Stream test plan

Infrastructure mapping (from worklog): **GH** = GitHub-hosted CI; **SHL** = self-hosted Linux (Kaby Lake i5-7200U, real GPU/sound card, receipts dir); **SHW** = self-hosted Windows (same machine class); **DF** = device farm (Redmi Note 7 Pro + iPhone 13 Pro via LambdaTest + API 34 emulator).

| # | Test | Runs where | Gates what | Frequency |
|---|---|---|---|---|
| 1 | Crate tests + clippy (550+62+24 tests, `-D warnings`, incl. no-alloc harness, hostile corpus, nan recovery) | GH (audio-ci `test`) | Any regression in engine/DSP/codecs; RT allocation proof | Every push + PR (after AU-15) |
| 2 | Fuzz all 8 targets, 300 s each + new targets (SF2/SFZ/preset/SMF/plugin state) | GH (audio-ci `fuzz`, nightly schedule) | Decoder/hostile-input robustness | Nightly + before release |
| 3 | Linux servers live: JACK(dummy)/Pulse(null)/PipeWire(null) with `VIEWW_EXPECT_BACKENDS`, soak under full-core spin, scheduling-class + load + xrun asserts | GH (audio-backends `linux-servers`) | Backend correctness under load, RT promotion, telemetry truth | Weekly + release |
| 4 | ALSA kernel loopback (virtme-ng VM): every PcmFormat, rate matching both directions, `snd-virmidi` MIDI round trip | GH (audio-backends `linux-alsa-loopback`) | ALSA negotiation/dither/resample, MIDI byte integrity | Weekly + release |
| 5 | Windows WASAPI shared + exclusive through VB-CABLE loopback (`VIEWW_REQUIRE_LOOPBACK=1`), WinMM MIDI port list | GH (audio-backends `windows-loopback`) | WASAPI both modes frame-accurate, endpoint enumeration | Weekly + release |
| 6 | macOS CoreAudio through BlackHole + CoreMIDI virtual endpoints | GH (audio-backends `macos-loopback`) | CoreAudio + iOS-shared code path, CoreMIDI | Weekly + release |
| 7 | Android (aarch64) + wasm32 clippy/type-check; web shim under Node (3 transports) | GH (audio-backends `targets`) | Cross-target compilation, web transport logic | Every push |
| 8 | Plugins: unit tests, own hosts, official-header C/C++ hosts, clap-validator, Steinberg validator, bundle layout, 4-OS matrix | GH (audio-plugins) | ABI transcription correctness both directions | Weekly + release |
| 9 | Real sound card: device list, live backends 10 s, default-output soak 600 s (gain 0 = silent but streamed), optional wired loopback + MIDI cable | SHL + SHW (hardware.yml `audio` step) | Real-driver behavior, long-run stability, xrun budget on silicon | Weekly + release; 24 h soak monthly (`VIEWW_HW_AUDIO_SOAK=86400`) |
| 10 | WASAPI exclusive latency + late-render budget on real endpoint; IAudioClient3 path once AU-10 lands | SHW | Latency contract on Windows silicon | Weekly |
| 11 | JACK/PipeWire/ALSA/Pulse matrix soak: same render through each backend, stats compared | SHL | Backend parity on one machine (same DSP, different clocks) | Weekly |
| 12 | Device farm audio (AU-14): AAudio 30 s play+capture with on-device loopback analyzer, latency/xrun receipt; iOS CoreAudio play + interruption simulation; Bluetooth pair/unpair recovery (Android) | DF | Mobile reality: AAudio on silicon, session lifecycle, BT resilience | Per release + monthly |
| 13 | Audio focus automaton: dispatch focus events via adb, assert duck/pause/resume + media session state | DF (Android) | AU-02/AU-04 behavior | Per release |
| 14 | Background playback: screen off 5 min, audio advancing, foreground service + notification present | DF (both platforms) | AU-04 | Per release |
| 15 | Large-media: 3 h file pull-decode, RSS < budget, seek storm, gapless chain | GH (memory assertions) + DF (real device RSS) | AU-05 | Per release |
| 16 | Golden-WAV receipts across GH platforms (+ SHL/SHW/DF once 6.2 lands) | GH (+SHL/SHW/DF) | Determinism contract (§6.1) | Every push (GH), per release (devices) |
| 17 | Live-Score vs offline-render null test (AU-17): ≥ 60 dB match | GH | Live playback equals offline render | Every push |
| 18 | Mutation-score job for decoders (§6.6) | GH (manual/nightly) | Harness quality metric | Monthly |

Loopback feasibility note: real-speaker + microphone capture is the only option on locked phones; the existing least-squares analyzer already tolerates acoustic paths poorly — for DF use wired loopback where hardware allows (USB-C audio interface), else in-app capture (Android `AudioRecord` / iOS `AVAudioEngine` tap) which is cabled by construction.

## 8. Sources

**Code files read (all under `/home/z/my-project/vieww_artifacts`):** `vieww_base/crates/vieww-audio/src/`: lib.rs, backend/{mod,aaudio,alsa,coreaudio,jack,pipewire,pulse,wasapi,web,resilient,dl}.rs, codec/{mod,aac,mp3,flac,vorbis,mp4,ogg,aiff,bits,tags,md5,mp3_tables,aac_tables}.rs, synth/ (mod,osc,fm,voice,sampler,sf2,sfz,physical,percussion,granular,tuning headers), fx/{mod,reverb,filter,eq,dynamics,delay,modfx,distortion,master,fir}.rs, player.rs, device.rs, loader.rs, stream.rs, graph.rs, mixer.rs, sequencer.rs, transport.rs, midi.rs, midi_io.rs, mts.rs, analysis.rs, dsp.rs, quality.rs, meter.rs, rt.rs, denormal.rs, spatial.rs, parallel.rs, preset.rs (headers/sections as cited); `tests/{realtime_no_alloc,live_backends,device_loopback,hostile_inputs,nan_recovery,audit_probes}.rs`, `tests/hostile/README.md`, `fuzz/Cargo.toml`, `fuzz/tests/oracle_on_fixtures.rs`, `fuzz/fuzz_targets/*`, `web/vieww-audio.js`, `web/test.mjs`, `Cargo.toml`, `build.rs`, `CHANGELOG.md`, `tools/THIRD_PARTY.md`; `vieww_base/crates/vieww-music/src/` (all 20 files, render.rs and score.rs in full); `vieww_base/crates/vieww-audio-plugin/src/{lib,clap/*,vst3/*}.rs` (heads + host docs), `THIRD_PARTY.md`, `tools/check-plugins.sh` (workflow usage); `.github/workflows/{audio-ci,audio-backends,audio-plugins,hardware,device-farm}.yml`; `vieww_base/ci/hardware/{linux.sh,windows.ps1}`; `vieww_base/{TRACKER,PENDING}.md` (audio mentions); `receipts/` (absence of audio receipts verified).

**Leader references (canonical documentation URLs, cited from knowledge — not fetched in this sandbox):**
- Apple AVAudioSession / interruption / route changes: https://developer.apple.com/documentation/avfaudio/avaudiosession ; AVAudioEngine: https://developer.apple.com/documentation/avfaudio/avaudioengine
- Android audio focus: https://developer.android.com/media/optimize/audio-focus ; AAudio: https://developer.android.com/ndk/guides/audio/aaudio/aaudio ; Oboe: https://github.com/google/oboe ; Media3 playback: https://developer.android.com/media/media3 ; AMidi: https://developer.android.com/media/midi
- Flutter: just_audio https://pub.dev/packages/just_audio ; audio_session https://pub.dev/packages/audio_session ; audio_service https://pub.dev/packages/audio_service ; just_audio_background https://pub.dev/packages/just_audio_background ; audioplayers https://pub.dev/packages/audioplayers ; flutter_soloud https://pub.dev/packages/flutter_soloud
- React Native: react-native-track-player https://github.com/react-native-kit/react-native-track-player ; expo-av https://docs.expo.dev/versions/latest/sdk/av/
- Web: Web Audio API https://www.w3.org/TR/webaudio/ ; Media Session API https://developer.mozilla.org/en-US/docs/Web/API/MediaSession
- Game engines: Godot audio buses https://docs.godotengine.org/en/stable/tutorials/audio/audio_buses.html ; kira https://github.com/tesselode/kira ; Bevy https://github.com/bevyengine/bevy (bevy_audio); FMOD https://www.fmod.com/ ; Wwise https://www.audiokinetic.com/
- Pro audio: JUCE https://juce.com/ (audio I/O and plugin tutorials); LV2 https://lv2plug.in/ ; CLAP https://github.com/free-audio/clap ; VST3 https://github.com/steinbergmedia/vst3_pluginterfaces ; MusicXML https://www.w3.org/community/musicxml/
- Rust peers: cpal https://github.com/RustAudio/cpal ; rodio https://github.com/RustAudio/rodio ; symphonia https://github.com/pdeljanov/Symphonia (codec list + patent caveat)
- Windows: low-latency shared mode https://learn.microsoft.com/windows/win32/coreaudio/low-latency-audio ; IMMNotificationClient https://learn.microsoft.com/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immnotificationclient
- Mozilla cubeb CI (VB-CABLE/BlackHole precedent): https://github.com/LABSN/sound-ci-helpers
