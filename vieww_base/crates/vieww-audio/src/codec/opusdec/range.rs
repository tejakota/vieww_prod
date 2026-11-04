//! The range decoder (RFC 6716 §4.1; libopus `celt/entdec.c`), shared by
//! SILK and CELT: symbols from the front of the frame, raw bits from its
//! back, and the bit-exact `tell` both layers' allocations depend on.

const SYM_BITS: u32 = 8;
const CODE_BITS: u32 = 32;
const SYM_MAX: u32 = (1 << SYM_BITS) - 1;
const CODE_TOP: u32 = 1 << (CODE_BITS - 1);
const CODE_BOT: u32 = CODE_TOP >> SYM_BITS;
const CODE_EXTRA: u32 = (CODE_BITS - 2) % SYM_BITS + 1;
const UINT_BITS: u32 = 8;
const WINDOW_SIZE: i32 = 32;
pub(crate) const BITRES: u32 = 3;

/// `EC_ILOG`: bits needed for `x` (0 for 0).
pub(crate) const fn ilog(x: u32) -> i32 {
    (32 - x.leading_zeros()) as i32
}

#[derive(Debug, Clone)]
pub(crate) struct RangeDecoder<'a> {
    buf: &'a [u8],
    storage: u32,
    end_offs: u32,
    end_window: u32,
    nend_bits: i32,
    nbits_total: i32,
    offs: u32,
    rng: u32,
    val: u32,
    ext: u32,
    rem: i32,
    /// Set when a decode ran past what the frame can mean; the caller
    /// treats the frame as corrupt.
    pub(crate) error: bool,
}

impl<'a> RangeDecoder<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        let storage = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let mut d = Self {
            buf,
            storage,
            end_offs: 0,
            end_window: 0,
            nend_bits: 0,
            nbits_total: (CODE_BITS + 1 - ((CODE_BITS - CODE_EXTRA) / SYM_BITS) * SYM_BITS) as i32,
            offs: 0,
            rng: 1 << CODE_EXTRA,
            val: 0,
            ext: 0,
            rem: 0,
            error: false,
        };
        d.rem = d.read_byte();
        d.val = d.rng - 1 - ((d.rem as u32) >> (SYM_BITS - CODE_EXTRA));
        d.normalize();
        d
    }

    fn read_byte(&mut self) -> i32 {
        if self.offs < self.storage {
            let b = self.buf[self.offs as usize];
            self.offs += 1;
            i32::from(b)
        } else {
            0
        }
    }

    fn read_byte_from_end(&mut self) -> i32 {
        if self.end_offs < self.storage {
            self.end_offs += 1;
            i32::from(self.buf[(self.storage - self.end_offs) as usize])
        } else {
            0
        }
    }

    fn normalize(&mut self) {
        while self.rng <= CODE_BOT {
            self.nbits_total += SYM_BITS as i32;
            self.rng <<= SYM_BITS;
            let mut sym = self.rem;
            self.rem = self.read_byte();
            sym = ((sym << SYM_BITS) | self.rem) >> (SYM_BITS - CODE_EXTRA);
            self.val =
                ((self.val << SYM_BITS).wrapping_add(SYM_MAX & !(sym as u32))) & (CODE_TOP - 1);
        }
    }

    pub(crate) fn decode(&mut self, ft: u32) -> u32 {
        self.ext = self.rng / ft;
        let s = self.val / self.ext;
        ft - (s + 1).min(ft)
    }

    pub(crate) fn decode_bin(&mut self, bits: u32) -> u32 {
        self.ext = self.rng >> bits;
        let s = self.val / self.ext;
        (1 << bits) - (s + 1).min(1 << bits)
    }

    pub(crate) fn update(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext.wrapping_mul(ft - fh);
        self.val = self.val.wrapping_sub(s);
        self.rng = if fl > 0 {
            self.ext.wrapping_mul(fh - fl)
        } else {
            self.rng.wrapping_sub(s)
        };
        self.normalize();
    }

    pub(crate) fn bit_logp(&mut self, logp: u32) -> bool {
        let r = self.rng;
        let d = self.val;
        let s = r >> logp;
        let ret = d < s;
        if !ret {
            self.val = d - s;
        }
        self.rng = if ret { s } else { r - s };
        self.normalize();
        ret
    }

    /// `ec_dec_icdf`: a symbol from an inverse CDF scaled to `1 << ftb`.
    pub(crate) fn icdf(&mut self, icdf: &[u8], ftb: u32) -> usize {
        let mut s = self.rng;
        let d = self.val;
        let r = s >> ftb;
        let mut ret = 0usize;
        let mut t;
        loop {
            t = s;
            // A hostile table walk stops at the table's end (every real
            // table ends in 0, which always terminates the loop).
            let v = icdf.get(ret).copied().unwrap_or(0);
            s = r.wrapping_mul(u32::from(v));
            if d >= s {
                break;
            }
            ret += 1;
        }
        self.val = d - s;
        self.rng = t - s;
        self.normalize();
        ret
    }

    pub(crate) fn uint(&mut self, ft: u32) -> u32 {
        debug_assert!(ft > 1);
        let ft1 = ft - 1;
        let mut ftb = ilog(ft1) as u32;
        if ftb > UINT_BITS {
            ftb -= UINT_BITS;
            let f = (ft1 >> ftb) + 1;
            let s = self.decode(f);
            self.update(s, s + 1, f);
            let t = (s << ftb) | self.bits(ftb);
            if t <= ft1 {
                return t;
            }
            self.error = true;
            ft1
        } else {
            let s = self.decode(ft);
            self.update(s, s + 1, ft);
            s
        }
    }

    pub(crate) fn bits(&mut self, bits: u32) -> u32 {
        let mut window = self.end_window;
        let mut available = self.nend_bits;
        if (available as u32) < bits {
            loop {
                window |= (self.read_byte_from_end() as u32) << available;
                available += SYM_BITS as i32;
                if available > WINDOW_SIZE - SYM_BITS as i32 {
                    break;
                }
            }
        }
        let ret = if bits >= 32 {
            window
        } else {
            window & ((1u32 << bits) - 1)
        };
        window = if bits >= 32 { 0 } else { window >> bits };
        available -= bits as i32;
        self.end_window = window;
        self.nend_bits = available;
        self.nbits_total += bits as i32;
        ret
    }

    pub(crate) fn tell(&self) -> i32 {
        self.nbits_total - ilog(self.rng)
    }

    pub(crate) fn tell_frac(&self) -> u32 {
        const CORRECTION: [u32; 8] = [35733, 38967, 42495, 46340, 50535, 55109, 60097, 65535];
        let nbits = (self.nbits_total as u32) << BITRES;
        let mut l = ilog(self.rng);
        let r = self.rng >> (l - 16);
        let mut b = (r >> 12) - 8;
        b += u32::from(r > CORRECTION[b as usize]);
        l = (l << 3) + b as i32;
        nbits.wrapping_sub(l as u32)
    }

    /// Bytes of the frame the decoder was built over.
    pub(crate) const fn storage(&self) -> u32 {
        self.storage
    }

    /// Shrink the frame (CELT's redundancy trick in hybrid mode).
    pub(crate) fn shrink(&mut self, storage: u32) {
        self.storage = storage.min(self.storage);
    }

    /// Advance the bit count without reading (CELT's silence flag).
    pub(crate) fn add_bits(&mut self, n: i32) {
        self.nbits_total += n;
    }

    pub(crate) const fn rng(&self) -> u32 {
        self.rng
    }
}
