//! Renderer setup shared by every window of this executable (the GUI and the mount popup).

/// Leaves out wgpu's GL backend when a primary backend (Vulkan) has an adapter.
///
/// The default setup initialises GL while enumerating adapters. Over a forwarded X11
/// connection, Mesa's EGL then sends a `DRI2Connect` that the X server rejects, and winit
/// aborts on the resulting X error. GL stays enabled when Vulkan has no adapter, and
/// `WGPU_BACKEND` still overrides the choice.
pub fn wgpu_options() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};

    let mut config = eframe::WgpuConfiguration::default();
    if wgpu::Backends::from_env().is_some() {
        return config;
    }
    let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup else {
        return config;
    };

    // Without a display handle, enumerating Vulkan adapters does not talk to the X server.
    let probe = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
    });
    let adapters = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime.block_on(probe.enumerate_adapters(wgpu::Backends::PRIMARY)),
        Err(err) => {
            log::warn!("Could not probe for wgpu adapters, keeping default backends: {err}");
            return config;
        }
    };
    if adapters.is_empty() {
        log::info!("No Vulkan adapter found, keeping the GL backend enabled");
    } else {
        setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    }
    config
}
