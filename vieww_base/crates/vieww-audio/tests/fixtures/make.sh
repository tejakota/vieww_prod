#!/usr/bin/env bash
# Regenerates the codec fixtures. Needs python3 and ffmpeg (with libmp3lame,
# libvorbis, libopus, the native aac encoder and the native alac encoder).
# The source is deterministic (a chirp, tones and seeded noise), so the
# fixtures are reproducible — with one footnote: the Ogg muxer picks Opus's
# page serial numbers at random, so `src.opus` differs from run to run in
# 24 serial/CRC bytes and nothing else (every payload byte, and every
# `.ref` decode, is identical). The `.ref` files are ffmpeg's own decodes
# (signed 16-bit little-endian, interleaved — `src24m_alac.m4a.ref` is
# 24-bit big-endian, matching the lossless stream's own depth) and are what
# the decoders are compared against.
set -euo pipefail
cd "$(dirname "$0")"
python3 - <<'PY'
import math, random, struct, wave
rate, n = 44100, 17640  # 0.4 s
random.seed(7)
lp = 0.0
L, R = [], []
for i in range(n):
    t = i / rate
    f = 100 + (8000 - 100) * t / 0.4
    phase = 2 * math.pi * (100 * t + (8000 - 100) * t * t / 0.8)
    l = 0.4 * math.sin(phase) + 0.1 * math.sin(2 * math.pi * 440 * t)
    lp += (random.uniform(-1, 1) - lp) * 0.2
    r = 0.3 * math.sin(2 * math.pi * 660 * t) + 0.15 * lp
    L.append(l); R.append(r)
with wave.open("source16.wav", "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes(b"".join(struct.pack("<hh", round(a * 32767), round(b * 32767)) for a, b in zip(L, R)))
with wave.open("source24m.wav", "wb") as w:
    w.setnchannels(1); w.setsampwidth(3); w.setframerate(rate)
    w.writeframes(b"".join(struct.pack("<i", round(a * 8388607))[:3] for a in L))
PY
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
ff -i source16.wav -c:a flac -compression_level 8 src16.flac
ff -i source24m.wav -c:a flac -compression_level 5 src24m.flac
ff -i source16.wav -c:a libmp3lame -b:a 128k -joint_stereo 1 src.mp3
ff -i source16.wav -ac 1 -c:a libmp3lame -q:a 4 src_mono_vbr.mp3
ff -i source16.wav -c:a libvorbis -q:a 4 src.ogg
# AAC with perceptual noise substitution off, so the decode is comparable
# sample-for-sample (PNS bands are, by definition, decoder-generated
# noise); and one ADTS stream with PNS on, graded by band energy instead.
ff -i source16.wav -c:a aac -aac_pns 0 -b:a 128k src.m4a
ff -i source16.wav -ac 1 -c:a aac -aac_pns 0 -b:a 64k -f adts src_mono.aac
ff -i source16.wav -ac 1 -c:a aac -b:a 64k -f adts src_mono_pns.aac
ff -i source16.wav -c:a pcm_s24be src24.aiff
# ALAC (Apple Lossless) — lossless, so the refs must match sample-for-sample.
# The encoder keeps the source depth: 16-bit stereo, and 24-bit mono straight
# from the 24-bit WAV.
ff -i source16.wav -c:a alac src_alac.m4a
ff -i source24m.wav -c:a alac src24m_alac.m4a
# Ogg Opus — the container fixture: pre-skip, TOC durations, granule
# arithmetic, and the by-name payload refusal the platform bridge matches.
ff -i source16.wav -c:a libopus -b:a 96k src.opus
for f in src.mp3 src_mono_vbr.mp3 src.ogg src.opus src.m4a src_mono.aac src_mono_pns.aac src_alac.m4a; do
  ff -i "$f" -f s16le -c:a pcm_s16le "$f.ref"
done
ff -i src24m_alac.m4a -f s24be -c:a pcm_s24be src24m_alac.m4a.ref
# Tagged files, one per container, for the tag readers (tests/tags.rs):
# a tenth of a second each, with Unicode text and a custom key.
ff -t 0.1 -i source16.wav -c:a pcm_s16le tagsrc.wav
meta=(-metadata "title=Vātāpi Gaṇapatim" -metadata artist=Ensemble -metadata album=Kritis
      -metadata date=2026 -metadata genre=Carnatic -metadata track=3/8
      -metadata "comment=tagged by ffmpeg" -metadata RAGA=Hamsadhwani)
ff -i tagsrc.wav "${meta[@]}" -c:a libmp3lame -b:a 64k -id3v2_version 4 -write_id3v1 1 tags24.mp3
ff -i tagsrc.wav "${meta[@]}" -c:a libmp3lame -b:a 64k -id3v2_version 3 tags23.mp3
ff -i tagsrc.wav "${meta[@]}" -c:a flac tags.flac
ff -i tagsrc.wav "${meta[@]}" -c:a libvorbis tags.ogg
ff -i tagsrc.wav "${meta[@]}" -c:a libopus -b:a 32k tags.opus
ff -i tagsrc.wav "${meta[@]}" -c:a aac -b:a 64k tags.m4a
ff -i tagsrc.wav "${meta[@]}" -c:a pcm_s16le tags.wav
ff -i tagsrc.wav "${meta[@]}" -c:a pcm_s16be -write_id3v2 1 tags.aiff
rm tagsrc.wav
