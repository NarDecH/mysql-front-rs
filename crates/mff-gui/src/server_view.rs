//! "Server" tab: the process list (with kill) and user account management.

use egui::Context;
use mff_core::session::Grid;

use crate::app::App;
use crate::data_view::cell_text;

impl App {
    pub fn show_server_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let (has_processes, has_users, error) = {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            (
                state.server.processes.is_some(),
                state.server.users.is_some(),
                state.server.error.clone(),
            )
        };

        ui.horizontal(|ui| {
            if ui.button("⟳ Refresh").clicked() {
                self.refresh_server_tab(conn_id);
            }
            if let Some(err) = &error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
            }
        });
        ui.separator();

        // ── Processes ──────────────────────────────────────────────────
        let mut kill_requests: Vec<u64> = Vec::new();
        let mut drop_requests: Vec<(String, String)> = Vec::new();
        ui.heading("Processes");
        ui.weak("SHOW FULL PROCESSLIST — kill a connection with ⋯");
        {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            if state.server.processes_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.small("loading…");
                });
            } else if let Some(grid) = &state.server.processes {
                render_process_list(ui, grid, &mut kill_requests);
            } else {
                ui.weak("(press Refresh to load)");
            }
        }
        ui.add_space(10.0);

        // ── Users ──────────────────────────────────────────────────────
        ui.heading("Users");
        ui.weak("accounts from mysql.user (needs privileges)");
        ui.horizontal(|ui| {
            if ui.button("＋ New user…").clicked() {
                if let Some(state) = self.sessions.get_mut(conn_id) {
                    state.server.new_user = Some(crate::session_view::NewUserDialog::default());
                }
            }
            if !has_users && !has_processes {
                ui.weak("(press Refresh to load)");
            }
        });
        {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            if state.server.users_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.small("loading…");
                });
            } else if let Some(grid) = &state.server.users {
                render_user_list(ui, grid, &mut drop_requests);
            }
        }

        // Route render events into the confirmation states.
        for id in kill_requests {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.server.confirm_kill = Some(id);
            }
        }
        for pair in drop_requests {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.server.confirm_drop_user = Some(pair);
            }
        }

        // Kill confirmation.
        let confirm_kill = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.server.confirm_kill);
        if let Some(id) = confirm_kill {
            let mut open = true;
            let mut confirmed = false;
            egui::Window::new("Kill connection")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Kill connection #{id}?"));
                    ui.weak("Any running statements are aborted.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("Kill").color(egui::Color32::LIGHT_RED))
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            if let Some(state) = self.sessions.get_mut(conn_id) {
                                state.server.confirm_kill = None;
                            }
                        }
                    });
                });
            if !open {
                if let Some(state) = self.sessions.get_mut(conn_id) {
                    state.server.confirm_kill = None;
                }
            }
            if confirmed {
                self.kill_process(conn_id, id);
            }
        }

        // Drop-user confirmation.
        let confirm_user = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.server.confirm_drop_user.clone());
        if let Some((user, host)) = confirm_user {
            let mut open = true;
            let mut confirmed = false;
            egui::Window::new("Drop user")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Drop user '{user}'@'{host}'?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("Drop").color(egui::Color32::LIGHT_RED))
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            if let Some(state) = self.sessions.get_mut(conn_id) {
                                state.server.confirm_drop_user = None;
                            }
                        }
                    });
                });
            if !open {
                if let Some(state) = self.sessions.get_mut(conn_id) {
                    state.server.confirm_drop_user = None;
                }
            }
            if confirmed {
                self.run_user_sql(conn_id, format!("DROP USER {}", quote_user(&user, &host)), format!("dropped user '{user}'@'{host}'"));
            }
        }

        // New-user dialog.
        if self
            .sessions
            .get(conn_id)
            .map(|s| s.server.new_user.is_some())
            .unwrap_or(false)
        {
            self.show_new_user_dialog(ctx, conn_id);
        }
    }

    /// Load both lists of the server tab (background jobs).
    pub fn refresh_server_tab(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        self.load_process_list(conn_id, session.clone());
        self.load_user_list(conn_id, session);
    }

    pub fn load_process_list(&mut self, conn_id: &str, session: mff_core::Session) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        state.server.processes_loading = true;
        state.server.error = None;
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session.query("SHOW FULL PROCESSLIST").map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::ProcessListLoaded {
                conn_id: id,
                result,
            });
        });
    }

    pub fn load_user_list(&mut self, conn_id: &str, session: mff_core::Session) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        state.server.users_loading = true;
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session
                .query("SELECT User, Host, IF(authentication_string='' OR authentication_string IS NULL, 'no password', 'has password') AS Password FROM mysql.user ORDER BY User, Host")
                .map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::UserListLoaded {
                conn_id: id,
                result,
            });
        });
    }

    fn kill_process(&mut self, conn_id: &str, id: u64) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        state.server.confirm_kill = None;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let cid = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session
                .run_statement(&format!("KILL {id}"))
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::Killed {
                conn_id: cid,
                id,
                result,
            });
        });
    }

    /// Run one CREATE USER / DROP USER statement (background job).
    fn run_user_sql(&mut self, conn_id: &str, sql: String, ok_msg: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session
                .run_statement(&sql)
                .map(|_| ok_msg)
                .map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::UserDone { conn_id: id, result });
        });
    }

    fn show_new_user_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let mut create_clicked = false;
        let mut cancel_clicked = false;
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let Some(dlg) = state.server.new_user.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("New user")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("new_user_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("User");
                            ui.add(
                                egui::TextEdit::singleline(&mut dlg.user).desired_width(200.0),
                            );
                            ui.end_row();
                            ui.label("Host")
                                .on_hover_text("e.g. % for any host, localhost for local only");
                            ui.add(
                                egui::TextEdit::singleline(&mut dlg.host).desired_width(200.0),
                            );
                            ui.end_row();
                            ui.label("Password");
                            ui.add(
                                egui::TextEdit::singleline(&mut dlg.password)
                                    .password(true)
                                    .desired_width(200.0),
                            );
                            ui.end_row();
                        });
                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !dlg.running && !dlg.user.trim().is_empty(),
                                egui::Button::new(
                                    egui::RichText::new("Create user")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(crate::theme::accent(ui.ctx()))
                                .min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            create_clicked = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_clicked = true;
                        }
                    });
                });
            if !open {
                cancel_clicked = true;
            }
        }
        if cancel_clicked {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.server.new_user = None;
            }
            return;
        }
        if create_clicked {
            let (user, host, password) = {
                let Some(state) = self.sessions.get(conn_id) else {
                    return;
                };
                let Some(d) = state.server.new_user.as_ref() else {
                    return;
                };
                (
                    d.user.trim().to_string(),
                    d.host.trim().to_string(),
                    d.password.clone(),
                )
            };
            let sql = if password.is_empty() {
                format!("CREATE USER {} ", quote_user(&user, &host))
            } else {
                format!(
                    "CREATE USER {} IDENTIFIED BY {}",
                    quote_user(&user, &host),
                    mff_core::sqlgen::quote_string(&password)
                )
            };
            self.run_user_sql(
                conn_id,
                sql,
                format!("created user '{user}'@'{host}'"),
            );
            // Close the dialog immediately; errors are reported via status.
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.server.new_user = None;
            }
        }
    }
}

fn quote_user(user: &str, host: &str) -> String {
    format!(
        "{}@{}",
        mff_core::sqlgen::quote_string(user),
        mff_core::sqlgen::quote_string(host)
    )
}

/// The process list table with a per-row kill menu.
fn render_process_list(ui: &mut egui::Ui, grid: &Grid, kill_requests: &mut Vec<u64>) {
    use egui_extras::{Column, TableBuilder};

    let ncols = grid.columns.len();
    // Id, User, Host, db, Command, Time, State, Info are the interesting ones.
    let widths = [70.0_f32, 90.0, 140.0, 90.0, 80.0, 70.0, 120.0];
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0)
        .max_scroll_height(220.0);
    for w in widths.iter().take(ncols) {
        table = table.columns(Column::initial(*w).resizable(true).clip(true), 1);
    }
    if ncols > widths.len() {
        table = table.columns(Column::initial(240.0).resizable(true).clip(true), ncols - widths.len());
    }
    table
        .header(22.0, |mut header| {
            for col in &grid.columns {
                header.col(|ui| {
                    ui.strong(&col.name);
                });
            }
            header.col(|ui| {
                ui.strong("");
            });
        })
        .body(|body| {
            body.rows(20.0, grid.rows.len(), |mut row| {
                let r = row.index();
                for (c, col) in grid.columns.iter().enumerate() {
                    row.col(|ui| {
                        let cell = &grid.rows[r][c];
                        let text = cell_text(cell);
                        let mut rich = egui::RichText::new(text);
                        if col.name == "Id" {
                            rich = rich.strong();
                        }
                        ui.add(egui::Label::new(rich));
                    });
                }
                row.col(|ui| {
                    let id: u64 = cell_text(&grid.rows[r][0]).parse().unwrap_or(0);
                    if id > 0
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Kill").color(egui::Color32::LIGHT_RED),
                                )
                                .small(),
                            )
                            .clicked()
                        {
                            kill_requests.push(id);
                        }
                });
            });
        });
}

/// The user list with a per-row drop menu.
fn render_user_list(ui: &mut egui::Ui, grid: &Grid, drop_requests: &mut Vec<(String, String)>) {
    use egui_extras::{Column, TableBuilder};

    let ncols = grid.columns.len().min(3);
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0)
        .max_scroll_height(220.0);
    table = table.columns(Column::initial(140.0).resizable(true), 1);
    table = table.columns(Column::initial(140.0).resizable(true), 1);
    table = table.columns(Column::initial(120.0).resizable(true), 1);
    table
        .header(22.0, |mut header| {
            for c in 0..ncols {
                header.col(|ui| {
                    ui.strong(
                        &grid
                            .columns
                            .get(c)
                            .map(|m| m.name.clone())
                            .unwrap_or_default(),
                    );
                });
            }
            header.col(|ui| {
                ui.strong("");
            });
        })
        .body(|body| {
            body.rows(20.0, grid.rows.len(), |mut row| {
                let r = row.index();
                for c in 0..ncols {
                    row.col(|ui| {
                        let text = grid
                            .rows
                            .get(r)
                            .and_then(|row| row.get(c))
                            .map(cell_text)
                            .unwrap_or_default();
                        ui.label(text);
                    });
                }
                row.col(|ui| {
                    let user = grid
                        .rows
                        .get(r)
                        .and_then(|row| row.get(0))
                        .map(cell_text)
                        .unwrap_or_default();
                    let host = grid
                        .rows
                        .get(r)
                        .and_then(|row| row.get(1))
                        .map(cell_text)
                        .unwrap_or_default();
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Drop").color(egui::Color32::LIGHT_RED),
                            )
                            .small(),
                        )
                        .clicked()
                    {
                        drop_requests.push((user, host));
                    }
                });
            });
        });
}

