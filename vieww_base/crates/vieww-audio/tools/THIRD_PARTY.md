# Standard tables and where they came from

The codecs in `src/codec/` are written in this repository — with one
exception, the Opus payload decoder, which is a port of the reference
implementation and is credited in its own section below. Four of the
others need fixed data that their standards define and no formula
reproduces; that data was taken from permissively licensed reference code,
by the generators in this folder, and is credited here and in each
generated file's header.

| Generated file | Data | Source | License |
|---|---|---|---|
| `src/codec/mp3_tables.rs` | ISO/IEC 11172-3 Annex B Layer III Huffman tables | `mp3_HUFFCODE.txt`, from minimp3 (https://github.com/lieff/minimp3) | CC0 1.0 |
| `src/codec/mp3_tables.rs` | Synthesis window C[i] (Table 3-B.3) | written out in `gen_mp3_tables.py`; verified entry by entry against minimp3 | CC0 1.0 |
| `src/codec/mp3_tables.rs` | Scalefactor band widths (Table 3-B.8) | minimp3's layout | CC0 1.0 |
| `src/codec/aac_tables.rs` | ISO/IEC 14496-3 spectral and scalefactor Huffman codebooks, scalefactor-band offsets | VisualOn vo-aacenc `aacenc/src/aac_rom.c` (https://github.com/mstorsjo/vo-aacenc), Copyright 2003-2010 VisualOn, Inc. | Apache License 2.0 |
| `src/codec/sbr_tables.rs` | ISO/IEC 14496-3 HE-AAC data: QMF prototype window (Table 4.A.89), SBR noise table V (4.A.88), the ten SBR envelope/noise Huffman books (4.A.6), the ten PS Huffman books and hybrid-filter prototypes (8.A) | transcribed from libfaad2 (`libfaad/sbr_*.{c,h}`, `ps_*.{c,h}`, as vendored by the faad2-sys 0.1.0 crate), Copyright Nero AG / M. Bakker; **and verified identical** to the Fraunhofer FDK AAC sources (fdk-aac-sys 0.5.0's vendored `libSBRdec`, `libFDK`) — Huffman books code for code, the noise table and the QMF window to FDK's fixed-point rounding | libfaad2: GPL-2.0-or-later; FDK: the FDK AAC licence — see the note below |
| `src/codec/vorbis.rs` (`INVERSE_DB`) | Vorbis I `floor1_inverse_dB_table` | stb_vorbis (https://github.com/nothings/stb) | Public domain / MIT |

Regenerate with:

    python3 tools/gen_mp3_tables.py tools/mp3_HUFFCODE.txt [path/to/minimp3.h] > src/codec/mp3_tables.rs
    python3 tools/gen_aac_tables.py path/to/aac_rom.c > src/codec/aac_tables.rs
    python3 tools/gen_sbr_tables.py path/to/faad2/libfaad [path/to/fdk-aac] > src/codec/sbr_tables.rs
    python3 tools/gen_opus_tables.py path/to/libopus celt > src/codec/opusdec/celt/tables.rs
    gcc -w -DOPUS_BUILD -I$O/include -I$O/silk -I$O/celt -I$O -o /tmp/gen_silk \
        tools/gen_silk_tables.c && /tmp/gen_silk > src/codec/opusdec/silk/tables.rs   # O=path/to/libopus

The generators check their output (prefix-code completeness, band totals,
window entries) before writing anything.

**The HE-AAC tables' licence posture is the owner's to confirm.** What
was taken is numeric data the standard itself specifies — a 640-tap
window, a 512-entry complex noise table, Huffman code trees — and nothing
else: no code, and nothing that has a closed form (the frequency-band
tables, the PS mixing matrices, the phase and fractional-delay tables, the
filter modulation are all computed from their defining formulas in
`sbr.rs` / `ps.rs`). Both implementations it was checked against carry
copyleft or bespoke licences (libfaad2 GPL, FDK its own), and neither
permissive pure-Rust crate found was an independent source (both are
ports of ffmpeg, LGPL). The working position here is that the numeric
tables of an ISO standard are the standard's, not an implementation's
expression; if counsel disagrees, the remedy is a clean transcription from
the ISO text itself into this generator's output format, which the FDK
cross-check would then verify unchanged.

## Formats that took no third-party data

Two decoders consult nothing generated here, and their posture is worth
writing down:

* **ALAC** (`src/codec/alac.rs`) — parameter-free bitstream: the rice
  parameters ride in every file's magic cookie, and no table the standard
  defines is larger than a shift. The decoder was written from the
  bitstream the published descriptions describe — David Hammerton's 2005
  public-domain reverse-engineering notes and Apple's Apache-2.0
  reference decoder (https://github.com/macosforge/alac) — and verified
  bit-exactly against ffmpeg's own decode of the same files. No code or
  table was copied; the reference was the oracle.
* **Ogg Opus, the container** (`src/codec/opus.rs`) — RFC 7845, a public
  IETF standard: OpusHead/OpusTags, TOC and multistream framing, granule
  arithmetic. Written from the RFC; no code or data copied.

## The Opus payload decoder: a port of libopus

`src/codec/opusdec/` (SILK, CELT, the Opus mode-switching layer and the
multistream decoder) is a line-by-line Rust port of the **libopus 1.3.1
decoder** — the reference implementation RFC 6716 itself publishes — as
vendored by the `audiopus_sys` 0.2.2 crate. It is a derived work of that
code, not an independent implementation, and is distributed under its
licence:

* **Licence:** BSD-3-Clause. Copyright 2001-2011 Xiph.Org, Skype Limited,
  Octasic, Jean-Marc Valin, Timothy B. Terriberry, CSIRO, Gregory Maxwell,
  Mark Borgerding, Erik de Castro Lopo. The full notice and conditions are
  reproduced in [`LICENSE-libopus.txt`](LICENSE-libopus.txt); a binary
  distribution must carry that notice in its documentation (the clause
  every BSD dependency already imposes — include it with the app's other
  open-source notices).
* **What was ported:** the decoder only — `silk/dec_API.c` and the
  SILK decode path (indices, pulses, NLSF/LPC, LTP synthesis, PLC, CNG,
  stereo unmixing, the 48 kHz resampler), CELT's `celt_decoder.c` with
  the range decoder, band/PVQ/allocation code, MDCT and FFT (float build),
  `opus_decoder.c` and `opus_multistream_decoder.c`. No encoder, no
  `dnn/`, no SIMD.
* **Tables:** `opusdec/celt/tables.rs` by `gen_opus_tables.py` and
  `opusdec/silk/tables.rs` by `gen_silk_tables.c`, both straight from
  libopus's own sources (the C generator compiles libopus's table files
  and prints them), so every constant is the reference decoder's.
* **How it is graded:** `tools/opus-ref` (libopus via `audiopus_sys`,
  test-only, never linked into the crate) encodes the fixtures and
  decodes them with `opus_multistream_decode_float`; the tests require
  every packet's final range-coder state to equal libopus's (RFC 6716
  §6's conformance fingerprint), SILK output to match sample for sample,
  and CELT/hybrid output to libopus's own 16-bit rounding. ffmpeg's
  independent decoder agrees with it at 88 dB on `src.opus`.

Opus is royalty-free under the IETF declarations its contributors filed;
the patent posture is in [`../PATENTS.md`](../PATENTS.md).

## Patents

The credits above are about copyright. What is known about patents on
each format (MP3's expired pool, AAC's active one, and the `aac` feature
for distributors who prefer to leave the AAC decoder out) is in
[`../PATENTS.md`](../PATENTS.md).
