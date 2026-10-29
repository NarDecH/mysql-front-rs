//! mysql-front-rs — desktop GUI entry point.
//!
//! A MySQL GUI client rewritten in Rust, based on the feature set of
//! MySQL-Front 6 (originally Delphi). See the workspace README for details.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backup_dlg;
mod data_view;
mod chart;
mod compare_dlg;
mod db_rename;
mod dialogs;
mod fonts;
mod import_dlg;
mod i18n;
mod info_view;
mod insert_row;
mod jobs;
mod new_dialog;
mod query_view;
mod schemadiff_dlg;
mod search_dlg;
mod server_admin;
mod server_view;
mod session_view;
mod transfer;
mod structure_view;
mod table_designer;
mod theme;

/// Decode the embedded logo for the window / taskbar icon.
fn load_icon() -> egui::IconData {
    let png = include_bytes!("../assets/logo_256.png");
    let img = image::load_from_memory(png).expect("embedded logo").to_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        width,
        height,
        rgba: img.into_raw(),
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("mysql-front-rs")
            .with_icon(std::sync::Arc::new(load_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "mysql-front-rs",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
    .expect("failed to start the GUI");
}
