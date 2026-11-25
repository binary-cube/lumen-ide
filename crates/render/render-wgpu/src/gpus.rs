//! The computer's GPUs: which one is asked for (the settings' preference as wgpu's), and
//! those found, noted once for the settings' list whichever way the GPU was chosen: on the
//! thread making it ahead, or — with the first window, on the UI thread — on a worker
//! (listing them opens each adapter: DX12 makes a device of each to test it).

use render_api::GpuPreference;

/// The computer's GPUs, as found when the GPU was chosen (empty before).
static GPUS: std::sync::OnceLock<Vec<render_api::GpuInfo>> = std::sync::OnceLock::new();

/// The computer's GPUs (the settings list them), the one drawing marked; empty until the
/// GPU was chosen and they were listed. Memory only.
pub fn gpus() -> &'static [render_api::GpuInfo] {
    GPUS.get().map_or(&[], Vec::as_slice)
}

/// The adapter asked of wgpu for `gpu`. The system's choice is what the system would give
/// an app that asks for nothing in particular:
/// - Windows: wgpu takes the first adapter DXGI lists, the one the system puts first for
///   the app (the user's choice in the system's graphics settings, else its own);
/// - macOS: the integrated GPU, as the system runs an app that doesn't ask for the
///   high-power one (wgpu would otherwise take the dedicated one first, as Metal lists the
///   devices: that switches a laptop to it for as long as the app runs). One GPU (every
///   Apple silicon Mac): that one.
/// - elsewhere: wgpu's order, as its API lists them.
pub(crate) fn power_preference(gpu: GpuPreference) -> wgpu::PowerPreference {
    match gpu {
        GpuPreference::System if cfg!(target_os = "macos") => wgpu::PowerPreference::LowPower,
        GpuPreference::System => wgpu::PowerPreference::None,
        // An editor rarely needs the dedicated GPU; the integrated one saves battery.
        GpuPreference::Integrated => wgpu::PowerPreference::LowPower,
        GpuPreference::Dedicated => wgpu::PowerPreference::HighPerformance,
    }
}

/// The worker pool the GPUs are listed on after a window chose one.
const LIST_POOL: &str = "gpu-list";

/// [`remember`] on a worker (the caller is the UI thread: a window choosing its GPU), once.
pub(crate) fn remember_later(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    chosen: &wgpu::Adapter,
) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if GPUS.get().is_some() || STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let instance = instance.clone();
    let used = chosen.get_info();
    kernel::work::run(LIST_POOL, move || {
        pollster::block_on(remember_of(&instance, backends, &used));
    });
}

/// The GPUs `backends` can draw with noted, once (each physical one once, the software ones
/// left out), `chosen` marked as used.
pub(crate) async fn remember(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    chosen: &wgpu::Adapter,
) {
    remember_of(instance, backends, &chosen.get_info()).await;
}

/// [`remember`] of the adapter `used` describes.
async fn remember_of(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    used: &wgpu::AdapterInfo,
) {
    if GPUS.get().is_some() {
        return;
    }
    let mut found: Vec<render_api::GpuInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for adapter in instance.enumerate_adapters(backends).await {
        let info = adapter.get_info();
        let physical = matches!(
            info.device_type,
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
        );
        if !physical || !seen.insert((info.vendor, info.device, info.name.clone())) {
            continue;
        }
        found.push(render_api::GpuInfo {
            dedicated: info.device_type == wgpu::DeviceType::DiscreteGpu,
            used: info.vendor == used.vendor
                && info.device == used.device
                && info.name == used.name,
            name: info.name,
        });
    }
    let _ = GPUS.set(found);
}

#[cfg(test)]
#[path = "../tests/unit/gpus.rs"]
mod tests;
