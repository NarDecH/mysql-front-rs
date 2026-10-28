//! Application shell: state, the connections screen, the top bar, job
//! polling and modal routing.

use std::collections::HashMap;

use egui::Context;
use mff_core::Registry;

use crate::dialogs::{ConnEditor, ExportDlg};
use crate::jobs::{Jobs, UiMsg};
use crate::session_view::SessionState;

pub struct App {
    pub registry: Registry,
    pub sessions: HashMap<String, SessionState>,
    pub active_conn: Option<String>,
    pub jobs: Jobs,
    pub status: String,
    pub conn_edit: Option<ConnEditor>,
    pub export_dlg: Option<ExportDlg>,
    pub confirm_delete: Option<String>,
    pub connect_error: Option<String>,
    pub font_note: Option<String>,
    pub theme: egui::Theme,
    pub new_dlg: Option<crate::new_dialog::NewDialog>,
    pub db_rename_dlg: Option<crate::db_rename::DbRenameDialog>,
    pub transfer_dlg: Option<crate::transfer::TransferDialog>,
    /// Persisted UI state (theme, last connection, query history, …).
    pub app_state: mff_core::AppState,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let font_note = crate::fonts::install_fonts(&cc.egui_ctx)
            .map(|n| format!("Thai font loaded: {n}"));
        let app_state = mff_core::AppState::load_default();
        let theme = match app_state.theme.as_deref() {
            Some("dark") => egui::Theme::Dark,
            _ => egui::Theme::Light,
        };
        cc.egui_ctx.set_theme(theme);
        Self {
            registry: Registry::load_default(),
            sessions: HashMap::new(),
            active_conn: None,
            jobs: Jobs::new(),
            status: if font_note.is_some() {
                "ready".into()
            } else {
                "ready (no Thai-capable system font found — Thai text may show boxes)".into()
            },
            conn_edit: None,
            export_dlg: None,
            confirm_delete: None,
            connect_error: None,
            font_note,
            theme,
            new_dlg: None,
            db_rename_dlg: None,
            transfer_dlg: None,
            app_state,
        }
    }

    /// Persist the current UI state (theme, per-connection database and
    /// query history). Cheap JSON write; called on the important changes
    /// and on app close.
    pub fn persist_state(&mut self) {
        self.app_state.theme = Some(
            match self.theme {
                egui::Theme::Dark => "dark",
                egui::Theme::Light => "light",
            }
            .to_string(),
        );
        self.app_state.last_conn = self.active_conn.clone();
        for (id, state) in &self.sessions {
            let mut st = self.app_state.conn(id);
            st.db = Some(state.current_db.clone()).filter(|d| !d.is_empty());
            st.history = state.history.clone();
            self.app_state.set_conn(id, st);
        }
        self.app_state.save_default();
    }

    /// Switch between the light and dark theme.
    pub fn toggle_theme(&mut self, ctx: &Context) {
        self.theme = match self.theme {
            egui::Theme::Dark => egui::Theme::Light,
            egui::Theme::Light => egui::Theme::Dark,
        };
        ctx.set_theme(self.theme);
        self.persist_state();
    }

    /// The theme toggle button (shows the theme you would switch to).
    pub fn theme_button(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let label = match self.theme {
            egui::Theme::Dark => "☀",
            egui::Theme::Light => "🌙",
        };
        if ui
            .button(label)
            .on_hover_text("Toggle light/dark theme")
            .clicked()
        {
            self.toggle_theme(ctx);
        }
    }

    /// Drain worker messages and route them into the session states.
    fn poll_jobs(&mut self) {
        for msg in self.jobs.drain() {
            if let UiMsg::Status(s) = &msg {
                if s.starts_with('✓') || s.starts_with('✗') {
                    if let Some(editor) = self.conn_edit.as_mut() {
                        if editor.testing {
                            editor.testing = false;
                            if s.starts_with('✗') {
                                editor.error = Some(s.clone());
                            }
                        }
                    }
                }
            }
            self.apply_msg(msg);
        }
    }

    /// The connections home screen.
    fn show_connections_screen(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.heading("mysql-front-rs");
                ui.weak(format!("v{}", mff_core::VERSION));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    self.theme_button(ui, ctx);
                    if ui.button("＋ New connection").clicked() {
                        self.open_conn_editor(None);
                    }
                });
            });
            ui.separator();
            ui.add_space(8.0);

            if let Some(err) = &self.connect_error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                ui.add_space(4.0);
            }

            let conns = self.registry.sorted();
            if conns.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label("no saved connections yet");
                    ui.add_space(8.0);
                    if ui.button("＋ Create your first connection").clicked() {
                        self.open_conn_editor(None);
                    }
                });
            } else {
                use egui_extras::{Column, TableBuilder};
                // Local clones of ids/actions to satisfy the borrow checker
                // inside the table body closure.
                let mut connect_id: Option<String> = None;
                let mut edit_id: Option<String> = None;
                let mut dup_id: Option<String> = None;
                let mut del_id: Option<String> = None;

                let rows: Vec<mff_core::Connection> =
                    conns.into_iter().cloned().collect();
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .columns(Column::initial(220.0).resizable(true), 1)
                    .columns(Column::initial(200.0).resizable(true), 1)
                    .columns(Column::initial(140.0).resizable(true), 1)
                    .columns(Column::initial(140.0).resizable(true), 1)
                    .columns(Column::initial(90.0).resizable(true), 1)
                    .columns(Column::auto(), 1)
                    .header(22.0, |mut h| {
                        for t in ["Name", "Host:port", "User", "Database", "SSL", ""] {
                            h.col(|ui| {
                                ui.strong(t);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(24.0, rows.len(), |mut row| {
                            let c = &rows[row.index()];
                            row.col(|ui| {
                                let is_last = self.app_state.last_conn.as_deref() == Some(&c.id);
                                if is_last {
                                    ui.strong(format!("{}  (last used)", c.name));
                                } else {
                                    ui.strong(&c.name);
                                }
                            });
                                row.col(|ui| {
                                    ui.label(c.host_port());
                                });
                                row.col(|ui| {
                                    ui.label(&c.user);
                                });
                                row.col(|ui| {
                                    ui.label(c.database.as_deref().unwrap_or("—"));
                                });
                                row.col(|ui| {
                                    ui.label(match c.ssl_mode {
                                        mff_core::SslMode::Disabled => "—",
                                        _ => "TLS",
                                    });
                                });
                                row.col(|ui| {
                                    ui.horizontal(|ui| {
                                        if crate::theme::primary_button(ui, "Connect").clicked() {
                                            connect_id = Some(c.id.clone());
                                        }
                                        if ui.small_button("Edit").clicked() {
                                            edit_id = Some(c.id.clone());
                                        }
                                        if ui.small_button("Duplicate").clicked() {
                                            dup_id = Some(c.id.clone());
                                        }
                                        if ui.small_button("Delete").clicked() {
                                            del_id = Some(c.id.clone());
                                        }
                                    });
                                });
                        });
                    });

                if let Some(id) = connect_id {
                    if let Some(c) = self.registry.get(&id).cloned() {
                        self.connect_error = None;
                        self.connect(c);
                    }
                }
                if let Some(id) = edit_id {
                    if let Some(c) = self.registry.get(&id).cloned() {
                        self.open_conn_editor(Some(&c));
                    }
                }
                if let Some(id) = dup_id {
                    if let Some(c) = self.registry.get(&id).cloned() {
                        let mut d = c.clone();
                        d.id = mff_core::Connection::new_id();
                        d.name = format!("{} (copy)", d.name);
                        self.open_conn_editor(Some(&d));
                        if let Some(editor) = self.conn_edit.as_mut() {
                            editor.editing_id = None; // save as a new entry
                        }
                    }
                }
                if let Some(id) = del_id {
                    self.confirm_delete = Some(id);
                }
            }

            // Status bar at the bottom of the panel.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.small(&self.status);
                    if let Some(note) = &self.font_note {
                        ui.small(format!("· {note}"));
                    }
                });
            });
        });

        // Delete confirmation.
        if let Some(id) = self.confirm_delete.clone() {
            let name = self
                .registry
                .get(&id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let mut open = true;
            let mut confirmed = false;
            egui::Window::new("Delete connection")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Delete connection “{name}”?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Delete").color(egui::Color32::LIGHT_RED)).clicked() {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete = None;
                        }
                    });
                });
            if !open {
                self.confirm_delete = None;
            }
            if confirmed {
                self.registry.remove(&id);
                let _ = self.registry.save_default();
                self.confirm_delete = None;
                self.status = format!("connection “{name}” deleted");
            }
        }
    }

    /// Top bar of the session workspace (copy cell + back home).
    fn show_session_top(&mut self, ctx: &Context, _conn_id: &str) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("⌂ Connections").clicked() {
                    self.active_conn = None;
                }
                self.theme_button(ui, ctx);
                ui.separator();
                // Copy button + Ctrl+C inside the data view.
                if ui
                    .small_button("⧉ Copy cell")
                    .on_hover_text("Copy the selected cell (Ctrl+C)")
                    .clicked()
                {
                    self.copy_selected_cell(ctx);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(&self.status);
                });
            });
        });
    }
}

impl eframe::App for App {
    /// Save the UI state when the window is closed.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_state();
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_jobs();

        // Ctrl+C copies the selected data cell.
        if ctx.input(|i| i.key_pressed(egui::Key::C) && i.modifiers.ctrl) {
            if self.active_conn.is_some() {
                self.copy_selected_cell(ctx);
            }
        }

        if self.active_conn.is_none() {
            self.show_connections_screen(ctx);
        } else {
            let conn_id = self.active_conn.clone().unwrap();
            if !self.sessions.contains_key(&conn_id) {
                self.active_conn = None;
                return;
            }
            self.show_session_top(ctx, &conn_id);
            self.show_session(ctx, conn_id);
        }

        // Connection editor modal (works on both screens).
        if self.conn_edit.is_some() {
            self.show_conn_editor(ctx);
        }

        // "New object" modal (needs the session it belongs to).
        if self.new_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_new_dialog(ctx, &id);
                } else {
                    self.new_dlg = None;
                }
            }
        }

        // "Rename database" modal.
        if self.db_rename_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_db_rename_dialog(ctx, &id);
                } else {
                    self.db_rename_dlg = None;
                }
            }
        }

        // "Data transfer" modal.
        if self.transfer_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_transfer_dialog(ctx, &id);
                } else {
                    self.transfer_dlg = None;
                }
            }
        }

        // Keep repainting while jobs are in flight so spinners move and
        // results appear promptly.
        let busy = self.sessions.values().map(|s| s.busy).sum::<usize>()
            + usize::from(self.conn_edit.as_ref().map(|e| e.testing).unwrap_or(false));
        if busy > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
