//! The GPU made ahead, before any window exists (see [`prepare`]): the adapter, the device,
//! the shaders and a first window's pipelines for the format it most likely gets, on a
//! thread of their own while the app reads its settings, loads its modules and opens the
//! window. The first window takes them; whatever doesn't fit its surface (another adapter,
//! another format) is made then, as without it.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use render_api::{GpuApi, RenderOptions};

use super::{Drawer, GpuContext};

/// What was made ahead: the context and, for the format guessed, a window's pipelines.
pub(super) struct Prepared {
    pub ctx: Arc<GpuContext>,
    pub drawer: Option<Drawer>,
}

/// The thread making it (taken, waited for, by the first window).
static PREPARING: Mutex<Option<JoinHandle<Option<Prepared>>>> = Mutex::new(None);

/// Its thread's name.
pub const THREAD: &str = "gpu-prepare";

/// The surface format a window most likely gets (the first without sRGB conversion, as
/// [`super::WgpuRenderer::surface_config`] picks): BGRA on Metal and DX12.
const LIKELY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Starts making the GPU ahead (once; on macOS and Windows, with Metal / DX12 — the APIs
/// that need no window to choose an adapter; elsewhere nothing changes).
pub fn prepare(options: &RenderOptions) {
    let backends = match options.preferred_api {
        GpuApi::Auto | GpuApi::Metal if cfg!(target_os = "macos") => wgpu::Backends::METAL,
        GpuApi::Auto | GpuApi::Dx12 if cfg!(windows) => wgpu::Backends::DX12,
        _ => return,
    };
    let mut slot = PREPARING.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_some() {
        return;
    }
    let transparent = options.transparent;
    let started = std::thread::Builder::new()
        .name(THREAD.into())
        .spawn(move || pollster::block_on(make(backends, transparent)));
    match started {
        Ok(handle) => *slot = Some(handle),
        Err(e) => tracing::debug!("the GPU isn't made ahead: {e}"),
    }
}

/// What was made ahead, if anything (waits for it if it's still being made).
pub(super) fn take() -> Option<Prepared> {
    let handle = PREPARING.lock().unwrap_or_else(PoisonError::into_inner).take()?;
    handle.join().ok().flatten()
}

async fn make(backends: wgpu::Backends, transparent: bool) -> Option<Prepared> {
    let started = std::time::Instant::now();
    // No display handle: Metal and DX12 don't use it.
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    // As `select_adapter` makes it (see there).
    if transparent {
        desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    desc.flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    let instance = wgpu::Instance::new(desc);
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .inspect_err(|e| tracing::debug!("no GPU made ahead: {e}"))
        .ok()?;
    let ctx = Arc::new(
        GpuContext::new(instance, adapter)
            .await
            .inspect_err(|e| tracing::debug!("no GPU made ahead: {e}"))
            .ok()?,
    );
    let drawer = Some(Drawer::new(&ctx, LIKELY_FORMAT, transparent));
    tracing::info!(took = ?started.elapsed(), "GPU made ahead");
    Some(Prepared { ctx, drawer })
}
