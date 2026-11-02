# Standard tables and where they came from

The codecs in `src/codec/` are written in this repository. Three of them need
fixed data that their standards define and no formula reproduces; that data
was taken from permissively licensed reference code, by the generators in
this folder, and is credited here and in each generated file's header.

| Generated file | Data | Source | License |
|---|---|---|---|
| `src/codec/mp3_tables.rs` | ISO/IEC 11172-3 Annex B Layer III Huffman tables | `mp3_HUFFCODE.txt`, from minimp3 (https://github.com/lieff/minimp3) | CC0 1.0 |
| `src/codec/mp3_tables.rs` | Synthesis window C[i] (Table 3-B.3) | written out in `gen_mp3_tables.py`; verified entry by entry against minimp3 | CC0 1.0 |
| `src/codec/mp3_tables.rs` | Scalefactor band widths (Table 3-B.8) | minimp3's layout | CC0 1.0 |
| `src/codec/aac_tables.rs` | ISO/IEC 14496-3 spectral and scalefactor Huffman codebooks, scalefactor-band offsets | VisualOn vo-aacenc `aacenc/src/aac_rom.c` (https://github.com/mstorsjo/vo-aacenc), Copyright 2003-2010 VisualOn, Inc. | Apache License 2.0 |
| `src/codec/vorbis.rs` (`INVERSE_DB`) | Vorbis I `floor1_inverse_dB_table` | stb_vorbis (https://github.com/nothings/stb) | Public domain / MIT |

Regenerate with:

    python3 tools/gen_mp3_tables.py tools/mp3_HUFFCODE.txt [path/to/minimp3.h] > src/codec/mp3_tables.rs
    python3 tools/gen_aac_tables.py path/to/aac_rom.c > src/codec/aac_tables.rs

Both generators check their output (prefix-code completeness, band totals,
window entries) before writing anything.

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
