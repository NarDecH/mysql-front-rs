//! "Backup database" and "Restore database" dialogs. The backup builds
//! one SQL script via `mff_core::backup`; the restore replays a script
//! file with `mff_core::import::import_sql` into the chosen database.

use egui::Context;

use crate::app::App;

pub struct BackupDlg {
    pub path: String,
    pub include_create: bool,
    pub include_data: bool,
    pub include_views: bool,
    pub batch_size: usize,
    pub message: Option<String>,
    pub running: bool,
}

impl Default for BackupDlg {
    fn default() -> Self {
        Self {
            path: String::new(),
            include_create: true,
            include_data: true,
            include_views: true,
            batch_size: 500,
            message: None,
            running: false,
        }
    }
}

pub struct RestoreDlg {
    pub path: String,
    /// Existing database to restore into.
    pub target_db: String,
    /// Restore into a brand-new database instead.
    pub create_new: bool,
    pub new_db: String,
    pub message: Option<String>,
    pub running: bool,
}

impl Default for RestoreDlg {
    fn default() -> Self {
        Self {
            path: String::new(),
            target_db: String::new(),
            create_new: false,
            new_db: format!("restored_{}", unique_suffix()),
            message: None,
            running: false,
        }
    }
}

fn unique_suffix() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:x}")
}

impl App {
    pub fn show_backup_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let db = self
            .sessions
            .get(conn_id)
            .map(|s| s.current_db.clone())
            .unwrap_or_default();

        let mut backup_clicked = false;
        let mut close_clicked = false;
        {
            let Some(dlg) = self.backup_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new(format!("Backup database “{db}”"))
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::Grid::new("backup_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Output file");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut dlg.path)
                                        .desired_width(250.0)
                                        .hint_text("backup.sql"),
                                );
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .set_file_name(format!("{db}_backup.sql"))
                                        .save_file()
                                    {
                                        dlg.path = p.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Include CREATE TABLE");
                            ui.checkbox(&mut dlg.include_create, "");
                            ui.end_row();
                            ui.label("Include data (INSERTs)");
                            ui.checkbox(&mut dlg.include_data, "");
                            ui.end_row();
                            ui.label("Include views");
                            ui.checkbox(&mut dlg.include_views, "");
                            ui.end_row();
                            ui.label("Rows per INSERT");
                            ui.add(
                                egui::DragValue::new(&mut dlg.batch_size).range(1..=100_000),
                            );
                            ui.end_row();
                        });

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(44.0)
                        .show(ui, |ui| {
                            if let Some(msg) = &dlg.message {
                                ui.label(msg);
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can = !dlg.path.trim().is_empty() && !dlg.running;
                        let label = if dlg.running {
                            "backing up…".to_string()
                        } else {
                            "Backup".to_string()
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
                            backup_clicked = true;
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
            self.backup_dlg = None;
        } else if backup_clicked {
            self.run_backup(conn_id, db);
        }
    }

    fn run_backup(&mut self, conn_id: &str, db: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(dlg) = self.backup_dlg.as_mut() else {
            return;
        };
        let opts = mff_core::backup::BackupOptions {
            include_create: dlg.include_create,
            include_data: dlg.include_data,
            include_views: dlg.include_views,
            batch_size: dlg.batch_size,
        };
        let path = dlg.path.trim().to_string();
        dlg.message = Some("backing up…".into());
        dlg.running = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = "backing up…".into();
        std::thread::spawn(move || {
            let result = mff_core::backup::backup_database(&session, &db, &opts, &|_, _| {})
                .and_then(|text| std::fs::write(&path, text).map_err(|e| {
                    mff_core::MffError::Other(format!("write failed: {e}"))
                }))
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::BackupDone {
                conn_id: id,
                path,
                result,
            });
        });
    }

    pub fn show_restore_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let (databases, current_db) = self
            .sessions
            .get(conn_id)
            .map(|s| (s.databases.clone(), s.current_db.clone()))
            .unwrap_or_default();

        let mut restore_clicked = false;
        let mut close_clicked = false;
        {
            let Some(dlg) = self.restore_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("Restore database")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::Grid::new("restore_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Script file");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut dlg.path)
                                        .desired_width(250.0)
                                        .hint_text("backup.sql"),
                                );
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter("SQL", &["sql"])
                                        .pick_file()
                                    {
                                        dlg.path = p.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Target");
                            ui.checkbox(&mut dlg.create_new, "create new database");
                            ui.end_row();

                            if dlg.create_new {
                                ui.label("New database");
                                ui.add(
                                    egui::TextEdit::singleline(&mut dlg.new_db)
                                        .desired_width(200.0),
                                );
                                ui.end_row();
                            } else {
                                ui.label("Database");
                                egui::ComboBox::from_id_salt("restore_db")
                                    .selected_text(if dlg.target_db.is_empty() {
                                        current_db.as_str()
                                    } else {
                                        dlg.target_db.as_str()
                                    })
                                    .show_ui(ui, |ui| {
                                        for d in &databases {
                                            ui.selectable_value(&mut dlg.target_db, d.clone(), d);
                                        }
                                    });
                                ui.end_row();
                            }
                        });

                    ui.colored_label(
                        egui::Color32::from_rgb(230, 190, 120),
                        "⚠ every statement in the file runs against the target database",
                    );

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(44.0)
                        .show(ui, |ui| {
                            if let Some(msg) = &dlg.message {
                                ui.label(msg);
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let target = if dlg.create_new {
                            dlg.new_db.trim().to_string()
                        } else if dlg.target_db.is_empty() {
                            current_db.clone()
                        } else {
                            dlg.target_db.clone()
                        };
                        let can = !dlg.path.trim().is_empty()
                            && !dlg.running
                            && !target.is_empty();
                        let label = if dlg.running {
                            "restoring…".to_string()
                        } else {
                            "Restore".to_string()
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
                            restore_clicked = true;
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
            self.restore_dlg = None;
        } else if restore_clicked {
            self.run_restore(conn_id);
        }
    }

    fn run_restore(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(dlg) = self.restore_dlg.as_mut() else {
            return;
        };
        let (target, create_new) = if dlg.create_new {
            (dlg.new_db.trim().to_string(), true)
        } else if dlg.target_db.is_empty() {
            (state.current_db.clone(), false)
        } else {
            (dlg.target_db.clone(), false)
        };
        let path = dlg.path.trim().to_string();
        dlg.message = Some("restoring…".into());
        dlg.running = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = "restoring…".into();
        std::thread::spawn(move || {
            let result = (|| -> std::result::Result<usize, String> {
                // Create the target when requested, then run everything in
                // its context (the session keeps USE across statements).
                if create_new {
                    session.run_statement(&format!(
                        "CREATE DATABASE IF NOT EXISTS {}",
                        mff_core::sqlgen::quote_ident(&target)
                    ))
                    .map_err(|e| e.to_string())?;
                }
                session.use_database(&target).map_err(|e| e.to_string())?;
                mff_core::import::import_sql(&session, std::path::Path::new(&path), false)
                    .map_err(|e| e.to_string())
            })();
            let _ = tx.send(crate::jobs::UiMsg::RestoreDone {
                conn_id: id,
                target,
                result,
            });
        });
    }
}
