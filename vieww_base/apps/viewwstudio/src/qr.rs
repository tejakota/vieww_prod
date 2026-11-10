//! A QR code encoder, for the share link a phone scans.
//!
//! # Why this exists
//!
//! `preview --share` serves the screen to the local network, and the person
//! sharing it is at a laptop holding a phone. Typing `http://192.168.1.23:4321`
//! on a phone keyboard is the step that turns "try it on your phone" into
//! "never mind", so the page and the terminal show a code instead.
//!
//! Byte mode, error correction level L, versions 1–10 (up to 271 bytes — a
//! URL, with room to spare), all eight masks scored by the standard's four
//! penalty rules. Written here rather than added as a dependency for the
//! reason the studio carries one third-party crate: a few hundred lines of
//! arithmetic is not worth a second.

/// A finished code: `size`×`size` modules, `true` is dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrCode {
    pub size: usize,
    modules: Vec<bool>,
}

/// Raw codewords per version (1–10).
const RAW: [usize; 10] = [26, 44, 70, 100, 134, 172, 196, 242, 292, 346];
/// Error-correction codewords per block at level L.
const ECC: [usize; 10] = [7, 10, 15, 20, 26, 18, 20, 24, 30, 18];
/// Blocks at level L.
const BLOCKS: [usize; 10] = [1, 1, 1, 1, 1, 2, 2, 2, 2, 4];
/// Alignment pattern centres per version.
const ALIGN: [&[usize]; 10] = [
    &[],
    &[6, 18],
    &[6, 22],
    &[6, 26],
    &[6, 30],
    &[6, 34],
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
];

impl QrCode {
    /// Encode `data` with the smallest version that holds it and the mask the
    /// penalty rules prefer.
    ///
    /// # Errors
    ///
    /// The data is longer than version 10 holds.
    pub fn encode(data: &[u8]) -> Result<Self, String> {
        Self::encode_with(data, None)
    }

    /// As [`Self::encode`], with the mask forced (for tests against a
    /// reference encoder).
    ///
    /// # Errors
    ///
    /// As [`Self::encode`].
    pub fn encode_with(data: &[u8], mask: Option<u8>) -> Result<Self, String> {
        let version = (1..=10)
            .find(|&v| {
                let count_bits = if v < 10 { 8 } else { 16 };
                let capacity = (RAW[v - 1] - ECC[v - 1] * BLOCKS[v - 1]) * 8;
                4 + count_bits + data.len() * 8 <= capacity
            })
            .ok_or_else(|| format!("{} bytes is more than a QR code here holds", data.len()))?;
        let codewords = codewords(data, version);
        let mut best: Option<(u32, Self)> = None;
        for m in 0..8_u8 {
            if mask.is_some_and(|forced| forced != m) {
                continue;
            }
            let code = build(version, &codewords, m);
            let score = code.penalty();
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, code));
            }
        }
        best.map(|(_, c)| c)
            .ok_or_else(|| "no mask was tried".to_owned())
    }

    /// Whether the module at column `x`, row `y` is dark.
    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }

    /// The code as text for a terminal, two rows per line in half blocks,
    /// with a four-module quiet zone. Light modules are drawn and dark ones
    /// left blank, so it reads on the dark background most terminals have.
    #[must_use]
    pub fn to_text(&self) -> String {
        let quiet = 4_isize;
        let n = self.size as isize;
        let dark = |x: isize, y: isize| {
            x >= 0 && y >= 0 && x < n && y < n && self.get(x as usize, y as usize)
        };
        let mut out = String::new();
        let mut y = -quiet;
        while y < n + quiet {
            for x in -quiet..n + quiet {
                let top = !dark(x, y);
                let bottom = !dark(x, y + 1);
                out.push(match (top, bottom) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                });
            }
            out.push('\n');
            y += 2;
        }
        out
    }

    /// The code as an SVG, one unit per module, with a quiet zone.
    #[must_use]
    pub fn to_svg(&self) -> String {
        let quiet = 4;
        let total = self.size + quiet * 2;
        let mut path = String::new();
        for y in 0..self.size {
            for x in 0..self.size {
                if self.get(x, y) {
                    path.push_str(&format!("M{},{}h1v1h-1z", x + quiet, y + quiet));
                }
            }
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {total} {total}\" \
             shape-rendering=\"crispEdges\"><rect width=\"{total}\" height=\"{total}\" \
             fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
        )
    }

    /// The standard's four penalty rules; lower reads better.
    fn penalty(&self) -> u32 {
        let n = self.size;
        let mut score = 0_u32;
        // 1: runs of five or more in a row or column.
        for horizontal in [true, false] {
            for a in 0..n {
                let mut run = 1;
                for b in 1..n {
                    let (cur, prev) = if horizontal {
                        (self.get(b, a), self.get(b - 1, a))
                    } else {
                        (self.get(a, b), self.get(a, b - 1))
                    };
                    if cur == prev {
                        run += 1;
                        if run == 5 {
                            score += 3;
                        } else if run > 5 {
                            score += 1;
                        }
                    } else {
                        run = 1;
                    }
                }
            }
        }
        // 2: 2×2 blocks of one colour.
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let c = self.get(x, y);
                if c == self.get(x + 1, y) && c == self.get(x, y + 1) && c == self.get(x + 1, y + 1)
                {
                    score += 3;
                }
            }
        }
        // 3: finder-like 1:1:3:1:1 patterns with four light modules beside.
        let pattern = [true, false, true, true, true, false, true];
        for horizontal in [true, false] {
            for a in 0..n {
                for b in 0..n.saturating_sub(6) {
                    // Along the line, at an absolute position; off the
                    // code counts as light, the quiet zone.
                    let along = |p: isize| {
                        p >= 0
                            && p < n as isize
                            && if horizontal {
                                self.get(p as usize, a)
                            } else {
                                self.get(a, p as usize)
                            }
                    };
                    let start = b as isize;
                    if (0..7).all(|k| along(start + k) == pattern[k as usize]) {
                        let light = |range: std::ops::Range<isize>| {
                            range.into_iter().all(|k| !along(start + k))
                        };
                        if light(-4..0) || light(7..11) {
                            score += 40;
                        }
                    }
                }
            }
        }
        // 4: balance of dark and light.
        let dark = self.modules.iter().filter(|&&d| d).count();
        let percent = dark * 100 / (n * n);
        let deviation = (percent as i32 - 50).unsigned_abs() / 5;
        score + deviation * 10
    }
}

/// The data bitstream, split into blocks with their error correction, and
/// interleaved — the codewords in placement order.
fn codewords(data: &[u8], version: usize) -> Vec<u8> {
    let raw = RAW[version - 1];
    let ecc = ECC[version - 1];
    let blocks = BLOCKS[version - 1];
    let capacity = raw - ecc * blocks;

    let mut bits: Vec<bool> = Vec::with_capacity(capacity * 8);
    let mut push = |value: usize, count: usize| {
        for i in (0..count).rev() {
            bits.push((value >> i) & 1 == 1);
        }
    };
    push(0b0100, 4);
    push(data.len(), if version < 10 { 8 } else { 16 });
    for &byte in data {
        push(usize::from(byte), 8);
    }
    let limit = capacity * 8;
    let terminator = (limit - bits.len()).min(4);
    bits.extend(std::iter::repeat_n(false, terminator));
    while !bits.len().is_multiple_of(8) {
        bits.push(false);
    }
    let mut bytes: Vec<u8> = bits
        .chunks(8)
        .map(|c| c.iter().fold(0_u8, |acc, &b| (acc << 1) | u8::from(b)))
        .collect();
    let mut pad = [0xEC_u8, 0x11].into_iter().cycle();
    while bytes.len() < capacity {
        bytes.push(pad.next().unwrap_or(0xEC));
    }

    let short_blocks = blocks - raw % blocks;
    let short_len = raw / blocks;
    let divisor = rs_divisor(ecc);
    let mut data_blocks: Vec<Vec<u8>> = Vec::with_capacity(blocks);
    let mut ecc_blocks: Vec<Vec<u8>> = Vec::with_capacity(blocks);
    let mut offset = 0;
    for i in 0..blocks {
        let len = short_len - ecc + usize::from(i >= short_blocks);
        let block = bytes[offset..offset + len].to_vec();
        offset += len;
        ecc_blocks.push(rs_remainder(&block, &divisor));
        data_blocks.push(block);
    }
    let mut out = Vec::with_capacity(raw);
    let longest = data_blocks.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for block in &data_blocks {
            if let Some(&b) = block.get(i) {
                out.push(b);
            }
        }
    }
    for i in 0..ecc {
        for block in &ecc_blocks {
            out.push(block[i]);
        }
    }
    out
}

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0_u8;
    while b != 0 {
        if b & 1 == 1 {
            product ^= a;
        }
        let carry = a & 0x80 != 0;
        a <<= 1;
        if carry {
            a ^= 0x1D;
        }
        b >>= 1;
    }
    product
}

fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut result = vec![0_u8; degree];
    result[degree - 1] = 1;
    let mut root = 1_u8;
    for _ in 0..degree {
        for j in 0..degree {
            result[j] = gf_mul(result[j], root);
            if j + 1 < degree {
                result[j] ^= result[j + 1];
            }
        }
        root = gf_mul(root, 0x02);
    }
    result
}

fn rs_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut result = vec![0_u8; divisor.len()];
    for &byte in data {
        let factor = byte ^ result.remove(0);
        result.push(0);
        for (r, &d) in result.iter_mut().zip(divisor) {
            *r ^= gf_mul(d, factor);
        }
    }
    result
}

/// Lay out a version with its codewords under mask `mask`.
fn build(version: usize, codewords: &[u8], mask: u8) -> QrCode {
    let size = version * 4 + 17;
    let mut modules = vec![false; size * size];
    let mut function = vec![false; size * size];
    let set = |m: &mut Vec<bool>, f: &mut Vec<bool>, x: usize, y: usize, dark: bool| {
        m[y * size + x] = dark;
        f[y * size + x] = true;
    };

    // Timing patterns.
    for i in 0..size {
        set(&mut modules, &mut function, 6, i, i % 2 == 0);
        set(&mut modules, &mut function, i, 6, i % 2 == 0);
    }
    // Finders, with their separators.
    for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
        for dy in -4_isize..=4 {
            for dx in -4_isize..=4 {
                let (x, y) = (cx as isize + dx, cy as isize + dy);
                if x < 0 || y < 0 || x >= size as isize || y >= size as isize {
                    continue;
                }
                let dist = dx.abs().max(dy.abs());
                set(
                    &mut modules,
                    &mut function,
                    x as usize,
                    y as usize,
                    dist != 2 && dist != 4,
                );
            }
        }
    }
    // Alignment patterns, except where they would sit on a finder.
    let centres = ALIGN[version - 1];
    let last = centres.len().saturating_sub(1);
    for (i, &ax) in centres.iter().enumerate() {
        for (j, &ay) in centres.iter().enumerate() {
            if (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0) {
                continue;
            }
            for dy in -2_isize..=2 {
                for dx in -2_isize..=2 {
                    set(
                        &mut modules,
                        &mut function,
                        (ax as isize + dx) as usize,
                        (ay as isize + dy) as usize,
                        dx.abs().max(dy.abs()) != 1,
                    );
                }
            }
        }
    }
    // Reserve the format areas (drawn per mask below) and draw the version.
    draw_format(&mut modules, &mut function, size, 0);
    if version >= 7 {
        let mut rem = version as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
        }
        let bits = (version as u32) << 12 | rem;
        for i in 0..18 {
            let dark = (bits >> i) & 1 == 1;
            let a = size - 11 + i % 3;
            let b = i / 3;
            set(&mut modules, &mut function, a, b, dark);
            set(&mut modules, &mut function, b, a, dark);
        }
    }

    // Data, in the zigzag.
    let total_bits = codewords.len() * 8;
    let mut i = 0;
    let mut right = size as isize - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = ((right + 1) & 2) == 0;
                let y = if upward { size - 1 - vert } else { vert };
                if !function[y * size + x] && i < total_bits {
                    modules[y * size + x] = (codewords[i >> 3] >> (7 - (i & 7))) & 1 == 1;
                    i += 1;
                }
            }
        }
        right -= 2;
    }

    // The mask, over data modules only.
    for y in 0..size {
        for x in 0..size {
            if function[y * size + x] {
                continue;
            }
            let invert = match mask {
                0 => (x + y) % 2 == 0,
                1 => y % 2 == 0,
                2 => x % 3 == 0,
                3 => (x + y) % 3 == 0,
                4 => (x / 3 + y / 2) % 2 == 0,
                5 => x * y % 2 + x * y % 3 == 0,
                6 => (x * y % 2 + x * y % 3) % 2 == 0,
                _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
            };
            if invert {
                modules[y * size + x] ^= true;
            }
        }
    }
    draw_format(&mut modules, &mut function, size, mask);
    QrCode { size, modules }
}

/// The format information (level L, `mask`) in both of its places.
fn draw_format(modules: &mut [bool], function: &mut [bool], size: usize, mask: u8) {
    let data = (1_u32 << 3) | u32::from(mask);
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    let bits = ((data << 10) | rem) ^ 0x5412;
    let bit = |i: u32| (bits >> i) & 1 == 1;
    let mut set = |x: usize, y: usize, dark: bool| {
        modules[y * size + x] = dark;
        function[y * size + x] = true;
    };
    for i in 0..=5 {
        set(8, i as usize, bit(i));
    }
    set(8, 7, bit(6));
    set(8, 8, bit(7));
    set(7, 8, bit(8));
    for i in 9..15 {
        set(14 - i as usize, 8, bit(i));
    }
    for i in 0..8 {
        set(size - 1 - i as usize, 8, bit(i));
    }
    for i in 8..15 {
        set(8, size - 15 + i as usize, bit(i));
    }
    set(8, size - 8, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smallest_version_that_fits_is_chosen() {
        assert_eq!(QrCode::encode(b"hi").unwrap().size, 21);
        assert_eq!(
            QrCode::encode(b"http://192.168.100.200:47321")
                .unwrap()
                .size,
            25
        );
        assert!(QrCode::encode(&[b'x'; 300]).is_err());
    }

    #[test]
    fn reed_solomon_matches_the_standards_worked_example() {
        // ISO/IEC 18004 Annex I: "01234567" at 1-M, data codewords and the
        // ten error-correction codewords that follow them.
        let data = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17,
        ];
        assert_eq!(
            rs_remainder(&data, &rs_divisor(10)),
            vec![165, 36, 212, 193, 237, 54, 199, 135, 44, 85]
        );
    }

    #[test]
    fn finders_and_timing_are_where_a_reader_looks() {
        let code = QrCode::encode(b"vieww").unwrap();
        for (x, y) in [
            (0, 0),
            (6, 0),
            (0, 6),
            (code.size - 1, 0),
            (0, code.size - 1),
        ] {
            assert!(code.get(x, y), "finder corner at {x},{y}");
        }
        assert!(code.get(8, 6) && !code.get(9, 6), "timing alternates");
        assert!(code.get(8, code.size - 8), "the dark module");
    }

    #[test]
    fn text_and_svg_render() {
        let code = QrCode::encode(b"vieww").unwrap();
        assert_eq!(code.to_text().lines().count(), (21_usize + 8).div_ceil(2));
        assert!(code.to_svg().starts_with("<svg"));
    }
}
