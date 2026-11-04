//! SILK's fixed-point primitives (`SigProc_FIX.h`, `macros.h`,
//! `Inlines.h`), as the 64-bit libopus build defines them. Arithmetic
//! wraps exactly where the C relies on two's-complement wrap (the
//! `_ovflw` forms) and, defensively, everywhere else too: a hostile
//! stream cannot make the decoder panic, and a valid one never overflows
//! where libopus does not.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

#[inline]
pub(crate) const fn smulwb(a: i32, b: i32) -> i32 {
    ((a as i64 * (b as i16) as i64) >> 16) as i32
}
#[inline]
pub(crate) const fn smlawb(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulwb(b, c))
}
#[inline]
pub(crate) const fn smulww(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}
#[inline]
pub(crate) const fn smlaww(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulww(b, c))
}
#[inline]
pub(crate) const fn smulbb(a: i32, b: i32) -> i32 {
    (a as i16 as i32) * (b as i16 as i32)
}
#[inline]
pub(crate) const fn smlabb(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulbb(b, c))
}
#[inline]
pub(crate) const fn smultt(a: i32, b: i32) -> i32 {
    (a >> 16).wrapping_mul(b >> 16)
}
#[inline]
pub(crate) const fn smmul(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 32) as i32
}
#[inline]
pub(crate) const fn smull(a: i32, b: i32) -> i64 {
    a as i64 * b as i64
}
#[inline]
pub(crate) const fn rshift_round(a: i32, s: u32) -> i32 {
    if s == 1 {
        (a >> 1) + (a & 1)
    } else {
        ((a >> (s - 1)) + 1) >> 1
    }
}
#[inline]
pub(crate) const fn rshift_round64(a: i64, s: u32) -> i64 {
    if s == 1 {
        (a >> 1) + (a & 1)
    } else {
        ((a >> (s - 1)) + 1) >> 1
    }
}
#[inline]
pub(crate) const fn sat16(a: i32) -> i32 {
    if a > i16::MAX as i32 {
        i16::MAX as i32
    } else if a < i16::MIN as i32 {
        i16::MIN as i32
    } else {
        a
    }
}
/// `silk_LIMIT`: clamp to the range the two limits span, in either order.
#[inline]
pub(crate) const fn limit(a: i32, l1: i32, l2: i32) -> i32 {
    if l1 > l2 {
        if a > l1 {
            l1
        } else if a < l2 {
            l2
        } else {
            a
        }
    } else if a > l2 {
        l2
    } else if a < l1 {
        l1
    } else {
        a
    }
}
#[inline]
pub(crate) const fn lshift_sat32(a: i32, s: u32) -> i32 {
    limit(a, i32::MIN >> s, i32::MAX >> s).wrapping_shl(s)
}
#[inline]
pub(crate) const fn clz32(x: i32) -> i32 {
    (x as u32).leading_zeros() as i32
}
#[inline]
const fn ror32(a: i32, rot: i32) -> i32 {
    let x = a as u32;
    if rot == 0 {
        a
    } else if rot < 0 {
        x.rotate_left((-rot) as u32) as i32
    } else {
        x.rotate_right(rot as u32) as i32
    }
}
/// `silk_CLZ_FRAC`: leading zeros and the 7 bits after the leading one.
#[inline]
pub(crate) const fn clz_frac(x: i32) -> (i32, i32) {
    let lz = clz32(x);
    (lz, ror32(x, 24 - lz) & 0x7f)
}
pub(crate) const fn sqrt_approx(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    let (lz, frac) = clz_frac(x);
    let mut y = if lz & 1 != 0 { 32768 } else { 46214 };
    y >>= lz >> 1;
    smlawb(y, y, smulbb(213, frac))
}
pub(crate) fn div32_varq(a32: i32, b32: i32, qres: i32) -> i32 {
    let a_headrm = clz32(a32.wrapping_abs()) - 1;
    let mut a32_nrm = a32.wrapping_shl(a_headrm as u32);
    let b_headrm = clz32(b32.wrapping_abs()) - 1;
    let b32_nrm = b32.wrapping_shl(b_headrm as u32);
    let den = b32_nrm >> 16;
    if den == 0 {
        return 0;
    }
    let b32_inv = (i32::MAX >> 2) / den;
    let mut result = smulwb(a32_nrm, b32_inv);
    a32_nrm = a32_nrm.wrapping_sub(smmul(b32_nrm, result).wrapping_shl(3));
    result = smlawb(result, a32_nrm, b32_inv);
    let lshift = 29 + a_headrm - b_headrm - qres;
    if lshift < 0 {
        lshift_sat32(result, (-lshift) as u32)
    } else if lshift < 32 {
        result >> lshift
    } else {
        0
    }
}
pub(crate) fn inverse32_varq(b32: i32, qres: i32) -> i32 {
    let b_headrm = clz32(b32.wrapping_abs()) - 1;
    let b32_nrm = b32.wrapping_shl(b_headrm as u32);
    let den = b32_nrm >> 16;
    if den == 0 {
        return 0;
    }
    let b32_inv = (i32::MAX >> 2) / den;
    let mut result = b32_inv.wrapping_shl(16);
    let err_q32 = ((1i32 << 29).wrapping_sub(smulwb(b32_nrm, b32_inv))).wrapping_shl(3);
    result = smlaww(result, err_q32, b32_inv);
    let lshift = 61 - b_headrm - qres;
    if lshift <= 0 {
        lshift_sat32(result, (-lshift) as u32)
    } else if lshift < 32 {
        result >> lshift
    } else {
        0
    }
}
pub(crate) const fn log2lin(in_log_q7: i32) -> i32 {
    if in_log_q7 < 0 {
        return 0;
    } else if in_log_q7 >= 3967 {
        return i32::MAX;
    }
    let mut out = 1i32 << (in_log_q7 >> 7);
    let frac = in_log_q7 & 0x7f;
    if in_log_q7 < 2048 {
        out = out.wrapping_add(out.wrapping_mul(smlawb(frac, smulbb(frac, 128 - frac), -174)) >> 7);
    } else {
        out =
            out.wrapping_add((out >> 7).wrapping_mul(smlawb(frac, smulbb(frac, 128 - frac), -174)));
    }
    out
}
pub(crate) const fn rand(seed: i32) -> i32 {
    907_633_515i32.wrapping_add(seed.wrapping_mul(196_314_165))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_match_their_c_definitions() {
        assert_eq!(smulwb(1 << 20, 0x1_8000), (1 << 4) * -32768);
        assert_eq!(rshift_round(5, 1), 3);
        assert_eq!(rshift_round(-5, 2), -1);
        assert_eq!(limit(5, 10, 0), 5);
        assert_eq!(limit(-5, 10, 0), 0);
        assert_eq!(lshift_sat32(i32::MAX, 4), (i32::MAX >> 4) << 4);
        assert_eq!(clz32(0), 32);
        assert_eq!(sqrt_approx(1 << 16), 256);
        assert_eq!(log2lin(7 << 7), 128);
        // The approximate dividers land one below exact, as the C does
        // (checked against libopus's own Inlines.h).
        assert_eq!(div32_varq(1 << 16, 1 << 16, 16), 65535);
        assert_eq!(inverse32_varq(1 << 16, 32), 65535);
        assert_eq!(rand(0), 907_633_515);
    }
}
