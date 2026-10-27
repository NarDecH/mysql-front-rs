//! Data view: paginated data grid with filter, sorting, cell selection and
//! export. The equivalent of the original `MySQLDBGrid` browsing mode.
//!
//! UI closures only mutate the session state; actions that need `&mut App`
//! (reloads, exports) are collected and executed after the borrow ends.

use egui::Context;
use mff_core::session::{Cell, Grid};

use crate::app::App;
use crate::session_view;

const PAGE_SIZES: [usize; 6] = [20, 50, 100, 200, 500, 1000];

/// Actions collected during rendering, executed afterwards.
enum DataAction {
    Reload {
        table: String,
        filter: String,
        sort: Option<(String, bool)>,
    },
    Structure(String),
    Export,
}

impl App {
    pub fn show_data_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let mut actions: Vec<DataAction> = Vec::new();
        let export_open;

        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let d = &mut state.data;

            if d.table.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.label("select a table in the object browser")
                });
                return;
            }

            // ── Toolbar ────────────────────────────────────────────────
            let mut apply_filter = false;
            ui.horizontal(|ui| {
                crate::theme::icon_label(
                    ui,
                    &crate::theme::table_icon(),
                    &format!("{}.{}", state.current_db, d.table),
                );
                ui.separator();
                if d.loading {
                    ui.spinner();
                    ui.small("loading…");
                } else if ui.button("⟳ Refresh").clicked() {
                    actions.push(DataAction::Reload {
                        table: d.table.clone(),
                        filter: d.filter_applied.clone(),
                        sort: d.sort.clone(),
                    });
                }
                if ui.button("⚒ Structure").clicked() {
                    actions.push(DataAction::Structure(d.table.clone()));
                }
                if ui.button("⭳ Export…").clicked() {
                    actions.push(DataAction::Export);
                }
                ui.separator();
                ui.label("filter (WHERE):");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut d.filter_input)
                        .hint_text("e.g. id > 100 AND name LIKE 'a%'")
                        .desired_width(320.0),
                );
                apply_filter |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                apply_filter |= ui.button("Apply").clicked();
                if !d.filter_applied.is_empty() && ui.button("✕ Clear").clicked() {
                    d.filter_input.clear();
                    apply_filter = true;
                }
            });
            if apply_filter {
                d.filter_applied = d.filter_input.trim().to_string();
                d.offset = 0;
                actions.push(DataAction::Reload {
                    table: d.table.clone(),
                    filter: d.filter_applied.clone(),
                    sort: d.sort.clone(),
                });
            }
            ui.add_space(6.0);

            if let Some(err) = &d.error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                ui.add_space(4.0);
            }

            // ── Grid ───────────────────────────────────────────────────
            let ncols = d.grid.as_ref().map(|g| g.columns.len()).unwrap_or(0);
            let sort = d.sort.clone();
            let mut sort_clicked: Option<usize> = None;
            if ncols == 0 && !d.loading {
                ui.label("(empty result)");
            } else if let Some(grid) = &d.grid {
                let mut sel = d.selected;
                sort_clicked = render_grid(ui, grid, &mut sel, sort.as_ref());
                d.selected = sel;
            }
            if let Some(col) = sort_clicked {
                let col_name = d.grid.as_ref().unwrap().columns[col].name.clone();
                let next_sort = match &sort {
                    Some((c, true)) if *c == col_name => Some((col_name, false)),
                    Some((c, false)) if *c == col_name => None,
                    _ => Some((col_name, true)),
                };
                d.selected = None;
                d.offset = 0;
                d.sort = next_sort.clone();
                actions.push(DataAction::Reload {
                    table: d.table.clone(),
                    filter: d.filter_applied.clone(),
                    sort: next_sort,
                });
            }

            // ── Footer: pagination ─────────────────────────────────────
            ui.add_space(6.0);
            ui.separator();
            let mut page_size_changed = false;
            ui.horizontal(|ui| {
                let total = d.total.unwrap_or(0);
                let page = d.offset / (d.page_size.max(1) as u64) + 1;
                let last_page =
                    ((total + (d.page_size.max(1) as u64) - 1) / (d.page_size.max(1) as u64))
                        .max(1);
                ui.label(format!(
                    "{} row(s) total, showing {}–{}",
                    total,
                    if total == 0 { 0 } else { d.offset + 1 },
                    (d.offset + (d.page_size as u64)).min(total)
                ));
                ui.separator();
                let mut reload = false;
                if ui.add_enabled(d.offset > 0, egui::Button::new("◀ Prev")).clicked() {
                    d.offset = d.offset.saturating_sub(d.page_size as u64);
                    reload = true;
                }
                ui.label(format!("page {page}/{last_page}"));
                let has_more = d.offset + (d.page_size as u64) < total;
                if ui.add_enabled(has_more, egui::Button::new("Next ▶")).clicked() {
                    d.offset += d.page_size as u64;
                    reload = true;
                }
                if reload {
                    actions.push(DataAction::Reload {
                        table: d.table.clone(),
                        filter: d.filter_applied.clone(),
                        sort: d.sort.clone(),
                    });
                }
                ui.separator();
                ui.label("page size:");
                egui::ComboBox::from_id_salt("page_size")
                    .selected_text(d.page_size.to_string())
                    .show_ui(ui, |ui| {
                        for s in PAGE_SIZES {
                            ui.selectable_value(&mut d.page_size, s, s.to_string());
                        }
                    });
                if ui.button("go").clicked() {
                    d.offset = 0;
                    page_size_changed = true;
                }
            });
            if page_size_changed {
                actions.push(DataAction::Reload {
                    table: d.table.clone(),
                    filter: d.filter_applied.clone(),
                    sort: d.sort.clone(),
                });
            }
            export_open = d.export_open;
        }

        // ── Deferred actions (need &mut self) ──────────────────────────
        for action in actions {
            match action {
                DataAction::Reload { table, filter, sort } => {
                    session_view::reopen_data(self, conn_id, table, filter, sort);
                }
                DataAction::Structure(table) => {
                    session_view::open_structure(self, conn_id, table);
                }
                DataAction::Export => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.ensure_schema(conn_id, table);
                    if let Some(s) = self.sessions.get_mut(conn_id) {
                        s.data.export_open = true;
                    }
                }
            }
        }
        if export_open {
            self.show_export_dialog(ctx, conn_id);
        }
    }
}

/// Render a result grid with selectable cells and clickable (sortable)
/// headers. Returns the index of the header column that was clicked.
pub fn render_grid(
    ui: &mut egui::Ui,
    grid: &Grid,
    selected: &mut Option<(usize, usize)>,
    sort: Option<&(String, bool)>,
) -> Option<usize> {
    use egui_extras::{Column, TableBuilder};

    let ncols = grid.columns.len();
    let nrows = grid.rows.len();
    let mut clicked_header: Option<usize> = None;
    let selection = *selected;

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0);
    table = table.columns(Column::initial(36.0).resizable(false), 1);
    table = table.columns(
        Column::initial(140.0).resizable(true).clip(true).at_least(40.0),
        ncols,
    );
    table
        .header(22.0, |mut header| {
            header.col(|ui| {
                ui.strong("#");
            });
            for (i, col) in grid.columns.iter().enumerate() {
                let arrow = match sort {
                    Some((c, true)) if *c == col.name => " ▲",
                    Some((c, false)) if *c == col.name => " ▼",
                    _ => "",
                };
                header.col(|ui| {
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new(format!("{}{arrow}", col.name)).strong(),
                        )
                        .small(),
                    );
                    if resp.clicked() {
                        clicked_header = Some(i);
                    }
                });
            }
        })
        .body(|body| {
            body.rows(20.0, nrows, |mut row| {
                let r = row.index();
                row.col(|ui| {
                    ui.small((r + 1).to_string());
                });
                for c in 0..ncols {
                    row.col(|ui| {
                        let cell = &grid.rows[r][c];
                        let is_sel = selection == Some((r, c));
                        let text = cell_text(cell);
                        let rich = match cell {
                            Cell::Null => egui::RichText::new(text).weak().italics(),
                            Cell::Bytes(_) => egui::RichText::new(text)
                                .monospace()
                                .color(egui::Color32::from_rgb(170, 130, 255)),
                            Cell::Text(_) => egui::RichText::new(text),
                        };
                        let resp = ui.add(egui::Label::new(rich).sense(egui::Sense::click()));
                        if is_sel {
                            ui.painter().rect_stroke(
                                resp.rect,
                                1.0,
                                egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(90, 160, 255)),
                                egui::StrokeKind::Inside,
                            );
                        }
                    });
                }
            });
        });
    if clicked_header.is_some() {
        *selected = None;
    }
    clicked_header
}

pub fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Null => "NULL".to_string(),
        Cell::Text(s) => s.clone(),
        Cell::Bytes(_) => cell.display("NULL"),
    }
}

impl App {
    /// Copy the selected data cell to the clipboard (Ctrl+C in data view).
    pub fn copy_selected_cell(&mut self, ctx: &Context) {
        let Some(conn_id) = self.active_conn.clone() else {
            return;
        };
        let Some(state) = self.sessions.get(&conn_id) else {
            return;
        };
        if let (Some((r, c)), Some(grid)) = (state.data.selected, &state.data.grid) {
            if let Some(row) = grid.rows.get(r) {
                if let Some(cell) = row.get(c) {
                    let text = cell_text(cell);
                    ctx.copy_text(text.clone());
                    self.status = format!("copied: {}", truncate(&text, 40));
                }
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    }
}
