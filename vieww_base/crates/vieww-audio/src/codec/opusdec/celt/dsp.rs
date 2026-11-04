//! CELT's signal processing: the mixed-radix FFT and inverse MDCT
//! (libopus `kiss_fft.c`, `mdct.c`), the pitch post-filter's comb filter
//! (`celt.c`), and the LPC/pitch tools the packet-loss concealment uses
//! (`celt_lpc.c`, `pitch.c`) — float build, operation order kept, so the
//! output tracks libopus to float rounding.

#![allow(clippy::needless_range_loop, clippy::many_single_char_names)]

use super::tables as t;

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Cpx {
    pub(crate) r: f32,
    pub(crate) i: f32,
}

pub(crate) struct FftState {
    shift: usize,
    factors: [usize; 16],
    bitrev: &'static [i16],
}

pub(crate) const FFTS: [FftState; 4] = [
    FftState {
        shift: 0,
        factors: [5, 96, 3, 32, 4, 8, 2, 4, 4, 1, 0, 0, 0, 0, 0, 0],
        bitrev: &t::FFT_BITREV480,
    },
    FftState {
        shift: 1,
        factors: [5, 48, 3, 16, 4, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &t::FFT_BITREV240,
    },
    FftState {
        shift: 2,
        factors: [5, 24, 3, 8, 2, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &t::FFT_BITREV120,
    },
    FftState {
        shift: 3,
        factors: [5, 12, 3, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &t::FFT_BITREV60,
    },
];

fn tw(i: usize) -> Cpx {
    Cpx {
        r: t::FFT_TWIDDLES[2 * i],
        i: t::FFT_TWIDDLES[2 * i + 1],
    }
}

fn cmul(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: a.r * b.r - a.i * b.i,
        i: a.r * b.i + a.i * b.r,
    }
}
fn cadd(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: a.r + b.r,
        i: a.i + b.i,
    }
}
fn csub(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: a.r - b.r,
        i: a.i - b.i,
    }
}

fn bfly2(f: &mut [Cpx], n: usize) {
    let tw = 0.707_106_77_f32;
    for k in 0..n {
        let o = k * 8;
        let mut t0 = f[o + 4];
        f[o + 4] = csub(f[o], t0);
        f[o] = cadd(f[o], t0);
        t0 = Cpx {
            r: (f[o + 5].r + f[o + 5].i) * tw,
            i: (f[o + 5].i - f[o + 5].r) * tw,
        };
        f[o + 5] = csub(f[o + 1], t0);
        f[o + 1] = cadd(f[o + 1], t0);
        t0 = Cpx {
            r: f[o + 6].i,
            i: -f[o + 6].r,
        };
        f[o + 6] = csub(f[o + 2], t0);
        f[o + 2] = cadd(f[o + 2], t0);
        t0 = Cpx {
            r: (f[o + 7].i - f[o + 7].r) * tw,
            i: -(f[o + 7].i + f[o + 7].r) * tw,
        };
        f[o + 7] = csub(f[o + 3], t0);
        f[o + 3] = cadd(f[o + 3], t0);
    }
}

fn bfly4(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    if m == 1 {
        for k in 0..n {
            let o = k * 4;
            let s0 = csub(f[o], f[o + 2]);
            f[o] = cadd(f[o], f[o + 2]);
            let mut s1 = cadd(f[o + 1], f[o + 3]);
            f[o + 2] = csub(f[o], s1);
            f[o] = cadd(f[o], s1);
            s1 = csub(f[o + 1], f[o + 3]);
            f[o + 1] = Cpx {
                r: s0.r + s1.i,
                i: s0.i - s1.r,
            };
            f[o + 3] = Cpx {
                r: s0.r - s1.i,
                i: s0.i + s1.r,
            };
        }
    } else {
        let (m2, m3) = (2 * m, 3 * m);
        for k in 0..n {
            let base = k * mm;
            for j in 0..m {
                let o = base + j;
                let s0 = cmul(f[o + m], tw(j * fstride));
                let s1 = cmul(f[o + m2], tw(j * fstride * 2));
                let s2 = cmul(f[o + m3], tw(j * fstride * 3));
                let s5 = csub(f[o], s1);
                f[o] = cadd(f[o], s1);
                let s3 = cadd(s0, s2);
                let s4 = csub(s0, s2);
                f[o + m2] = csub(f[o], s3);
                f[o] = cadd(f[o], s3);
                f[o + m] = Cpx {
                    r: s5.r + s4.i,
                    i: s5.i - s4.r,
                };
                f[o + m3] = Cpx {
                    r: s5.r - s4.i,
                    i: s5.i + s4.r,
                };
            }
        }
    }
}

fn bfly3(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let m2 = 2 * m;
    let epi3 = tw(fstride * m);
    for k in 0..n {
        let base = k * mm;
        for j in 0..m {
            let o = base + j;
            let s1 = cmul(f[o + m], tw(j * fstride));
            let s2 = cmul(f[o + m2], tw(j * fstride * 2));
            let s3 = cadd(s1, s2);
            let mut s0 = csub(s1, s2);
            f[o + m] = Cpx {
                r: f[o].r - s3.r * 0.5,
                i: f[o].i - s3.i * 0.5,
            };
            s0 = Cpx {
                r: s0.r * epi3.i,
                i: s0.i * epi3.i,
            };
            f[o] = cadd(f[o], s3);
            f[o + m2] = Cpx {
                r: f[o + m].r + s0.i,
                i: f[o + m].i - s0.r,
            };
            f[o + m] = Cpx {
                r: f[o + m].r - s0.i,
                i: f[o + m].i + s0.r,
            };
        }
    }
}

fn bfly5(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let ya = tw(fstride * m);
    let yb = tw(fstride * 2 * m);
    for k in 0..n {
        let base = k * mm;
        for u in 0..m {
            let (o0, o1, o2, o3, o4) = (
                base + u,
                base + u + m,
                base + u + 2 * m,
                base + u + 3 * m,
                base + u + 4 * m,
            );
            let s0 = f[o0];
            let s1 = cmul(f[o1], tw(u * fstride));
            let s2 = cmul(f[o2], tw(2 * u * fstride));
            let s3 = cmul(f[o3], tw(3 * u * fstride));
            let s4 = cmul(f[o4], tw(4 * u * fstride));
            let s7 = cadd(s1, s4);
            let s10 = csub(s1, s4);
            let s8 = cadd(s2, s3);
            let s9 = csub(s2, s3);
            f[o0] = Cpx {
                r: f[o0].r + (s7.r + s8.r),
                i: f[o0].i + (s7.i + s8.i),
            };
            let s5 = Cpx {
                r: s0.r + (s7.r * ya.r + s8.r * yb.r),
                i: s0.i + (s7.i * ya.r + s8.i * yb.r),
            };
            let s6 = Cpx {
                r: s10.i * ya.i + s9.i * yb.i,
                i: -(s10.r * ya.i + s9.r * yb.i),
            };
            f[o1] = csub(s5, s6);
            f[o4] = cadd(s5, s6);
            let s11 = Cpx {
                r: s0.r + (s7.r * yb.r + s8.r * ya.r),
                i: s0.i + (s7.i * yb.r + s8.i * ya.r),
            };
            let s12 = Cpx {
                r: s9.i * ya.i - s10.i * yb.i,
                i: s10.r * yb.i - s9.r * ya.i,
            };
            f[o2] = cadd(s11, s12);
            f[o3] = csub(s11, s12);
        }
    }
}

/// `opus_fft_impl`: in place, input already bit-reversed.
fn fft_impl(st: &FftState, fout: &mut [Cpx]) {
    let shift = st.shift;
    let mut fstride = [1usize; 8];
    let mut l = 0;
    loop {
        let p = st.factors[2 * l];
        let m = st.factors[2 * l + 1];
        fstride[l + 1] = fstride[l] * p;
        l += 1;
        if m == 1 {
            break;
        }
    }
    let mut m = st.factors[2 * l - 1];
    for i in (0..l).rev() {
        let m2 = if i != 0 { st.factors[2 * i - 1] } else { 1 };
        match st.factors[2 * i] {
            2 => bfly2(fout, fstride[i]),
            4 => bfly4(fout, fstride[i] << shift, m, fstride[i], m2),
            3 => bfly3(fout, fstride[i] << shift, m, fstride[i], m2),
            5 => bfly5(fout, fstride[i] << shift, m, fstride[i], m2),
            _ => {}
        }
        m = m2;
    }
}

/// `clt_mdct_backward`: `input` read at `stride`, `out` gets `n/2 + overlap`
/// samples with the window's overlap-add applied to the first `overlap`.
pub(crate) fn mdct_backward(
    input: &[f32],
    out: &mut [f32],
    overlap: usize,
    shift: usize,
    stride: usize,
) {
    let mut n = 1920usize;
    let mut trig = 0usize;
    for _ in 0..shift {
        n >>= 1;
        trig += n;
    }
    let tr = &t::MDCT_TWIDDLES[trig..];
    let n2 = n >> 1;
    let n4 = n >> 2;
    let st = &FFTS[shift];
    let half = overlap >> 1;
    {
        let mut buf = vec![Cpx::default(); n4];
        let mut xp1 = 0usize;
        let mut xp2 = stride * (n2 - 1);
        for i in 0..n4 {
            let rev = st.bitrev[i] as usize;
            let yr = input[xp2] * tr[i] + input[xp1] * tr[n4 + i];
            let yi = input[xp1] * tr[i] - input[xp2] * tr[n4 + i];
            buf[rev] = Cpx { r: yi, i: yr };
            xp1 += 2 * stride;
            xp2 = xp2.wrapping_sub(2 * stride);
        }
        fft_impl(st, &mut buf);
        for (k, c) in buf.iter().enumerate() {
            out[half + 2 * k] = c.r;
            out[half + 2 * k + 1] = c.i;
        }
    }
    {
        let mut yp0 = half;
        let mut yp1 = half + n2 - 2;
        for i in 0..(n4 + 1) >> 1 {
            let mut re = out[yp0 + 1];
            let mut im = out[yp0];
            let mut t0 = tr[i];
            let mut t1 = tr[n4 + i];
            let mut yr = re * t0 + im * t1;
            let mut yi = re * t1 - im * t0;
            re = out[yp1 + 1];
            im = out[yp1];
            out[yp0] = yr;
            out[yp1 + 1] = yi;
            t0 = tr[n4 - i - 1];
            t1 = tr[n2 - i - 1];
            yr = re * t0 + im * t1;
            yi = re * t1 - im * t0;
            out[yp1] = yr;
            out[yp0 + 1] = yi;
            yp0 += 2;
            yp1 = yp1.wrapping_sub(2);
        }
    }
    {
        let w = &t::WINDOW;
        for i in 0..overlap / 2 {
            let x1 = out[overlap - 1 - i];
            let x2 = out[i];
            out[i] = w[overlap - 1 - i] * x2 - w[i] * x1;
            out[overlap - 1 - i] = w[i] * x2 + w[overlap - 1 - i] * x1;
        }
    }
}

pub(crate) const COMBFILTER_MINPERIOD: i32 = 15;

/// `comb_filter`: `y[i] = x[i] + taps around x[i−T]`, cross-faded from the
/// old filter (`t0`, `g0`, `tapset0`) to the new over the window's
/// overlap. With `src == None` it runs in place on `buf` (`x == y`), and —
/// exactly as libopus does — later taps then read samples the filter has
/// already rewritten. With a separate `src`, `x` indexes it and `y` indexes
/// `buf`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn comb_filter(
    buf: &mut [f32],
    y: usize,
    src: Option<&[f32]>,
    x: usize,
    t0: i32,
    t1: i32,
    n: usize,
    g0: f32,
    g1: f32,
    tapset0: usize,
    tapset1: usize,
    window: Option<&[f32]>,
    overlap: usize,
) {
    const GAINS: [[f32; 3]; 3] = [
        [0.306_640_63, 0.217_041_02, 0.129_638_67],
        [0.463_867_2, 0.268_066_4, 0.0],
        [0.799_804_7, 0.100_097_66, 0.0],
    ];
    macro_rules! rd {
        ($i:expr) => {
            match src {
                Some(s) => s[$i],
                None => buf[$i],
            }
        };
    }
    if g0 == 0.0 && g1 == 0.0 {
        if let Some(s) = src {
            buf[y..y + n].copy_from_slice(&s[x..x + n]);
        }
        return;
    }
    let t0 = t0.max(COMBFILTER_MINPERIOD) as usize;
    let t1 = t1.max(COMBFILTER_MINPERIOD) as usize;
    let g00 = g0 * GAINS[tapset0][0];
    let g01 = g0 * GAINS[tapset0][1];
    let g02 = g0 * GAINS[tapset0][2];
    let g10 = g1 * GAINS[tapset1][0];
    let g11 = g1 * GAINS[tapset1][1];
    let g12 = g1 * GAINS[tapset1][2];
    let mut x1 = rd!(x + 1 - t1);
    let mut x2 = rd!(x - t1);
    let mut x3 = rd!(x - t1 - 1);
    let mut x4 = rd!(x - t1 - 2);
    let overlap = if g0 == g1 && t0 == t1 && tapset0 == tapset1 {
        0
    } else {
        overlap
    };
    let win = window.unwrap_or(&[]);
    let mut i = 0;
    while i < overlap {
        let x0 = rd!(x + i + 2 - t1);
        let f = win[i] * win[i];
        let v = rd!(x + i)
            + ((1.0 - f) * g00) * rd!(x + i - t0)
            + ((1.0 - f) * g01) * (rd!(x + i + 1 - t0) + rd!(x + i - t0 - 1))
            + ((1.0 - f) * g02) * (rd!(x + i + 2 - t0) + rd!(x + i - t0 - 2))
            + (f * g10) * x2
            + (f * g11) * (x1 + x3)
            + (f * g12) * (x0 + x4);
        buf[y + i] = v;
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
        i += 1;
    }
    if g1 == 0.0 {
        if let Some(s) = src {
            buf[y + overlap..y + n].copy_from_slice(&s[x + overlap..x + n]);
        }
        return;
    }
    // comb_filter_const, from where the cross-fade stopped.
    let mut x4 = rd!(x + i - t1 - 2);
    let mut x3 = rd!(x + i - t1 - 1);
    let mut x2 = rd!(x + i - t1);
    let mut x1 = rd!(x + i + 1 - t1);
    for k in i..n {
        let x0 = rd!(x + k + 2 - t1);
        let v = rd!(x + k) + g10 * x2 + g11 * (x1 + x3) + g12 * (x0 + x4);
        buf[y + k] = v;
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
    }
}

pub(crate) fn inner_prod(x: &[f32], y: &[f32], n: usize) -> f32 {
    let mut s = 0f32;
    for i in 0..n {
        s += x[i] * y[i];
    }
    s
}

fn xcorr_kernel(x: &[f32], y: &[f32], sum: &mut [f32; 4], len: usize) {
    let (mut xi, mut yi) = (0usize, 0usize);
    let mut y3;
    let mut y0 = y[yi];
    yi += 1;
    let mut y1 = y[yi];
    yi += 1;
    let mut y2 = y[yi];
    yi += 1;
    let mut j = 0usize;
    while j + 3 < len {
        let mut tmp = x[xi];
        xi += 1;
        y3 = y[yi];
        yi += 1;
        sum[0] += tmp * y0;
        sum[1] += tmp * y1;
        sum[2] += tmp * y2;
        sum[3] += tmp * y3;
        tmp = x[xi];
        xi += 1;
        y0 = y[yi];
        yi += 1;
        sum[0] += tmp * y1;
        sum[1] += tmp * y2;
        sum[2] += tmp * y3;
        sum[3] += tmp * y0;
        tmp = x[xi];
        xi += 1;
        y1 = y[yi];
        yi += 1;
        sum[0] += tmp * y2;
        sum[1] += tmp * y3;
        sum[2] += tmp * y0;
        sum[3] += tmp * y1;
        tmp = x[xi];
        xi += 1;
        y2 = y[yi];
        yi += 1;
        sum[0] += tmp * y3;
        sum[1] += tmp * y0;
        sum[2] += tmp * y1;
        sum[3] += tmp * y2;
        j += 4;
    }
    let mut y3v = 0f32;
    if j < len {
        let tmp = x[xi];
        xi += 1;
        y3v = y[yi];
        yi += 1;
        sum[0] += tmp * y0;
        sum[1] += tmp * y1;
        sum[2] += tmp * y2;
        sum[3] += tmp * y3v;
    }
    j += 1;
    if j < len {
        let tmp = x[xi];
        xi += 1;
        y0 = y[yi];
        yi += 1;
        sum[0] += tmp * y1;
        sum[1] += tmp * y2;
        sum[2] += tmp * y3v;
        sum[3] += tmp * y0;
    }
    j += 1;
    if j < len {
        let tmp = x[xi];
        y1 = y[yi];
        sum[0] += tmp * y2;
        sum[1] += tmp * y3v;
        sum[2] += tmp * y0;
        sum[3] += tmp * y1;
    }
}

fn pitch_xcorr(x: &[f32], y: &[f32], xcorr: &mut [f32], len: usize, max_pitch: usize) {
    let mut i = 0;
    while i + 3 < max_pitch {
        let mut sum = [0f32; 4];
        xcorr_kernel(x, &y[i..], &mut sum, len);
        xcorr[i..i + 4].copy_from_slice(&sum);
        i += 4;
    }
    while i < max_pitch {
        xcorr[i] = inner_prod(x, &y[i..], len);
        i += 1;
    }
}

pub(crate) fn lpc(lpc: &mut [f32], ac: &[f32], p: usize) {
    let mut error = ac[0];
    for v in lpc.iter_mut().take(p) {
        *v = 0.0;
    }
    if ac[0] != 0.0 {
        for i in 0..p {
            let mut rr = 0f32;
            for j in 0..i {
                rr += lpc[j] * ac[i - j];
            }
            rr += ac[i + 1];
            let r = -(rr / error);
            lpc[i] = r;
            for j in 0..(i + 1) >> 1 {
                let tmp1 = lpc[j];
                let tmp2 = lpc[i - 1 - j];
                lpc[j] = tmp1 + r * tmp2;
                lpc[i - 1 - j] = tmp2 + r * tmp1;
            }
            error -= (r * r) * error;
            if error < 0.001 * ac[0] {
                break;
            }
        }
    }
}

/// `celt_fir`: `x` is read from `x[off − ord ..]`.
pub(crate) fn fir(x: &[f32], off: usize, num: &[f32], y: &mut [f32], n: usize, ord: usize) {
    let rnum: Vec<f32> = (0..ord).map(|i| num[ord - i - 1]).collect();
    let mut i = 0;
    while i + 3 < n {
        let mut sum = [x[off + i], x[off + i + 1], x[off + i + 2], x[off + i + 3]];
        xcorr_kernel(&rnum, &x[off + i - ord..], &mut sum, ord);
        y[i..i + 4].copy_from_slice(&sum);
        i += 4;
    }
    while i < n {
        let mut sum = x[off + i];
        for j in 0..ord {
            sum += rnum[j] * x[off + i + j - ord];
        }
        y[i] = sum;
        i += 1;
    }
}

pub(crate) fn iir(x: &[f32], den: &[f32], out: &mut [f32], n: usize, ord: usize, mem: &mut [f32]) {
    let rden: Vec<f32> = (0..ord).map(|i| den[ord - i - 1]).collect();
    let mut y = vec![0f32; n + ord];
    for i in 0..ord {
        y[i] = -mem[ord - i - 1];
    }
    let mut i = 0;
    while i + 3 < n {
        let mut sum = [x[i], x[i + 1], x[i + 2], x[i + 3]];
        xcorr_kernel(&rden, &y[i..], &mut sum, ord);
        y[i + ord] = -sum[0];
        out[i] = sum[0];
        sum[1] += y[i + ord] * den[0];
        y[i + ord + 1] = -sum[1];
        out[i + 1] = sum[1];
        sum[2] += y[i + ord + 1] * den[0];
        sum[2] += y[i + ord] * den[1];
        y[i + ord + 2] = -sum[2];
        out[i + 2] = sum[2];
        sum[3] += y[i + ord + 2] * den[0];
        sum[3] += y[i + ord + 1] * den[1];
        sum[3] += y[i + ord] * den[2];
        y[i + ord + 3] = -sum[3];
        out[i + 3] = sum[3];
        i += 4;
    }
    while i < n {
        let mut sum = x[i];
        for j in 0..ord {
            sum -= rden[j] * y[i + j];
        }
        y[i + ord] = sum;
        out[i] = sum;
        i += 1;
    }
    for i in 0..ord {
        mem[i] = out[n - i - 1];
    }
}

pub(crate) fn autocorr(
    x: &[f32],
    ac: &mut [f32],
    window: Option<&[f32]>,
    overlap: usize,
    lag: usize,
    n: usize,
) {
    let fast_n = n - lag;
    let mut xx: Vec<f32> = x[..n].to_vec();
    if let (Some(w), true) = (window, overlap > 0) {
        for i in 0..overlap {
            xx[i] = x[i] * w[i];
            xx[n - i - 1] = x[n - i - 1] * w[i];
        }
    }
    pitch_xcorr(&xx, &xx, ac, fast_n, lag + 1);
    for k in 0..=lag {
        let mut d = 0f32;
        for i in k + fast_n..n {
            d += xx[i] * xx[i - k];
        }
        ac[k] += d;
    }
}

fn find_best_pitch(xcorr: &[f32], y: &[f32], len: usize, max_pitch: usize, best: &mut [usize; 2]) {
    let mut syy = 1f32;
    let mut best_num = [-1f32, -1.0];
    let mut best_den = [0f32, 0.0];
    *best = [0, 1];
    for j in 0..len {
        syy += y[j] * y[j];
    }
    for i in 0..max_pitch {
        if xcorr[i] > 0.0 {
            let mut x16 = xcorr[i];
            x16 *= 1e-12;
            let num = x16 * x16;
            if num * best_den[1] > best_num[1] * syy {
                if num * best_den[0] > best_num[0] * syy {
                    best_num[1] = best_num[0];
                    best_den[1] = best_den[0];
                    best[1] = best[0];
                    best_num[0] = num;
                    best_den[0] = syy;
                    best[0] = i;
                } else {
                    best_num[1] = num;
                    best_den[1] = syy;
                    best[1] = i;
                }
            }
        }
        syy += y[i + len] * y[i + len] - y[i] * y[i];
        syy = syy.max(1.0);
    }
}

pub(crate) fn pitch_downsample(x: [&[f32]; 2], x_lp: &mut [f32], len: usize, c: usize) {
    for i in 1..len >> 1 {
        x_lp[i] = 0.5 * (0.5 * (x[0][2 * i - 1] + x[0][2 * i + 1]) + x[0][2 * i]);
    }
    x_lp[0] = 0.5 * (0.5 * x[0][1] + x[0][0]);
    if c == 2 {
        for i in 1..len >> 1 {
            x_lp[i] += 0.5 * (0.5 * (x[1][2 * i - 1] + x[1][2 * i + 1]) + x[1][2 * i]);
        }
        x_lp[0] += 0.5 * (0.5 * x[1][1] + x[1][0]);
    }
    let mut ac = [0f32; 5];
    autocorr(x_lp, &mut ac, None, 0, 4, len >> 1);
    ac[0] *= 1.0001;
    for i in 1..=4 {
        ac[i] -= ac[i] * (0.008 * i as f32) * (0.008 * i as f32);
    }
    let mut l = [0f32; 4];
    lpc(&mut l, &ac, 4);
    let mut tmp = 1f32;
    for v in &mut l {
        tmp *= 0.9;
        *v *= tmp;
    }
    let c1 = 0.8f32;
    let lpc2 = [
        l[0] + 0.8,
        l[1] + c1 * l[0],
        l[2] + c1 * l[1],
        l[3] + c1 * l[2],
        c1 * l[3],
    ];
    // celt_fir5, in place.
    let (mut m0, mut m1, mut m2, mut m3, mut m4) = (0f32, 0f32, 0f32, 0f32, 0f32);
    for i in 0..len >> 1 {
        let mut sum = x_lp[i];
        sum += lpc2[0] * m0;
        sum += lpc2[1] * m1;
        sum += lpc2[2] * m2;
        sum += lpc2[3] * m3;
        sum += lpc2[4] * m4;
        m4 = m3;
        m3 = m2;
        m2 = m1;
        m1 = m0;
        m0 = x_lp[i];
        x_lp[i] = sum;
    }
}

pub(crate) fn pitch_search(x_lp: &[f32], y: &[f32], len: usize, max_pitch: usize) -> usize {
    let lag = len + max_pitch;
    let x4: Vec<f32> = (0..len >> 2).map(|j| x_lp[2 * j]).collect();
    let y4: Vec<f32> = (0..lag >> 2).map(|j| y[2 * j]).collect();
    let mut xcorr = vec![0f32; max_pitch >> 1];
    let mut best = [0usize; 2];
    pitch_xcorr(&x4, &y4, &mut xcorr, len >> 2, max_pitch >> 2);
    find_best_pitch(&xcorr, &y4, len >> 2, max_pitch >> 2, &mut best);
    for i in 0..max_pitch >> 1 {
        xcorr[i] = 0.0;
        if (i as i32 - 2 * best[0] as i32).abs() > 2 && (i as i32 - 2 * best[1] as i32).abs() > 2 {
            continue;
        }
        let sum = inner_prod(x_lp, &y[i..], len >> 1);
        xcorr[i] = sum.max(-1.0);
    }
    find_best_pitch(&xcorr, y, len >> 1, max_pitch >> 1, &mut best);
    let offset: i32 = if best[0] > 0 && best[0] < (max_pitch >> 1) - 1 {
        let (a, b, c) = (xcorr[best[0] - 1], xcorr[best[0]], xcorr[best[0] + 1]);
        if (c - a) > 0.7 * (b - a) {
            1
        } else if (a - c) > 0.7 * (b - c) {
            -1
        } else {
            0
        }
    } else {
        0
    };
    (2 * best[0] as i32 - offset) as usize
}
