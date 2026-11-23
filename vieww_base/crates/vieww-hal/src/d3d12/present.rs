//! Presenting to a window through a DXGI flip-model swapchain — the D3D12
//! backend's window path, so Windows windows stop presenting through Vulkan.
//!
//! # Why Windows presents through D3D12
//!
//! On the reference laptop (i5-7200U, Intel HD 620 + GeForce 920MX), the
//! Vulkan path could not be made both fast and correct (2026-11-20, runs on
//! the self-hosted runner): the Vulkan loader loads every installed driver at
//! instance creation, the NVIDIA one powers its GPU up to answer (16.1 s cold,
//! 2–3 s warm, of a 19.3 s first frame), and the loader shipped with that
//! driver is too old for `VK_LOADER_DRIVERS_DISABLE`; and Vulkan on the Intel
//! GPU, the one that drives the panel, crashed the process with an access
//! violation right after its device came up — forced with `VIEWW_VK_DEVICE`
//! on unmodified code. D3D12 on that same Intel GPU passed the full parity
//! matrix (27 tests). DXGI lists adapters from the OS's own records, so the
//! sleeping GPU is never woken to be asked.
//!
//! # What this is
//!
//! [`D3d12WindowPresenter`] owns a direct queue, a flip-discard swapchain of
//! two `R8G8B8A8_UNORM` buffers on the window's `HWND`, its RTVs, and one
//! pipeline that copies a frame onto the back buffer: a full-screen
//! triangle whose pixel shader `Load`s the source texel at its own position.
//!
//! * **A GPU frame** — [`render_planned_to_window`] records the scene into
//!   `L[0]` and this copy in one command list, on this presenter's queue
//!   (the renderer is built on it: [`D3d12SceneRenderer::with_queue`]).
//! * **A CPU frame** — [`D3d12WindowPresenter::present_pixels`] copies the
//!   CPU rasterizer's RGBA8 rows straight into the back buffer: same
//!   format, no shader. A window swapchain's alpha is ignored by DWM
//!   (`DXGI_ALPHA_MODE_IGNORE`), so straight and premultiplied agree.
//!
//! # Pacing and what is measured
//!
//! The swapchain is made with a frame-latency waitable object and a maximum
//! latency of one. Each frame first waits on it — that wait is the
//! **drawable wait** ([`PresentTimings::acquire`]), the compositor's pacing
//! rather than work, reported apart as on every other backend — and then
//! renders, waits for its own fence (this backend never leaves a frame in
//! flight) and presents with sync interval 1.
//!
//! [`render_planned_to_window`]: super::D3d12SceneRenderer::render_planned_to_window
//! [`D3d12SceneRenderer::with_queue`]: super::D3d12SceneRenderer::with_queue

use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory2, IDXGISwapChain1, IDXGISwapChain3, DXGI_MWA_NO_ALT_ENTER,
    DXGI_PRESENT, DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::Threading::WaitForSingleObject;

use super::{D3d12Device, D3d12Error};

/// Back buffers in the swapchain: the flip model's minimum.
const BUFFERS: u32 = 2;

/// The copy-to-back-buffer shaders, in HLSL directly: three lines with no
/// WGSL counterpart to share.
const PRESENT_HLSL: &str = r"
Texture2D<float4> src : register(t0);

float4 vs_present(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}

float4 ps_present(float4 position : SV_Position) : SV_Target {
    return src.Load(int3(int2(position.xy), 0));
}
";

/// Where a presented frame's time went on this thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentTimings {
    /// Waiting on the swapchain's frame-latency object — pacing, not work.
    pub acquire: Duration,
    /// `Present` itself (and, for a CPU frame, the copy and its fence wait).
    pub present: Duration,
}

/// A window's DXGI swapchain, its queue, and the copy pass. See the module
/// docs.
pub struct D3d12WindowPresenter {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    fence_value: u64,
    swapchain: IDXGISwapChain3,
    waitable: HANDLE,
    rtv_heap: ID3D12DescriptorHeap,
    rtv_increment: u32,
    /// One shader-visible SRV slot: the frame's source.
    srv_heap: ID3D12DescriptorHeap,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    extent: (u32, u32),
    timings: PresentTimings,
    /// The CPU frames' copy plumbing, made at the first CPU frame and kept:
    /// creating a 1.9 MB upload buffer, an allocator and a list per frame
    /// cost ~8 ms a frame on the HD 620 (run 45143917742).
    cpu: Option<CpuCopy>,
}

/// A persistently mapped upload buffer sized for the window, and one
/// allocator/list pair reset per frame. Safe to reuse because every CPU
/// frame waits for its fence before returning.
struct CpuCopy {
    staging: ID3D12Resource,
    /// The mapping's address (upload heaps may stay mapped for life).
    mapped: usize,
    pitch: u32,
    extent: (u32, u32),
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
}

impl std::fmt::Debug for D3d12WindowPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3d12WindowPresenter")
            .field("extent", &self.extent)
            .finish_non_exhaustive()
    }
}

impl D3d12WindowPresenter {
    /// A swapchain on `hwnd` at `width x height` physical pixels.
    ///
    /// # Safety
    ///
    /// `hwnd` must be a live window that outlives the presenter.
    ///
    /// # Errors
    ///
    /// Any DXGI or D3D12 call failing, or the copy shaders failing to
    /// compile.
    pub unsafe fn for_hwnd(
        device: &D3d12Device,
        hwnd: isize,
        width: u32,
        height: u32,
    ) -> Result<Self, D3d12Error> {
        let raw = device.raw().clone();
        let (width, height) = (width.max(1), height.max(1));
        let trace = std::env::var_os("VIEWW_TRACE_FRAMES").is_some();
        let opening = Instant::now();
        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };
        // SAFETY: creation calls on a live device; `hwnd` per the contract.
        unsafe {
            let queue: ID3D12CommandQueue = raw.CreateCommandQueue(&queue_desc)?;
            let fence: ID3D12Fence = raw.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let factory: IDXGIFactory2 = CreateDXGIFactory1()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: BUFFERS,
                Scaling: DXGI_SCALING_NONE,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: swapchain_flags().0 as u32,
            };
            let hwnd = HWND(hwnd as *mut core::ffi::c_void);
            let swapchain: IDXGISwapChain1 = factory
                .CreateSwapChainForHwnd(&queue, hwnd, &desc, None, None)
                .map_err(|error| D3d12Error::Api(swapchain_error(&error)))?;
            // The window, not DXGI, owns full-screen: no Alt+Enter takeover.
            let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);
            let swapchain: IDXGISwapChain3 = swapchain.cast()?;
            swapchain
                .SetMaximumFrameLatency(1)
                .map_err(|e| D3d12Error::Api(format!("SetMaximumFrameLatency: {e}")))?;
            let waitable = swapchain.GetFrameLatencyWaitableObject();
            if trace {
                eprintln!(
                    "d3d12: swapchain ready in {:.1} ms",
                    opening.elapsed().as_secs_f64() * 1e3
                );
            }
            let compiling = Instant::now();

            let rtv_heap: ID3D12DescriptorHeap =
                raw.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                    NumDescriptors: BUFFERS,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
                    NodeMask: 0,
                })?;
            let rtv_increment =
                raw.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let srv_heap: ID3D12DescriptorHeap =
                raw.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: 1,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })?;

            let range = D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 1,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 0,
            };
            let parameter = D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            };
            let root = device.root_signature(&D3D12_ROOT_SIGNATURE_DESC {
                NumParameters: 1,
                pParameters: &parameter,
                NumStaticSamplers: 0,
                pStaticSamplers: std::ptr::null(),
                Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
            })?;
            let vs = compile_hlsl("vs_present", b"vs_5_0\0")?;
            let ps = compile_hlsl("ps_present", b"ps_5_0\0")?;
            let pso = device.pipeline(
                &root,
                &vs,
                &ps,
                &[],
                DXGI_FORMAT_R8G8B8A8_UNORM,
                D3D12_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: false.into(),
                    LogicOpEnable: false.into(),
                    SrcBlend: D3D12_BLEND_ONE,
                    DestBlend: D3D12_BLEND_ZERO,
                    BlendOp: D3D12_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D12_BLEND_ONE,
                    DestBlendAlpha: D3D12_BLEND_ZERO,
                    BlendOpAlpha: D3D12_BLEND_OP_ADD,
                    LogicOp: D3D12_LOGIC_OP_NOOP,
                    #[expect(clippy::cast_possible_truncation, reason = "the mask is 0xF")]
                    RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                },
            )?;
            if trace {
                eprintln!(
                    "d3d12: present shaders and pipeline in {:.1} ms",
                    compiling.elapsed().as_secs_f64() * 1e3
                );
            }
            let mut presenter = Self {
                device: raw,
                queue,
                fence,
                fence_value: 0,
                swapchain,
                waitable,
                rtv_heap,
                rtv_increment,
                srv_heap,
                root,
                pso,
                extent: (width, height),
                timings: PresentTimings::default(),
                cpu: None,
            };
            presenter.write_rtvs()?;
            Ok(presenter)
        }
    }

    /// The queue the scene renderer must be built on
    /// ([`super::D3d12SceneRenderer::with_queue`]): a swapchain presents
    /// from the queue it was made with.
    #[must_use]
    pub const fn queue(&self) -> &ID3D12CommandQueue {
        &self.queue
    }

    /// The back buffers' size in physical pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.extent
    }

    /// The last presented frame's split — see [`PresentTimings`].
    #[must_use]
    pub const fn last_timings(&self) -> PresentTimings {
        self.timings
    }

    /// Match the back buffers to the window's new physical size. The GPU is
    /// idle here: every frame on this backend waits for its fence.
    ///
    /// # Errors
    ///
    /// `ResizeBuffers` failing.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), D3d12Error> {
        let (width, height) = (width.max(1), height.max(1));
        if self.extent == (width, height) {
            return Ok(());
        }
        self.wait_idle()?;
        // SAFETY: no reference to a back buffer is held (they are fetched per
        // frame and dropped), and the queue is idle.
        unsafe {
            self.swapchain.ResizeBuffers(
                BUFFERS,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                swapchain_flags(),
            )?;
        }
        self.extent = (width, height);
        self.write_rtvs()
    }

    /// Wait for the swapchain to accept another frame — the drawable wait.
    /// Called at the start of every frame.
    pub(crate) fn wait_for_frame(&mut self) {
        self.timings = PresentTimings::default();
        let waiting = Instant::now();
        // SAFETY: the swapchain's own waitable handle; a bounded wait so a
        // hidden window cannot hang the loop.
        unsafe {
            WaitForSingleObject(self.waitable, 1000);
        }
        self.timings.acquire = waiting.elapsed();
    }

    /// Record, onto the scene renderer's open list, the copy of `source`
    /// (`L[0]`, already in `PIXEL_SHADER_RESOURCE`) onto the current back
    /// buffer. The caller executes the list and then calls [`Self::present`].
    pub(crate) fn record_copy(
        &self,
        list: &ID3D12GraphicsCommandList,
        source: &ID3D12Resource,
    ) -> Result<(), D3d12Error> {
        // SAFETY: recording onto an open list; the back buffer is fetched
        // for this frame and its barriers released after recording.
        unsafe {
            let index = self.swapchain.GetCurrentBackBufferIndex();
            let back: ID3D12Resource = self.swapchain.GetBuffer(index)?;
            let to_target = super::transition(
                &back,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            );
            list.ResourceBarrier(std::slice::from_ref(&to_target));
            super::release_barrier(to_target);

            let srv = self.srv_heap.GetCPUDescriptorHandleForHeapStart();
            self.device.CreateShaderResourceView(source, None, srv);
            list.SetDescriptorHeaps(&[Some(self.srv_heap.clone())]);
            list.SetGraphicsRootSignature(&self.root);
            list.SetPipelineState(&self.pso);
            list.SetGraphicsRootDescriptorTable(
                0,
                self.srv_heap.GetGPUDescriptorHandleForHeapStart(),
            );
            let (width, height) = self.extent;
            #[expect(clippy::cast_precision_loss, reason = "window extents")]
            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            let scissor = windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: i32::try_from(width).unwrap_or(i32::MAX),
                bottom: i32::try_from(height).unwrap_or(i32::MAX),
            };
            list.RSSetViewports(std::slice::from_ref(&viewport));
            list.RSSetScissorRects(std::slice::from_ref(&scissor));
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let rtv = self.rtv(index);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.DrawInstanced(3, 1, 0, 0);

            let to_present = super::transition(
                &back,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            );
            list.ResourceBarrier(std::slice::from_ref(&to_present));
            super::release_barrier(to_present);
        }
        Ok(())
    }

    /// Present the back buffer the last executed list drew, sync interval 1.
    ///
    /// # Errors
    ///
    /// `Present` failing (a removed device, an occluded window is not an
    /// error).
    pub(crate) fn present(&mut self) -> Result<(), D3d12Error> {
        let presenting = Instant::now();
        // SAFETY: a live swapchain whose current back buffer was just drawn.
        let hr = unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) };
        self.timings.present += presenting.elapsed();
        hr.ok().map_err(D3d12Error::from)
    }

    /// Present the CPU rasterizer's straight-alpha RGBA8 frame (`extent`
    /// sized, row-major, no padding): staged through an upload buffer and
    /// copied into the back buffer, the same format, no shader.
    ///
    /// # Errors
    ///
    /// A frame of the wrong length, or D3D12/DXGI failing.
    pub fn present_pixels(&mut self, pixels: &[u8]) -> Result<(), D3d12Error> {
        let (width, height) = self.extent;
        let row = width as usize * 4;
        if pixels.len() != row * height as usize {
            return Err(D3d12Error::Invalid(format!(
                "present_pixels: {} bytes for a {width}x{height} window",
                pixels.len()
            )));
        }
        self.wait_for_frame();
        let started = Instant::now();
        if self
            .cpu
            .as_ref()
            .is_none_or(|c| c.extent != (width, height))
        {
            self.cpu = None;
            self.cpu = Some(self.cpu_copy(width, height)?);
        }
        let cpu = self.cpu.as_ref().expect("made above");
        let pitch = cpu.pitch;
        // SAFETY: the mapping is `pitch * height` bytes and the GPU is done
        // with it (the previous CPU frame waited for its fence); recording
        // and execution on our own queue; the fence wait below keeps the
        // staging buffer's contents valid for the copy.
        unsafe {
            let base = cpu.mapped as *mut u8;
            for y in 0..height as usize {
                std::ptr::copy_nonoverlapping(
                    pixels.as_ptr().add(y * row),
                    base.add(y * pitch as usize),
                    row,
                );
            }
            cpu.allocator.Reset()?;
            let list = &cpu.list;
            list.Reset(&cpu.allocator, None)?;
            let index = self.swapchain.GetCurrentBackBufferIndex();
            let back: ID3D12Resource = self.swapchain.GetBuffer(index)?;
            let to_copy = super::transition(
                &back,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            list.ResourceBarrier(std::slice::from_ref(&to_copy));
            super::release_barrier(to_copy);
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(cpu.staging.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            Width: width,
                            Height: height,
                            Depth: 1,
                            RowPitch: pitch,
                        },
                    },
                },
            };
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(back.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            drop(ManuallyDrop::into_inner(src.pResource));
            drop(ManuallyDrop::into_inner(dst.pResource));
            let to_present = super::transition(
                &back,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PRESENT,
            );
            list.ResourceBarrier(std::slice::from_ref(&to_present));
            super::release_barrier(to_present);
            list.Close()?;
            self.queue
                .ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>()?)]);
            self.signal_and_wait()?;
        }
        self.timings.present = started.elapsed();
        self.present()
    }

    /// The persistent CPU-frame plumbing for a `width x height` window.
    fn cpu_copy(&self, width: u32, height: u32) -> Result<CpuCopy, D3d12Error> {
        let pitch = (width * 4).div_ceil(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT)
            * D3D12_TEXTURE_DATA_PITCH_ALIGNMENT;
        let size = u64::from(pitch) * u64::from(height);
        // SAFETY: creation calls on a live device; the buffer stays mapped
        // for its life, which upload heaps allow.
        unsafe {
            let mut staging: Option<ID3D12Resource> = None;
            self.device.CreateCommittedResource(
                &D3D12_HEAP_PROPERTIES {
                    Type: D3D12_HEAP_TYPE_UPLOAD,
                    ..Default::default()
                },
                D3D12_HEAP_FLAG_NONE,
                &D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                    Alignment: 0,
                    Width: size,
                    Height: 1,
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    Format: DXGI_FORMAT_UNKNOWN,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                    Flags: D3D12_RESOURCE_FLAG_NONE,
                },
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut staging,
            )?;
            let staging = staging.ok_or_else(|| D3d12Error::Api("no staging buffer".into()))?;
            let mut ptr = std::ptr::null_mut();
            staging.Map(0, None, Some(&mut ptr))?;
            let allocator: ID3D12CommandAllocator = self
                .device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = self.device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None,
            )?;
            // Lists are created open; each frame resets it.
            list.Close()?;
            Ok(CpuCopy {
                staging,
                mapped: ptr as usize,
                pitch,
                extent: (width, height),
                allocator,
                list,
            })
        }
    }

    /// Block until everything submitted to this queue has finished.
    fn wait_idle(&mut self) -> Result<(), D3d12Error> {
        // SAFETY: signalling and waiting on our own fence.
        unsafe { self.signal_and_wait() }
    }

    unsafe fn signal_and_wait(&mut self) -> Result<(), D3d12Error> {
        self.fence_value += 1;
        let value = self.fence_value;
        // SAFETY: the caller's; our own queue and fence.
        unsafe {
            self.queue.Signal(&self.fence, value)?;
            super::wait_fence(&self.device, &self.fence, value, "a present copy")?;
        }
        Ok(())
    }

    fn rtv(&self, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: a live heap of `BUFFERS` descriptors; `index < BUFFERS`.
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: unsafe { self.rtv_heap.GetCPUDescriptorHandleForHeapStart() }.ptr
                + (index * self.rtv_increment) as usize,
        }
    }

    /// (Re)write the back buffers' RTVs — after creation and every resize.
    fn write_rtvs(&mut self) -> Result<(), D3d12Error> {
        for index in 0..BUFFERS {
            // SAFETY: a valid buffer index of a live swapchain; the RTV is
            // written into our own heap.
            unsafe {
                let back: ID3D12Resource = self.swapchain.GetBuffer(index)?;
                self.device
                    .CreateRenderTargetView(&back, None, self.rtv(index));
            }
        }
        Ok(())
    }
}

impl Drop for D3d12WindowPresenter {
    fn drop(&mut self) {
        let _ = self.wait_idle();
        if !self.waitable.is_invalid() {
            // SAFETY: the swapchain's waitable handle, closed once.
            let _ = unsafe { CloseHandle(self.waitable) };
        }
    }
}

const fn swapchain_flags() -> DXGI_SWAP_CHAIN_FLAG {
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT
}

fn compile_hlsl(entry: &str, target: &[u8]) -> Result<ID3DBlob, D3d12Error> {
    let entry_c = std::ffi::CString::new(entry)
        .map_err(|_| D3d12Error::Invalid("entry point name".into()))?;
    let flags = D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3;
    if let Some(cached) = super::shader_cache::load(PRESENT_HLSL, entry, target, flags) {
        return Ok(cached);
    }
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source pointer and length describe `PRESENT_HLSL`; every
    // string argument is NUL-terminated and outlives the call.
    let compiled = unsafe {
        D3DCompile(
            PRESENT_HLSL.as_ptr().cast(),
            PRESENT_HLSL.len(),
            PCSTR(c"vieww-present.hlsl".as_ptr().cast()),
            None,
            None,
            PCSTR(entry_c.as_ptr().cast()),
            PCSTR(target.as_ptr()),
            flags,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = compiled {
        let log = errors
            // SAFETY: a blob's buffer is `GetBufferSize` bytes.
            .map(|b| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    b.GetBufferPointer().cast::<u8>(),
                    b.GetBufferSize(),
                ))
                .into_owned()
            })
            .unwrap_or_default();
        return Err(D3d12Error::Shader(format!("{entry}: {error}: {log}")));
    }
    let code = code.ok_or_else(|| D3d12Error::Shader(format!("{entry}: no code")))?;
    super::shader_cache::store(PRESENT_HLSL, entry, target, flags, &code);
    Ok(code)
}

/// `CreateSwapChainForHwnd`'s error, with what it usually means here.
fn swapchain_error(error: &windows::core::Error) -> String {
    // DXGI_ERROR_NOT_CURRENTLY_AVAILABLE
    let hint = if error.code().0 as u32 == 0x887A_0022 {
        " — no desktop composition for this window: the process is most likely \
         running in a non-interactive session (a service, e.g. a CI runner \
         installed as a Windows service, session 0), where flip-model \
         swapchains — the only kind D3D12 has — cannot present. Run it from a \
         logged-in desktop session"
    } else {
        ""
    };
    format!("CreateSwapChainForHwnd: {error}{hint}")
}
