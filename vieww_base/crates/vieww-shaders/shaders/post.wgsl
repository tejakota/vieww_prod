// Offscreen compositing: every per-pixel operation a `vieww_gpu::Step` needs
// that is not geometry.
//
// One full-screen triangle, scissored by the backend to the region the step
// touches, and one fragment program selected by `pc.args.x`. Each mode is a
// line-for-line port of the `NativeRenderer` function named beside it, so the
// GPU/CPU parity suite compares two executions of one definition:
//
//   0 COPY     src                         Target::snapshot_from
//   1 BOX_H    horizontal box, radius r    shadow::box_blur_horizontal / effects::blur
//   2 BOX_V    vertical box, radius r      … _vertical
//   3 MATRIX   4×5 colour matrix           effects::color_matrix
//   4 MASK     src × mask                  PopLayer's shaped-clip multiply
//   5 BLEND    blend(mode, src×α, dst)     Target::composite_layer, 28 modes
//   6 OVER     src × α (pipeline blends)   composite_layer, Normal
//   7 SHADOW   colour × mask [× clip]      render_shadow's tint + clip multiply,
//                                          over coverage the CPU code blurred
//
// Targets are premultiplied RGBA16F, fetched with `textureLoad`. For the blur
// passes, pixels outside `pc.region` read one of two ways, chosen by
// `pc.args.z`:
//
//   0  transparent — "outside the buffer is transparent", which is what makes
//      a shadow fade at its own edge (`shadow::box_blur_*`);
//   1  the nearest edge pixel — `effects::blur`'s edge-clamped sampling, which
//      is CSS's `duplicate` edge mode for `filter` and `backdrop-filter`. A
//      backdrop blur read as transparent past its rect loses alpha along the
//      edge, and the composite then shows the *unblurred* page through a
//      frosted panel's rim.

struct Post {
    // x0, y0, x1, y1 in device pixels.
    region: vec4<f32>,
    // mode, radius, blend mode, alpha.
    args: vec4<f32>,
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
    p4: vec4<f32>,
    p5: vec4<f32>,
};

var<immediate> pc: Post;

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var t_aux: texture_2d<f32>;
@group(0) @binding(2) var t_mask: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // A triangle covering the whole target; the scissor narrows it.
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn src_at(p: vec2<i32>) -> vec4<f32> {
    if f32(p.x) < pc.region.x || f32(p.y) < pc.region.y || f32(p.x) >= pc.region.z || f32(p.y) >= pc.region.w {
        return vec4<f32>(0.0);
    }
    return textureLoad(t_src, p, 0);
}

fn src_clamped(p: vec2<i32>) -> vec4<f32> {
    let lo = vec2<i32>(i32(pc.region.x), i32(pc.region.y));
    let hi = vec2<i32>(i32(pc.region.z) - 1, i32(pc.region.w) - 1);
    return textureLoad(t_src, clamp(p, lo, hi), 0);
}

fn mask_at(p: vec2<i32>, offset: vec2<f32>) -> f32 {
    return textureLoad(t_mask, p - vec2<i32>(round(offset)), 0).r;
}

fn unpremul(c: vec4<f32>) -> vec3<f32> {
    if c.a <= 1e-6 {
        return vec3<f32>(0.0);
    }
    return c.rgb / c.a;
}

fn over(s: vec4<f32>, d: vec4<f32>) -> vec4<f32> {
    return s + d * (1.0 - s.a);
}

fn hard_light(a: f32, b: f32) -> f32 {
    if a <= 0.5 {
        return 2.0 * a * b;
    }
    return 1.0 - 2.0 * (1.0 - a) * (1.0 - b);
}

fn soft_light(cs: f32, cb: f32) -> f32 {
    if cs <= 0.5 {
        return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    }
    var d: f32;
    if cb <= 0.25 {
        d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
    } else {
        d = sqrt(cb);
    }
    return cb + (2.0 * cs - 1.0) * (d - cb);
}

fn separable_channel(mode: i32, cs: f32, cb: f32) -> f32 {
    switch mode {
        case 13: { return cs * cb; }
        case 14: { return cs + cb - cs * cb; }
        case 15: { return hard_light(cb, cs); }
        case 16: { return min(cs, cb); }
        case 17: { return max(cs, cb); }
        case 18: {
            if cb == 0.0 { return 0.0; }
            if cs >= 1.0 { return 1.0; }
            return min(cb / (1.0 - cs), 1.0);
        }
        case 19: {
            if cb >= 1.0 { return 1.0; }
            if cs <= 0.0 { return 0.0; }
            return 1.0 - min((1.0 - cb) / cs, 1.0);
        }
        case 20: { return hard_light(cs, cb); }
        case 21: { return soft_light(cs, cb); }
        case 22: { return abs(cs - cb); }
        case 23: { return cs + cb - 2.0 * cs * cb; }
        default: { return cs; }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

fn clip_color(c_in: vec3<f32>) -> vec3<f32> {
    var c = c_in;
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    if n < 0.0 {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * l / max(l - n, 1e-6);
    }
    if x > 1.0 {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * (1.0 - l) / max(x - l, 1e-6);
    }
    return c;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(c);
    return clip_color(c + vec3<f32>(d));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

// Component `i` of `c`, by comparison rather than `c[i]`: FXC (the D3D12
// backend's compiler) mishandles runtime indexing of local vectors.
fn comp(c: vec3<f32>, i: i32) -> f32 {
    return select(select(c.b, c.g, i == 1), c.r, i == 0);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    // A stable sort of three indices by value, as `native/color.rs` sorts.
    var i0 = 0;
    var i1 = 1;
    var i2 = 2;
    if comp(c, i1) < comp(c, i0) { let t = i0; i0 = i1; i1 = t; }
    if comp(c, i2) < comp(c, i1) {
        let t = i1; i1 = i2; i2 = t;
        if comp(c, i1) < comp(c, i0) { let u = i0; i0 = i1; i1 = u; }
    }
    let lo = comp(c, i0);
    let mid = comp(c, i1);
    let hi = comp(c, i2);
    var new_mid = 0.0;
    var new_hi = 0.0;
    if hi > lo {
        new_mid = (mid - lo) * s / (hi - lo);
        new_hi = s;
    }
    // Scatter back by index without `out[i] = ...`: each component is the
    // new max, the new mid, or zero (the min), by which index it was.
    let r = select(select(0.0, new_mid, i1 == 0), new_hi, i2 == 0);
    let g = select(select(0.0, new_mid, i1 == 1), new_hi, i2 == 1);
    let b = select(select(0.0, new_mid, i1 == 2), new_hi, i2 == 2);
    return vec3<f32>(r, g, b);
}

// `native/color.rs`'s `blend`, on premultiplied colours.
fn blend(mode: i32, s: vec4<f32>, d: vec4<f32>) -> vec4<f32> {
    switch mode {
        case 0: { return over(s, d); }
        case 1: { return vec4<f32>(0.0); }
        case 2: { return s; }
        case 3: { return d; }
        case 4: { return over(d, s); }
        case 5: { return s * d.a; }
        case 6: { return d * s.a; }
        case 7: { return s * (1.0 - d.a); }
        case 8: { return d * (1.0 - s.a); }
        case 9: { return s * d.a + d * (1.0 - s.a); }
        case 10: { return s * (1.0 - d.a) + d * s.a; }
        case 11: { return s * (1.0 - d.a) + d * (1.0 - s.a); }
        case 12: { return min(s + d, vec4<f32>(1.0)); }
        default: {}
    }
    let cs = unpremul(s);
    let cb = unpremul(d);
    var blended: vec3<f32>;
    if mode <= 23 {
        blended = vec3<f32>(
            separable_channel(mode, cs.r, cb.r),
            separable_channel(mode, cs.g, cb.g),
            separable_channel(mode, cs.b, cb.b),
        );
    } else {
        switch mode {
            case 24: { blended = set_lum(set_sat(cs, sat(cb)), lum(cb)); }
            case 25: { blended = set_lum(set_sat(cb, sat(cs)), lum(cb)); }
            case 26: { blended = set_lum(cs, lum(cb)); }
            case 27: { blended = set_lum(cb, lum(cs)); }
            default: { blended = cs; }
        }
    }
    let mixed = (1.0 - d.a) * cs + d.a * blended;
    return over(vec4<f32>(mixed * s.a, s.a), d);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shade(frag, true);
}

// The D3D12 backend's split of `fs_main` (the other backends use `fs_main`
// itself): WARP removes the device running a pixel shader that holds both
// the blur loop and the blend modes, though either alone runs (the
// `post_shader_bisection_on_the_group_opacity_scene` suite). FXC inlines
// `shade` and drops the branch a constant `false` turns off, so each entry
// point carries only half. The arithmetic is `fs_main`'s, unchanged.
@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy));
    return box_blur(p, i32(round(pc.args.x)));
}

@fragment
fn fs_no_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return shade(frag, false);
}

fn shade(frag: vec4<f32>, with_blur: bool) -> vec4<f32> {
    // An if-chain, not a `switch`: FXC (D3DCompile, the D3D12 backend's
    // compiler) aborts with "internal error: compilation aborted
    // unexpectedly" on the blur loop when it sits inside a switch case — a
    // known FXC defect with loops in switch bodies. The arithmetic is
    // unchanged, so every backend still draws the same bytes.
    let p = vec2<i32>(floor(frag.xy));
    let mode = i32(round(pc.args.x));
    if mode == 0 {
        return src_at(p);
    }
    if with_blur && (mode == 1 || mode == 2) {
        return box_blur(p, mode);
    }
    if mode == 3 {
        let c = src_at(p);
        let a = max(c.a, 1e-6);
        let v = vec4<f32>(c.rgb / a, c.a);
        let nr = clamp(dot(pc.p0, v) + pc.p4.x, 0.0, 1.0);
        let ng = clamp(dot(pc.p1, v) + pc.p4.y, 0.0, 1.0);
        let nb = clamp(dot(pc.p2, v) + pc.p4.z, 0.0, 1.0);
        let na = clamp(dot(pc.p3, v) + pc.p4.w, 0.0, 1.0);
        return vec4<f32>(nr * na, ng * na, nb * na, na);
    }
    if mode == 4 {
        return src_at(p) * mask_at(p, pc.p0.xy);
    }
    if mode == 5 {
        let s = src_at(p) * pc.args.w;
        let d = textureLoad(t_aux, p, 0);
        return blend(i32(round(pc.args.z)), s, d);
    }
    if mode == 6 {
        return src_at(p) * pc.args.w;
    }
    if mode == 7 {
        var a = mask_at(p, pc.p0.xy);
        if pc.p1.z > 0.5 {
            a = a * mask_at(p, pc.p1.xy);
        }
        return pc.p2 * a;
    }
    return vec4<f32>(0.0);
}

// The blur's tap at `q` pixels along a scanline from `anchor`, read in the
// pass's edge mode — the same two reads the per-pixel tap loop made at each
// offset.
fn blur_tap(anchor: vec2<i32>, step: vec2<i32>, q: i32, clamped: bool) -> vec4<f32> {
    if clamped {
        return src_clamped(anchor + step * q);
    }
    return src_at(anchor + step * q);
}

// Modes 1 (horizontal) and 2 (vertical): a box blur of radius args.y,
// clamped to the source rectangle when args.z > 0.5.
//
// A running sum — the form the CPU kernels already use (`effects.rs`'s
// `horizontal`/`vertical` for the clamp mode, `shadow.rs`'s
// `box_blur_horizontal`/`box_blur_vertical` for the transparent one): seed
// the window once at the scanline's first pixel, then advance it with one
// add and one sub per pixel — `sum + (entering - leaving)` — instead of
// gathering all `2r + 1` taps again at every pixel.
//
// **Seed derivation** (the window's content at x = 0 / y = 0, which the
// tap loop gathered as taps `-r..=r`):
//
// * *Duplicate* (`args.z > 0.5`, the backdrop/layer edge mode): taps before
//   the near edge read the edge pixel, so the first sample counts `r + 1`
//   times, then offsets `1..=min(r, span - 1)` follow, and when
//   `span <= r` the taps past the far edge repeat the last sample
//   `r + 1 - span` times — `src_clamped`'s clamp written out as a sum, the
//   same three adds `effects.rs`'s seed makes, in the same order.
// * *Transparent* (the shadow edge mode): taps before the near edge are
//   transparent, so the seed sums offsets `0..=min(r, span - 1)` over a
//   zero — the out-of-window half of `shadow.rs`'s seed, which starts its
//   sum at 0 for exactly this reason; taps past the far edge read as
//   transparent too, just as `src_at` reads them.
//
// The slide reads the entering tap (`i + r + 1`) and the leaving tap
// (`i - r`) through the same edge-mode read the tap loop used, so every
// pixel's window holds the same samples the tap loop gathered; only the
// order the additions happen in differs, which is the one place the parity
// suites' 2/255 tolerance is the referee rather than byte identity — and
// the CPU oracle runs this same running order, so the shader now follows
// it addition for addition.
//
// Still divided by `2r + 1`, never multiplied by a reciprocal: the CPU
// divides (`effects.rs`: "`x / w` and `x * (1/w)` differ in the last bit"),
// and the running form's subtract-then-add is already the drift that
// tolerance budget exists to cover.
fn box_blur(p: vec2<i32>, mode: i32) -> vec4<f32> {
    let r = i32(round(pc.args.y));
    var step = vec2<i32>(1, 0);
    if mode == 2 {
        step = vec2<i32>(0, 1);
    }
    let clamped = pc.args.z > 0.5;
    let lo = vec2<i32>(i32(pc.region.x), i32(pc.region.y));
    // This pixel's scanline — its row for a horizontal pass, its column for
    // a vertical one — anchored at the region's near edge: `anchor` is the
    // scanline's first pixel, `span` its length, `pos` this pixel's offset
    // along it. Fragments run only inside the scissored band, which the
    // executor keeps inside the read rectangle, so `pos` is in 0..span.
    var anchor = vec2<i32>(lo.x, p.y);
    var span = i32(pc.region.z) - i32(pc.region.x);
    var pos = p.x - lo.x;
    if mode == 2 {
        anchor = vec2<i32>(p.x, lo.y);
        span = i32(pc.region.w) - i32(pc.region.y);
        pos = p.y - lo.y;
    }
    let far = min(r, span - 1);

    // **The cheaper of the two forms, per pixel.** A fragment cannot carry a
    // running sum from its neighbour, so the running form re-derives its
    // window from the scanline's seed: `far + 1` loads (plus one for a
    // short span's far edge) and then two per slide step to reach `pos` -
    // `2 * pos + far + 2` reads against the tap loop's `2r + 1`. That only
    // wins near the scanline's start: past `pos ~ r / 2` it costs more,
    // and on a 1080-px-wide radius-24 layer the average pixel paid ~1,100
    // reads instead of 49 (the running form as first landed, 77c6a3c).
    // Both forms gather the same window and both sit inside the parity
    // suites' 2/255 tolerance (the tap loop is the form those suites
    // certified for years), so taking the cheaper one per pixel moves
    // nothing the tolerance does not already cover. The one-add-one-sub
    // per pixel win needs rows carried across pixels - a compute pass -
    // which is the follow-up 77c6a3c names.
    if 2 * pos + far + 2 >= 2 * r + 1 {
        var taps = vec4<f32>(0.0);
        // `0 - r`, not `-r`: naga lowers integer negation to
        // `asint(-asuint(v))`, and FXC aborts compiling that (see fs_main).
        for (var k = 0 - r; k <= r; k = k + 1) {
            if clamped {
                taps = taps + src_clamped(p + step * k);
            } else {
                taps = taps + src_at(p + step * k);
            }
        }
        return taps / f32(2 * r + 1);
    }

    var sum = vec4<f32>(0.0);
    if clamped {
        // Duplicate: the first sample counts r + 1 times — once here, r
        // more below; one load, the same r adds the CPU's seed makes of
        // `src_row[0]` (likewise the far-edge repeats at the bottom).
        let first = blur_tap(anchor, step, 0, true);
        sum = first;
        for (var i = 0; i < r; i = i + 1) {
            sum = sum + first;
        }
        for (var i = 1; i <= far; i = i + 1) {
            sum = sum + blur_tap(anchor, step, i, true);
        }
        if span <= r {
            let last = blur_tap(anchor, step, span - 1, true);
            for (var i = 0; i < r + 1 - span; i = i + 1) {
                sum = sum + last;
            }
        }
    } else {
        // Transparent: only the in-range taps contribute; the rest of the
        // window is the zeros the sum started at.
        for (var i = 0; i <= far; i = i + 1) {
            sum = sum + blur_tap(anchor, step, i, false);
        }
    }

    // One add and one sub per pixel between the seed and this one. All
    // offsets are binary subtractions (`i - r`, never `-r`): naga lowers
    // integer negation to `asint(-asuint(v))`, which FXC aborts on (see
    // fs_main).
    for (var i = 0; i < pos; i = i + 1) {
        sum = sum + (blur_tap(anchor, step, i + r + 1, clamped)
            - blur_tap(anchor, step, i - r, clamped));
    }
    return sum / f32(2 * r + 1);
}
