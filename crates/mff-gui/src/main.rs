//! mysql-front-rs — desktop GUI entry point.
//!
//! A MySQL GUI client rewritten in Rust, based on the feature set of
//! MySQL-Front 6 (originally Delphi). See the workspace README for details.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod data_view;
mod dialogs;
mod fonts;
mod info_view;
mod jobs;
mod query_view;
mod session_view;
mod structure_view;

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("mysql-front-rs"),
        ..Default::default()
    };
    eframe::run_native(
        "mysql-front-rs",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
    .expect("failed to start the GUI");
}
