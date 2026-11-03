// One level of the environment prefilter chain — the split sum's first
// half (Karis 2013, "Real Shading in Unreal Engine 4" §3.2), written out
// line for line from `vieww_3d::environment`'s CPU oracle so the GPU
// convolves the same lobe with the same samples:
//
//   for each destination texel, direction r (the equirect mapping of its
//   centre), with a = roughness²:
//     (t, b) = the orthonormal frame around r            (the CPU's `onb`)
//     for i in 0..samples:
//       (xi1, xi2) = Hammersley(i, samples)               (radical inverse
//                                                          via reverseBits)
//       phi   = 2π · xi1
//       cos_t = sqrt((1 − xi2) / (1 + (a² − 1) · xi2))    (GGX half-vector
//                                                          distribution, NDF
//                                                          importance sample)
//       sin_t = sqrt(max(0, 1 − cos_t²))
//       hv    = normalize(t·(sin_t·cosφ) + b·(sin_t·sinφ) + r·cos_t)
//       l     = normalize(hv · (2·r·hv) − r)              (the reflection of
//                                                          r about hv)
//       ndl   = r·l
//       if ndl > 0: acc += source(l) · ndl;  wsum += ndl
//     result = wsum > 0 ? acc / wsum : source(r)
//
// That is `Environment::with_levels`'s `EnvMap::from_fn` closure verbatim,
// the same arithmetic in the same order — `vieww_3d::environment::
// prefilter_texel` is the named Rust half of this pin, and
// `vieww-shaders`' `library.rs` tests hold both to the formulas below by
// string, so neither side can drift without a red test somewhere. The
// numeric parity itself is `vieww-3d`'s `tests/gpu_ibl.rs` (CI lavapipe
// leg): GPU level vs CPU `Environment::specular[k]`, per texel.
//
// # What the pass consumes and produces
//
// The SOURCE is one mip of the environment's own mip chain — the CPU's
// "filtered importance sampling" picks the mip whose texel footprint
// roughly matches the GGX lobe (`round(roughness · (mips − 1))`), and the
// caller uploads exactly that mip as its own single-mip RGBA16F texture
// (`vieww_hal::vulkan::VulkanDevice::render_env_prefilter_level` takes it
// as `[[f32; 4]]` and does the f16 conversion the 3D passes' textures do).
// Sampling it here is `textureSampleLevel(…, 0.0)` with a bilinear,
// u-REPEAT sampler — which is the CPU `EnvMap::sample` to within the f16
// rounding of the upload:
//
// * `u` wraps on the CPU by `rem_euclid`, the sampler by REPEAT — the same
//   for every u in (0, 1).
// * `v` clamps on the CPU (the equirect poles), so it is clamped here to
//   the row centres `[0.5/h, 1 − 0.5/h]` before sampling, which is
//   clamp-to-edge — the CPU's `texel` row clamp — to the bit.
// * `u` exactly 1.0 (a direction exactly along −z with x ≥ 0) is the one
//   coordinate where floor-based wrapping and REPEAT disagree on which two
//   columns mix; unreachable from real convolution directions, and the
//   parity suite would catch it if it ever were.
//
// The DESTINATION is one level of the specular chain: a fullscreen quad
// over `(width, height)`, each fragment one texel, its `r` the equirect
// direction of the texel centre — `(u, v) = ((x + 0.5)/w, (y + 0.5)/h)`
// through `uv_to_dir`, the CPU's own mapping (`phi = (u − 0.5)·2π`,
// `theta = v·π`). Row 0 is the zenith, as `EnvMap`'s rows are.
//
// The output is the prefiltered radiance in LINEAR light with alpha 1 —
// values above 1 (the sun in an HDRI) survive because the caller reads the
// f16 target back as linear f32, not as sRGB-encoded bytes. That is the
// one thing this pass needs that the 3D display passes do not, and why it
// has its own entry point instead of reusing `render_lit3d_to_pixels`.
//
// # What is deliberately NOT here
//
// The split sum's second half (the environment BRDF) is not a convolution:
// `vieww_3d::environment::env_brdf` is Karis' analytic fit, three lines of
// ALU with no texture and no reason to be one. The diffuse half of IBL is
// nine SH coefficients, also not a convolution at run time. GPU shading
// terms that USE this chain (the lit pass's `ambient_ibl`) are the next
// increment — `vieww-3d/src/gpu.rs` still falls back to the CPU renderer
// for any scene with an environment, and says so in its module doc.

const PI: f32 = 3.14159265359;

struct Uniforms {
    // x = roughness, y = a = roughness², z = sample count, w unused.
    params: vec4<f32>,
    // x = destination width, y = destination height,
    // z = source width, w = source height.
    dims: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

// The source mip — see the module doc for which mip and why level 0.
@group(0) @binding(2)
var t_src: texture_2d<f32>;
@group(0) @binding(3)
var s_src: sampler;

struct VertexIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(v.pos, 0.0, 1.0);
    out.uv = v.uv;
    return out;
}

// The CPU's `EnvMap::uv_to_dir`: `phi = (u − 0.5)·2π`, `theta = v·π`.
fn uv_to_dir(uv: vec2<f32>) -> vec3<f32> {
    let phi = (uv.x - 0.5) * 2.0 * PI;
    let theta = uv.y * PI;
    let s = sin(theta);
    return vec3<f32>(s * sin(phi), cos(theta), -s * cos(phi));
}

// The CPU's `onb`: `up = |n.y| < 0.999 ? Y : X`, `t = normalize(up × n)`,
// `b = n × t`. WGSL has no tuples, so the frame rides a struct.
struct Onb {
    t: vec3<f32>,
    b: vec3<f32>,
};
fn onb(n: vec3<f32>) -> Onb {
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.999);
    let t = normalize(cross(up, n));
    var out: Onb;
    out.t = t;
    out.b = cross(n, t);
    return out;
}

// The CPU's `EnvMap::sample`: the equirect mapping, then bilinear at the
// texel centres with u wrapping and v clamping — see the module doc.
fn source(d: vec3<f32>) -> vec3<f32> {
    let du = normalize(d);
    let uv = vec2<f32>(
        0.5 + atan2(du.x, -du.z) / (2.0 * PI),
        acos(clamp(du.y, -1.0, 1.0)) / PI,
    );
    let half_texel = 0.5 / u.dims.zw;
    let v = clamp(uv.y, half_texel.y, 1.0 - half_texel.y);
    return textureSampleLevel(t_src, s_src, vec2<f32>(uv.x, v), 0.0).rgb;
}

@fragment
fn fs_main(f: VertexOut) -> @location(0) vec4<f32> {
    let r = uv_to_dir(f.uv);
    let a = u.params.y;
    let samples = u32(u.params.z);
    let frame = onb(r);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0u; i < samples; i++) {
        // Hammersley: the radical inverse by bit reversal, the CPU's
        // `i.reverse_bits() as f32 * 2.328_306_4e-10`.
        let xi1 = f32(i) / f32(samples);
        let xi2 = f32(reverseBits(i)) * 2.3283064e-10;
        let phi = 2.0 * PI * xi1;
        // The GGX half-vector around N = V = R.
        let cos_t = sqrt((1.0 - xi2) / (1.0 + (a * a - 1.0) * xi2));
        let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
        let hv = normalize(frame.t * (sin_t * cos(phi)) + frame.b * (sin_t * sin(phi)) + r * cos_t);
        let l = normalize(hv * (2.0 * dot(r, hv)) - r);
        let ndl = dot(r, l);
        if (ndl > 0.0) {
            acc = acc + source(l) * ndl;
            wsum = wsum + ndl;
        }
    }
    var out = vec3<f32>(0.0);
    if (wsum > 0.0) {
        out = acc / wsum;
    } else {
        out = source(r);
    }
    return vec4<f32>(out, 1.0);
}
