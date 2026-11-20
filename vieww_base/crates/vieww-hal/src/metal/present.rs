//! Presenting to a `CAMetalLayer` — the Metal backend's window path, the one
//! that lets iOS (and, opted in, macOS) stop presenting through MoltenVK
//! (PENDING 1.2).
//!
//! # What this is
//!
//! [`MetalLayerPresenter`] owns a `CAMetalLayer` on the window's view and
//! one tiny render pipeline that copies a frame onto the layer's drawable:
//! a full-screen triangle whose fragment `read`s the source texel at its own
//! pixel position. Two sources:
//!
//! * **A GPU frame** — [`render_planned_to_layer`](super::MetalSceneRenderer::render_planned_to_layer)
//!   leaves the scene in `L[0]` (premultiplied RGBA16F) and this pass writes
//!   it, unchanged, into the drawable's `BGRA8Unorm`. `CAMetalLayer`
//!   composites premultiplied alpha, so a translucent window needs no
//!   division here — the case Vulkan's blit route had to send through the
//!   readback.
//! * **A CPU frame** — [`MetalLayerPresenter::present_pixels`] stages the
//!   CPU rasterizer's straight-alpha RGBA8 into a texture and the second
//!   fragment premultiplies it on the way to the drawable.
//!
//! # Pacing and what is measured
//!
//! Nothing here waits for the GPU. A frame is committed with
//! `presentDrawable:` and left in flight; `nextDrawable` is the only
//! blocking call, and it blocks exactly when the compositor has every
//! drawable (`maximumDrawableCount`, three) — so its time is the
//! **drawable wait**, reported apart from work ([`PresentTimings::acquire`]).
//! The GPU's own time is the finished command buffer's
//! `GPUEndTime - GPUStartTime`, read without blocking from the *previous*
//! frame when it has completed ([`MetalLayerPresenter::last_gpu_time`]) —
//! the whole-frame number the Device Farm / Firebase iPhone runs record
//! against the 8.3 ms (120 Hz) budget. Per-pass counter samples
//! (`MTLCounterSampleBuffer`) are not needed for that budget and Apple GPUs
//! only sample at stage boundaries; they stay a follow-up.
//!
//! # Hazards
//!
//! Every command buffer goes to the renderer's one queue, and every texture
//! is hazard-tracked (Metal's default), so the next frame's writes to `L[0]`
//! are ordered after this frame's read of it without a fence of ours.

// `objc` 0.2's `msg_send!` expands a `cfg(feature = "cargo-clippy")` this
// crate does not declare; the expansion is the dependency's, not ours.
#![allow(unexpected_cfgs)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use metal::foreign_types::ForeignType;
use metal::objc::runtime::Object;
use metal::objc::{msg_send, sel, sel_impl};
use metal::{
    CommandBuffer, CommandBufferRef, CommandQueue, CompileOptions, Device, MTLCommandBufferStatus,
    MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType, MTLResourceOptions, MTLSize,
    MTLStoreAction, MTLTextureUsage, MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor,
    RenderPipelineState, Texture, TextureDescriptor, TextureRef,
};

use super::{MetalDevice, MetalError};

/// The drawable format. `BGRA8Unorm` is what every `CAMetalLayer` supports
/// and what the Vulkan path presented through MoltenVK, so the pixels on
/// screen are the same bytes either way.
pub const DRAWABLE_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

/// The copy-to-drawable shaders. Written in MSL directly: the pass is three
/// lines and has no WGSL counterpart to share (Vulkan's equivalent is a
/// `vkCmdBlitImage`, which Metal's blit encoder cannot do across formats).
const PRESENT_MSL: &str = r"
#include <metal_stdlib>
using namespace metal;

struct PresentOut { float4 position [[position]]; };

vertex PresentOut vs_present(uint vid [[vertex_id]]) {
    float2 uv = float2(float((vid << 1) & 2u), float(vid & 2u));
    PresentOut out;
    out.position = float4(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

static uint2 texel_at(float4 position, uint w, uint h) {
    return min(uint2(position.xy), uint2(w - 1u, h - 1u));
}

// The scene's L[0]: already premultiplied, which CAMetalLayer composites.
fragment float4 fs_premultiplied(PresentOut in [[stage_in]],
                                 texture2d<float, access::read> src [[texture(0)]]) {
    return src.read(texel_at(in.position, src.get_width(), src.get_height()));
}

// The CPU rasterizer's straight-alpha RGBA8, premultiplied on the way.
fragment float4 fs_straight(PresentOut in [[stage_in]],
                            texture2d<float, access::read> src [[texture(0)]]) {
    float4 c = src.read(texel_at(in.position, src.get_width(), src.get_height()));
    return float4(c.rgb * c.a, c.a);
}
";

/// Where a presented frame's time went on this thread, plus the GPU's own
/// time for the most recent frame that has finished.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentTimings {
    /// Waiting in `nextDrawable` — the compositor's pacing, not work.
    pub acquire: Duration,
    /// Encoding the copy pass and committing.
    pub present: Duration,
}

/// `CGRect`/`CGSize` as UIKit/AppKit pass them (64-bit Apple targets only).
#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgSize {
    w: f64,
    h: f64,
}

/// A `CAMetalLayer` on a window's view, and the pass that puts frames on it.
/// See the module docs.
pub struct MetalLayerPresenter {
    layer: MetalLayer,
    device: Device,
    queue: CommandQueue,
    premultiplied: RenderPipelineState,
    straight: RenderPipelineState,
    /// The CPU frames' staging texture, at the drawable's extent.
    upload: Option<Texture>,
    extent: (u32, u32),
    /// The most recent committed frame, retained so its GPU time can be read
    /// once it completes.
    in_flight: Option<CommandBuffer>,
    last_gpu: Option<Duration>,
    timings: PresentTimings,
}

impl std::fmt::Debug for MetalLayerPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalLayerPresenter")
            .field("extent", &self.extent)
            .finish_non_exhaustive()
    }
}

impl MetalLayerPresenter {
    /// Put a `CAMetalLayer` on `view` (a `UIView *` on iOS, an `NSView *` on
    /// macOS, as `raw-window-handle` hands them out) at `width x height`
    /// physical pixels and `scale` pixels per point.
    ///
    /// On iOS a `UIView`'s own layer cannot be replaced, so the Metal layer
    /// is a sublayer sized to the view's bounds — what `raw-window-metal`
    /// does for the MoltenVK path; [`Self::resize`] keeps its frame in step.
    /// On macOS the view is made layer-backed by this layer.
    ///
    /// # Safety
    ///
    /// `view` must be a live `UIView`/`NSView` that outlives the presenter,
    /// and this must run on the main thread.
    ///
    /// # Errors
    ///
    /// The present pipelines failing to compile.
    pub unsafe fn for_view(
        device: &MetalDevice,
        view: NonNull<c_void>,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Result<Self, MetalError> {
        let layer = MetalLayer::new();
        layer.set_device(device.raw());
        layer.set_pixel_format(DRAWABLE_FORMAT);
        layer.set_framebuffer_only(true);
        layer.set_presents_with_transaction(false);
        layer.set_maximum_drawable_count(3);
        layer.set_contents_scale(scale);
        let view = view.as_ptr().cast::<Object>();
        let raw_layer = layer.as_ptr().cast::<Object>();
        // SAFETY: the caller's contract — a live view, main thread. The
        // selectors are UIView/NSView/CALayer's documented ones.
        unsafe {
            #[cfg(target_os = "ios")]
            {
                let host: *mut Object = msg_send![view, layer];
                let bounds: CgRect = msg_send![host, bounds];
                let () = msg_send![raw_layer, setFrame: bounds];
                let () = msg_send![host, addSublayer: raw_layer];
            }
            #[cfg(target_os = "macos")]
            {
                let () = msg_send![view, setWantsLayer: true];
                let () = msg_send![view, setLayer: raw_layer];
            }
        }
        let mut presenter = Self {
            layer,
            device: device.raw().clone(),
            queue: device.queue().clone(),
            premultiplied: present_pipeline(device.raw(), "fs_premultiplied")?,
            straight: present_pipeline(device.raw(), "fs_straight")?,
            upload: None,
            extent: (0, 0),
            in_flight: None,
            last_gpu: None,
            timings: PresentTimings::default(),
        };
        presenter.resize(width, height);
        Ok(presenter)
    }

    /// A presenter whose layer is in no view — for the macOS CI suite,
    /// which has no window to put one in. `nextDrawable` still vends
    /// drawables for a detached layer; when it does not, a frame commits
    /// without presenting, which is the same path an off-screen window takes.
    ///
    /// # Errors
    ///
    /// As [`Self::for_view`].
    pub fn offscreen(device: &MetalDevice, width: u32, height: u32) -> Result<Self, MetalError> {
        let layer = MetalLayer::new();
        layer.set_device(device.raw());
        layer.set_pixel_format(DRAWABLE_FORMAT);
        layer.set_framebuffer_only(true);
        layer.set_maximum_drawable_count(3);
        let mut presenter = Self {
            layer,
            device: device.raw().clone(),
            queue: device.queue().clone(),
            premultiplied: present_pipeline(device.raw(), "fs_premultiplied")?,
            straight: present_pipeline(device.raw(), "fs_straight")?,
            upload: None,
            extent: (0, 0),
            in_flight: None,
            last_gpu: None,
            timings: PresentTimings::default(),
        };
        presenter.resize(width, height);
        Ok(presenter)
    }

    /// Wait for the frame in flight, if any, and record its GPU time — for
    /// tests and teardown; the frame path never calls this.
    pub fn wait_idle(&mut self) {
        if let Some(commands) = self.in_flight.as_ref() {
            commands.wait_until_completed();
        }
        self.poll_in_flight();
    }

    /// Match the drawable to the window's new physical size. The layer's
    /// frame follows the host layer's bounds (iOS: rotation, split view).
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        // SAFETY: plain property sends on our own layer and its superlayer.
        unsafe {
            let raw_layer = self.layer.as_ptr().cast::<Object>();
            let host: *mut Object = msg_send![raw_layer, superlayer];
            if !host.is_null() {
                let bounds: CgRect = msg_send![host, bounds];
                let () = msg_send![raw_layer, setFrame: bounds];
            }
            let size = CgSize {
                w: f64::from(width),
                h: f64::from(height),
            };
            let () = msg_send![raw_layer, setDrawableSize: size];
        }
        if self.extent != (width, height) {
            self.upload = None;
        }
        self.extent = (width, height);
    }

    /// The drawable's size in physical pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.extent
    }

    /// The last presented frame's CPU-side split — see [`PresentTimings`].
    #[must_use]
    pub const fn last_timings(&self) -> PresentTimings {
        self.timings
    }

    /// The GPU time of the most recent frame known to have finished
    /// (`GPUEndTime - GPUStartTime` of its command buffer); `None` until one
    /// has. Never blocks.
    #[must_use]
    pub fn last_gpu_time(&mut self) -> Option<Duration> {
        self.poll_in_flight();
        self.last_gpu
    }

    /// The device the layer draws with, for a renderer sharing it.
    #[must_use]
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Present the CPU rasterizer's straight-alpha RGBA8 frame (`extent`
    /// sized, row-major, no padding).
    ///
    /// # Errors
    ///
    /// A frame of the wrong length.
    pub fn present_pixels(&mut self, pixels: &[u8]) -> Result<(), MetalError> {
        let (width, height) = self.extent;
        let len = width as usize * height as usize * 4;
        if pixels.len() != len {
            return Err(MetalError::Invalid(format!(
                "present_pixels: {} bytes for a {width}x{height} drawable",
                pixels.len()
            )));
        }
        let upload = self
            .upload
            .get_or_insert_with(|| {
                let desc = TextureDescriptor::new();
                desc.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
                desc.set_width(u64::from(width));
                desc.set_height(u64::from(height));
                desc.set_usage(MTLTextureUsage::ShaderRead);
                self.device.new_texture(&desc)
            })
            .clone();
        let queue = self.queue.clone();
        metal::objc::rc::autoreleasepool(|| {
            let commands = queue.new_command_buffer();
            // Staged through a buffer and a blit on the queue, not
            // `replaceRegion` on the CPU: the previous frame may still be
            // reading this texture, and the queue orders the blit after it.
            let staging = self.device.new_buffer_with_data(
                pixels.as_ptr().cast(),
                len as u64,
                MTLResourceOptions::StorageModeShared,
            );
            let blit = commands.new_blit_command_encoder();
            blit.copy_from_buffer_to_texture(
                &staging,
                0,
                u64::from(width) * 4,
                len as u64,
                MTLSize {
                    width: u64::from(width),
                    height: u64::from(height),
                    depth: 1,
                },
                &upload,
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
                metal::MTLBlitOption::empty(),
            );
            blit.end_encoding();
            self.encode_and_present(commands, &upload, false);
        });
        Ok(())
    }

    /// Encode the copy of `source` onto the next drawable into `commands`,
    /// present it and commit — without waiting. `premultiplied` picks the
    /// fragment: a scene target as-is, or a straight CPU frame. Called by the
    /// scene renderer with its frame's own command buffer, so the scene and
    /// its present are one submission.
    ///
    /// Must run inside an autorelease pool (the drawable is autoreleased).
    pub(crate) fn encode_and_present(
        &mut self,
        commands: &CommandBufferRef,
        source: &TextureRef,
        premultiplied: bool,
    ) {
        self.timings = PresentTimings::default();
        self.poll_in_flight();
        let acquiring = Instant::now();
        let drawable = self.layer.next_drawable();
        self.timings.acquire = acquiring.elapsed();
        let presenting = Instant::now();
        if let Some(drawable) = drawable {
            let pass = RenderPassDescriptor::new();
            let attachment = pass
                .color_attachments()
                .object_at(0)
                .expect("attachment 0 exists");
            attachment.set_texture(Some(drawable.texture()));
            attachment.set_load_action(MTLLoadAction::DontCare);
            attachment.set_store_action(MTLStoreAction::Store);
            let encoder = commands.new_render_command_encoder(pass);
            encoder.set_render_pipeline_state(if premultiplied {
                &self.premultiplied
            } else {
                &self.straight
            });
            encoder.set_fragment_texture(0, Some(source));
            encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            encoder.end_encoding();
            commands.present_drawable(drawable);
        }
        // No drawable (the layer is off screen, or zero-sized): the frame's
        // work still commits, so `L[0]` stays the retained frame.
        commands.commit();
        self.in_flight = Some(commands.to_owned());
        self.timings.present = presenting.elapsed();
    }

    /// Read the GPU time off the frame in flight if it has finished.
    fn poll_in_flight(&mut self) {
        let Some(commands) = self.in_flight.as_ref() else {
            return;
        };
        match commands.status() {
            MTLCommandBufferStatus::Completed => {
                // SAFETY: `GPUStartTime`/`GPUEndTime` are `CFTimeInterval`
                // (double) properties of a completed `MTLCommandBuffer`.
                let (start, end): (f64, f64) = unsafe {
                    let raw = commands.as_ptr().cast::<Object>();
                    (msg_send![raw, GPUStartTime], msg_send![raw, GPUEndTime])
                };
                if end >= start && start > 0.0 {
                    self.last_gpu = Some(Duration::from_secs_f64(end - start));
                }
                self.in_flight = None;
            }
            MTLCommandBufferStatus::Error => self.in_flight = None,
            _ => {}
        }
    }
}

impl Drop for MetalLayerPresenter {
    fn drop(&mut self) {
        if let Some(commands) = self.in_flight.take() {
            commands.wait_until_completed();
        }
        // SAFETY: detach our layer from the view's layer tree; the view is
        // alive per `for_view`'s contract (the window drops after us).
        unsafe {
            let raw_layer = self.layer.as_ptr().cast::<Object>();
            let () = msg_send![raw_layer, removeFromSuperlayer];
        }
    }
}

fn present_pipeline(device: &Device, fragment: &str) -> Result<RenderPipelineState, MetalError> {
    let library = device
        .new_library_with_source(PRESENT_MSL, &CompileOptions::new())
        .map_err(|e| MetalError::Invalid(format!("present shaders: {e}")))?;
    let vs = library
        .get_function("vs_present", None)
        .map_err(|e| MetalError::Invalid(format!("vs_present: {e}")))?;
    let fs = library
        .get_function(fragment, None)
        .map_err(|e| MetalError::Invalid(format!("{fragment}: {e}")))?;
    let desc = RenderPipelineDescriptor::new();
    desc.set_vertex_function(Some(&vs));
    desc.set_fragment_function(Some(&fs));
    desc.color_attachments()
        .object_at(0)
        .expect("attachment 0 exists")
        .set_pixel_format(DRAWABLE_FORMAT);
    device
        .new_render_pipeline_state(&desc)
        .map_err(|e| MetalError::Invalid(format!("present pipeline: {e}")))
}
