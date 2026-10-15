//! CraftStudio: Unified Open-Source Creative Suite
//!
//! Replaces Adobe Photoshop, Premiere Pro, and After Effects with a single,
//! interconnected application in pure Rust.

mod interconnect;
mod app;

use app::CraftStudioApp;

fn main() -> eframe::Result<()> {
    let _ = env_logger::try_init();

    // Register Windows Media Foundation hardware video decoding (Direct3D 11 / DXVA)
    let _ = filmcraft_platform::register();

    let mut options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("CraftStudio — Unified Creative Suite")
            .with_inner_size([1680.0, 980.0])
            .with_min_inner_size([1024.0, 640.0]),
        ..Default::default()
    };

    // Configure WGPU for High Performance discrete GPU and VSync frame capping
    options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoVsync;
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut create) = options.wgpu_options.wgpu_setup {
        create.power_preference = eframe::wgpu::PowerPreference::HighPerformance;
        create.instance_descriptor.backends = eframe::wgpu::Backends::PRIMARY;
    }

    eframe::run_native(
        "CraftStudio",
        options,
        Box::new(|cc| Ok(Box::new(CraftStudioApp::new(cc)))),
    )
}
