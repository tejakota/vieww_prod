//! Live backends under load: telemetry end to end.
//!
//! Needs running sound servers (as the `audio-backends` workflow starts
//! them: `jackd -d dummy`, `pulseaudio` with a null sink), so it is
//! ignored by default. Run with
//!
//! `VIEWW_SOAK_SECS=10 cargo test --release -p vieww-audio --test live_backends -- --ignored --nocapture`
//!
//! `VIEWW_LIVE_GAIN` (default 0.2) scales what is played — 0 streams
//! silence, every frame still rendered and delivered (unattended runs on a
//! desktop's real speakers).
//!
//! For each Linux server backend: a 16-voice synth through a reverb is
//! played for `VIEWW_SOAK_SECS` (default 5) seconds while every core is
//! saturated by spinning threads, and the stream's own statistics are
//! printed and checked — the callback's scheduling class, render load,
//! renders that overran their block, and xruns the server reported.
//!
//! `alsa_seq_virtual_ports_round_trip` needs none of that — only the
//! kernel's sequencer (`sudo modprobe snd-seq`; the audio-backends VM leg
//! boots one). With `VIEWW_REQUIRE_SEQ=1` a missing sequencer fails the
//! test instead of skipping it, which is how the VM leg asks.

#![cfg(all(target_os = "linux", not(target_os = "android")))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vieww_audio::backend::{jack::Jack, pulse::Pulse, Backend, BlockInfo, StreamConfig};
use vieww_audio::fx::reverb::Freeverb;
use vieww_audio::fx::Stereo;
use vieww_audio::synth::voice::{Poly, SubtractiveVoice};

fn soak_secs() -> u64 {
    std::env::var("VIEWW_SOAK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
}

fn run(backend: &dyn Backend) {
    let rate = 48_000.0;
    let mut poly = Poly::new((0..16).map(|_| SubtractiveVoice::new(rate)).collect(), rate);
    for k in 0..16u8 {
        poly.note_on(0, 36 + k * 2, 0.6);
    }
    let mut verb = Freeverb::new(0.8, 0.3, rate);
    let gain: f32 = std::env::var("VIEWW_LIVE_GAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.2);
    let nonfinite = Arc::new(Mutex::new(0usize));
    let nf = Arc::clone(&nonfinite);
    let render = Box::new(move |out: &mut [f32], info: &BlockInfo| {
        let ch = info.channels.max(1);
        for f in out.chunks_mut(ch) {
            let (l, r) = poly.next_stereo();
            let (l, r) = verb.process_stereo(l, r);
            f[0] = l * gain;
            if ch > 1 {
                f[1] = r * gain;
            }
        }
        if out.iter().any(|v| !v.is_finite()) {
            if let Ok(mut n) = nf.try_lock() {
                *n += 1;
            }
        }
    });
    let stream = match backend.open_output(
        StreamConfig {
            rate: 48_000,
            channels: 2,
            block: 256,
        },
        render,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: unavailable ({e:?}) — skipped", backend.name());
            return;
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    let load: Vec<_> = (0..cores)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 1u64;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                }
                x
            })
        })
        .collect();
    std::thread::sleep(Duration::from_secs(soak_secs()));
    stop.store(true, Ordering::Relaxed);
    for l in load {
        let _ = l.join();
    }
    let st = stream.stats();
    drop(stream);

    eprintln!(
        "{:>6}: {} callbacks, {:.1} s of audio | callback thread {:?} | render mean {:?}, max {:?}, peak load {:.0}% | {} late renders | {} xruns",
        backend.name(),
        st.callbacks,
        st.frames as f64 / 48_000.0,
        st.priority,
        st.render_mean,
        st.render_max,
        st.load_peak * 100.0,
        st.late,
        st.xruns,
    );
    assert!(st.callbacks > 10, "{}: the stream ran", backend.name());
    assert!(
        st.priority.is_some(),
        "{}: priority recorded",
        backend.name()
    );
    assert!(st.render_max >= st.render_mean);
    assert_eq!(*nonfinite.lock().unwrap(), 0);
}

#[test]
#[ignore = "needs running JACK / PulseAudio servers"]
fn jack_under_load() {
    run(&Jack::default());
}

#[test]
#[ignore = "needs running JACK / PulseAudio servers"]
fn pulse_under_load() {
    run(&Pulse);
}

/// Hot-plug, for real: a resilient stream on a dedicated PulseAudio null
/// sink survives the sink being removed and loaded again, with the same
/// render closure (its frame count carries on).
#[test]
#[ignore = "needs a running PulseAudio server and pactl"]
fn pulse_sink_unplug_and_replug() {
    use std::process::Command;
    use std::sync::atomic::AtomicU64;
    use vieww_audio::backend::resilient::{ResilientStream, Target};

    let pactl = |args: &[&str]| {
        Command::new("pactl")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let load = || pactl(&["load-module", "module-null-sink", "sink_name=vieww-hotplug"]);
    let Some(mut module) = load() else {
        eprintln!("no pactl / server — skipped");
        return;
    };
    let frames = Arc::new(AtomicU64::new(0));
    let f2 = Arc::clone(&frames);
    let rs = ResilientStream::open(
        Box::new(Pulse),
        Target::Named {
            id: "vieww-hotplug".into(),
            fallback_after: None,
        },
        StreamConfig::default(),
        Box::new(move |out: &mut [f32], info: &BlockInfo| {
            f2.fetch_add(info.frames as u64, Ordering::Relaxed);
            out.fill(0.0);
        }),
    )
    .expect("open on the dedicated sink");
    let wait_for_frames = |more: u64, secs: u64| {
        let start = frames.load(Ordering::Relaxed);
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(secs) {
            if frames.load(Ordering::Relaxed) >= start + more {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    assert!(wait_for_frames(48_000, 10), "playing");
    for round in 0..2 {
        pactl(&["unload-module", &module]).expect("unplug");
        // The server moves the stream to its fallback sink rather than
        // ending it: no silence while the device is away.
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(
            rs.reopens(),
            round,
            "round {round}: not reopened while away"
        );
        module = load().expect("replug");
        let t = std::time::Instant::now();
        while rs.reopens() == round && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            rs.reopens(),
            round + 1,
            "round {round}: back on the named sink"
        );
        assert!(
            wait_for_frames(48_000, 15),
            "round {round}: playback resumed"
        );
        let inputs = pactl(&["list", "sink-inputs"]).unwrap_or_default();
        let sinks = pactl(&["list", "sinks", "short"]).unwrap_or_default();
        let ours = sinks
            .lines()
            .find(|l| l.contains("vieww-hotplug"))
            .and_then(|l| l.split_whitespace().next())
            .expect("sink listed");
        assert!(
            inputs.lines().any(|l| l.trim() == format!("Sink: {ours}")),
            "round {round}: a stream plays on vieww-hotplug ({ours})"
        );
    }
    assert!(!rs.following_default(), "it waited for its own device");
    drop(rs);
    pactl(&["unload-module", &module]);
}

/// AU-09 on a real server: the default sink moves while a default-following
/// stream plays. The server moves the stream itself (no reopen, no gap);
/// the route event reaches the supervisor within 100 ms, not the second a
/// poll would take; and the stream is on the new default afterwards.
#[test]
#[ignore = "needs a running PulseAudio server and pactl"]
fn pulse_default_sink_change_is_an_event() {
    use std::process::Command;
    use std::sync::atomic::AtomicU64;
    use vieww_audio::backend::resilient::{ResilientStream, Target};

    let pactl = |args: &[&str]| {
        Command::new("pactl")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let Some(a) = pactl(&["load-module", "module-null-sink", "sink_name=vieww-route-a"]) else {
        eprintln!("no pactl / server — skipped");
        return;
    };
    let b = pactl(&["load-module", "module-null-sink", "sink_name=vieww-route-b"])
        .expect("second sink");
    let before = pactl(&["get-default-sink"]).unwrap_or_default();
    pactl(&["set-default-sink", "vieww-route-a"]).expect("default a");
    let frames = Arc::new(AtomicU64::new(0));
    let f2 = Arc::clone(&frames);
    let rs = ResilientStream::open(
        Box::new(Pulse),
        Target::Default,
        StreamConfig::default(),
        Box::new(move |out: &mut [f32], info: &BlockInfo| {
            f2.fetch_add(info.frames as u64, Ordering::Relaxed);
            out.fill(0.0);
        }),
    )
    .expect("open on the default sink");
    let t = std::time::Instant::now();
    while frames.load(Ordering::Relaxed) < 24_000 && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    // Which sink our stream (the only sink input) is on.
    let sink_of_stream = || {
        let sinks = pactl(&["list", "short", "sinks"]).unwrap_or_default();
        let inputs = pactl(&["list", "short", "sink-inputs"]).unwrap_or_default();
        let idx = inputs.lines().last()?.split_whitespace().nth(1)?.to_owned();
        sinks
            .lines()
            .find(|l| l.split_whitespace().next() == Some(idx.as_str()))
            .and_then(|l| l.split_whitespace().nth(1))
            .map(str::to_owned)
    };
    assert_eq!(
        sink_of_stream().as_deref(),
        Some("vieww-route-a"),
        "playing on a"
    );
    let events = rs.route_events();
    let reopens = rs.reopens();
    let t = std::time::Instant::now();
    pactl(&["set-default-sink", "vieww-route-b"]).expect("default b");
    while rs.route_events() == events && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(1));
    }
    let took = t.elapsed();
    eprintln!(
        "route event after {took:?} ({} event(s))",
        rs.route_events() - events
    );
    assert!(
        rs.route_events() > events,
        "no route event for the default sink moving"
    );
    assert!(
        took < Duration::from_millis(100),
        "the route event took {took:?}"
    );
    let t = std::time::Instant::now();
    while sink_of_stream().as_deref() != Some("vieww-route-b")
        && t.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        sink_of_stream().as_deref(),
        Some("vieww-route-b"),
        "the stream followed the default"
    );
    let at = frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        frames.load(Ordering::Relaxed) > at,
        "still playing after the move"
    );
    assert_eq!(
        rs.reopens(),
        reopens,
        "the server moved it: nothing reopened"
    );
    drop(rs);
    if !before.is_empty() {
        pactl(&["set-default-sink", &before]);
    }
    pactl(&["unload-module", &b]);
    pactl(&["unload-module", &a]);
}

// ---- the ALSA sequencer (AU-19) ---------------------------------------------

/// AU-19, live: the sequencer's virtual ports, end to end through the
/// kernel. A virtual input (`vieww-audio:vieww-rt-in`) and a virtual
/// output (`vieww-audio:vieww-rt-out`) are published; each is found in
/// the port lists under its `seq:C,P` id; and messages cross both ways —
/// a subscribed output writing into the virtual input, and the virtual
/// output broadcasting to a subscribed input — including a SysEx, whose
/// bytes ride the sequencer's variable-length event path. When the ports
/// drop, they leave the lists again.
#[test]
#[ignore = "needs the ALSA sequencer (sudo modprobe snd-seq)"]
fn alsa_seq_virtual_ports_round_trip() {
    use vieww_audio::midi::Message;
    use vieww_audio::midi_io::{input_ports, output_ports, MidiInput, MidiOutput};

    let sent = vec![
        Message::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        },
        Message::ControlChange {
            channel: 3,
            controller: 74,
            value: 12,
        },
        Message::PitchBend {
            channel: 1,
            value: -4_096,
        },
        Message::SysEx(vec![0x7D, 0x01, 0x02, 0x03, 0x04]),
        Message::TimingClock,
        Message::NoteOff {
            channel: 0,
            key: 60,
            velocity: 0,
        },
    ];
    // A note-off sent as velocity 0 may arrive as either spelling.
    let norm = |m: &Message| match m {
        Message::NoteOn {
            channel,
            key,
            velocity: 0,
        } => Message::NoteOff {
            channel: *channel,
            key: *key,
            velocity: 0,
        },
        m => m.clone(),
    };
    let want: Vec<Message> = sent.iter().map(norm).collect();

    let require = std::env::var("VIEWW_REQUIRE_SEQ").is_ok_and(|v| v == "1");
    // A virtual input: a destination other applications connect to.
    let input = match MidiInput::open_virtual("vieww-rt-in") {
        Ok(i) => i,
        Err(e) => {
            assert!(!require, "the sequencer is required but missing: {e}");
            eprintln!("no ALSA sequencer — skipped ({e})");
            return;
        }
    };
    // A virtual output: a source other applications connect to.
    let mut vout = MidiOutput::open_virtual("vieww-rt-out").expect("publish the virtual output");

    // The lists carry both, under the client `vieww-audio`, by `seq:` id:
    // a virtual input is a MIDI *destination*, a virtual output a source.
    let in_id = output_ports()
        .into_iter()
        .find(|p| p.name == "vieww-audio:vieww-rt-in")
        .expect("the virtual input is listed as an output port")
        .id;
    let out_id = input_ports()
        .into_iter()
        .find(|p| p.name == "vieww-audio:vieww-rt-out")
        .expect("the virtual output is listed as an input port")
        .id;
    assert!(
        in_id.starts_with("seq:") && out_id.starts_with("seq:"),
        "{in_id} {out_id}"
    );

    // Both ways: a subscribed output writes into the virtual input; the
    // virtual output broadcasts to a subscribed input.
    let mut to_in = MidiOutput::open(&in_id).expect("subscribe an output to the virtual input");
    let from_out = MidiInput::open(&out_id).expect("subscribe an input to the virtual output");
    // Let the subscriptions settle before the first event, as
    // midi_port_round_trip does.
    std::thread::sleep(Duration::from_millis(100));

    for (label, send, recv) in [
        ("into the virtual input", &mut to_in, &input),
        ("from the virtual output", &mut vout, &from_out),
    ] {
        for m in &sent {
            send.send(m).expect("send");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut timed = Vec::new();
        let t = std::time::Instant::now();
        while timed.len() < sent.len() && t.elapsed() < Duration::from_secs(5) {
            timed.extend(recv.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        let got: Vec<Message> = timed.iter().map(|t| norm(&t.message)).collect();
        assert_eq!(got, want, "{label}: every message, in order");
        // Arrival timestamps: monotonic, and the clock has not run
        // backwards past them.
        let times: Vec<f64> = timed.iter().map(|t| t.time).collect();
        assert!(
            times.windows(2).all(|w| w[0] <= w[1]),
            "{label}: times not monotonic: {times:?}"
        );
        assert!(
            recv.now() >= *times.last().unwrap_or(&0.0),
            "{label}: now {} before the last arrival {}",
            recv.now(),
            times.last().copied().unwrap_or(0.0)
        );
    }

    // Dropping the ports withdraws them from the lists again.
    drop(from_out);
    drop(to_in);
    drop(vout);
    drop(input);
    let gone = |f: fn() -> Vec<vieww_audio::midi_io::MidiPort>, name: &str| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if !f().iter().any(|p| p.name == name) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    assert!(
        gone(output_ports, "vieww-audio:vieww-rt-in"),
        "the virtual input left the list"
    );
    assert!(
        gone(input_ports, "vieww-audio:vieww-rt-out"),
        "the virtual output left the list"
    );
}
