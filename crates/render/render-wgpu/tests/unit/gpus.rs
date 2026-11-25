use super::*;

/// The settings' choice reaches wgpu: the integrated GPU as low power, the dedicated one as
/// high performance, the system's as what the system gives an app asking for nothing.
#[test]
fn each_preference_asks_for_its_gpu() {
    assert_eq!(power_preference(GpuPreference::Integrated), wgpu::PowerPreference::LowPower);
    assert_eq!(power_preference(GpuPreference::Dedicated), wgpu::PowerPreference::HighPerformance);
    let system = if cfg!(target_os = "macos") {
        wgpu::PowerPreference::LowPower
    } else {
        wgpu::PowerPreference::None
    };
    assert_eq!(power_preference(GpuPreference::System), system);
}

/// The GPU made ahead (before any window) lists the computer's GPUs for the settings, the
/// one it chose marked: nothing else lists them when the first window takes it.
#[test]
fn the_gpu_made_ahead_lists_the_gpus() {
    let options = render_api::RenderOptions::default();
    crate::prepared::prepare(&options);
    // Made where it's made ahead (macOS, Windows) and where there's a GPU.
    let Some(prepared) = crate::prepared::take() else { return };
    // A software adapter (a machine without a GPU) isn't listed.
    let physical = matches!(
        prepared.ctx.adapter_info.device_type,
        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
    );
    if !physical {
        return;
    }
    let found = gpus();
    assert!(!found.is_empty(), "the GPUs listed");
    assert!(found.iter().any(|gpu| gpu.used), "the one drawing marked");
    drop(prepared);
}
