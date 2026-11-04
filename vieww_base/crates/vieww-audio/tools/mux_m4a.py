#!/usr/bin/env python3
"""mux_m4a.py <raw> <asc-file> <out.m4a> <rate> <channels> <priming> <frames>

A minimal MP4 muxer for the HE-AAC fixtures: `raw` is he-aac-enc's
length-prefixed access units, the AudioSpecificConfig goes into esds
verbatim (explicit signalling survives, which ffmpeg's ADTS->MP4 path would
replace with an implicit LC config), and an edit list skips the encoder's
priming the way fdkaac/iTunes files do (a negative `priming` writes no
edit list, keeping the encoder delay in the decode). Media timescale = output rate,
2048 output samples per access unit."""
import struct, sys
raw, ascf, out, rate, chans, priming, frames = sys.argv[1:8]
rate, chans, priming, frames = int(rate), int(chans), int(priming), int(frames)
b = open(raw, "rb").read(); asc = open(ascf, "rb").read()
aus = []; i = 0
while i < len(b):
    n = struct.unpack(">I", b[i:i+4])[0]; aus.append(b[i+4:i+4+n]); i += 4 + n
def box(t, body): return struct.pack(">I", 8 + len(body)) + t + body
def full(t, v, f, body): return box(t, struct.pack(">I", (v << 24) | f) + body)
def desc(tag, body):
    n = len(body); return bytes([tag, 0x80 | (n >> 21 & 0x7f), 0x80 | (n >> 14 & 0x7f), 0x80 | (n >> 7 & 0x7f), n & 0x7f]) + body
dur = len(aus) * 2048
ftyp = box(b"ftyp", b"M4A \0\0\0\0M4A mp42isom")
mdat_body = b"".join(aus); mdat_off = len(ftyp) + 8
offs = []; o = mdat_off
for a in aus: offs.append(o); o += len(a)
dsi = desc(0x05, asc)
dcd = desc(0x04, bytes([0x40, 0x15]) + b"\0\0\0" + struct.pack(">II", 0, 0) + dsi)
esd = desc(0x03, struct.pack(">HB", 1, 0) + dcd + desc(0x06, b"\x02"))
esds = full(b"esds", 0, 0, esd)
mp4a = box(b"mp4a", b"\0" * 6 + struct.pack(">H", 1) + b"\0" * 8 + struct.pack(">HHHH", chans, 16, 0, 0) + struct.pack(">I", rate << 16) + esds)
stsd = full(b"stsd", 0, 0, struct.pack(">I", 1) + mp4a)
stts = full(b"stts", 0, 0, struct.pack(">III", 1, len(aus), 2048))
stsc = full(b"stsc", 0, 0, struct.pack(">IIII", 1, 1, 1, 1))
stsz = full(b"stsz", 0, 0, struct.pack(">II", 0, len(aus)) + b"".join(struct.pack(">I", len(a)) for a in aus))
stco = full(b"stco", 0, 0, struct.pack(">I", len(aus)) + b"".join(struct.pack(">I", x) for x in offs))
stbl = box(b"stbl", stsd + stts + stsc + stsz + stco)
minf = box(b"minf", full(b"smhd", 0, 0, b"\0" * 4) + box(b"dinf", full(b"dref", 0, 0, struct.pack(">I", 1) + full(b"url ", 0, 1, b""))) + stbl)
hdlr = full(b"hdlr", 0, 0, b"\0" * 4 + b"soun" + b"\0" * 12 + b"\0")
mdhd = full(b"mdhd", 0, 0, struct.pack(">IIII", 0, 0, rate, dur) + struct.pack(">HH", 0x55c4, 0))
mdia = box(b"mdia", mdhd + hdlr + minf)
elst = full(b"elst", 0, 0, struct.pack(">IIiHH", 1, frames, priming, 1, 0))
tkhd = full(b"tkhd", 0, 7, struct.pack(">IIIII", 0, 0, 1, 0, frames) + b"\0" * 8 + struct.pack(">hhhh", 0, 0, 0x0100, 0) + struct.pack(">9I", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000) + struct.pack(">II", 0, 0))
trak = box(b"trak", tkhd + (box(b"edts", elst) if priming >= 0 else b"") + mdia)
mvhd = full(b"mvhd", 0, 0, struct.pack(">IIII", 0, 0, rate, frames) + struct.pack(">IH", 0x10000, 0x0100) + b"\0" * 10 + struct.pack(">9I", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000) + b"\0" * 24 + struct.pack(">I", 2))
open(out, "wb").write(ftyp + box(b"mdat", mdat_body) + box(b"moov", mvhd + trak))
