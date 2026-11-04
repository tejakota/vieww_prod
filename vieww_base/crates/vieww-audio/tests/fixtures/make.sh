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
# Ogg Opus — ffmpeg's encode, graded against ffmpeg's own decoder (a
# second oracle beside libopus, below).
ff -i source16.wav -c:a libopus -b:a 96k src.opus
# HE-AAC (AU-06): SBR and SBR+PS, ADTS (implicit signalling, as ADTS must)
# and MP4 (explicit hierarchical signalling — mux_m4a.py keeps the
# encoder's AudioSpecificConfig, where ffmpeg's ADTS->MP4 path would
# rewrite it as implicit LC). FDK is deterministic, so these regenerate
# byte-identically. The refs are ffmpeg's decodes like every other `.ref`;
# tools/he-aac-enc's `he-aac-dec` is the FDK decoder, a second oracle
# (it agrees with ffmpeg's PS decode to ~75 dB).
cargo build -q --release --manifest-path ../../tools/he-aac-enc/Cargo.toml --target-dir ../../../../target/he-aac-enc
he=../../../../target/he-aac-enc/release/he-aac-enc
ff -i source16.wav -ac 1 he_mono.wav
"$he" source16.wav src_he.aac 5 48000 adts
"$he" source16.wav src_hev2.aac 29 32000 adts
"$he" he_mono.wav src_he_mono.aac 5 32000 adts
for p in "src_he 5 48000" "src_hev2 29 32000"; do
  set -- $p
  "$he" source16.wav "$1.raw" "$2" "$3" asc
  python3 ../../tools/mux_m4a.py "$1.raw" "$1.raw.asc" "$1.m4a" 44100 2 -1 17640
  rm "$1.raw" "$1.raw.asc"
done
rm he_mono.wav
ff -i src_he_mono.aac -ac 1 -f s16le -c:a pcm_s16le src_he_mono.aac.ref
for f in src.mp3 src_mono_vbr.mp3 src.ogg src.opus src.m4a src_mono.aac src_mono_pns.aac src_alac.m4a \
         src_he.aac src_hev2.aac src_he.m4a src_hev2.m4a; do
  ff -i "$f" -f s16le -c:a pcm_s16le "$f.ref"
done
ff -i src24m_alac.m4a -f s24be -c:a pcm_s24be src24m_alac.m4a.ref
# Opus conformance fixtures (AU-06): encoded by libopus with forced modes,
# bandwidths and frame sizes, decoded by libopus's float API — `.ref` its
# output rounded to s16le, `.rng` every packet's final range state (u32le),
# the bit-exactness fingerprint. tools/opus-ref is its own workspace and
# fixes the page serial, so these regenerate byte-identically.
cargo build -q --release --manifest-path ../../tools/opus-ref/Cargo.toml --target-dir ../../../../target/opus-ref
or=../../../../target/opus-ref/release/opus-ref
"$or" enc opus_silk.opus 2 "6,silk,nb,12000,20;6,silk,mb,16000,20;6,silk,wb,24000,10;3,silk,wb,24000,40;2,silk,nb,12000,60;5,silk,wb,24000,20,mono;6,silk,wb,24000,20,fec"
"$or" enc opus_celt.opus 2 "40,celt,fb,64000,2.5;20,celt,swb,48000,5;10,celt,wb,32000,10;5,celt,nb,16000,20;5,celt,fb,256000,20;5,celt,fb,96000,20,mono;5,celt,fb,24000,20,cbr"
"$or" enc opus_switch.opus 2 "6,hybrid,swb,32000,20;6,hybrid,fb,48000,10;5,celt,fb,64000,20;5,silk,wb,24000,20;5,celt,fb,64000,10;5,hybrid,fb,40000,20,mono;5,silk,nb,12000,20;8,celt,swb,48000,5;5,hybrid,swb,32000,20" 800
"$or" enc opus_dtx.opus 1 "10,silk,wb,16000,20,dtx;25,silk,wb,16000,20,dtx,silent;10,silk,wb,16000,20,dtx;10,hybrid,swb,24000,20,dtx,silent;5,silk,wb,16000,20"
"$or" enc opus_51.opus 6 "10,auto,fb,256000,20;5,celt,fb,192000,10"
"$or" enc long.opus 2 "150,auto,fb,24000,20;100,auto,fb,48000,20;200,celt,fb,32000,10;100,silk,wb,16000,40;150,auto,fb,32000,20"
for f in opus_silk opus_celt opus_switch opus_dtx opus_51; do "$or" decf $f.opus $f.opus.ref s16; done
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
