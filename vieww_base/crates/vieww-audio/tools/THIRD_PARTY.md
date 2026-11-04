# Standard tables and where they came from

The codecs in `src/codec/` are written in this repository. Four of them need
fixed data that their standards define and no formula reproduces; that data
was taken from permissively licensed reference code, by the generators in
this folder, and is credited here and in each generated file's header.

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
* **Ogg Opus** (`src/codec/opus.rs`) — the container layer only
  (RFC 7845, a public IETF standard): OpusHead/OpusTags, TOC framing,
  granule arithmetic. The SILK/CELT payload (RFC 6716 §4-6) is refused
  by name and handed to the platform bridge instead. Opus is
  royalty-free under the IETF declarations its contributors filed — the
  patent posture is in [`../PATENTS.md`](../PATENTS.md) — so the day a
  pure payload decoder lands, nothing about the licence story changes.

## Patents

The credits above are about copyright. What is known about patents on
each format (MP3's expired pool, AAC's active one, and the `aac` feature
for distributors who prefer to leave the AAC decoder out) is in
[`../PATENTS.md`](../PATENTS.md).
