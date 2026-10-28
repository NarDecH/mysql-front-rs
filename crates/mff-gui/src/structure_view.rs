//! Structure view: columns, indexes, foreign keys and the CREATE statement
//! of a table (a read-only version of the original table designer).

use egui::Context;

use crate::app::App;
use crate::data_view::render_grid;
use crate::session_view::{open_data, ActiveView};
use mff_core::model::KeyKind;
use mff_core::session::{Cell, ColMeta, Grid};

impl App {
    pub fn show_structure_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let mut refresh_clicked = false;
        let mut browse_clicked = false;
        let mut edit_clicked = false;
        let mut copy_clicked = false;
        let mut open_query_clicked = false;
        let create_stmt;

        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let s = &mut state.structure;

            if s.table.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.label("select a table in the object browser")
                });
                return;
            }

            ui.horizontal(|ui| {
                ui.strong(format!("⚒ {}.{}", state.current_db, s.table));
                if s.loading {
                    ui.spinner();
                    ui.small("loading…");
                } else if ui.button("⟳ Refresh").clicked() {
                    refresh_clicked = true;
                }
                if ui.button("▤ Browse data").clicked() {
                    browse_clicked = true;
                }
                if ui
                    .button("✎ Edit structure")
                    .on_hover_text("Open the table designer (ALTER)")
                    .clicked()
                {
                    edit_clicked = true;
                }
            });
            ui.add_space(6.0);

            if let Some(err) = &s.error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                return;
            }
            let Some(schema) = s.schema.clone() else {
                if !s.loading {
                    ui.label("(not loaded)");
                }
                return;
            };

            // ── Columns ────────────────────────────────────────────────
            ui.heading("Columns");
            let g = Grid {
                columns: vec![
                    ColMeta::new("Name", "VARCHAR"),
                    ColMeta::new("Type", "VARCHAR"),
                    ColMeta::new("Null", "VARCHAR"),
                    ColMeta::new("Key", "VARCHAR"),
                    ColMeta::new("Default", "VARCHAR"),
                    ColMeta::new("Extra", "VARCHAR"),
                    ColMeta::new("Comment", "VARCHAR"),
                ],
                rows: schema
                    .columns
                    .iter()
                    .map(|c| {
                        vec![
                            Cell::Text(c.name.clone()),
                            Cell::Text(c.column_type.clone()),
                            Cell::Text(if c.nullable { "NULL" } else { "NOT NULL" }.into()),
                            Cell::Text(c.key.clone()),
                            Cell::Text(c.default.clone().unwrap_or_default()),
                            Cell::Text(c.extra.clone()),
                            Cell::Text(c.comment.clone()),
                        ]
                    })
                    .collect(),
            };
            let mut sel = None;
            render_grid(ui, &g, &mut sel, None);

            // ── Indexes ────────────────────────────────────────────────
            ui.add_space(8.0);
            ui.heading("Indexes");
            egui::Grid::new("idx_grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Kind");
                    ui.strong("Columns");
                    ui.end_row();
                    for k in &schema.keys {
                        let kind = match k.kind {
                            KeyKind::Primary => "PRIMARY",
                            KeyKind::Unique => "UNIQUE",
                            KeyKind::Index => "INDEX",
                            KeyKind::Fulltext => "FULLTEXT",
                            KeyKind::Spatial => "SPATIAL",
                        };
                        ui.label(&k.name);
                        ui.label(kind);
                        ui.label(
                            k.columns
                                .iter()
                                .map(|(c, sub)| match sub {
                                    Some(n) => format!("{c}({n})"),
                                    None => c.clone(),
                                })
                                .collect::<Vec<_>>()
                                .join(", "),
                        );
                        ui.end_row();
                    }
                });

            // ── Foreign keys ───────────────────────────────────────────
            if !schema.foreign_keys.is_empty() {
                ui.add_space(8.0);
                ui.heading("Foreign keys");
                egui::Grid::new("fk_grid")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Columns");
                        ui.strong("References");
                        ui.strong("On delete");
                        ui.strong("On update");
                        ui.end_row();
                        for fk in &schema.foreign_keys {
                            ui.label(&fk.name);
                            ui.label(fk.columns.join(", "));
                            ui.label(format!(
                                "{}({})",
                                fk.ref_table,
                                fk.ref_columns.join(", ")
                            ));
                            ui.label(&fk.on_delete);
                            ui.label(&fk.on_update);
                            ui.end_row();
                        }
                    });
            }

            // ── CREATE statement ───────────────────────────────────────
            ui.add_space(8.0);
            ui.heading("CREATE statement");
            ui.horizontal(|ui| {
                if ui.button("⧉ Copy").clicked() {
                    copy_clicked = true;
                }
                if ui.button("Open in query editor").clicked() {
                    open_query_clicked = true;
                }
            });
            create_stmt = schema.create_statement.clone().unwrap_or_default();
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    let mut shown = create_stmt.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width()),
                    );
                });
        }

        // ── Deferred actions ───────────────────────────────────────────
        if refresh_clicked {
            let table = self
                .sessions
                .get(conn_id)
                .map(|s| s.structure.table.clone())
                .unwrap_or_default();
            self.load_schema(conn_id, table);
        }
        if browse_clicked {
            let table = self
                .sessions
                .get(conn_id)
                .map(|s| s.structure.table.clone())
                .unwrap_or_default();
            open_data(self, conn_id, table);
        }
        if edit_clicked {
            self.open_designer(conn_id);
        }
        if copy_clicked {
            ctx.copy_text(create_stmt.clone());
            self.status = "CREATE statement copied".into();
        }
        if open_query_clicked {
            let code = create_stmt.clone();
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            state.view = ActiveView::Query;
            let title = format!("Query {}", state.queries.len() + 1);
            let mut tab = crate::session_view::QueryTab::new(title);
            tab.code = code;
            state.queries.push(tab);
            state.active_query = state.queries.len() - 1;
        }
    }
}
