//! "Data transfer" dialog: copy selected tables/views (structure + data)
//! from the current database to another database — on this server or on a
//! different server (another saved connection).

use egui::Context;
use mff_core::session::Session;
use mff_core::transfer::{self, TransferItem, TransferOptions, TransferOutcome};

use crate::app::App;
use crate::jobs::UiMsg;

#[derive(Debug, Clone)]
pub struct TransferDialog {
    /// (item, selected) snapshot of the source database objects.
    pub items: Vec<(TransferItem, bool)>,
    /// (connection id, display name) options for the target.
    pub targets: Vec<(String, String)>,
    pub target_idx: usize,
    pub target_db: String,
    pub create_db: bool,
    pub include_data: bool,
    pub drop_existing: bool,
    pub running: bool,
    pub result: Option<String>,
}

impl App {
    pub fn open_transfer(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get(conn_id) else {
            return;
        };
        let items: Vec<(TransferItem, bool)> = state
            .objects
            .as_ref()
            .map(|o| {
                o.tables
                    .iter()
                    .map(|t| TransferItem { name: t.name.clone(), is_view: false })
                    .chain(
                        o.views
                            .iter()
                            .map(|v| TransferItem { name: v.name.clone(), is_view: true }),
                    )
                    .map(|i| (i, true))
                    .collect()
            })
            .unwrap_or_default();
        let targets: Vec<(String, String)> = self
            .registry
            .sorted()
            .into_iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        let target_db = state.current_db.clone();
        self.transfer_dlg = Some(crate::transfer::TransferDialog {
            items,
            targets,
            target_idx: 0,
            target_db,
            create_db: true,
            include_data: true,
            drop_existing: false,
            running: false,
            result: None,
        });
    }

    pub fn show_transfer_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let source_db = self.current_db_for(conn_id);
        let source_name = self
            .sessions
            .get(conn_id)
            .map(|s| s.conn_name.clone())
            .unwrap_or_default();
        let Some(dlg) = self.transfer_dlg.as_mut() else {
            return;
        };
        let selected_count = dlg.items.iter().filter(|(_, sel)| *sel).count();

        let mut open = true;
        let mut start_clicked = false;
        let mut close_clicked = false;

        egui::Window::new("Data transfer")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Source:");
                    ui.label(
                        egui::RichText::new(format!("{source_name} · {source_db}")).strong(),
                    );
                });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.strong(format!("Objects ({selected_count} selected)"));
                    if ui.small_button("Select all").clicked() {
                        for (_, sel) in dlg.items.iter_mut() {
                            *sel = true;
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        for (_, sel) in dlg.items.iter_mut() {
                            *sel = false;
                        }
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (item, sel) in dlg.items.iter_mut() {
                            let icon = if item.is_view {
                                crate::theme::view_icon()
                            } else {
                                crate::theme::table_icon()
                            };
                            ui.horizontal(|ui| {
                                ui.checkbox(sel, "");
                                crate::theme::icon_label(ui, &icon, &item.name);
                            });
                        }
                    });
                ui.separator();

                egui::Grid::new("xfer_target")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("Target connection");
                        let names: Vec<String> =
                            dlg.targets.iter().map(|(_, n)| n.clone()).collect();
                        egui::ComboBox::from_id_salt("xfer_conn")
                            .selected_text(
                                names.get(dlg.target_idx).cloned().unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    ui.selectable_value(&mut dlg.target_idx, i, n);
                                }
                            });
                        ui.end_row();
                        ui.label("Target database");
                        ui.add(
                            egui::TextEdit::singleline(&mut dlg.target_db)
                                .desired_width(220.0),
                        );
                        ui.end_row();
                    });
                ui.checkbox(&mut dlg.create_db, "Create the target database if missing");
                ui.checkbox(&mut dlg.include_data, "Include table data (rows)");
                ui.checkbox(
                    &mut dlg.drop_existing,
                    "Drop existing tables/views on the target first",
                );

                ui.add_space(6.0);
                if dlg.running {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("transferring… (progress in the status bar)");
                    });
                }
                if let Some(res) = &dlg.result {
                    ui.label(res);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if dlg.running {
                        ui.add_enabled(false, egui::Button::new("Start transfer"));
                    } else if ui.add_enabled(
                        selected_count > 0,
                        crate::theme::primary_button_widget(ui, "Start transfer"),
                    ).clicked() {
                        start_clicked = true;
                    }
                    if ui.button(if dlg.running { "Hide" } else { "Close" }).clicked() {
                        close_clicked = true;
                    }
                });
            });

        if close_clicked && !dlg_running(self) {
            self.transfer_dlg = None;
            return;
        }
        if !open && !dlg_running(self) {
            self.transfer_dlg = None;
            return;
        }

        if start_clicked {
            let Some(snapshot) = self.transfer_dlg.clone() else {
                return;
            };
            let items: Vec<TransferItem> = snapshot
                .items
                .iter()
                .filter(|(_, sel)| *sel)
                .map(|(i, _)| i.clone())
                .collect();
            let Some(source_state) = self.sessions.get(conn_id) else {
                return;
            };
            let source_session = source_state.session.clone();
            let source_db = source_state.current_db.clone();
            let target_conn = snapshot
                .targets
                .get(snapshot.target_idx)
                .map(|(id, _)| id.clone())
                .unwrap_or_default();
            let same_server = target_conn == conn_id;
            let target_conn_def = self.registry.get(&target_conn).cloned();
            let tx = self.jobs.sender();
            let id = conn_id.to_string();
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.busy += 1;
                if let Some(d) = self.transfer_dlg.as_mut() {
                    d.running = true;
                    d.result = None;
                }
            }
            std::thread::spawn(move || {
                let outcome = (|| -> TransferOutcome {
                    let empty = mff_core::transfer::TransferSummary::default();
                    let tsession = if same_server {
                        source_session.clone()
                    } else {
                        let Some(def) = target_conn_def else {
                            return TransferOutcome {
                                summary: empty,
                                error: Some("target connection not found".into()),
                            };
                        };
                        match Session::connect(&def) {
                            Ok(s) => s,
                            Err(e) => {
                                return TransferOutcome {
                                    summary: empty,
                                    error: Some(format!("connect to target failed: {e}")),
                                }
                            }
                        }
                    };
                    let opts = TransferOptions {
                        include_data: snapshot.include_data,
                        drop_existing: snapshot.drop_existing,
                        create_target_db: snapshot.create_db,
                        batch_rows: 1000,
                    };
                    transfer::transfer(
                        &source_session,
                        &source_db,
                        &tsession,
                        &snapshot.target_db.trim(),
                        &items,
                        &opts,
                        &|m| {
                            let _ = tx.send(UiMsg::Status(m.to_string()));
                        },
                    )
                })();
                let _ = tx.send(UiMsg::TransferDone {
                    conn_id: id,
                    text: outcome.describe(),
                    ok: outcome.is_ok(),
                    target_same_server: same_server,
                    target_db: snapshot.target_db.trim().to_string(),
                });
            });
        }

        fn dlg_running(app: &App) -> bool {
            app.transfer_dlg
                .as_ref()
                .map(|d| d.running)
                .unwrap_or(false)
        }
    }
}
