//! # PNG decoding, in-house (WR-11: `assert-visual`)
//!
//! `assert-visual` must compare screenshots pixel by pixel, and
//! `wright-tools` is std-only (AGENTS.md rule 1: one dependency, and only
//! the app's `ui` feature gets it). So the decoder is built here the way
//! the crypto ports are: a small, reference-shaped implementation of the
//! two public specs it needs, pinned to fixtures from encoders this
//! repository does not contain.
//!
//! * The DEFLATE inflater follows the structure of Mark Adler's `puff.c`
//!   (the zlib reference decoder, public domain): LSB-first bit reader,
//!   canonical Huffman tables, stored / fixed / dynamic blocks, and a
//!   32 KiB window realized as "copy from the output built so far".
//!   Safe Rust; the one deliberate deviation from puff is that an
//!   *incomplete* Huffman table is accepted at build time and refused
//!   lazily — a pattern no encoder emits is an error only if the stream
//!   actually contains one (zlib rejects the same streams at build time;
//!   the verdict on corrupt data is identical).
//! * The PNG reader follows RFC 2083 (the W3C PNG spec, ISO/IEC 15948):
//!   8-byte signature, chunks with verified CRC-32, one IHDR, the IDAT
//!   chunks concatenated into ONE zlib stream, then the five scanline
//!   filters. Adler-32 is checked before unfiltering, so a tampered
//!   golden fails as *tampered*, not as "different pixels".
//!
//! What is accepted is exactly what headless Chrome writes for
//! `--screenshot`: 8-bit truecolor, with or without alpha,
//! non-interlaced. Anything else is refused with a sentence that says
//! how to regenerate the file instead of half-reading it. Both image
//! dimensions and the inflated size are capped, so a hostile PNG cannot
//! turn a comparison into a memory bomb.
//!
//! The [diff](diff) at the bottom is the metric `assert-visual` reports:
//! pixels whose channels differ beyond a tolerance (ImageMagick's AE
//! metric shape), the worst channel delta, and the total absolute error
//! — the same design as this repository's film-lab goldens (feature
//! shots re-baselined per renderer), kept integer so the verdict is
//! reproducible byte for byte.

/// One decoded image: 8-bit RGB (`channels == 3`) or RGBA (4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// 3 (truecolor) or 4 (truecolor + alpha).
    pub channels: u8,
    /// `height * width * channels` bytes, rows top to bottom.
    pub bytes: Vec<u8>,
}

impl Image {
    /// The pixel at (x, y): `channels` bytes.
    pub fn pixel(&self, x: u32, y: u32) -> &[u8] {
        let at = (y as usize * self.width as usize + x as usize) * self.channels as usize;
        &self.bytes[at..at + self.channels as usize]
    }
}

/// Hard caps: a screenshot is evidence, not a wallpaper.
const MAX_DIMENSION: u32 = 16_384;
/// Largest inflated pixel buffer accepted (RGBA of 2048x2048).
const MAX_PIXEL_BYTES: usize = 64 * 1024 * 1024;
/// Largest single chunk (IDAT streams are concatenated, each capped).
const MAX_CHUNK: usize = 64 * 1024 * 1024;

/// Decode a PNG. See the module doc for the subset accepted.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 8 || bytes[..8] != SIG {
        return Err("not a PNG (the 8-byte signature is missing)".into());
    }
    let mut at = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut channels = 0u8;
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_ihdr = false;
    let mut saw_iend = false;
    while at + 12 <= bytes.len() && !saw_iend {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if len > MAX_CHUNK {
            return Err(format!("a PNG chunk claims {len} bytes; the limit is {MAX_CHUNK}"));
        }
        let kind = &bytes[at + 4..at + 8];
        let data_at = at + 8;
        let crc_at = data_at + len;
        if crc_at + 4 > bytes.len() {
            return Err("the PNG ends in the middle of a chunk".into());
        }
        let claimed = u32::from_be_bytes(bytes[crc_at..crc_at + 4].try_into().unwrap());
        let actual = crc32(&bytes[at + 4..crc_at]);
        if claimed != actual {
            return Err(format!(
                "chunk '{}' failed its CRC check: the file was changed or truncated after it was written",
                String::from_utf8_lossy(kind)
            ));
        }
        let data = &bytes[data_at..crc_at];
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err("the IHDR chunk is not 13 bytes".into());
                }
                width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                let (depth, color, compression, filter, interlace) =
                    (data[8], data[9], data[10], data[11], data[12]);
                if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                    return Err(format!(
                        "{width}x{height} is not a size a screenshot has (limit {MAX_DIMENSION} per side)"
                    ));
                }
                if depth != 8 {
                    return Err(format!(
                        "the PNG is {depth}-bit; assert-visual reads the 8-bit truecolor PNGs headless Chrome writes — regenerate the golden with `chrome --headless --screenshot`"
                    ));
                }
                channels = match color {
                    2 => 3,
                    6 => 4,
                    other => {
                        return Err(format!(
                            "the PNG is color type {other} (grayscale, palette or gray+alpha); assert-visual reads truecolor with or without alpha — regenerate the golden with `chrome --headless --screenshot`"
                        ))
                    }
                };
                if compression != 0 || filter != 0 || interlace != 0 {
                    return Err(
                        "the PNG is not the plain compression/filter/no-interlace a screenshot has"
                            .into(),
                    );
                }
                saw_ihdr = true;
            }
            b"IDAT" => {
                if !saw_ihdr {
                    return Err("an IDAT chunk came before IHDR".into());
                }
                if idat.len() + data.len() > MAX_CHUNK {
                    return Err("the PNG's IDAT stream is past the size limit".into());
                }
                idat.extend_from_slice(data);
            }
            b"IEND" => saw_iend = true,
            // tEXt, pHYs, … : skipped, checksummed above like everything else.
            _ => {}
        }
        at = crc_at + 4;
    }
    if !saw_ihdr {
        return Err("the PNG has no IHDR chunk".into());
    }
    if !saw_iend {
        return Err("the PNG has no IEND chunk (truncated?)".into());
    }
    if idat.is_empty() {
        return Err("the PNG holds no image data (no IDAT chunks)".into());
    }
    let stride = width as usize * channels as usize;
    let expected = height as usize * (1 + stride);
    // The bomb guard: a tiny file may claim a gigantic image, but it
    // cannot inflate past this no matter what its header says.
    if expected > MAX_PIXEL_BYTES {
        return Err(format!(
            "the PNG promises {} bytes of pixel data; the limit is {MAX_PIXEL_BYTES}",
            expected
        ));
    }
    let mut raw = Vec::with_capacity(expected.min(1 << 20) + 64);
    inflate(&idat, &mut raw, expected)?;

    // Unfilter, row by row (RFC 2083 §6, filters 0-4). `raw` holds each
    // row as one filter byte followed by `stride` payload bytes.
    let mut out = vec![0u8; height as usize * stride];
    let bpp = channels as usize;
    for y in 0..height as usize {
        let row_at = y * (1 + stride);
        let filter = raw[row_at];
        let src = &raw[row_at + 1..row_at + 1 + stride];
        let (above, dst): (&[u8], &mut [u8]) = if y == 0 {
            // Rows above the first read as zero (RFC 2083 §6.3).
            (&[], &mut out[0..stride])
        } else {
            let (prev, rest) = out.split_at_mut(y * stride);
            (&prev[(y - 1) * stride..y * stride], &mut rest[..stride])
        };
        unfilter_row(filter, bpp, src, above, dst)
            .map_err(|e| format!("row {}: {e}", y + 1))?;
    }
    Ok(Image {
        width,
        height,
        channels,
        bytes: out,
    })
}

/// Undo one scanline filter in place into `dst` (RFC 2083 §6).
fn unfilter_row(
    filter: u8,
    bpp: usize,
    src: &[u8],
    above: &[u8],
    dst: &mut [u8],
) -> Result<(), String> {
    let up = |i: usize| above.get(i).copied().unwrap_or(0);
    match filter {
        0 => dst.copy_from_slice(src),
        1 => {
            for i in 0..dst.len() {
                let left = if i >= bpp { dst[i - bpp] } else { 0 };
                dst[i] = src[i].wrapping_add(left);
            }
        }
        2 => {
            for i in 0..dst.len() {
                dst[i] = src[i].wrapping_add(up(i));
            }
        }
        3 => {
            for i in 0..dst.len() {
                let left = if i >= bpp { dst[i - bpp] as u16 } else { 0 };
                dst[i] = src[i].wrapping_add(((left + up(i) as u16) / 2) as u8);
            }
        }
        4 => {
            for i in 0..dst.len() {
                let a = if i >= bpp { dst[i - bpp] as i32 } else { 0 };
                let b = up(i) as i32;
                let c = if i >= bpp { up(i - bpp) as i32 } else { 0 };
                dst[i] = src[i].wrapping_add(paeth(a, b, c) as u8);
            }
        }
        other => {
            return Err(format!(
                "filter type {other} (a PNG has 0-4; the file is corrupt)"
            ))
        }
    }
    Ok(())
}

/// The Paeth predictor (RFC 2083 §6.6, named for Alan Paeth).
fn paeth(a: i32, b: i32, c: i32) -> u8 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

// ── zlib + DEFLATE (the puff.c shape) ──────────────────────────────

/// Inflate a zlib stream (RFC 1950 wrapper + RFC 1951 data) into `out`.
/// `size_hint` is the exact expected size when the caller knows it (PNG
/// always does): the stream must produce exactly that many bytes, so a
/// header that lies about its own image is refused.
fn inflate(src: &[u8], out: &mut Vec<u8>, size_hint: usize) -> Result<(), String> {
    if src.len() < 6 {
        return Err("the zlib stream is too short to hold a header and checksum".into());
    }
    let (cmf, flg) = (src[0], src[1]);
    if cmf & 0x0f != 8 || (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err("the zlib header is not valid (CM must be 8, FCHECK must hold)".into());
    }
    if flg & 0x20 != 0 {
        return Err("the zlib stream uses a preset dictionary, which PNG forbids".into());
    }
    let mut br = BitReader {
        data: src,
        pos: 2,
        bitbuf: 0,
        bitcnt: 0,
    };
    let end = inflate_blocks(&mut br, out, size_hint)?;
    if end + 4 > src.len() {
        return Err("the zlib stream ends before its Adler-32 checksum".into());
    }
    let claimed = u32::from_be_bytes(src[end..end + 4].try_into().unwrap());
    let actual = adler32(out);
    if claimed != actual {
        return Err(
            "the zlib stream failed its Adler-32 check: the compressed data was changed after it was written"
                .into(),
        );
    }
    if out.len() != size_hint {
        return Err(format!(
            "the PNG's data is {} bytes but its header promised {}",
            out.len(),
            size_hint
        ));
    }
    Ok(())
}

/// LSB-first bit reader over the byte stream (RFC 1951 §3.1.1).
///
/// After every [`bits`] call fewer than 8 bits remain buffered, so a
/// stored block's byte alignment is always "drop what is left of one
/// byte".
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bitbuf: u64,
    bitcnt: u32,
}

impl<'a> BitReader<'a> {
    fn bits(&mut self, need: u32) -> Result<u32, String> {
        debug_assert!(need <= 16);
        while self.bitcnt < need {
            let b = *self
                .data
                .get(self.pos)
                .ok_or("the DEFLATE stream ended mid-symbol")?;
            self.pos += 1;
            self.bitbuf |= (b as u64) << self.bitcnt;
            self.bitcnt += 8;
        }
        let v = (self.bitbuf & ((1u64 << need) - 1)) as u32;
        self.bitbuf >>= need;
        self.bitcnt -= need;
        Ok(v)
    }
}

/// One canonical Huffman table (puff's `struct huffman`).
struct Huffman {
    /// Count of codes of each length 0..=15.
    count: [u16; 16],
    /// Symbols in canonical order.
    symbol: Vec<u16>,
}

/// Build a table from code lengths (puff's `construct`). Over-subscribed
/// sets are refused here; incomplete sets are accepted and refuse lazily
/// in [`decode_sym`] (see the module doc for why).
fn build_huffman(lengths: &[u8]) -> Result<Huffman, String> {
    let mut count = [0u16; 16];
    for &l in lengths {
        if l > 15 {
            return Err("a Huffman code length is over 15 bits".into());
        }
        count[l as usize] += 1;
    }
    let mut left = 1i32;
    for len in 1..16 {
        left <<= 1;
        left -= count[len] as i32;
        if left < 0 {
            return Err("an over-subscribed Huffman code (corrupt stream)".into());
        }
    }
    let mut offs = [0u16; 16];
    for len in 2..16 {
        offs[len] = offs[len - 1] + count[len - 1];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (sym, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbol[offs[l as usize] as usize] = sym as u16;
            offs[l as usize] += 1;
        }
    }
    Ok(Huffman { count, symbol })
}

/// Decode one symbol (puff's `decode`): walk the code lengths.
fn decode_sym(br: &mut BitReader<'_>, h: &Huffman) -> Result<u16, String> {
    let mut code = 0i32;
    let mut first = 0i32;
    let mut index = 0i32;
    for len in 1..16usize {
        code |= br.bits(1)? as i32;
        let count = h.count[len] as i32;
        if code - first < count {
            let at = index + (code - first);
            return h
                .symbol
                .get(at as usize)
                .copied()
                .ok_or_else(|| "an invalid Huffman code (corrupt stream)".to_string());
        }
        index += count;
        first = (first + count) << 1;
        code <<= 1;
    }
    Err("an invalid Huffman code (ran out of code lengths)".into())
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];
/// The code-length reading order (RFC 1951 §3.2.7).
const CLEN_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Inflate the DEFLATE blocks; returns the byte offset at which the
/// stream ended (the trailing Adler-32 starts there).
fn inflate_blocks(br: &mut BitReader<'_>, out: &mut Vec<u8>, cap: usize) -> Result<usize, String> {
    loop {
        let last = br.bits(1)? == 1;
        let kind = br.bits(2)?;
        match kind {
            0 => {
                // Stored: drop the rest of the current byte, LEN/NLEN, copy.
                debug_assert!(br.bitcnt < 8);
                br.bitbuf = 0;
                br.bitcnt = 0;
                let p = br.pos;
                if p + 4 > br.data.len() {
                    return Err("a stored block ends before its length".into());
                }
                let len = u16::from_le_bytes([br.data[p], br.data[p + 1]]) as usize;
                let nlen = u16::from_le_bytes([br.data[p + 2], br.data[p + 3]]);
                if (len as u16) != !nlen {
                    return Err("a stored block's LEN and NLEN disagree (corrupt stream)".into());
                }
                br.pos = p + 4;
                if br.pos + len > br.data.len() {
                    return Err("a stored block ends before its data".into());
                }
                if out.len() + len > cap {
                    return Err("the inflated data is larger than the PNG header promised".into());
                }
                out.extend_from_slice(&br.data[br.pos..br.pos + len]);
                br.pos += len;
            }
            1 => {
                // Fixed Huffman (RFC 1951 §3.2.6).
                let mut lengths = [0u8; 288];
                for (i, l) in lengths.iter_mut().enumerate() {
                    *l = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let lit = build_huffman(&lengths)?;
                let dist = build_huffman(&[5u8; 30])?;
                inflate_block(br, out, cap, &lit, &dist)?;
            }
            2 => {
                // Dynamic Huffman (RFC 1951 §3.2.7).
                let hlit = br.bits(5)? as usize + 257;
                let hdist = br.bits(5)? as usize + 1;
                let hclen = br.bits(4)? as usize + 4;
                if hlit > 286 || hdist > 30 {
                    return Err("a dynamic block declares more codes than DEFLATE has".into());
                }
                let mut clen = [0u8; 19];
                for &pos in CLEN_ORDER.iter().take(hclen) {
                    clen[pos] = br.bits(3)? as u8;
                }
                let cl = build_huffman(&clen)?;
                let mut lengths = vec![0u8; hlit + hdist];
                let mut i = 0usize;
                while i < lengths.len() {
                    let sym = decode_sym(br, &cl)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return Err("a code-length repeat with nothing to repeat".into());
                            }
                            let n = 3 + br.bits(2)? as usize;
                            if i + n > lengths.len() {
                                return Err("a code-length repeat runs past the tables".into());
                            }
                            let prev = lengths[i - 1];
                            for _ in 0..n {
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 | 18 => {
                            let n = if sym == 17 {
                                3 + br.bits(3)? as usize
                            } else {
                                11 + br.bits(7)? as usize
                            };
                            if i + n > lengths.len() {
                                return Err("a zero-run runs past the tables".into());
                            }
                            i += n;
                        }
                        _ => return Err("an invalid code-length symbol".into()),
                    }
                }
                let lit = build_huffman(&lengths[..hlit])?;
                let dist = build_huffman(&lengths[hlit..])?;
                inflate_block(br, out, cap, &lit, &dist)?;
            }
            _ => return Err("a reserved DEFLATE block type (corrupt stream)".into()),
        }
        if last {
            // Fewer than 8 bits can be buffered after any bits() call, so
            // what is left is the padding of the current byte; the
            // Adler-32 starts at the next byte boundary.
            debug_assert!(br.bitcnt < 8);
            return Ok(br.pos);
        }
    }
}

/// One Huffman-compressed block's LZ loop (puff's `codes`).
fn inflate_block(
    br: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    cap: usize,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let sym = decode_sym(br, lit)?;
        if sym < 256 {
            if out.len() + 1 > cap {
                return Err("the inflated data is larger than the PNG header promised".into());
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let li = (sym - 257) as usize;
            if li >= LENGTH_BASE.len() {
                return Err("an invalid length symbol".into());
            }
            let len = LENGTH_BASE[li] as usize + br.bits(LENGTH_EXTRA[li] as u32)? as usize;
            let dsym = decode_sym(br, dist)? as usize;
            if dsym >= DIST_BASE.len() {
                return Err("an invalid distance symbol".into());
            }
            let back = DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
            if back > out.len() {
                return Err(
                    "a match reaches before the start of the output (corrupt stream)".into(),
                );
            }
            if out.len() + len > cap {
                return Err("the inflated data is larger than the PNG header promised".into());
            }
            let from = out.len() - back;
            for k in 0..len {
                let b = out[from + k];
                out.push(b);
            }
        }
    }
}

// ── checksums ─────────────────────────────────────────────────────

/// CRC-32 (the PNG chunk checksum, ISO 3309 / ITU-T V.42, reflected
/// polynomial 0xEDB88320). Bitwise — images are megabytes, not gigabytes.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Adler-32 (RFC 1950 §8.2.2).
pub fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

// ── the assert-visual metric ──────────────────────────────────────

/// The result of comparing two images with a per-channel tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    /// Pixels where any channel differs by more than the tolerance.
    pub differing: u64,
    /// Total pixels compared.
    pub total: u64,
    /// The worst single-channel delta anywhere in the image.
    pub max_delta: u8,
    /// Sum of every channel delta (the total absolute error).
    pub total_delta: u64,
}

impl Diff {
    /// True when the images match under the tolerance.
    pub fn ok(&self) -> bool {
        self.differing == 0
    }
}

/// Compare pixel by pixel. Sizes must match (the caller reports them);
/// the tolerance is the per-channel delta allowed (0 = exact).
pub fn diff(a: &Image, b: &Image, tolerance: u8) -> Result<Diff, String> {
    if a.width != b.width || a.height != b.height || a.channels != b.channels {
        return Err(format!(
            "the images are {}x{} ({} channels) and {}x{} ({} channels); there is nothing to compare",
            a.width, a.height, a.channels, b.width, b.height, b.channels
        ));
    }
    let (mut differing, mut max_delta, mut total_delta) = (0u64, 0u8, 0u64);
    let tol = tolerance as i32;
    for y in 0..a.height {
        for x in 0..a.width {
            let (pa, pb) = (a.pixel(x, y), b.pixel(x, y));
            let mut pixel_differs = false;
            for (&va, &vb) in pa.iter().zip(pb.iter()) {
                let d = (va as i32 - vb as i32).abs();
                max_delta = max_delta.max(d as u8);
                total_delta += d as u64;
                if d > tol {
                    pixel_differs = true;
                }
            }
            if pixel_differs {
                differing += 1;
            }
        }
    }
    Ok(Diff {
        differing,
        total: a.width as u64 * a.height as u64,
        max_delta,
        total_delta,
    })
}

// ── tests ─────────────────────────────────────────────────────────
//
// A tiny in-test PNG encoder that writes STORED deflate blocks (zlib's
// no-compression mode) exercises every filter path with no external
// tool; `testdata/gradient.png` is a real zlib-compressed PNG written by
// an independent encoder (CPython's zlib, level 9), pinned below with
// its exact pixels, so the dynamic-Huffman path is covered by data this
// repository never produced itself.

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG with the given IHDR color type, each row = filter byte +
    /// raw payload, compressed as one stored deflate block.
    fn png_with_color(width: u32, height: u32, color: u8, rows: &[Vec<u8>]) -> Vec<u8> {
        let mut raw = Vec::new();
        for r in rows {
            raw.push(r[0]);
            raw.extend_from_slice(&r[1..]);
        }
        // zlib: header, one final stored block, adler32.
        let mut z = vec![0x78, 0x01, 0x01];
        z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        z.extend_from_slice(&raw);
        z.extend_from_slice(&adler32(&raw).to_be_bytes());
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, color, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    /// Truecolor (RGB) variant of [`png_with_color`].
    fn stored_png(width: u32, height: u32, rows: &[Vec<u8>]) -> Vec<u8> {
        png_with_color(width, height, 2, rows)
    }

    fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc_input = Vec::new();
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }

    /// Where the IDAT chunk's type bytes sit in `png`.
    fn idat_at(png: &[u8]) -> usize {
        (0..png.len().saturating_sub(4))
            .find(|&i| &png[i..i + 4] == b"IDAT")
            .expect("an IDAT chunk")
    }

    #[test]
    fn decodes_a_stored_png_with_the_none_sub_up_filters() {
        // 2x2 RGB: row 0 filter none, row 1 sub, row 2 up (height 3 in
        // the second test; here 2 rows exercise none + sub).
        let rows = vec![
            vec![0, 10, 20, 30, 11, 21, 31],
            vec![1, 1, 1, 1, 2, 2, 2], // sub: pixel0 raw, pixel1 += pixel0
            vec![2, 1, 1, 1, 1, 1, 1], // up: += the row above
        ];
        let png = stored_png(2, 3, &rows);
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height, img.channels), (2, 3, 3));
        assert_eq!(img.pixel(0, 0), &[10, 20, 30]);
        assert_eq!(img.pixel(1, 0), &[11, 21, 31]);
        // Row 1 (sub): (1,1,1) then (2+1, 2+1, 2+1)
        assert_eq!(img.pixel(0, 1), &[1, 1, 1]);
        assert_eq!(img.pixel(1, 1), &[3, 3, 3]);
        // Row 2 (up): row above + (1,1,1)
        assert_eq!(img.pixel(0, 2), &[2, 2, 2]);
        assert_eq!(img.pixel(1, 2), &[4, 4, 4]);
    }

    #[test]
    fn average_and_paeth_filters_decode() {
        // 1 pixel per row, 3 rows: none, average, paeth.
        let rows = vec![
            vec![0, 100, 100, 100],
            vec![3, 10, 10, 10], // avg(a=0, b=100) = 50 -> 60
            vec![4, 5, 5, 5],    // paeth(a=0, b=60, c=0) = b -> 65
        ];
        let png = stored_png(1, 3, &rows);
        let img = decode(&png).unwrap();
        assert_eq!(img.pixel(0, 0), &[100, 100, 100]);
        assert_eq!(img.pixel(0, 1), &[60, 60, 60]);
        assert_eq!(img.pixel(0, 2), &[65, 65, 65]);
    }

    #[test]
    fn a_real_zlib_png_decodes_byte_for_byte() {
        let png = include_bytes!("testdata/gradient.png");
        let img = decode(png).unwrap();
        assert_eq!((img.width, img.height, img.channels), (8, 4, 3));
        for y in 0..4u32 {
            for x in 0..8u32 {
                assert_eq!(
                    img.pixel(x, y),
                    &[(x * 30) as u8, (y * 60) as u8, 128u8],
                    "pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn tampering_is_caught_by_a_checksum() {
        let png = stored_png(1, 1, &[vec![0, 1, 2, 3]]);
        // Break the CRC: flip a byte inside IHDR's data.
        let mut bad = png.clone();
        bad[16 + 4] ^= 0xff;
        assert!(decode(&bad).unwrap_err().contains("CRC"));
        // Break the Adler-32 while keeping the chunk CRC right, so the
        // failure is the zlib stream's own checksum, not the chunk's.
        let at = idat_at(&png);
        let data_at = at + 4;
        let mut bad = png.clone();
        bad[data_at + 7] ^= 0xff; // first payload byte of the stored block
        let mut crc_input = Vec::new();
        crc_input.extend_from_slice(&bad[at..at + 4]);
        crc_input.extend_from_slice(&bad[data_at..data_at + 15]);
        let crc = crc32(&crc_input).to_be_bytes();
        bad[data_at + 15..data_at + 19].copy_from_slice(&crc);
        assert!(decode(&bad).unwrap_err().contains("Adler"));
        // Truncation inside the last chunk.
        assert!(decode(&png[..png.len() - 5])
            .unwrap_err()
            .contains("truncated"));
    }

    #[test]
    fn unsupported_pngs_are_refused_with_sentences() {
        // Grayscale (color type 0), written as a legal PNG.
        let png = png_with_color(1, 1, 0, &[vec![0, 1, 2, 3]]);
        assert!(decode(&png).unwrap_err().contains("regenerate"));
        // Not a PNG at all.
        assert!(
            decode(b"hello world, definitely not a png!!")
                .unwrap_err()
                .contains("not a PNG")
        );
    }

    #[test]
    fn the_diff_counts_beyond_tolerance() {
        let mk = |bytes: &[u8]| Image {
            width: 2,
            height: 1,
            channels: 3,
            bytes: bytes.to_vec(),
        };
        let a = mk(&[10, 10, 10, 10, 10, 10]);
        let b = mk(&[10, 10, 10, 13, 10, 10]); // one channel +3
        let exact = diff(&a, &b, 0).unwrap();
        assert_eq!((exact.differing, exact.max_delta, exact.total_delta), (1, 3, 3));
        assert!(!exact.ok());
        let within = diff(&a, &b, 3).unwrap();
        assert!(within.ok() && within.differing == 0);
        assert_eq!(within.max_delta, 3, "the worst delta is still reported");
        // Sizes differ: refused, never half-compared.
        let c = Image {
            width: 1,
            height: 1,
            channels: 3,
            bytes: vec![0, 0, 0],
        };
        assert!(diff(&a, &c, 0).is_err());
        // Identical images.
        assert!(diff(&a, &a, 0).unwrap().ok());
    }
}
