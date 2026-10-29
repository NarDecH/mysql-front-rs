//! "Diff schema with…" dialog and the result window: compare the
//! structure of the current table with a target table (possibly on
//! another connection) and generate ALTER statements for the target.

use egui::Context;

use crate::app::App;

pub struct SchemaDiffDlg {
    pub target_conn: String,
    pub target_db: String,
    pub target_table: String,
    pub running: bool,
    pub message: Option<String>,
}

/// A finished schema diff kept on `App` for the result window.
#[derive(Clone)]
pub struct SchemaDiffResultUi {
    pub diff: mff_core::schemadiff::SchemaDiff,
}

impl App {
    pub fn open_schema_diff(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get(conn_id) else {
            return;
        };
        if state.data.table.is_empty() {
            self.status = "open a table's data view first".into();
            return;
        }
        self.schemadiff_dlg = Some(SchemaDiffDlg {
            target_conn: conn_id.to_string(),
            target_db: state.current_db.clone(),
            target_table: String::new(),
            running: false,
            message: None,
        });
    }

    pub fn show_schema_diff_dialog(&mut self, ctx: &Context, conn_id: &str) {
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

        let mut go_clicked = false;
        let mut close_clicked = false;
        {
            let Some(dlg) = self.schemadiff_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("Diff schema")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.weak(format!("source: {src_table} — target receives the ALTERs"));
                    ui.add_space(4.0);
                    egui::Grid::new("schemadiff_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Target connection");
                            egui::ComboBox::from_id_salt("sd_conn")
                                .selected_text(
                                    conns
                                        .iter()
                                        .find(|(id, _)| *id == dlg.target_conn)
                                        .map(|(_, n)| n.as_str())
                                        .unwrap_or("—"),
                                )
                                .show_ui(ui, |ui| {
                                    for (id, n) in &conns {
                                        ui.selectable_value(&mut dlg.target_conn, id.clone(), n);
                                    }
                                });
                            ui.end_row();
                            ui.label("Target database");
                            let dbs = self
                                .sessions
                                .get(&dlg.target_conn)
                                .map(|s| s.databases.clone())
                                .unwrap_or_default();
                            egui::ComboBox::from_id_salt("sd_db")
                                .selected_text(if dlg.target_db.is_empty() {
                                    "—"
                                } else {
                                    &dlg.target_db
                                })
                                .show_ui(ui, |ui| {
                                    for d in &dbs {
                                        ui.selectable_value(&mut dlg.target_db, d.clone(), d);
                                    }
                                });
                            ui.end_row();
                            ui.label("Target table");
                            let tables = self
                                .sessions
                                .get(&dlg.target_conn)
                                .and_then(|s| s.objects.as_ref())
                                .map(|o| o.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>())
                                .unwrap_or_default();
                            egui::ComboBox::from_id_salt("sd_table")
                                .selected_text(if dlg.target_table.is_empty() {
                                    "(pick a table)"
                                } else {
                                    &dlg.target_table
                                })
                                .show_ui(ui, |ui| {
                                    for t in &tables {
                                        ui.selectable_value(&mut dlg.target_table, t.clone(), t);
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
                            "diffing…".to_string()
                        } else {
                            "Diff".to_string()
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
                            go_clicked = true;
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
            self.schemadiff_dlg = None;
        } else if go_clicked {
            self.run_schema_diff(conn_id);
        }
    }

    fn run_schema_diff(&mut self, conn_id: &str) {
        let (src_db, src_table, tgt_conn, tgt_db, tgt_table) = {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            let Some(dlg) = self.schemadiff_dlg.as_ref() else {
                return;
            };
            (
                state.current_db.clone(),
                state.data.table.clone(),
                dlg.target_conn.clone(),
                dlg.target_db.clone(),
                dlg.target_table.clone(),
            )
        };
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(dlg) = self.schemadiff_dlg.as_mut() else {
            return;
        };
        dlg.running = true;
        dlg.message = Some("diffing…".into());
        dlg.target_conn = tgt_conn.clone();
        let session = state.session.clone();
        drop(state);
        let target_session = self
            .sessions
            .get(&tgt_conn)
            .map(|s| s.session.clone());
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        if let Some(state) = self.sessions.get_mut(conn_id) {
            state.busy += 1;
        }
        self.status = "diffing…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let tgt_session = target_session.ok_or("target session is gone")?;
                let mut diff = mff_core::schemadiff::diff_tables(
                    &session,
                    &src_db,
                    &src_table,
                    &tgt_session,
                    &tgt_db,
                    &tgt_table,
                )
                .map_err(|e| e.to_string())?;
                diff.target_conn = tgt_conn.clone();
                Ok(diff)
            })();
            let _ = tx.send(crate::jobs::UiMsg::SchemaDiffDone {
                conn_id: id,
                result,
            });
        });
    }

    pub fn show_schema_diff_result(&mut self, ctx: &Context) {
        let Some(res) = self.schemadiff_result.as_ref() else {
            return;
        };
        let diff = &res.diff;
        let mut open = true;
        let mut close = false;
        let mut apply = false;
        egui::Window::new("Schema diff result")
            .open(&mut open)
            .default_width(760.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                ui.weak(format!(
                    "source: {}.{} · target receives the ALTERs: {}.{}",
                    diff.source_db, diff.source_table, diff.target_db, diff.target_table
                ));
                ui.separator();
                if diff.alter.is_empty() {
                    ui.colored_label(
                        egui::Color32::from_rgb(120, 200, 120),
                        "✓ structures are identical",
                    );
                } else {
                    ui.strong(format!("{} ALTER statement(s):", diff.alter.len()));
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.monospace(diff.alter.join("\n\n"));
                        });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !diff.alter.is_empty(),
                            egui::Button::new(
                                egui::RichText::new("Apply to target")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(crate::theme::accent(ui.ctx()))
                            .min_size(egui::vec2(0.0, 24.0)),
                        )
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if close || !open {
            self.schemadiff_result = None;
        }
        if apply {
            self.apply_schema_diff();
        }
    }

    fn apply_schema_diff(&mut self) {
        let Some(res) = self.schemadiff_result.clone() else {
            return;
        };
        let Some(state) = self.sessions.get_mut(&res.diff.target_conn) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = res.diff.target_conn.clone();
        let stmts = res.diff.alter.clone();
        let target_label = format!("{}.{}", res.diff.target_db, res.diff.target_table);
        state.busy += 1;
        self.status = "applying schema changes…".into();
        self.schemadiff_result = None;
        std::thread::spawn(move || {
            let mut result: std::result::Result<(), String> = Ok(());
            for r in session.run_script(&stmts, false) {
                if let Err(e) = r {
                    result = Err(e.to_string());
                    break;
                }
            }
            let _ = tx.send(crate::jobs::UiMsg::SchemaDiffApplied {
                conn_id: id,
                target: target_label,
                result,
            });
        });
    }
}
