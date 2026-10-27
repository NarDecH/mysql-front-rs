//! Query view: SQL editor with multiple tabs, statement splitting and
//! stacked result sections — the workbench of the original application.

use egui::Context;
use mff_core::session::Grid;

use crate::app::App;
use crate::data_view::render_grid;
use crate::jobs::QuerySection;
use crate::session_view::QueryTab;

impl App {
    pub fn show_query_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        // ── Tab bar ────────────────────────────────────────────────────
        let (active, ntabs, running) = {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            (
                state.active_query,
                state.queries.len(),
                state
                    .queries
                    .get(state.active_query)
                    .map(|t| t.running)
                    .unwrap_or(false),
            )
        };
        let mut switch_to: Option<usize> = None;
        let mut add_tab = false;
        let mut close_tab = false;
        ui.horizontal(|ui| {
            for i in 0..ntabs {
                let busy = self
                    .sessions
                    .get(conn_id)
                    .and_then(|s| s.queries.get(i))
                    .map(|t| t.running)
                    .unwrap_or(false);
                let label = if busy {
                    format!("⏳ Tab {}", i + 1)
                } else {
                    format!("Tab {}", i + 1)
                };
                if ui.selectable_label(i == active, label).clicked() {
                    switch_to = Some(i);
                }
            }
            if ui.button("+").clicked() {
                add_tab = true;
            }
            if ntabs > 1 && ui.button("✕ close").clicked() {
                close_tab = true;
            }
        });
        if let Some(i) = switch_to {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.active_query = i;
            }
        }
        if add_tab {
            let state = self.sessions.get_mut(conn_id).unwrap();
            state
                .queries
                .push(QueryTab::new(format!("Query {}", state.queries.len() + 1)));
            state.active_query = state.queries.len() - 1;
        }
        if close_tab && !running {
            let state = self.sessions.get_mut(conn_id).unwrap();
            state.queries.remove(active);
            state.active_query = state.active_query.saturating_sub(1).min(state.queries.len() - 1);
        }
        ui.separator();

        // ── Editor + toolbar (mutate only the tab) ─────────────────────
        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;
        let mut run_clicked = false;
        let mut clear_clicked = false;
        let elapsed: Option<u128>;
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let Some(tab) = state.queries.get_mut(active) else {
                return;
            };
            ui.horizontal(|ui| {
                let run = ui.add_enabled(
                    !tab.running,
                    egui::Button::new(egui::RichText::new("▶ Run (Ctrl+Enter)").strong()),
                );
                if run.clicked() {
                    run_clicked = true;
                }
                if ui.button("✕ Clear results").clicked() {
                    clear_clicked = true;
                }
                if let Some(ms) = tab.elapsed_ms {
                    ui.label(format!("last run: {ms} ms"));
                }
                if tab.running {
                    ui.spinner();
                    ui.small("executing…");
                }
            });
            elapsed = tab.elapsed_ms;
            code = tab.code.clone();
            let mut edited = code.clone();
            let editor = egui::TextEdit::multiline(&mut edited)
                .hint_text("type SQL here — multiple statements allowed")
                .font(egui::TextStyle::Monospace)
                .desired_rows(8)
                .desired_width(f32::INFINITY)
                .code_editor();
            ui.add(editor);
            if edited != tab.code {
                tab.code = edited;
            }
            code = tab.code.clone();
        }

        let ctrl_enter = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);
        if clear_clicked {
            if let Some(tab) = self
                .sessions
                .get_mut(conn_id)
                .and_then(|s| s.queries.get_mut(active))
            {
                tab.sections.clear();
                tab.elapsed_ms = None;
            }
        }
        if (run_clicked || ctrl_enter) && !running {
            self.run_query(conn_id, active, code);
        }

        // ── Results ────────────────────────────────────────────────────
        ui.add_space(6.0);
        ui.separator();
        let sections: Vec<QuerySection> = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.queries.get(active))
            .map(|t| t.sections.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if sections.is_empty() {
                    ui.small("no results yet");
                }
                for (i, section) in sections.iter().enumerate() {
                    match section {
                        QuerySection::Grid(grid) => {
                            ui.horizontal(|ui| {
                                ui.strong(format!(
                                    "Result {} — {} row(s)",
                                    i + 1,
                                    grid.row_count()
                                ));
                                if ui.small_button("⭳ Export…").clicked() {
                                    export_grid_interactive(grid);
                                }
                            });
                            let mut sel = None;
                            render_grid(ui, grid, &mut sel, None);
                            ui.add_space(8.0);
                        }
                        QuerySection::Affected {
                            affected,
                            last_insert_id,
                            info,
                        } => {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 200, 120),
                                    format!(
                                        "✓ Result {}: {affected} row(s) affected, last_insert_id={last_insert_id}",
                                        i + 1
                                    ),
                                );
                                if let Some(info) = info {
                                    ui.small(info);
                                }
                            });
                        }
                        QuerySection::Error(e) => {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!("✗ Result {}: {e}", i + 1),
                            );
                        }
                    }
                }
                if let Some(ms) = elapsed {
                    ui.small(format!("total: {ms} ms"));
                }
            });
        let _ = ctx;
    }
}

/// Ask for a file name and write one grid to it (format from extension).
fn export_grid_interactive(grid: &Grid) {
    let file = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .add_filter("JSON", &["json"])
        .add_filter("SQL inserts", &["sql"])
        .add_filter("HTML", &["html"])
        .add_filter("XML", &["xml"])
        .set_file_name("result.csv")
        .save_file();
    let Some(path) = file else { return };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("csv")
        .to_string();
    let Some(format) = mff_core::ExportFormat::parse(&ext) else {
        return;
    };
    let opts = mff_core::export::ExportOptions {
        format,
        ..Default::default()
    };
    if let Ok(text) = mff_core::export::export_to_string(grid, None, &opts) {
        let _ = std::fs::write(&path, text);
    }
}
