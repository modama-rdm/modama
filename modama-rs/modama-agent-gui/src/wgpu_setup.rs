//! Renderer setup shared by every window of this executable (the GUI and the mount popup).

use eframe::egui_wgpu::wgpu;

/// What to try when no window could be opened
pub const BACKEND_HINT: &str = "Another graphics backend may work: set the environment variable \
    WGPU_BACKEND to dx12, vulkan or gl, then start again.";

/// Leaves out wgpu's GL backend when a primary backend (Vulkan, DX12) has an adapter.
///
/// The default setup also initialises GL, and lets it compete for the adapter. That fails in
/// two known ways:
///
/// - Over a forwarded X11 connection, Mesa's EGL sends a `DRI2Connect` that the X server
///   rejects while adapters are enumerated, and winit aborts on the resulting X error.
/// - On Windows 10 with basic Intel graphics, the GUI crashed with the default backends but
///   worked with DX12 alone.
///
/// GL stays enabled when no primary backend has an adapter (Windows 7 has no DX12), and
/// `WGPU_BACKEND` still overrides the choice.
pub fn wgpu_options() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::WgpuSetup;

    let mut config = eframe::WgpuConfiguration::default();
    if let Some(backends) = wgpu::Backends::from_env() {
        log::info!("WGPU_BACKEND is set, using {backends:?}");
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
    for adapter in &adapters {
        log::info!("Found {}", describe(&adapter.get_info()));
    }
    if adapters.is_empty() {
        log::info!("No Vulkan or DX12 adapter found, keeping the GL backend enabled");
    } else {
        setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    }
    config
}

/// Log the adapter a window ended up rendering with
pub fn log_adapter(cctx: &eframe::CreationContext<'_>) {
    if let Some(render_state) = &cctx.wgpu_render_state {
        log::info!(
            "Rendering with {}",
            describe(&render_state.adapter.get_info())
        );
    }
}

fn describe(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{:?} adapter {:?} ({:?}, driver {:?} {:?})",
        info.backend, info.name, info.device_type, info.driver, info.driver_info
    )
}
