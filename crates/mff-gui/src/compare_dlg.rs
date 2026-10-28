//! "Compare table with…" dialog and the resulting diff window.
//!
//! Source = the table currently open in the data view of this session.
//! Target = another connection/database/table; both are streamed by key
//! (primary key when present, whole row otherwise) and diffed in
//! `mff_core::compare`.

use egui::Context;
use mff_core::compare::CompareReport;
use mff_core::session::{Cell, Grid};

use crate::app::App;
use crate::data_view::render_grid;

/// Dialog state (lives on `App`).
pub struct CompareDlg {
    pub target_conn: String,
    pub target_db: String,
    pub target_table: String,
    pub running: bool,
    pub message: Option<String>,
}

/// The finished report (lives on `App`, separate from the dialog so the
/// window can outlive the chooser).
pub struct CompareReportUi {
    pub report: CompareReport,
}

impl App {
    pub fn open_compare_dialog(&mut self, conn_id: &str) {
        // Source table comes from the data view.
        let Some(state) = self.sessions.get(conn_id) else {
            return;
        };
        if state.data.table.is_empty() {
            self.status = "open a table's data view first".into();
            return;
        }
        // Default target: this very session and database.
        let target_db = state.current_db.clone();
        self.compare_dlg = Some(CompareDlg {
            target_conn: conn_id.to_string(),
            target_db,
            target_table: String::new(),
            running: false,
            message: None,
        });
    }

    pub fn show_compare_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let conns: Vec<(String, String)> = self
            .sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.conn_name.clone()))
            .collect();
        let src_table = self
            .sessions
            .get(conn_id)
            .map(|s| s.data.table.clone())
            .unwrap_or_default();

        let mut compare_clicked = false;
        let mut close_clicked = false;
        {
            let Some(dlg) = self.compare_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("Compare table")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.weak(format!("source: {}", src_table));
                    ui.add_space(4.0);
                    egui::Grid::new("compare_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Target connection");
                            egui::ComboBox::from_id_salt("cmp_conn")
                                .selected_text(
                                    conns
                                        .iter()
                                        .find(|(id, _)| *id == dlg.target_conn)
                                        .map(|(_, n)| n.as_str())
                                        .unwrap_or("—"),
                                )
                                .show_ui(ui, |ui| {
                                    for (id, n) in &conns {
                                        ui.selectable_value(
                                            &mut dlg.target_conn,
                                            id.clone(),
                                            n,
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label("Target database");
                            let dbs = self
                                .sessions
                                .get(&dlg.target_conn)
                                .map(|s| s.databases.clone())
                                .unwrap_or_default();
                            egui::ComboBox::from_id_salt("cmp_db")
                                .selected_text(if dlg.target_db.is_empty() {
                                    "—"
                                } else {
                                    &dlg.target_db
                                })
                                .show_ui(ui, |ui| {
                                    for d in &dbs {
                                        ui.selectable_value(
                                            &mut dlg.target_db,
                                            d.clone(),
                                            d,
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label("Target table");
                            let tables = self
                                .sessions
                                .get(&dlg.target_conn)
                                .and_then(|s| s.objects.as_ref())
                                .map(|o| {
                                    o.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            egui::ComboBox::from_id_salt("cmp_table")
                                .selected_text(if dlg.target_table.is_empty() {
                                    if dlg.target_table.is_empty() && tables.len() == 1 {
                                        "(pick a table)"
                                    } else {
                                        "(pick a table)"
                                    }
                                } else {
                                    &dlg.target_table
                                })
                                .show_ui(ui, |ui| {
                                    for t in &tables {
                                        ui.selectable_value(
                                            &mut dlg.target_table,
                                            t.clone(),
                                            t,
                                        );
                                    }
                                });
                            ui.end_row();
                        });

                    if let Some(msg) = &dlg.message {
                        ui.label(msg);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can = !dlg.running
                            && !dlg.target_db.is_empty()
                            && !dlg.target_table.is_empty();
                        let label = if dlg.running {
                            "comparing…".to_string()
                        } else {
                            "Compare".to_string()
                        };
                        if ui
                            .add_enabled(
                                can,
                                egui::Button::new(
                                    egui::RichText::new(label)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(crate::theme::accent(ui.ctx()))
                                .min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            compare_clicked = true;
                        }
                        if dlg.running {
                            ui.spinner();
                        }
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            if !open {
                close_clicked = true;
            }
        }

        if close_clicked {
            self.compare_dlg = None;
        } else if compare_clicked {
            self.run_compare(conn_id);
        }
    }

    /// Run the comparison on a background thread.
    fn run_compare(&mut self, conn_id: &str) {
        let (src_db, src_table, tgt_conn, tgt_db, tgt_table, running) = {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            let Some(dlg) = self.compare_dlg.as_ref() else {
                return;
            };
            (
                state.current_db.clone(),
                state.data.table.clone(),
                dlg.target_conn.clone(),
                dlg.target_db.clone(),
                dlg.target_table.clone(),
                dlg.running,
            )
        };
        if running {
            return;
        }
        let mut dlg = self.compare_dlg.as_mut().unwrap();
        dlg.message = Some("comparing…".into());
        dlg.running = true;
        if tgt_conn == conn_id {
            // Same session — clone the session handle.
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let session = state.session.clone();
            let tx = self.jobs.sender();
            let id = conn_id.to_string();
            state.busy += 1;
            self.status = "comparing…".into();
            std::thread::spawn(move || {
                let result = mff_core::compare::compare_tables(
                    &session,
                    &src_db,
                    &src_table,
                    &session,
                    &tgt_db,
                    &tgt_table,
                    100_000,
                )
                .map_err(|e| e.to_string());
                let _ = tx.send(crate::jobs::UiMsg::CompareDone {
                    conn_id: id,
                    result,
                });
            });
        } else {
            // Different connection — open a second session handle.
            let Some(target_state) = self.sessions.get(&tgt_conn) else {
                dlg.message = Some("target session is gone".into());
                return;
            };
            let target_session = target_state.session.clone();
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let session = state.session.clone();
            let tx = self.jobs.sender();
            let id = conn_id.to_string();
            state.busy += 1;
            self.status = "comparing…".into();
            std::thread::spawn(move || {
                let result = mff_core::compare::compare_tables(
                    &session,
                    &src_db,
                    &src_table,
                    &target_session,
                    &tgt_db,
                    &tgt_table,
                    100_000,
                )
                .map_err(|e| e.to_string());
                let _ = tx.send(crate::jobs::UiMsg::CompareDone {
                    conn_id: id,
                    result,
                });
            });
        }
    }

    /// The diff report window.
    pub fn show_compare_report(&mut self, ctx: &Context) {
        let Some(ui_state) = self.compare_report.as_ref() else {
            return;
        };
        let report = &ui_state.report;
        let mut open = true;
        let mut close = false;

        let mk_grid = |cols: &Vec<mff_core::session::ColMeta>,
                       rows: &Vec<Vec<Cell>>|
         -> Grid {
            Grid {
                columns: cols.clone(),
                rows: rows.clone(),
            }
        };

        egui::Window::new("Comparison result")
            .open(&mut open)
            .default_width(900.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                ui.strong(report.summary());
                if report.truncated {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 190, 120),
                        "⚠ results truncated at 500 rows per bucket",
                    );
                }
                ui.weak(format!(
                    "key: {}",
                    if report.key_columns.is_empty() {
                        "(whole rows)".to_string()
                    } else {
                        report.key_columns.join(", ")
                    }
                ));
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let sections: [(&str, &Vec<Vec<Cell>>); 2] = [
                            (format!("Only in {}", report.src_table).leak(), &report.only_source),
                            (format!("Only in {}", report.tgt_table).leak(), &report.only_target),
                        ];
                        for (title, rows) in sections {
                            if !rows.is_empty() {
                                ui.heading(title);
                                let mut sel = None;
                                render_grid(ui, &mk_grid(&report.columns, rows), &mut sel, None);
                                ui.add_space(8.0);
                            }
                        }
                        if !report.different.is_empty() {
                            ui.heading("Different rows (source ⟶ target)");
                            for (i, (srow, trow)) in report.different.iter().enumerate() {
                                ui.collapsing(format!("row {}", i + 1), |ui| {
                                    ui.label("source:");
                                    let mut sel = None;
                                    render_grid(
                                        ui,
                                        &mk_grid(&report.columns, &vec![srow.clone()]),
                                        &mut sel,
                                        None,
                                    );
                                    ui.label("target:");
                                    render_grid(
                                        ui,
                                        &mk_grid(&report.columns, &vec![trow.clone()]),
                                        &mut sel,
                                        None,
                                    );
                                });
                            }
                        }
                        if report.only_source.is_empty()
                            && report.only_target.is_empty()
                            && report.different.is_empty()
                        {
                            ui.colored_label(
                                egui::Color32::from_rgb(120, 200, 120),
                                "✓ both tables have identical data",
                            );
                        }
                    });

                ui.add_space(6.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            });

        if !open || close {
            self.compare_report = None;
        }
    }
}
