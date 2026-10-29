//! Global object search (Ctrl+F / 🔍 in the tree): find tables, views and
//! columns by name across ALL databases of the connected server.

use egui::Context;

use crate::app::App;

pub struct SearchDlg {
    pub query: String,
    pub running: bool,
    /// (database, table, kind, column) — column empty for tables/views.
    pub results: Vec<(String, String, String, String)>,
    pub error: Option<String>,
    pub done: bool,
}

impl App {
    pub fn open_search(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get(conn_id) else {
            return;
        };
        let q = state.tree_filter.clone();
        self.search_dlg = Some(crate::search_dlg::SearchDlg {
            query: q,
            running: false,
            results: Vec::new(),
            error: None,
            done: false,
        });
    }

    pub fn show_search_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let mut go_clicked = false;
        let mut close_clicked = false;
        let mut picked: Option<(String, String)> = None;
        {
            let Some(dlg) = self.search_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("Search objects")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("name contains:");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut dlg.query)
                                .desired_width(240.0)
                                .hint_text("e.g. user"),
                        );
                        go_clicked |= resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui
                            .add_enabled(
                                !dlg.running && !dlg.query.trim().is_empty(),
                                egui::Button::new("Search"),
                            )
                            .clicked()
                        {
                            go_clicked = true;
                        }
                    });
                    ui.weak("searches tables, views and columns in every database");

                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    }
                    if dlg.running {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.small("searching…");
                        });
                    }

                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut results = Vec::new();
                            results.extend(dlg.results.iter().cloned());
                            for (db, table, kind, column) in &results {
                                let label = if column.is_empty() {
                                    format!("{db} › {table}  ({kind})")
                                } else {
                                    format!("{db} › {table}.{column}  ({kind})")
                                };
                                if ui.selectable_label(false, label).clicked() {
                                    picked = Some((db.clone(), table.clone()));
                                }
                            }
                        });
                    if dlg.done && dlg.results.is_empty() && dlg.error.is_none() {
                        ui.weak("no matches");
                    }

                    ui.add_space(6.0);
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            if !open {
                close_clicked = true;
            }
        }

        if let Some((db, table)) = picked {
            // Jump: switch database and open the table's data view.
            let needs_switch = {
                let session = self.sessions.get(conn_id).map(|s| s.session.clone());
                let mut switch = false;
                if let Some(state) = self.sessions.get_mut(conn_id) {
                    if state.current_db != db {
                        if let Some(session) = &session {
                            let _ = session.use_database(&db);
                        }
                        state.current_db = db.clone();
                        state.objects = None;
                        switch = true;
                    }
                    state.view = crate::session_view::ActiveView::Data;
                }
                switch
            };
            if needs_switch {
                self.load_objects(conn_id, db.clone());
            }
            crate::session_view::open_data(self, conn_id, table);
            self.search_dlg = None;
            return;
        }
        if close_clicked {
            self.search_dlg = None;
        } else if go_clicked {
            self.run_search(conn_id);
        }
    }

    fn run_search(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(dlg) = self.search_dlg.as_mut() else {
            return;
        };
        let q = dlg.query.trim().to_string();
        if q.is_empty() {
            return;
        }
        dlg.running = true;
        dlg.error = None;
        dlg.done = false;
        dlg.results.clear();
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let pattern = format!("%{}%", q.replace('%', ""));
            let result: std::result::Result<
                Vec<(String, String, String, String)>,
                String,
            > = (|| {
                let mut out = Vec::new();
                let tables = session.query(&format!(
                    "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
                     WHERE TABLE_NAME LIKE '{}' AND TABLE_SCHEMA NOT IN \
                     ('information_schema','performance_schema','sys','mysql') \
                     ORDER BY TABLE_SCHEMA, TABLE_NAME LIMIT 200",
                    pattern.replace('\'', "''")
                ))
                .map_err(|e| e.to_string())?;
                for row in &tables.rows {
                    let get = |i: usize| {
                        row.get(i)
                            .map(|c| c.display("NULL"))
                            .unwrap_or_default()
                    };
                    out.push((get(0), get(1), get(2), String::new()));
                }
                let columns = session.query(&format!(
                    "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
                     WHERE COLUMN_NAME LIKE '{}' AND TABLE_SCHEMA NOT IN \
                     ('information_schema','performance_schema','sys','mysql') \
                     ORDER BY TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME LIMIT 200",
                    pattern.replace('\'', "''")
                ))
                .map_err(|e| e.to_string())?;
                for row in &columns.rows {
                    let get = |i: usize| {
                        row.get(i)
                            .map(|c| c.display("NULL"))
                            .unwrap_or_default()
                    };
                    out.push((get(0), get(1), "column".into(), get(2)));
                }
                Ok(out)
            })();
            let _ = tx.send(crate::jobs::UiMsg::SearchDone {
                conn_id: id,
                result,
            });
        });
    }
}
