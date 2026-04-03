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
