//! "Insert row" dialog: one input per column, built from the table
//! schema. Empty fields use the column default, the word `NULL` inserts
//! SQL NULL.

use egui::Context;
use mff_core::model::Table;

use crate::app::App;

/// One column of the dialog.
#[derive(Clone)]
pub struct InsCol {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub auto_inc: bool,
    pub has_default: bool,
}

/// The insert-row dialog state (lives in `DataState`).
pub struct InsertRowDialog {
    pub table: String,
    pub cols: Vec<InsCol>,
    pub values: Vec<String>,
    pub error: Option<String>,
    pub running: bool,
}

impl InsertRowDialog {
    /// Build the dialog from a loaded table schema.
    pub fn from_schema(table: &str, schema: &Table) -> Self {
        let cols: Vec<InsCol> = schema
            .columns
            .iter()
            .map(|c| InsCol {
                name: c.name.clone(),
                col_type: c.column_type.clone(),
                nullable: c.nullable,
                auto_inc: c.is_auto_increment(),
                has_default: c.default.is_some(),
            })
            .collect();
        let values = vec![String::new(); cols.len()];
        Self {
            table: table.to_string(),
            cols,
            values,
            error: None,
            running: false,
        }
    }
}

impl App {
    pub fn show_insert_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let mut insert_clicked = false;
        let mut cancel_clicked = false;
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let Some(dlg) = state.data.insert_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new(format!("Insert row — {}", dlg.table))
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.weak("empty = column default · type NULL for SQL NULL");
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            egui::Grid::new("insert_form")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    for (i, col) in dlg.cols.iter().enumerate() {
                                        let mut hint = col.col_type.clone();
                                        if col.auto_inc {
                                            hint.push_str(" · auto increment");
                                        } else if col.has_default {
                                            hint.push_str(" · has default");
                                        } else if col.nullable {
                                            hint.push_str(" · nullable");
                                        }
                                        ui.label(format!("{}:", col.name))
                                            .on_hover_text(&hint);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut dlg.values[i])
                                                .hint_text(if col.auto_inc {
                                                    "(auto)"
                                                } else {
                                                    ""
                                                })
                                                .desired_width(240.0),
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !dlg.running,
                                egui::Button::new(
                                    egui::RichText::new("Insert")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(crate::theme::accent(ui.ctx()))
                                .min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            insert_clicked = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_clicked = true;
                        }
                        if dlg.running {
                            ui.spinner();
                            ui.small("inserting…");
                        }
                    });
                });
            if !open {
                cancel_clicked = true;
            }
        }

        if cancel_clicked {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.data.insert_dlg = None;
            }
            return;
        }
        if insert_clicked {
            self.run_insert(conn_id);
        }
    }

    /// Build and run the INSERT statement (background job).
    fn run_insert(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let db = state.current_db.clone();
        let Some(dlg) = state.data.insert_dlg.as_mut() else {
            return;
        };
        dlg.error = None;
        let cols: Vec<mff_core::edit::InsertCol> = dlg
            .cols
            .iter()
            .map(|c| mff_core::edit::InsertCol {
                name: c.name.clone(),
                col_type: c.col_type.clone(),
            })
            .collect();
        let sql = match mff_core::edit::insert_sql(&db, &dlg.table, &cols, &dlg.values) {
            Ok(sql) => sql,
            Err(e) => {
                dlg.error = Some(e.to_string());
                return;
            }
        };
        dlg.running = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session
                .run_statement(&sql)
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::RowInserted { conn_id: id, result });
        });
    }
}
