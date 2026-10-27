# Sound

How an app makes sound with vieww: short UI sounds, a media player, a score
played live, and what each platform needs (AU-21, AU-24).

Three crates are involved:

* `vieww-audio`: the engine, codecs, effects and devices;
* `vieww-music`: scores, tunings and their renderer;
* `vieww-platform-winit`: registers the app's audio player on desktop and
  Android.

## A UI sound

The platform registers an `AudioPlayer` service. It is an
[`EnginePlayer`](../../crates/vieww-audio/src/engine_player.rs), and it
opens the output device the first time something plays, never at startup.
Ask the app's services for it:

```rust
use vieww_audio::{AudioPlayer, Sound, Tone, Waveform};

let player = services.get::<dyn AudioPlayer>().expect("registered by the platform");
let click = Sound::tone(Tone::new(880.0, Waveform::Sine));
let handle = player.play(&click)?;
// later, if it is still sounding:
player.stop(handle)?;
```

On a platform with no audio, the service answers `ServiceError::Unsupported`
rather than pretending to play.

## A media player

`MediaPlayer` is one track's transport: load, play, pause, seek, volume,
looping, and an `Ended` event. It plays on an `Engine`:

```rust
use std::sync::Arc;
use vieww_audio::{Engine, MediaPlayer, MediaSource, Normalization, DEFAULT_TARGET_LUFS};

let engine = Arc::new(Engine::open_default()?);
let mut player = MediaPlayer::new(Arc::clone(&engine));
player.set_normalization(Normalization::Track { target_lufs: DEFAULT_TARGET_LUFS });
player.load(MediaSource::File("song.flac".into()))?;
player.play()?;
```

* **Formats.** `MediaSource::File` and `MediaSource::Bytes` decode WAV,
  AIFF, FLAC, MP3, Ogg Vorbis and AAC (ADTS, MP4/M4A). Media wider than
  stereo is folded to stereo with the standard down-mix.
* **Loudness.** `Normalization::Track` brings every track to one
  loudness, without raising peaks above −1 dBTP. `Normalization::Album`
  takes one gain for a whole album, from `album_gain_db`.
* **AAC.** Before shipping a product, read
  [`crates/vieww-audio/PATENTS.md`](../../crates/vieww-audio/PATENTS.md).
  If your counsel prefers to leave the AAC decoder out, build with
  `default-features = false`.

## A score, live

`vieww-music` renders a `Score` to a mastered file offline
(`render::render`). `live::LiveScore` plays the same score through a
running stream:

```rust
use vieww_music::live::LiveScore;
use vieww_music::render::Settings;

let mut live = LiveScore::new(score, Settings::default());
live.play();
// in the audio callback, interleaved stereo at Settings::rate:
live.render(out);
```

The live mix is the offline mix, sample for sample, whatever block size
the device asks for. Mastering is the exception: it measures the whole
piece first, so it cannot run live. Set a fixed gain instead with
`set_gain_db`. `seek_beat` moves the playhead exactly; it is not meant
for the audio thread.

## Platforms

| Platform | Output | What to know |
|---|---|---|
| Linux | PipeWire / PulseAudio / ALSA | Nothing to configure. |
| Windows | WASAPI shared mode | Nothing to configure. |
| macOS, iOS | Core Audio | iOS audio-session handling (interruptions, background audio) is tracked as AU-03. |
| Android | AAudio | Audio focus and background playback are tracked as AU-02 and AU-04. |
| Web | Web Audio (AudioWorklet) | See below. |

## The web

The browser side is
[`crates/vieww-audio/web/vieww-audio.js`](../../crates/vieww-audio/web/vieww-audio.js).
Map the wasm import module `vieww_audio` to it, then call
`attach(wasmExports)`.

### Autoplay

Browsers start every AudioContext *suspended* until the page has had a
user gesture. Opt into the shim's handling:

```js
import { attach, onAudioState, resumeAll } from "vieww_audio";

attach(wasm, { autoResume: true }); // resume on the first pointer, key or touch
onAudioState((stream, state) => {
  // "suspended" → show a "tap to play" hint; "running" → hide it
});
playButton.onclick = () => resumeAll(); // safe to call any number of times
```

The wasm side can export `vieww_audio_state(handle, code)` to hear the
same changes: 0 suspended, 1 running, 2 interrupted, 3 closed.

### Now playing

`setMediaSession({ title, artist, album, artwork }, { play, pause, seekto,
previoustrack, nexttrack })` fills the browser's now-playing card. It also
routes the card's buttons, and the hardware media keys, to your handlers.

### Which transport, and the headers it needs

| Mode | When | Page needs |
|---|---|---|
| Worker | The app's wasm runs in a Web Worker; the page calls `hostAudio(worker)` | Cross-origin isolation |
| Shared ring | Single-threaded, cross-origin isolated | Cross-origin isolation |
| Messages | Neither | Nothing |

Cross-origin isolation means serving the page with both headers:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Every subresource the page loads must then allow it, either same-origin or
sent with `Cross-Origin-Resource-Policy`. Without isolation, the shim falls
back to messages. Worker mode refuses to start, rather than running
without shared memory.
