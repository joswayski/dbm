#![allow(clippy::collapsible_if, clippy::possible_missing_else)]
// Release builds are GUI apps on Windows: no console window behind DBM.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod backend;
mod icons;
mod messages;
mod table_view;
mod theme;
mod workbench;

use eframe::{Renderer, egui};

fn main() -> eframe::Result {
    let demo = std::env::args().any(|arg| arg == "--demo");
    let options = eframe::NativeOptions {
        renderer: Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(if demo { "DBM — DEMO" } else { "DBM" })
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(std::sync::Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../desktop/src-tauri/icons/icon.png"
                ))
                .expect("bundled app icon is a valid PNG"),
            )),
        // Layout (window size, sidebar width) persists; the demo fixture keeps
        // its layout out of the real app directory.
        wgpu_options: wgpu_options(),
        persistence_path: demo.then(|| std::env::temp_dir().join("dbm-demo-layout.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "DBM",
        options,
        Box::new(move |cc| Ok(Box::new(workbench::Workbench::new(cc, demo)))),
    )
}

/// One graphics backend per platform on the integrated GPU. egui's default
/// initialises every compiled backend (DirectX 12 and Vulkan on Windows) and
/// prefers the discrete GPU, which wakes it on dual-GPU laptops; a data grid
/// needs neither. `WGPU_BACKEND` and `WGPU_POWER_PREF` still override.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let native = if cfg!(windows) {
        wgpu::Backends::DX12
    } else if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::VULKAN
    };
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    setup.instance_descriptor.backends = wgpu::Backends::from_env().unwrap_or(native);
    setup.power_preference =
        wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
    eframe::egui_wgpu::WgpuConfiguration {
        wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}
