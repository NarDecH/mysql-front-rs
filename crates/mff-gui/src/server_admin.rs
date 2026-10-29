//! Server overview stats + GRANT editor + edit-definition loading,
//! attached to the Server tab and object browser.

use egui::Context;

use crate::app::App;
use crate::session_view::ServerStats;

impl App {
    /// Load the server overview (background job).
    pub fn load_server_stats(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        if state.server.stats.loading {
            return;
        }
        state.server.stats.loading = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result: std::result::Result<Vec<(String, String)>, String> = (|| {
                let mut out: Vec<(String, String)> = Vec::new();
                // SHOW VARIABLES/STATUS work on every server (and when
                // performance_schema is disabled, e.g. XAMPP MariaDB).
                let vars = session
                    .query(
                        "SHOW VARIABLES WHERE Variable_name IN \
                         ('version','version_comment','max_connections')",
                    )
                    .map_err(|e| e.to_string())?;
                let get = |g: &mff_core::session::Grid, name: &str| -> String {
                    g.rows
                        .iter()
                        .find(|r| {
                            r.first()
                                .map(|c| c.display("").eq_ignore_ascii_case(name))
                                .unwrap_or(false)
                        })
                        .and_then(|r| r.get(1))
                        .map(|c| c.display("NULL"))
                        .unwrap_or_else(|| "—".into())
                };
                out.push(("version".into(), format!("{} {}", get(&vars, "version"), get(&vars, "version_comment"))));
                out.push(("max_connections".into(), get(&vars, "max_connections")));
                let status = session
                    .query(
                        "SHOW GLOBAL STATUS WHERE Variable_name IN \
                         ('Threads_connected','Threads_running','Uptime','Queries')",
                    )
                    .map_err(|e| e.to_string())?;
                let sget = |name: &str| -> String {
                    status
                        .rows
                        .iter()
                        .find(|r| {
                            r.first()
                                .map(|c| c.display("").eq_ignore_ascii_case(name))
                                .unwrap_or(false)
                        })
                        .and_then(|r| r.get(1))
                        .map(|c| c.display("NULL"))
                        .unwrap_or_else(|| "—".into())
                };
                for name in ["Threads_connected", "Threads_running", "Queries"] {
                    out.push((name.into(), sget(name)));
                }
                let uptime: u64 = sget("Uptime").parse().unwrap_or(0);
                let days = uptime / 86400;
                let hours = (uptime % 86400) / 3600;
                let mins = (uptime % 3600) / 60;
                out.push(("uptime".into(), format!("{days}d {hours}h {mins}m")));
                Ok(out)
            })();
            let _ = tx.send(crate::jobs::UiMsg::ServerStatsLoaded {
                conn_id: id,
                result,
            });
        });
    }

    /// Load SHOW GRANTS for one account (background job).
    pub fn load_grants(&mut self, conn_id: &str, user: String, host: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        let quoted = format!(
            "{}@{}",
            mff_core::sqlgen::quote_string(&user),
            mff_core::sqlgen::quote_string(&host)
        );
        std::thread::spawn(move || {
            let result: std::result::Result<Vec<String>, String> = (|| {
                let g = session
                    .query(&format!("SHOW GRANTS FOR {quoted}"))
                    .map_err(|e| e.to_string())?;
                Ok(g.rows
                    .iter()
                    .map(|r| {
                        r.get(0)
                            .map(|c| c.display("NULL"))
                            .unwrap_or_default()
                    })
                    .collect())
            })();
            let _ = tx.send(crate::jobs::UiMsg::GrantsLoaded {
                conn_id: id,
                user,
                host,
                result,
            });
        });
    }

    /// Apply GRANT/REVOKE statements for one account (background job).
    pub fn apply_grants(
        &mut self,
        conn_id: &str,
        user: String,
        host: String,
        statements: Vec<String>,
    ) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let mut result: std::result::Result<String, String> =
                Ok(format!("{} statement(s) applied", statements.len()));
            for stmt in &statements {
                if let Err(e) = session.run_statement(stmt) {
                    result = Err(format!("{} failed: {e}", stmt.split(' ').take(3).collect::<Vec<_>>().join(" ")));
                    break;
                }
            }
            let _ = tx.send(crate::jobs::UiMsg::GrantsApplied {
                conn_id: id,
                result,
            });
        });
    }

    /// Load an object definition for editing (background job).
    pub fn load_definition(&mut self, conn_id: &str, kind: String, name: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let db = state.current_db.clone();
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let sql = match kind.as_str() {
                "VIEW" => format!("SHOW CREATE VIEW {}", mff_core::sqlgen::quote_qualified(&[&db, &name])),
                "PROCEDURE" => format!("SHOW CREATE PROCEDURE {}", mff_core::sqlgen::quote_qualified(&[&db, &name])),
                "FUNCTION" => format!("SHOW CREATE FUNCTION {}", mff_core::sqlgen::quote_qualified(&[&db, &name])),
                "TRIGGER" => format!("SHOW CREATE TRIGGER {}", mff_core::sqlgen::quote_qualified(&[&db, &name])),
                "EVENT" => format!("SHOW CREATE EVENT {}", mff_core::sqlgen::quote_qualified(&[&db, &name])),
                other => {
                    let _ = tx.send(crate::jobs::UiMsg::DefinitionLoaded {
                        conn_id: id,
                        kind: other.to_string(),
                        name,
                        result: Err(format!("unknown kind {other}")),
                    });
                    return;
                }
            };
            let result: std::result::Result<String, String> = (|| {
                let g = session.query(&sql).map_err(|e| e.to_string())?;
                if g.row_count() >= 1 && g.columns.len() >= 2 {
                    match &g.rows[0][1] {
                        mff_core::session::Cell::Text(s) => {
                            return Ok(mff_core::sqlgen::make_replaceable(&kind, s));
                        }
                        _ => {}
                    }
                }
                Err("no definition returned".into())
            })();
            let _ = tx.send(crate::jobs::UiMsg::DefinitionLoaded {
                conn_id: id,
                kind,
                name,
                result,
            });
        });
    }
}

/// GRANT editor dialog state lives on App; render helper here.
impl App {
    pub fn show_grants_editor(&mut self, ctx: &Context, conn_id: &str) {
        use crate::session_view::GrantsEditor;
        let mut apply_clicked = false;
        let mut close_clicked = false;
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let Some(ed) = state.server.grants_editor.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new(format!("Privileges — '{}@{}'", ed.user, ed.host))
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.strong("Global privileges (ON *.*):");
                    egui::Grid::new("grant_grid")
                        .num_columns(4)
                        .spacing([6.0, 4.0])
                        .show(ui, |ui| {
                            for priv_name in GrantsEditor::PRIVILEGES.chunks(4) {
                                for p in priv_name {
                                    let mut checked = ed.selected.contains(*p);
                                    if ui.checkbox(&mut checked, *p).changed() {
                                        if checked {
                                            ed.selected.insert((*p).to_string());
                                        } else {
                                            ed.selected.remove(*p);
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    if !ed.current_grants.is_empty() {
                        ui.strong("Current grants:");
                        egui::ScrollArea::vertical()
                            .max_height(110.0)
                            .show(ui, |ui| {
                                for g in &ed.current_grants {
                                    ui.monospace(g);
                                }
                            });
                    }
                    if let Some(err) = &ed.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !ed.running,
                                egui::Button::new(
                                    egui::RichText::new("Apply")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(crate::theme::accent(ui.ctx()))
                                .min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            apply_clicked = true;
                        }
                        if ed.running {
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
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.server.grants_editor = None;
            }
            return;
        }
        if apply_clicked {
            let (user, host, statements) = {
                let Some(state) = self.sessions.get(conn_id) else {
                    return;
                };
                let Some(ed) = state.server.grants_editor.as_ref() else {
                    return;
                };
                let quoted = format!(
                    "{}@{}",
                    mff_core::sqlgen::quote_string(&ed.user),
                    mff_core::sqlgen::quote_string(&ed.host)
                );
                let all: Vec<&str> = GrantsEditor::PRIVILEGES.to_vec();
                let mut stmts = Vec::new();
                let grant_list: Vec<&str> = all
                    .iter()
                    .filter(|p| ed.selected.contains(**p))
                    .copied()
                    .collect();
                let revoke_list: Vec<&str> = all
                    .iter()
                    .filter(|p| !ed.selected.contains(**p))
                    .copied()
                    .collect();
                if !grant_list.is_empty() {
                    stmts.push(format!(
                        "GRANT {} ON *.* TO {quoted}",
                        grant_list.join(", ")
                    ));
                }
                if !revoke_list.is_empty() {
                    stmts.push(format!(
                        "REVOKE {} ON *.* FROM {quoted}",
                        revoke_list.join(", ")
                    ));
                }
                (ed.user.clone(), ed.host.clone(), stmts)
            };
            if statements.is_empty() {
                self.status = "nothing to apply".into();
                return;
            }
            if let Some(state) = self.sessions.get_mut(conn_id) {
                if let Some(ed) = state.server.grants_editor.as_mut() {
                    ed.running = true;
                }
            }
            self.apply_grants(conn_id, user, host, statements);
        }
    }
}
