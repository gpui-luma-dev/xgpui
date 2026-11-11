//! Windows transport: shared NT-handle D3D12 textures and a shared fence.
//!
//! The engine is a wgpu device of its own (D3D12). `WgpuSurfaceTarget` creates
//! one shared committed resource per slot (`D3D12_HEAP_FLAG_SHARED`,
//! simultaneous access) and an NT handle for it. The D3D11 compositor opens
//! each slot once per device, after checking that both use the same adapter
//! (LUID), and adopts it through `Dx11CustomContext::adopt_texture` without a
//! copy (issue #198, spike 3).
//!
//! Simultaneous access lets a slot decay to the common state when the engine's
//! command lists complete. The engine signals a shared fence after them and
//! presents without waiting; the compositor opens the fence once and its
//! preparation batch issues `ID3D11DeviceContext4::Wait` on the immediate
//! context (#197 K3). Without a shared fence the engine waits on its own
//! thread; without D3D11.4 fences, preparation waits on the CPU.
use crate::{EngineSurface, SurfaceDesc, SurfaceFormat, SurfaceSlot, Unsupported};
use anyhow::{Context as _, Result, ensure};
use gpui_windows::custom_draw::Dx11CustomContext;
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device1, ID3D11Device5, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
    D3D12_RESOURCE_STATE_COMMON, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Device, ID3D12Fence,
    ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{Interface, PCWSTR};

#[cfg(test)]
mod tests;

/// Readiness of a presented slot: already complete, or a value on the engine's
/// shared fence that the compositor waits for on the GPU.
#[derive(Clone)]
pub struct SyncPoint(Readiness);
#[derive(Clone)]
enum Readiness {
    Completed,
    Fence(Arc<SharedFence>, u64),
}
impl std::fmt::Debug for SyncPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Readiness::Completed => f.write_str("SyncPoint::Completed"),
            Readiness::Fence(_, value) => write!(f, "SyncPoint::Fence({value})"),
        }
    }
}
impl SyncPoint {
    /// The slot's writes have completed on the engine's GPU queue.
    pub fn completed() -> Self {
        Self(Readiness::Completed)
    }
    /// Whether the compositor waits for this point on the GPU.
    pub fn is_gpu_wait(&self) -> bool {
        matches!(self.0, Readiness::Fence(..))
    }
}

/// The engine's shared D3D12 fence and the compositor's opened view of it.
pub(crate) struct SharedFence {
    fence: ID3D12Fence,
    handle: SharedHandle,
    host: Mutex<Option<(usize, Option<ID3D11Fence>)>>,
}
impl SharedFence {
    fn reached(&self, value: u64) -> bool {
        unsafe { self.fence.GetCompletedValue() >= value }
    }
    /// The compositor's fence, or `None` when it lacks D3D11.4 fences.
    fn host_fence(&self, context: &Dx11CustomContext) -> Result<Option<ID3D11Fence>> {
        let key = context.cache_key();
        let mut host = self.host.lock().unwrap();
        if let Some((cached, fence)) = host.as_ref()
            && *cached == key
        {
            return Ok(fence.clone());
        }
        let opened = context
            .device
            .cast::<ID3D11Device5>()
            .ok()
            .and_then(|device| {
                let mut fence: Option<ID3D11Fence> = None;
                unsafe { device.OpenSharedFence(self.handle.0, &mut fence) }.ok()?;
                fence
            });
        *host = Some((key, opened.clone()));
        Ok(opened)
    }
    /// Fallback for a compositor that cannot open the fence.
    fn cpu_wait(&self, value: u64) -> Result<()> {
        // A null event makes SetEventOnCompletion block until the value.
        unsafe { self.fence.SetEventOnCompletion(value, HANDLE::default()) }
            .context("engine fence wait failed")
    }
}

fn dxgi_format(format: SurfaceFormat) -> DXGI_FORMAT {
    match format {
        SurfaceFormat::Bgra8Srgb => DXGI_FORMAT_B8G8R8A8_UNORM,
        SurfaceFormat::Rgba16FloatLinearP3 => DXGI_FORMAT_R16G16B16A16_FLOAT,
    }
}
fn wgpu_format(format: SurfaceFormat) -> wgpu::TextureFormat {
    match format {
        SurfaceFormat::Bgra8Srgb => wgpu::TextureFormat::Bgra8Unorm,
        SurfaceFormat::Rgba16FloatLinearP3 => wgpu::TextureFormat::Rgba16Float,
    }
}

/// An owned NT handle, closed on drop.
struct SharedHandle(HANDLE);
// SAFETY: an NT handle is a process-wide kernel object reference; it is only
// passed to OpenSharedResource1 and closed once.
unsafe impl Send for SharedHandle {}
unsafe impl Sync for SharedHandle {}
impl Drop for SharedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Temporary teardown diagnostics (#197 K4): `XGPUI_TRACE_DROPS=1` prints
/// which thread releases engine-shared objects.
fn trace_drop(what: &str) {
    if std::env::var_os("XGPUI_TRACE_DROPS").is_some() {
        eprintln!(
            "trace: drop {what} on thread {:?}",
            std::thread::current().name().unwrap_or("unnamed")
        );
    }
}
impl Drop for SharedFence {
    fn drop(&mut self) {
        trace_drop("SharedFence");
    }
}
impl Drop for SlotResources {
    fn drop(&mut self) {
        trace_drop("SlotResources");
    }
}

/// One slot's shared handle plus the compositor's opened view of it.
pub(crate) struct SlotResources {
    handle: SharedHandle,
    adapter: (u32, i32),
    host: Mutex<Option<(usize, ID3D11Texture2D)>>,
}
impl std::fmt::Debug for SlotResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotResources")
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

impl SlotResources {
    /// The compositor's texture over this slot, opened once per host device.
    fn host_texture(&self, context: &Dx11CustomContext) -> Result<ID3D11Texture2D> {
        let key = context.cache_key();
        let mut host = self.host.lock().unwrap();
        if let Some((cached, texture)) = host.as_ref()
            && *cached == key
        {
            return Ok(texture.clone());
        }
        let dxgi: IDXGIDevice = context.device.cast()?;
        let desc = unsafe { dxgi.GetAdapter()?.GetDesc()? };
        let luid = (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart);
        ensure!(
            luid == self.adapter,
            Unsupported("engine and compositor use different GPUs".into())
        );
        let device: ID3D11Device1 = context
            .device
            .cast()
            .map_err(|_| Unsupported("the compositor lacks ID3D11Device1".into()))?;
        let texture: ID3D11Texture2D = unsafe { device.OpenSharedResource1(self.handle.0) }
            .context("opening the engine's shared texture failed")?;
        *host = Some((key, texture.clone()));
        Ok(texture)
    }
}

/// The D3D11 compositor retires frames on its fence worker thread.
pub(crate) fn retire_completed(_slots: &[Arc<SlotResources>]) {}

pub(crate) fn prepare_frame(
    _desc: &SurfaceDesc,
    resources: &SlotResources,
    sync: &SyncPoint,
    backend: &dyn Any,
) -> Result<Box<dyn Any>> {
    let context = backend
        .downcast_ref::<Dx11CustomContext>()
        .ok_or_else(|| Unsupported("engine surfaces require the D3D11 compositor".into()))?;
    let texture = resources.host_texture(context)?;
    if let Readiness::Fence(fence, value) = &sync.0
        && !fence.reached(*value)
    {
        match fence.host_fence(context)? {
            // SAFETY: opened on the host device from the engine's shared fence,
            // and the engine submitted the signal before presenting.
            Some(host) => unsafe { context.wait_for_fence(&host, *value)? },
            None => fence.cpu_wait(*value)?,
        }
    }
    // SAFETY: the texture was opened on the host device; the engine's writes
    // completed before present and simultaneous access needs no transition;
    // the slot's lease keeps it unmodified while this frame can be sampled.
    Ok(Box::new(unsafe { context.adopt_texture(texture) }?))
}

/// A wgpu engine (D3D12) on its own device rendering into surface slots.
///
/// The engine must run on the compositor's adapter. `present` waits for the
/// engine's GPU work on the calling thread, so call it from the engine thread.
pub struct WgpuSurfaceTarget {
    surface: EngineSurface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    textures: Vec<wgpu::Texture>,
    fence: Option<Arc<SharedFence>>,
    next: u64,
}

impl WgpuSurfaceTarget {
    /// Features the engine's wgpu device must enable.
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::empty()
    }

    /// Allocate shared slots on the engine device and create the surface.
    pub fn new(desc: SurfaceDesc, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self> {
        desc.validate()?;
        let raw: ID3D12Device = unsafe { device.as_hal::<Dx12>() }
            .ok_or_else(|| Unsupported("wgpu engine surfaces require the D3D12 backend".into()))?
            .raw_device()
            .clone();
        let luid = unsafe { raw.GetAdapterLuid() };
        let adapter = (luid.LowPart, luid.HighPart);
        let fence = unsafe { raw.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_SHARED) }
            .ok()
            .and_then(|fence| {
                let handle =
                    unsafe { raw.CreateSharedHandle(&fence, None, GENERIC_ALL.0, PCWSTR::null()) }
                        .ok()?;
                Some(Arc::new(SharedFence {
                    fence,
                    handle: SharedHandle(handle),
                    host: Mutex::new(None),
                }))
            });
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: desc.width as u64,
            Height: desc.height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: dxgi_format(desc.format),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
                | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
        };
        let size = wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: 1,
        };
        let mut slots = Vec::new();
        let mut textures = Vec::new();
        for _ in 0..desc.slots {
            let mut resource: Option<ID3D12Resource> = None;
            unsafe {
                raw.CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_SHARED,
                    &resource_desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
            }
            .context("shared slot allocation failed")?;
            let resource = resource.context("missing shared slot resource")?;
            let handle = SharedHandle(
                unsafe { raw.CreateSharedHandle(&resource, None, GENERIC_ALL.0, PCWSTR::null()) }
                    .context("shared slot handle creation failed")?,
            );
            let hal = unsafe {
                wgpu::hal::dx12::Device::texture_from_raw(
                    resource,
                    wgpu_format(desc.format),
                    wgpu::TextureDimension::D2,
                    size,
                    1,
                    1,
                )
            };
            textures.push(unsafe {
                device.create_texture_from_hal::<Dx12>(
                    hal,
                    &wgpu::TextureDescriptor {
                        label: Some("engine_surface_slot"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu_format(desc.format),
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::UNINITIALIZED,
                )
            });
            slots.push(Arc::new(SlotResources {
                handle,
                adapter,
                host: Mutex::new(None),
            }));
        }
        Ok(Self {
            surface: EngineSurface::from_slots(desc, slots)?,
            device: device.clone(),
            queue: queue.clone(),
            textures,
            fence,
            next: 0,
        })
    }

    /// Whether frames are handed over with a GPU-side wait (shared fence)
    /// rather than the engine waiting on its own thread.
    pub fn gpu_sync(&self) -> bool {
        self.fence.is_some()
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        wgpu_format(self.surface.desc().format)
    }

    /// The engine's texture over `slot`.
    pub fn texture(&mut self, slot: &SurfaceSlot) -> Result<wgpu::Texture> {
        Ok(self.textures[slot.index()].clone())
    }

    /// Submit `commands`, wait for the engine's GPU work to complete, then
    /// publish the slot.
    pub fn present(
        &mut self,
        slot: SurfaceSlot,
        commands: impl IntoIterator<Item = wgpu::CommandBuffer>,
    ) -> Result<()> {
        if let Some(fence) = &self.fence {
            // Signal after this submission; present only once it is submitted.
            self.next += 1;
            unsafe { self.queue.as_hal::<Dx12>() }
                .context("missing D3D12 engine queue")?
                .add_signal_fence(fence.fence.clone(), self.next);
            self.queue.submit(commands);
            return self
                .surface
                .present(slot, SyncPoint(Readiness::Fence(fence.clone(), self.next)));
        }
        let index = self.queue.submit(commands);
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(Duration::from_secs(5)),
            })
            .context("engine GPU work did not complete")?;
        self.surface.present(slot, SyncPoint::completed())
    }

    pub fn surface(&self) -> &EngineSurface {
        &self.surface
    }
}
