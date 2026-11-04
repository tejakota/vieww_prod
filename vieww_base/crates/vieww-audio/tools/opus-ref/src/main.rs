//! `opus-ref enc <out.opus> <channels> <spec>` — encode the deterministic
//! test signal with libopus following `spec`, a `;`-separated list of
//! segments `frames,mode,bandwidth,bitrate,frame_ms[,opt...]`
//! (mode `silk|hybrid|celt|auto`; bandwidth `nb|mb|wb|swb|fb`; options
//! `mono` (force one coded channel), `cbr`, `fec` (in-band FEC at 20 %
//! loss), `dtx`, `silent` (the segment's input zeroed)), into an Ogg Opus file;
//! an optional fifth argument is the header's output gain (Q7.8 dB).
//! Above two channels the surround multistream encoder (mapping family 1)
//! is used.
//!
//! `opus-ref dec <in.opus> <out.s16le>` — libopus's own decode (int16
//! API, soft clip and all) of an Ogg
//! Opus file with the container's rules applied (pre-skip dropped, output
//! gain, the last page's granule trimming), interleaved s16le at 48 kHz.

use std::ffi::c_int;

use audiopus_sys as o;

const RATE: usize = 48_000;

fn signal(channels: usize, frames: usize) -> Vec<i16> {
    // Voiced pulses through two formants (SILK's material), a sweeping
    // tone pair and noise (CELT's), and castanet-like clicks (transients,
    // short blocks) — the same every run.
    let mut seed: u32 = 0x0905_2026;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0
    };
    let mut out = Vec::with_capacity(frames * channels);
    let (mut f1a, mut f1b, mut f2a, mut f2b) = (0.0f64, 0.0, 0.0, 0.0);
    let mut lp = 0.0;
    for n in 0..frames {
        let t = n as f64 / RATE as f64;
        let pitch = 110.0 + 40.0 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
        let pulse = if (t * pitch).fract() < pitch / RATE as f64 {
            1.0
        } else {
            0.0
        };
        // Two resonators (700 Hz and 1.9 kHz, crude formants).
        let res = |x: f64, a: &mut f64, b: &mut f64, f: f64| {
            let r = 0.995f64;
            let c = 2.0 * r * (2.0 * std::f64::consts::PI * f / RATE as f64).cos();
            let y = x + c * *a - r * r * *b;
            *b = *a;
            *a = y;
            y
        };
        let v1 = res(pulse, &mut f1a, &mut f1b, 700.0);
        let v2 = res(pulse, &mut f2a, &mut f2b, 1_900.0);
        let speech = 0.02 * (v1 + 0.6 * v2) * if (t * 2.0).fract() < 0.7 { 1.0 } else { 0.05 };
        let sweep = 0.15 * (2.0 * std::f64::consts::PI * (300.0 * t + 2_000.0 * t * t)).sin();
        lp += (rnd() - lp) * 0.3;
        let click = if (t * 3.0).fract() < 0.0015 {
            rnd() * 0.8
        } else {
            0.0
        };
        let l = speech + sweep * 0.7 + 0.05 * lp + click;
        let r = speech * 0.6
            + 0.12 * (2.0 * std::f64::consts::PI * 5_500.0 * t).sin()
            + 0.08 * rnd()
            + click * 0.5;
        let q = |v: f64| (v.clamp(-1.0, 1.0) * 32_000.0) as i16;
        out.push(q(l));
        if channels >= 2 {
            out.push(q(r));
        }
        // Surround: each further channel a distinct mix (centre, LFE-ish
        // low tone, rears), so every coded stream carries real content.
        for c in 2..channels {
            let v = match c {
                2 => 0.5 * (l + r),
                3 => 0.3 * (2.0 * std::f64::consts::PI * 60.0 * t).sin(),
                _ => {
                    if c % 2 == 0 {
                        0.6 * l - 0.2 * r
                    } else {
                        0.6 * r - 0.2 * l
                    }
                }
            };
            out.push(q(v));
        }
    }
    out
}

fn crc(data: &[u8]) -> u32 {
    let mut c = 0u32;
    for &b in data {
        c ^= u32::from(b) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04c1_1db7
            } else {
                c << 1
            };
        }
    }
    c
}

fn page(out: &mut Vec<u8>, packets: &[Vec<u8>], granule: i64, seq: u32, flags: u8) {
    let mut lacing = Vec::new();
    for p in packets {
        let mut n = p.len();
        while n >= 255 {
            lacing.push(255u8);
            n -= 255;
        }
        lacing.push(n as u8);
    }
    let mut h = b"OggS".to_vec();
    h.push(0);
    h.push(flags);
    h.extend(granule.to_le_bytes());
    h.extend(0x7655_0001u32.to_le_bytes());
    h.extend(seq.to_le_bytes());
    h.extend([0; 4]);
    h.push(lacing.len() as u8);
    h.extend(&lacing);
    for p in packets {
        h.extend(p);
    }
    let c = crc(&h);
    h[22..26].copy_from_slice(&c.to_le_bytes());
    out.extend(h);
}

/// The encoder behind `enc`: one stream (mapping family 0) for 1–2
/// channels, libopus's surround multistream encoder (family 1) above.
enum Enc {
    One(*mut o::OpusEncoder),
    Ms(*mut o::OpusMSEncoder),
}

unsafe fn ectl(e: &Enc, req: c_int, v: c_int) {
    match e {
        Enc::One(e) => ctl(*e, req, v),
        Enc::Ms(e) => assert_eq!(o::opus_multistream_encoder_ctl(*e, req, v), 0, "ctl {req}"),
    }
}

unsafe fn ctl(e: *mut o::OpusEncoder, req: c_int, v: c_int) {
    assert_eq!(o::opus_encoder_ctl(e, req, v), 0, "ctl {req} {v}");
}

fn enc(path: &str, channels: usize, spec: &str, gain_q8: i16) {
    let segs: Vec<Vec<&str>> = spec.split(';').map(|s| s.split(',').collect()).collect();
    let total: usize = segs
        .iter()
        .map(|s| s[0].parse::<usize>().unwrap() * (s[4].parse::<f64>().unwrap() * 48.0) as usize)
        .sum();
    let mut pcm = signal(channels, total);
    // `silent` segments: digital silence (drives DTX, CNG and the SILK
    // concealment the 1-byte DTX packets ask for).
    let mut at0 = 0usize;
    for s in &segs {
        let n = s[0].parse::<usize>().unwrap() * (s[4].parse::<f64>().unwrap() * 48.0) as usize;
        if s.contains(&"silent") {
            pcm[at0 * channels..(at0 + n) * channels].fill(0);
        }
        at0 += n;
    }
    let mut file = Vec::new();
    let mut err = 0;
    unsafe {
        let mut lookahead: c_int = 0;
        let (mut streams, mut coupled) = (0 as c_int, 0 as c_int);
        let mut mapping = [0u8; 8];
        let e = if channels <= 2 {
            let e = o::opus_encoder_create(RATE as i32, channels as c_int, 2049, &mut err);
            assert_eq!(err, 0);
            assert_eq!(
                o::opus_encoder_ctl(e, 4027, &mut lookahead as *mut c_int),
                0
            );
            Enc::One(e)
        } else {
            let e = o::opus_multistream_surround_encoder_create(
                RATE as i32,
                channels as c_int,
                1,
                &mut streams,
                &mut coupled,
                mapping.as_mut_ptr(),
                2049,
                &mut err,
            );
            assert_eq!(err, 0);
            assert_eq!(
                o::opus_multistream_encoder_ctl(e, 4027, &mut lookahead as *mut c_int),
                0
            );
            Enc::Ms(e)
        };
        let mut head = b"OpusHead".to_vec();
        head.push(1);
        head.push(channels as u8);
        head.extend((lookahead as u16).to_le_bytes());
        head.extend((RATE as u32).to_le_bytes());
        head.extend(gain_q8.to_le_bytes());
        if channels <= 2 {
            head.push(0);
        } else {
            head.push(1);
            head.push(streams as u8);
            head.push(coupled as u8);
            head.extend_from_slice(&mapping[..channels]);
        }
        page(&mut file, &[head], 0, 0, 2);
        let mut tags = b"OpusTags".to_vec();
        tags.extend(7u32.to_le_bytes());
        tags.extend(b"opusref");
        tags.extend(0u32.to_le_bytes());
        page(&mut file, &[tags], 0, 1, 0);
        let mut seq = 2;
        let mut at = 0usize;
        let mut granule: i64 = 0;
        let mut buf = vec![0u8; 16000];
        let nsegs = segs.len();
        for (si, s) in segs.iter().enumerate() {
            let frames: usize = s[0].parse().unwrap();
            let mode = match s[1] {
                "silk" => 1000,
                "hybrid" => 1001,
                "celt" => 1002,
                _ => -1000,
            };
            let bw = match s[2] {
                "nb" => 1101,
                "mb" => 1102,
                "wb" => 1103,
                "swb" => 1104,
                _ => 1105,
            };
            let br: c_int = s[3].parse().unwrap();
            let ms: f64 = s[4].parse().unwrap();
            let fs = (ms * 48.0) as usize;
            let dur = match s[4] {
                "2.5" => 5001,
                "5" => 5002,
                "10" => 5003,
                "20" => 5004,
                "40" => 5005,
                "60" => 5006,
                _ => panic!("frame ms"),
            };
            ectl(&e, 11002, mode);
            ectl(&e, 4008, bw);
            ectl(&e, 4002, br);
            ectl(&e, 4040, dur);
            if channels <= 2 {
                ectl(&e, 4022, if s.contains(&"mono") { 1 } else { -1000 });
            }
            ectl(&e, 4006, if s.contains(&"cbr") { 0 } else { 1 });
            let fec = s.contains(&"fec");
            ectl(&e, 4012, i32::from(fec));
            ectl(&e, 4014, if fec { 20 } else { 0 });
            ectl(&e, 4016, i32::from(s.contains(&"dtx")));
            for f in 0..frames {
                let src = pcm.as_ptr().add(at * channels);
                let n = match e {
                    Enc::One(e) => {
                        o::opus_encode(e, src, fs as c_int, buf.as_mut_ptr(), buf.len() as i32)
                    }
                    Enc::Ms(e) => o::opus_multistream_encode(
                        e,
                        src,
                        fs as c_int,
                        buf.as_mut_ptr(),
                        buf.len() as i32,
                    ),
                };
                assert!(n > 0, "encode {n}");
                at += fs;
                granule += fs as i64;
                let last = si + 1 == nsegs && f + 1 == frames;
                // One packet per page keeps granule bookkeeping exact.
                let end = if last { granule - 0 } else { granule };
                page(
                    &mut file,
                    &[buf[..n as usize].to_vec()],
                    end,
                    seq,
                    if last { 4 } else { 0 },
                );
                seq += 1;
            }
        }
        match e {
            Enc::One(e) => o::opus_encoder_destroy(e),
            Enc::Ms(e) => o::opus_multistream_encoder_destroy(e),
        }
        eprintln!("{path}: {total} frames, pre-skip {lookahead}");
    }
    std::fs::write(path, file).unwrap();
}

fn ogg_packets(b: &[u8]) -> (Vec<Vec<u8>>, i64) {
    let (mut out, mut cur, mut last_granule) = (Vec::new(), Vec::new(), 0i64);
    let mut at = 0;
    while at + 27 <= b.len() {
        assert_eq!(&b[at..at + 4], b"OggS");
        let granule = i64::from_le_bytes(b[at + 6..at + 14].try_into().unwrap());
        let nseg = b[at + 26] as usize;
        let lacing = &b[at + 27..at + 27 + nseg];
        let mut p = at + 27 + nseg;
        for &l in lacing {
            cur.extend_from_slice(&b[p..p + l as usize]);
            p += l as usize;
            if l < 255 {
                out.push(std::mem::take(&mut cur));
            }
        }
        if granule >= 0 {
            last_granule = granule;
        }
        at = p;
    }
    (out, last_granule)
}

fn dec(input: &str, output: &str) {
    let b = std::fs::read(input).unwrap();
    let (packets, last) = ogg_packets(&b);
    let head = &packets[0];
    let channels = head[9] as usize;
    let preskip = u16::from_le_bytes([head[10], head[11]]) as usize;
    let gain = i16::from_le_bytes([head[16], head[17]]);
    let mut err = 0;
    let mut pcm = Vec::new();
    unsafe {
        let d = o::opus_decoder_create(RATE as i32, channels as c_int, &mut err);
        assert_eq!(err, 0);
        if gain != 0 {
            assert_eq!(o::opus_decoder_ctl(d, 4034, gain as c_int), 0);
        }
        let mut buf = vec![0i16; 5760 * channels];
        for p in &packets[2..] {
            let n = o::opus_decode(d, p.as_ptr(), p.len() as i32, buf.as_mut_ptr(), 5760, 0);
            assert!(n >= 0, "decode {n}");
            pcm.extend_from_slice(&buf[..n as usize * channels]);
        }
        o::opus_decoder_destroy(d);
    }
    let end = (last as usize).saturating_sub(preskip) * channels;
    let body: Vec<i16> = pcm[preskip * channels..]
        .iter()
        .copied()
        .take(end)
        .collect();
    let bytes: Vec<u8> = body.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(output, bytes).unwrap();
    eprintln!(
        "{input}: {} ch, pre-skip {preskip}, {} frames",
        channels,
        body.len() / channels
    );
}

/// `decf <in> <out> [s16]`: as `dec`, but `opus_decode_float` (no soft
/// clip), written as f32le — or, with `s16`, rounded to s16le with
/// clamping (the test fixtures' `.ref` format) — plus `<out>.rng`: each
/// packet's final range-coder state, u32le, the bit-exactness
/// fingerprint. Channel mapping families 1/255 go through the
/// multistream decoder.
fn decf(input: &str, output: &str, s16: bool) {
    let b = std::fs::read(input).unwrap();
    let (packets, last) = ogg_packets(&b);
    let head = &packets[0];
    let channels = head[9] as usize;
    let preskip = u16::from_le_bytes([head[10], head[11]]) as usize;
    let gain = i16::from_le_bytes([head[16], head[17]]);
    let family = head[18];
    let (streams, coupled, mapping): (c_int, c_int, Vec<u8>) = if family == 0 {
        (
            1,
            (channels == 2) as c_int,
            if channels == 2 { vec![0, 1] } else { vec![0] },
        )
    } else {
        (
            head[19] as c_int,
            head[20] as c_int,
            head[21..21 + channels].to_vec(),
        )
    };
    let mut err = 0;
    let mut pcm = Vec::new();
    let mut rngs = Vec::new();
    unsafe {
        let d = o::opus_multistream_decoder_create(
            RATE as i32,
            channels as c_int,
            streams,
            coupled,
            mapping.as_ptr(),
            &mut err,
        );
        assert_eq!(err, 0);
        if gain != 0 {
            assert_eq!(o::opus_multistream_decoder_ctl(d, 4034, gain as c_int), 0);
        }
        let mut buf = vec![0f32; 5760 * channels];
        for p in &packets[2..] {
            let n = o::opus_multistream_decode_float(
                d,
                p.as_ptr(),
                p.len() as i32,
                buf.as_mut_ptr(),
                5760,
                0,
            );
            assert!(n >= 0, "decode {n}");
            pcm.extend_from_slice(&buf[..n as usize * channels]);
            let mut r: u32 = 0;
            o::opus_multistream_decoder_ctl(d, 4031, &mut r as *mut u32);
            rngs.extend(r.to_le_bytes());
        }
        o::opus_multistream_decoder_destroy(d);
    }
    let end = (last as usize).saturating_sub(preskip) * channels;
    let body: Vec<f32> = pcm[preskip * channels..]
        .iter()
        .copied()
        .take(end)
        .collect();
    let bytes: Vec<u8> = if s16 {
        body.iter()
            .flat_map(|v| ((v * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes())
            .collect()
    } else {
        body.iter().flat_map(|s| s.to_le_bytes()).collect()
    };
    std::fs::write(output, bytes).unwrap();
    let rng_path = if s16 {
        output.trim_end_matches(".ref").to_owned() + ".rng"
    } else {
        format!("{output}.rng")
    };
    std::fs::write(rng_path, rngs).unwrap();
    eprintln!(
        "{input}: {} ch, pre-skip {preskip}, {} frames",
        channels,
        body.len() / channels
    );
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "enc" => enc(
            &a[2],
            a[3].parse().unwrap(),
            &a[4],
            a.get(5).map_or(0, |g| g.parse().unwrap()),
        ),
        "dec" => dec(&a[2], &a[3]),
        "decf" => decf(&a[2], &a[3], a.get(4).is_some_and(|v| v == "s16")),
        _ => panic!("enc|dec"),
    }
}
