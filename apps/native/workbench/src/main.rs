#![allow(clippy::collapsible_if, clippy::possible_missing_else)]

mod backend;
mod icons;
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
            .with_min_inner_size([900.0, 600.0]),
        // Layout (window size, sidebar width) persists; the demo fixture keeps
        // its layout out of the real app directory.
        persistence_path: demo.then(|| std::env::temp_dir().join("dbm-demo-layout.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "DBM",
        options,
        Box::new(move |cc| Ok(Box::new(workbench::Workbench::new(cc, demo)))),
    )
}
