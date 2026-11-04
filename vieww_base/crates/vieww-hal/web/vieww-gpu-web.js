// The browser side of vieww-hal's WebGPU backend (src/webgpu.rs, FW-09).
//
// The Rust code declares the import module "vieww_gpu_web" and calls it
// through raw `extern "C"` — no wasm-bindgen, the same shape
// `vieww-audio`'s web backend uses. This file is that module: every
// function the Rust side imports is exported here by its exact name, so
// a page can pass the module itself as the import object:
//
//   <script type="importmap">
//     { "imports": { "vieww_gpu_web": "./vieww-gpu-web.js" } }
//   </script>
//   import * as gpu from "vieww_gpu_web";
//   const { instance } = await WebAssembly.instantiateStreaming(
//     fetch("app.wasm"), { vieww_gpu_web: gpu });
//   gpu.attach(instance.exports);
//
// `attach` hands the shim the wasm memory — every pointer argument below
// is an offset into it. The shim is also usable under any other runtime
// that can supply the module (the audio shim's contract, kept here).
//
// Everything that crosses the boundary is u32 handles, byte counts and
// byte slices; no JS values cross, so nothing here needs a bindings
// crate. Handles are registry indices this file owns:
//
//   * device handles from `vieww_gpu_request` — adapter/device bring-up,
//     kept in a registry because bring-up is asynchronous and the Rust
//     side polls `vieww_gpu_state`;
//   * texture handles from `vieww_gpu_target_create` / `vieww_gpu_atlas`
//     — one registry per device. A texture handle is valid until freed
//     (targets) or replaced (an atlas slot's texture is recreated).
//
// # The frame contract (mirrors the Vulkan/Metal/D3D12 scene ports)
//
// Targets are premultiplied rgba16float (RENDER_ATTACHMENT |
// TEXTURE_BINDING | COPY_SRC). The four atlas slots are r8unorm glyphs
// (0), rgba8unorm images (1), r8unorm masks (2) and rgba32float ramps
// (3). The scene pipeline is `vieww-shaders`' scene.wgsl verbatim; the
// post pipelines are post.wgsl with its `var<immediate>` block rewritten
// to a `var<uniform>` at group 1 binding 0 (WebGPU has no push
// constants) — the rewrite happens on the Rust side, this side only
// compiles the text it is handed.
//
// # Asynchrony
//
// WebGPU's two asynchronous moments (adapter/device bring-up, the
// readback's mapAsync) live behind poll-shaped imports: 0 pending, 1
// ready, 2 failed. Validation errors that WebGPU reports asynchronously
// (a rejected pipeline, a bad bind group) are recorded on the device
// entry and surface through the same state imports — the Rust side never
// blocks on a promise, because wasm cannot.

"use strict";

let mem = null;

/** Hand the shim the wasm exports (the memory, at minimum). */
export function attach(exports) {
  mem = exports.memory || exports;
}

function needMem() {
  if (!mem) throw new Error("vieww-gpu-web: attach(exports) was not called first");
  return mem;
}

function bytes(ptr, len) {
  return new Uint8Array(needMem().buffer, ptr, len);
}

function text(ptr, len) {
  return new TextDecoder().decode(bytes(ptr, len));
}

// ── the registry ───────────────────────────────────────────────────────────

const devices = [];

function entryOf(handle) {
  return devices[handle] || null;
}

function record(entry, message) {
  if (entry && entry.state !== 2) {
    entry.failure = String(message);
    if (entry.state === 1) entry.state = 2;
  }
}

function takeHandle() {
  for (let i = 0; i < devices.length; i++) {
    if (!devices[i]) return i;
  }
  devices.push(null);
  return devices.length - 1;
}

// f32 -> f16 bits, the standard round-to-nearest conversion. Region
// clears copy raw half-float texels, so the clear colour must be encoded.
function toHalf(value) {
  const f = new Float32Array(1);
  const u = new Uint32Array(f.buffer);
  f[0] = value;
  const x = u[0];
  const sign = (x >>> 16) & 0x8000;
  const exponent = (x >>> 23) & 0xff;
  const mantissa = x & 0x7fffff;
  if (exponent === 0xff) return sign | 0x7c00 | (mantissa ? 1 : 0);
  if (exponent < 0x71) return sign; // underflow (and zero)
  if (exponent > 0x8e) return sign | 0x7bff; // saturate
  return sign | ((exponent - 127 + 15) << 10) | (mantissa >>> 13);
}

// Row stride for a `width`-texel row of `bpp`-byte texels: WebGPU
// requires copy/write rows to be multiples of 256 bytes.
function strideFor(width, bpp) {
  return Math.max(256, Math.ceil((width * bpp) / 256) * 256);
}

function align4(n) {
  return Math.ceil(n / 4) * 4;
}

// Writes `message` into the wasm buffer at `ptr` (at most `cap` bytes,
// NUL-terminated) and returns the message's full byte length — the
// two-call length-then-buffer contract the Rust side's `shim_string`
// implements. `ptr` 0 is a length-only query.
function writeString(message, ptr, cap) {
  const encoded = new TextEncoder().encode(String(message));
  if (ptr !== 0 && cap > 0) {
    const dst = bytes(ptr, cap);
    const n = Math.min(encoded.length, cap - 1);
    dst.set(encoded.subarray(0, n), 0);
    dst[n] = 0;
  }
  return encoded.length;
}

// ── availability and bring-up ──────────────────────────────────────────────

export function vieww_gpu_available() {
  return typeof navigator !== "undefined" && navigator.gpu ? 1 : 0;
}

export function vieww_gpu_request() {
  const handle = takeHandle();
  const entry = {
    state: 0,
    failure: "",
    adapter: null,
    device: null,
    info: "",
    textures: new Map(),
    atlases: [0, 0, 0, 0],
    nextTexture: 1,
    dummy: 0,
    layouts: null,
    pipelines: null,
    vertexBuffer: null,
    vertexSize: 0,
    indexBuffer: null,
    indexSize: 0,
    viewportBuffer: null,
    pushBuffer: null,
    pushBindGroup: null,
    // The immediates ring's write cursor — 0 from the start (the first
    // browser run found it missing: `undefined * 256` is NaN, and
    // writeBuffer's offset must be an unsigned long long).
    pushCursor: 0,
    encoder: null,
    openPass: null,
    openTarget: 0,
    frameW: 0,
    frameH: 0,
    sceneGroup: null,
    sceneGroupKey: "",
    postGroups: new Map(),
    readback: null,
    staging: [],
  };
  devices[handle] = entry;
  bringUp(entry).catch((e) => {
    entry.state = 2;
    entry.failure = String(e);
  });
  return handle;
}

async function bringUp(entry) {
  if (!navigator.gpu) {
    entry.state = 2;
    entry.failure = "navigator.gpu is absent";
    return;
  }
  // `let`, not `const`: the fallback request below rebinds it when the
  // first request returns null (a headless engine without a hardware
  // adapter). A `const` here would surface as "TypeError: Assignment to
  // constant variable" in the catch below — a JS engine would find it in
  // one run; review found it first.
  let adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    // No hardware adapter — a headless CI browser, or a machine whose GPU
    // WebGPU will not speak. The fallback adapter (SwiftShader in
    // Chromium) is WebGPU's own CPU answer, and asking for it explicitly
    // is the same decision the rasterizer_choice makes one level up: the
    // GPU path stays available on machines without a GPU, and a browser
    // that has no fallback either reports null here again — which is the
    // real "no WebGPU" this entry's failure message names.
    adapter = await navigator.gpu.requestAdapter({
      forceFallbackAdapter: true,
    });
  }
  if (!adapter) {
    entry.state = 2;
    entry.failure =
      "requestAdapter() returned null (no adapter; a headless engine without software WebGPU does this)";
    return;
  }
  const device = await adapter.requestDevice();
  device.onuncapturederror = (event) => {
    record(entry, (event.error && event.error.message) || "uncaptured WebGPU error");
  };
  const info = adapter.info || {};
  entry.adapter = adapter;
  entry.device = device;
  entry.info = info.description || info.vendor || "WebGPU adapter";
  entry.state = 1;
}

export function vieww_gpu_state(handle) {
  const entry = entryOf(handle);
  return entry ? entry.state : 2;
}

export function vieww_gpu_failure(handle, buf, cap) {
  const entry = entryOf(handle);
  return writeString(entry ? entry.failure : "no such device handle", buf, cap);
}

export function vieww_gpu_adapter_name(handle, buf, cap) {
  const entry = entryOf(handle);
  return writeString(entry ? entry.info : "no such device handle", buf, cap);
}

export function vieww_gpu_release(handle) {
  const entry = entryOf(handle);
  if (!entry) return;
  try {
    if (entry.device && entry.device.destroy) entry.device.destroy();
  } catch (_) {
    /* a lost device is already gone */
  }
  devices[handle] = null;
}

// ── resources ──────────────────────────────────────────────────────────────

const ATLAS_FORMATS = ["r8unorm", "rgba8unorm", "r8unorm", "rgba32float"];
const ATLAS_BPP = [1, 4, 1, 16];

function registerTexture(entry, texture, kind, width, height, bpp) {
  const handle = entry.nextTexture++;
  entry.textures.set(handle, { texture, kind, width, height, bpp });
  return handle;
}

export function vieww_gpu_target_create(handle, width, height) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return 0;
  const texture = entry.device.createTexture({
    format: "rgba16float",
    size: [Math.max(1, width), Math.max(1, height), 1],
    usage:
      GPUTextureUsage.RENDER_ATTACHMENT |
      GPUTextureUsage.TEXTURE_BINDING |
      GPUTextureUsage.COPY_SRC,
  });
  return registerTexture(entry, texture, "target", Math.max(1, width), Math.max(1, height), 8);
}

export function vieww_gpu_target_free(handle, target) {
  const entry = entryOf(handle);
  if (!entry) return;
  const held = entry.textures.get(target);
  if (held) {
    held.texture.destroy();
    entry.textures.delete(target);
    // Cached bind groups may still name it; drop them all (rebuilding a
    // bind group is cheap, a stale one is a validation error).
    entry.postGroups.clear();
    entry.sceneGroupKey = "";
  }
}

export function vieww_gpu_atlas(handle, slot, width, height) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return 0;
  const old = entry.atlases[slot];
  if (old) {
    const held = entry.textures.get(old);
    if (held) {
      held.texture.destroy();
      entry.textures.delete(old);
    }
  }
  const texture = entry.device.createTexture({
    format: ATLAS_FORMATS[slot],
    size: [Math.max(1, width), Math.max(1, height), 1],
    usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
  });
  const h = registerTexture(
    entry,
    texture,
    "atlas",
    Math.max(1, width),
    Math.max(1, height),
    ATLAS_BPP[slot]
  );
  entry.atlases[slot] = h;
  entry.sceneGroupKey = ""; // the scene bind group binds all four slots
  return h;
}

export function vieww_gpu_atlas_write(handle, slot, data, len) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return;
  const held = entry.textures.get(entry.atlases[slot]);
  if (!held || len === 0) return;
  const { width, height, bpp } = held;
  const rowBytes = width * bpp;
  const stride = strideFor(width, bpp);
  const wanted = rowBytes * height;
  const source = bytes(data, Math.min(len, wanted));
  let payload = source;
  if (stride !== rowBytes || source.length < wanted) {
    // Re-lay the rows into a 256-byte-aligned stride, zero-filled where
    // the Rust side sent fewer texels than the texture's extent (an
    // atlas shorter than its declared side reads as transparent).
    payload = new Uint8Array(stride * height);
    for (let y = 0; y < height; y++) {
      const from = Math.min(source.length, y * rowBytes);
      const count = Math.min(rowBytes, source.length - from);
      if (count > 0) payload.set(source.subarray(from, from + count), y * stride);
    }
  }
  entry.device.queue.writeTexture(
    { texture: held.texture },
    payload,
    { bytesPerRow: stride, rowsPerImage: height },
    [width, height, 1]
  );
}

export function vieww_gpu_buffer_write(handle, which, data, len) {
  const entry = entryOf(handle);
  if (!entry || !entry.device || len === 0) return;
  const d = entry.device;
  let buffer = which === 0 ? entry.vertexBuffer : entry.indexBuffer;
  let size = which === 0 ? entry.vertexSize : entry.indexSize;
  if (!buffer || size < len) {
    if (buffer) buffer.destroy();
    size = Math.max(64, align4(len));
    buffer = d.createBuffer({
      size,
      usage: GPUBufferUsage.COPY_DST | (which === 0 ? GPUBufferUsage.VERTEX : GPUBufferUsage.INDEX),
    });
    if (which === 0) {
      entry.vertexBuffer = buffer;
      entry.vertexSize = size;
    } else {
      entry.indexBuffer = buffer;
      entry.indexSize = size;
    }
  }
  d.queue.writeBuffer(buffer, 0, needMem().buffer, data, len);
}

// ── pipelines ──────────────────────────────────────────────────────────────

// Runs `build` under a validation error scope, recording any error it
// produces onto the entry (where the state imports report it). JS
// exceptions are recorded synchronously; WebGPU validation errors arrive
// when the scope pops. Both paths leave `failure` set, so the Rust side
// discovers a rejected pipeline the next time it polls.
function guarded(entry, build) {
  const d = entry.device;
  let ok = true;
  try {
    d.pushErrorScope("validation");
    build();
  } catch (e) {
    ok = false;
    record(entry, e && e.message ? e.message : String(e));
  }
  d.popErrorScope().then(
    (err) => {
      if (err) record(entry, err.message);
    },
    () => {}
  );
  return ok ? 1 : 0;
}

// `vieww_gpu::Vertex` as WebGPU sees it: 27 f32s, 108 bytes, with WGSL
// location 7 (mask) at float offset 23 and location 8 (shape) at 19 —
// the Rust declaration order and the shader's location order differ for
// those two, exactly as the Vulkan port's attribute table records.
const SCENE_VERTEX = [
  { shaderLocation: 0, offset: 0, format: "float32x2" }, // position
  { shaderLocation: 1, offset: 8, format: "float32x4" }, // color
  { shaderLocation: 2, offset: 24, format: "float32x2" }, // uv
  { shaderLocation: 3, offset: 32, format: "float32" }, // texture kind
  { shaderLocation: 4, offset: 36, format: "float32x4" }, // params
  { shaderLocation: 5, offset: 52, format: "float32x2" }, // local
  { shaderLocation: 6, offset: 60, format: "float32x4" }, // extra
  { shaderLocation: 7, offset: 92, format: "float32x4" }, // mask
  { shaderLocation: 8, offset: 76, format: "float32x4" }, // shape
];
const SCENE_STRIDE = 108;

// Premultiplied source-over — the blend every scene draw composites with.
const OVER_BLEND = {
  color: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
  alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
};

function layoutsOf(entry) {
  if (entry.layouts) return entry.layouts;
  const d = entry.device;
  const sceneGroup = d.createBindGroupLayout({
    entries: [
      { binding: 0, visibility: GPUShaderStage.VERTEX, buffer: { type: "uniform" } },
      { binding: 1, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
      { binding: 2, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
      { binding: 3, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
      // rgba32float ramps: textureLoad only, hence unfilterable.
      { binding: 4, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "unfilterable-float" } },
    ],
  });
  const postGroup0 = d.createBindGroupLayout({
    entries: [
      { binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
      { binding: 1, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
      { binding: 2, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
    ],
  });
  // post.wgsl's immediates block as a uniform: dynamic offsets must be
  // 256-byte aligned, so the ring slots are 256 bytes for a 128-byte
  // payload — the standard WebGPU push-constant workaround.
  const postGroup1 = d.createBindGroupLayout({
    entries: [
      {
        binding: 0,
        visibility: GPUShaderStage.FRAGMENT,
        buffer: { type: "uniform", hasDynamicOffset: true, minBindingSize: 128 },
      },
    ],
  });
  entry.layouts = {
    sceneGroup,
    postGroup0,
    postGroup1,
    scenePipelineLayout: d.createPipelineLayout({ bindGroupLayouts: [sceneGroup] }),
    postPipelineLayout: d.createPipelineLayout({ bindGroupLayouts: [postGroup0, postGroup1] }),
  };
  return entry.layouts;
}

export function vieww_gpu_pipeline_scene(handle, wgsl, len) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return 0;
  const layouts = layoutsOf(entry);
  return guarded(entry, () => {
    const module = entry.device.createShaderModule({ code: text(wgsl, len) });
    entry.pipelines = entry.pipelines || {};
    entry.pipelines.scene = entry.device.createRenderPipeline({
      layout: layouts.scenePipelineLayout,
      vertex: {
        module,
        entryPoint: "vs_main",
        buffers: [{ arrayStride: SCENE_STRIDE, attributes: SCENE_VERTEX }],
      },
      fragment: {
        module,
        entryPoint: "fs_main",
        targets: [{ format: "rgba16float", blend: OVER_BLEND }],
      },
      primitive: { topology: "triangle-list", cullMode: "none" },
    });
  });
}

export function vieww_gpu_pipeline_post(handle, wgsl, len, over) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return 0;
  const layouts = layoutsOf(entry);
  return guarded(entry, () => {
    const module = entry.device.createShaderModule({ code: text(wgsl, len) });
    entry.pipelines = entry.pipelines || {};
    entry.pipelines[over ? "postOver" : "postReplace"] = entry.device.createRenderPipeline({
      layout: layouts.postPipelineLayout,
      vertex: { module, entryPoint: "vs_main" },
      fragment: {
        module,
        entryPoint: "fs_main",
        targets: [{ format: "rgba16float", blend: over ? OVER_BLEND : undefined }],
      },
      primitive: { topology: "triangle-list", cullMode: "none" },
    });
    if (!entry.pushBuffer) {
      entry.pushBuffer = entry.device.createBuffer({
        size: 256 * 1024,
        usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
      });
      entry.pushBindGroup = entry.device.createBindGroup({
        layout: layouts.postGroup1,
        entries: [{ binding: 0, resource: { buffer: entry.pushBuffer, size: 128 } }],
      });
    }
  });
}

// ── one frame ──────────────────────────────────────────────────────────────

export function vieww_gpu_frame_begin(handle, width, height) {
  const entry = entryOf(handle);
  if (!entry || !entry.device) return;
  if (!entry.viewportBuffer) {
    entry.viewportBuffer = entry.device.createBuffer({
      size: 16,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
  }
  if (entry.openPass) {
    entry.openPass.end();
    entry.openPass = null;
  }
  entry.device.queue.writeBuffer(entry.viewportBuffer, 0, new Float32Array([width, height, 0, 0]));
  entry.encoder = entry.device.createCommandEncoder();
  entry.openTarget = 0;
  entry.frameW = width;
  entry.frameH = height;
}

export function vieww_gpu_pass_begin(handle, target, clear) {
  const entry = entryOf(handle);
  if (!entry || !entry.device || !entry.encoder) return;
  if (entry.openPass) {
    entry.openPass.end();
    entry.openPass = null;
  }
  const held = entry.textures.get(target);
  if (!held) {
    record(entry, `pass_begin on unknown texture handle ${target}`);
    return;
  }
  const clearValue = { r: 0, g: 0, b: 0, a: 0 };
  if (clear !== 0) {
    const c = new Float32Array(needMem().buffer, clear, 4);
    clearValue.r = c[0];
    clearValue.g = c[1];
    clearValue.b = c[2];
    clearValue.a = c[3];
  }
  entry.openPass = entry.encoder.beginRenderPass({
    colorAttachments: [
      {
        view: held.texture.createView(),
        loadOp: clear === 0 ? "load" : "clear",
        storeOp: "store",
        clearValue,
      },
    ],
  });
  entry.openTarget = target;
}

export function vieww_gpu_pass_end(handle) {
  const entry = entryOf(handle);
  if (!entry) return;
  if (entry.openPass) {
    entry.openPass.end();
    entry.openPass = null;
  }
}

export function vieww_gpu_pass_scissor(handle, x, y, w, h) {
  const entry = entryOf(handle);
  if (!entry || !entry.openPass) return;
  entry.openPass.setScissorRect(x, y, w, h);
}

function sceneBindGroup(entry) {
  const key = entry.atlases.join("|");
  if (entry.sceneGroup && entry.sceneGroupKey === key) return entry.sceneGroup;
  const group = entry.device.createBindGroup({
    layout: layoutsOf(entry).sceneGroup,
    entries: [
      { binding: 0, resource: { buffer: entry.viewportBuffer } },
      ...entry.atlases.map((h, i) => ({
        binding: i + 1,
        resource: entry.textures.get(h).texture.createView(),
      })),
    ],
  });
  entry.sceneGroup = group;
  entry.sceneGroupKey = key;
  return group;
}

export function vieww_gpu_pass_scene(handle) {
  const entry = entryOf(handle);
  if (!entry || !entry.openPass || !entry.pipelines || !entry.pipelines.scene) return;
  const pass = entry.openPass;
  pass.setPipeline(entry.pipelines.scene);
  if (entry.vertexBuffer) pass.setVertexBuffer(0, entry.vertexBuffer);
  if (entry.indexBuffer) pass.setIndexBuffer(entry.indexBuffer, "uint32");
  pass.setBindGroup(0, sceneBindGroup(entry));
}

export function vieww_gpu_pass_draw(handle, first_index, count) {
  const entry = entryOf(handle);
  if (!entry || !entry.openPass) return;
  entry.openPass.drawIndexed(count, 1, first_index, 0, 0);
}

function postBindGroup(entry, src, aux) {
  const key = src + "|" + aux;
  let group = entry.postGroups.get(key);
  if (group) return group;
  const srcHeld = entry.textures.get(src);
  const auxHeld = entry.textures.get(aux);
  const maskHeld = entry.textures.get(entry.atlases[2]);
  if (!srcHeld || !auxHeld || !maskHeld) {
    record(entry, `post bind on unknown texture (${src}/${aux})`);
    return null;
  }
  group = entry.device.createBindGroup({
    layout: layoutsOf(entry).postGroup0,
    entries: [
      { binding: 0, resource: srcHeld.texture.createView() },
      { binding: 1, resource: auxHeld.texture.createView() },
      { binding: 2, resource: maskHeld.texture.createView() },
    ],
  });
  entry.postGroups.set(key, group);
  return group;
}

export function vieww_gpu_pass_post(handle, src, aux, over, push) {
  const entry = entryOf(handle);
  if (!entry || !entry.openPass || !entry.pipelines) return;
  const pass = entry.openPass;
  const pipelines = entry.pipelines;
  pass.setPipeline(over ? pipelines.postOver : pipelines.postReplace);
  const group0 = postBindGroup(entry, src, aux);
  if (!group0) return;
  pass.setBindGroup(0, group0);
  // One ring slot per post op: writes to distinct 256-byte regions of
  // the same buffer queue safely ahead of this frame's submit, and the
  // dynamic offset hands each op its own immediates block.
  const offset = entry.pushCursor * 256;
  entry.pushCursor = (entry.pushCursor + 1) % 1024;
  entry.device.queue.writeBuffer(
    entry.pushBuffer,
    offset,
    new Float32Array(needMem().buffer, push, 32)
  );
  pass.setBindGroup(1, entry.pushBindGroup, [offset]);
}

export function vieww_gpu_pass_post_draw(handle) {
  const entry = entryOf(handle);
  if (!entry || !entry.openPass) return;
  entry.openPass.draw(3);
}

export function vieww_gpu_clear_regions(handle, rects, count, color) {
  const entry = entryOf(handle);
  if (!entry || !entry.device || !entry.encoder || count === 0) return;
  const held = entry.textures.get(entry.openTarget);
  if (!held) return;
  // Metal's decision, kept: a region clear is buffer-to-texture copies,
  // no shader and no binding. The pass must end while the copies run and
  // reopen (loading) afterwards, so the interpreter's "a pass is open"
  // bookkeeping stays true on both sides of the boundary.
  if (entry.openPass) {
    entry.openPass.end();
    entry.openPass = null;
  }
  const c = new Float32Array(needMem().buffer, color, 4);
  const rgba = [toHalf(c[0]), toHalf(c[1]), toHalf(c[2]), toHalf(c[3])];
  const rects32 = new Uint32Array(needMem().buffer, rects, count * 4);
  for (let i = 0; i < count; i++) {
    const x = rects32[i * 4];
    const y = rects32[i * 4 + 1];
    const w = rects32[i * 4 + 2];
    const h = rects32[i * 4 + 3];
    if (w === 0 || h === 0) continue;
    const rowBytes = w * 8;
    const stride = strideFor(w, 8);
    const staging = entry.device.createBuffer({
      size: align4(stride * h),
      usage: GPUBufferUsage.MAP_WRITE | GPUBufferUsage.COPY_SRC,
      mappedAtCreation: true,
    });
    const mapped = new Uint8Array(staging.getMappedRange());
    const row = new Uint8Array(rowBytes);
    for (let t = 0; t < w; t++) {
      for (let ch = 0; ch < 4; ch++) {
        row[t * 8 + ch * 2] = rgba[ch] & 0xff;
        row[t * 8 + ch * 2 + 1] = rgba[ch] >>> 8;
      }
    }
    for (let yy = 0; yy < h; yy++) {
      mapped.set(row, yy * stride);
    }
    staging.unmap();
    entry.encoder.copyBufferToTexture(
      { buffer: staging, bytesPerRow: stride, rowsPerImage: h },
      { texture: held.texture, origin: { x, y, z: 0 } },
      [w, h, 1]
    );
    entry.staging.push(staging);
  }
  // Reopen the interrupted pass, loading what the copies left behind.
  entry.openPass = entry.encoder.beginRenderPass({
    colorAttachments: [
      { view: held.texture.createView(), loadOp: "load", storeOp: "store" },
    ],
  });
}

export function vieww_gpu_frame_submit(handle, target) {
  const entry = entryOf(handle);
  if (!entry || !entry.device || !entry.encoder) return;
  if (entry.openPass) {
    entry.openPass.end();
    entry.openPass = null;
  }
  const staging = entry.staging;
  entry.staging = [];
  const held = entry.textures.get(target);
  if (!held) {
    entry.device.queue.submit([entry.encoder.finish()]);
    entry.encoder = null;
    record(entry, `submit of unknown texture handle ${target}`);
    return;
  }
  // The readback: one MAP_READ buffer, rows padded to 256 bytes.
  const { width, height } = held;
  const stride = strideFor(width, 8);
  const buffer = entry.device.createBuffer({
    size: align4(stride * height),
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  entry.encoder.copyTextureToBuffer(
    { texture: held.texture },
    { buffer, bytesPerRow: stride, rowsPerImage: height },
    [width, height, 1]
  );
  entry.device.queue.submit([entry.encoder.finish()]);
  entry.encoder = null;
  for (const s of staging) s.destroy();
  // Drop any readback nobody took (the bring-up clears of fresh targets).
  if (entry.readback && entry.readback.buffer) {
    try {
      entry.readback.buffer.destroy();
    } catch (_) {
      /* already gone */
    }
  }
  entry.readback = { buffer, width, height, stride, state: 0 };
  buffer.mapAsync(GPUMapMode.READ).then(
    () => {
      if (entry.readback && entry.readback.buffer === buffer) entry.readback.state = 1;
    },
    (e) => {
      if (entry.readback && entry.readback.buffer === buffer) {
        entry.readback.state = 2;
        record(entry, (e && e.message) || "readback mapAsync failed");
      }
    }
  );
}

export function vieww_gpu_readback_state(handle) {
  const entry = entryOf(handle);
  if (!entry) return 2;
  if (entry.state === 2) return 2;
  return entry.readback ? entry.readback.state : 0;
}

export function vieww_gpu_readback_take(handle, out, len) {
  const entry = entryOf(handle);
  const rb = entry && entry.readback;
  if (!rb || rb.state !== 1) return 0;
  const rowBytes = rb.width * 8;
  const mapped = new Uint8Array(rb.buffer.getMappedRange());
  const dst = bytes(out, len);
  let copied = 0;
  for (let y = 0; y < rb.height && copied < len; y++) {
    const n = Math.min(rowBytes, len - copied);
    dst.set(mapped.subarray(y * rb.stride, y * rb.stride + n), copied);
    copied += n;
  }
  rb.buffer.unmap();
  rb.buffer.destroy();
  entry.readback = null;
  return copied;
}

export function vieww_gpu_present(handle, canvas, len, target) {
  const entry = entryOf(handle);
  if (!entry || !entry.device || !entry.pipelines || !entry.pipelines.postReplace) return 0;
  const id = text(canvas, len);
  const element = (typeof document !== "undefined" && document.getElementById(id)) || null;
  if (!element || !element.getContext) {
    record(entry, `present: no element with id "${id}"`);
    return 0;
  }
  const ctx = element.getContext("webgpu");
  if (!ctx) {
    record(entry, `present: canvas "${id}" has no webgpu context`);
    return 0;
  }
  ctx.configure({
    device: entry.device,
    format: navigator.gpu.getPreferredCanvasFormat(),
    alphaMode: "premultiplied",
  });
  const held = entry.textures.get(target);
  if (!held) {
    record(entry, `present: unknown texture handle ${target}`);
    return 0;
  }
  const d = entry.device;
  const encoder = d.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [
      {
        view: ctx.getCurrentTexture().createView(),
        loadOp: "clear",
        storeOp: "store",
        clearValue: { r: 0, g: 0, b: 0, a: 0 },
      },
    ],
  });
  // One copy-mode post pass: post.wgsl mode 0, reading the whole frame
  // target, drawn over the whole canvas — the blit the other backends'
  // present paths make.
  pass.setPipeline(entry.pipelines.postReplace);
  const group0 = postBindGroup(entry, target, dummyOf(entry));
  if (!group0) return 0;
  pass.setBindGroup(0, group0);
  const offset = entry.pushCursor * 256;
  entry.pushCursor = (entry.pushCursor + 1) % 1024;
  const push = new Float32Array(32);
  push[0] = 0;
  push[1] = 0;
  push[2] = entry.frameW;
  push[3] = entry.frameH;
  d.queue.writeBuffer(entry.pushBuffer, offset, push);
  pass.setBindGroup(1, entry.pushBindGroup, [offset]);
  pass.draw(3);
  pass.end();
  d.queue.submit([encoder.finish()]);
  return 1;
}

// The 1x1 transparent stand-in present's copy pass binds as `aux` (mode 0
// never reads it, but the bind group still needs a texture there).
function dummyOf(entry) {
  if (entry.dummy) return entry.dummy;
  const texture = entry.device.createTexture({
    format: "rgba16float",
    size: [1, 1],
    usage:
      GPUTextureUsage.RENDER_ATTACHMENT |
      GPUTextureUsage.TEXTURE_BINDING |
      GPUTextureUsage.COPY_SRC,
  });
  entry.dummy = registerTexture(entry, texture, "target", 1, 1, 8);
  return entry.dummy;
}
