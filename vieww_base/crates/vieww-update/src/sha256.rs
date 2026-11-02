//! SHA-256 (FIPS 180-4).
//!
//! # Why this is a port rather than a dependency
//!
//! This crate has no dependencies, by its own manifest's argument: an
//! updater runs inside a shipped app and must be auditable as *this code*,
//! not as a tree. The port is a direct transcription of the standard — pad,
//! then 64 rounds per 64-byte block — pinned byte-for-byte to the NIST
//! vectors below.
//!
//! # Provenance
//!
//! Copied from `apps/wright/core/src/sha256.rs` (Wright's audited port,
//! which itself carries the NIST vectors and a padding-edge suite), with
//! the crate-specific doc header adjusted and nothing else changed: the
//! same compression function, the same streaming shape, the same tests.
//! The audit trail for those bytes therefore stays one hop long — this
//! file's history names where they came from, and both files carry the
//! standard's own vectors so a transcription error cannot survive either
//! crate's test run.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*word);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(y);
    }
}

/// SHA-256 over data that arrives in pieces — an update being downloaded is
/// hashed while it streams, so a gigabyte never needs a second copy in
/// memory to be checked.
#[derive(Clone, Debug)]
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    fill: usize,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: H0,
            buf: [0; 64],
            fill: 0,
            len: 0,
        }
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.fill > 0 {
            let take = (64 - self.fill).min(data.len());
            self.buf[self.fill..self.fill + take].copy_from_slice(&data[..take]);
            self.fill += take;
            data = &data[take..];
            if self.fill < 64 {
                return;
            }
            let block = self.buf;
            compress(&mut self.h, &block);
            self.fill = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.h, block);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.fill = rest.len();
    }

    pub fn finish(mut self) -> [u8; 32] {
        // Padding: 0x80, zeros, then the length in bits (big-endian u64).
        let bits = self.len.wrapping_mul(8);
        let mut tail = [0u8; 128];
        tail[..self.fill].copy_from_slice(&self.buf[..self.fill]);
        tail[self.fill] = 0x80;
        let total = if self.fill < 56 { 64 } else { 128 };
        tail[total - 8..total].copy_from_slice(&bits.to_be_bytes());
        for block in tail[..total].as_chunks::<64>().0 {
            compress(&mut self.h, block);
        }
        let mut out = [0u8; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.h) {
            *chunk = word.to_be_bytes();
        }
        out
    }

    pub fn finish_hex(self) -> String {
        hex(&self.finish())
    }
}

/// The SHA-256 digest of `data`.
pub fn digest(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

/// Lowercase hex of `bytes` — the encoding manifests carry digests in.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 32 raw bytes from a 64-character lowercase-or-uppercase hex string.
/// `None` on any other length or any non-hex character — a manifest's
/// digest is attacker-controlled text, and lenient parsing of it is how a
/// "mostly verified" update happens.
pub fn unhex32(s: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    unhex_into(s, &mut out).then_some(out)
}

/// 64 raw bytes from a 128-character hex string (a signature).
pub fn unhex64(s: &str) -> Option<[u8; 64]> {
    let mut out = [0u8; 64];
    unhex_into(s, &mut out).then_some(out)
}

/// The shared body: fill `out` from `s`, byte by byte, refusing anything
/// that is not two hex digits per byte.
fn unhex_into(s: &str, out: &mut [u8]) -> bool {
    if s.len() != out.len() * 2 {
        return false;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        match u8::from_str_radix(&s[i * 2..i * 2 + 2], 16) {
            Ok(byte) => *slot = byte,
            Err(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nist_vectors() {
        assert_eq!(
            hex(&digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(&digest(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn padding_edges() {
        // 55, 56 and 64 bytes straddle the one-block / two-block padding.
        assert_eq!(
            hex(&digest(&[b'a'; 55])),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        assert_eq!(
            hex(&digest(&[b'a'; 56])),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
        assert_eq!(
            hex(&digest(&[b'a'; 64])),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    #[test]
    fn pieces_hash_like_the_whole() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        for cut in [0, 1, 55, 56, 63, 64, 65, 500, 999, 1000] {
            let mut s = Sha256::new();
            s.update(&data[..cut]);
            s.update(&data[cut..]);
            assert_eq!(s.finish(), digest(&data), "split at {cut}");
        }
        let mut s = Sha256::new();
        for b in data.chunks(7) {
            s.update(b);
        }
        assert_eq!(s.finish_hex(), hex(&digest(&data)));
    }

    #[test]
    fn hex_round_trip_is_strict() {
        let d = digest(b"strict");
        let text = hex(&d);
        assert_eq!(unhex32(&text), Some(d));
        assert_eq!(unhex32(&text.to_uppercase()), Some(d), "case-insensitive");
        assert_eq!(unhex32(""), None);
        assert_eq!(unhex32("zz"), None);
        assert_eq!(unhex32(&text[..62]), None, "too short");
        assert_eq!(unhex64(&text), None, "wrong length for 64");
    }
}
