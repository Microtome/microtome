//! Isosurface viewer application entry point.
//!
//! A standalone viewer for isosurface extraction via dual contouring,
//! supporting both octree and k-d tree structures.

mod app;
mod camera;
mod renderer;
mod viewport;

fn main() {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Isosurface Viewer"),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew({
                let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
                setup.device_descriptor = std::sync::Arc::new(|adapter| {
                    let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    };
                    wgpu::DeviceDescriptor {
                        required_features: wgpu::Features::POLYGON_MODE_LINE,
                        required_limits: base_limits,
                        ..Default::default()
                    }
                });
                setup
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "Isosurface Viewer",
        options,
        Box::new(|cc| Ok(Box::new(app::IsosurfaceApp::new(cc)))),
    ) {
        log::error!("Failed to run application: {e}");
    }
}
