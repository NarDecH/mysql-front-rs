//! Data view: paginated, *editable* data grid with filter, sorting, find,
//! cell viewer and row insert/delete. The equivalent of the original
//! `MySQLDBGrid` in its editable mode.
//!
//! UI closures only mutate the session state; actions that need `&mut App`
//! (reloads, edits, exports) are collected and executed after the borrow
//! ends. Editing runs UPDATE/DELETE/INSERT statements built by
//! [`mff_core::edit`] in the background and reloads the page on success.

use egui::Context;
use mff_core::session::{Cell, Grid};

use crate::app::App;
use crate::session_view::{self, CellEdit};

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
    Import,
    /// Follow a foreign-key cell to the referenced row.
    FollowFk {
        table: String,
        column: String,
        value: String,
    },
    /// UPDATE one cell (`text` is the user-typed value; `NULL` → SQL NULL).
    ApplyEdit {
        row: usize,
        col: usize,
        text: String,
    },
    /// Open the "new row" dialog.
    InsertRow,
}

impl App {
    pub fn show_data_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let mut actions: Vec<DataAction> = Vec::new();
        let export_open;
        let mut d_status: Option<String> = None;

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
                if ui
                    .button("⇪ Import…")
                    .on_hover_text("Load a CSV / JSON / SQL file into a table")
                    .clicked()
                {
                    actions.push(DataAction::Import);
                }
                ui.separator();
                if ui
                    .button("＋ Row")
                    .on_hover_text("Insert a new row")
                    .clicked()
                {
                    actions.push(DataAction::InsertRow);
                }
                if ui
                    .button("⤢ View cell")
                    .on_hover_text("Show the selected cell's full value")
                    .clicked()
                {
                    d.cell_view = d.selected;
                }
            });
            ui.horizontal(|ui| {
                ui.label("filter (WHERE):");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut d.filter_input)
                        .hint_text("e.g. id > 100 AND name LIKE 'a%'")
                        .desired_width(300.0),
                );
                apply_filter |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                apply_filter |= ui.button("Apply").clicked();
                if !d.filter_applied.is_empty() && ui.button("✕ Clear").clicked() {
                    d.filter_input.clear();
                    apply_filter = true;
                }
                ui.separator();
                ui.label("find:");
                ui.add(
                    egui::TextEdit::singleline(&mut d.find)
                        .hint_text("search all columns…")
                        .desired_width(140.0),
                );
                if !d.find.is_empty() {
                    if ui.small_button("✕").clicked() {
                        d.find.clear();
                    }
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
            if d.pk_cols.is_empty() && d.total.unwrap_or(0) > 0 {
                ui.weak("no primary key — edits match whole rows (LIMIT 1)");
            }

            // ── Grid ───────────────────────────────────────────────────
            let ncols = d.grid.as_ref().map(|g| g.columns.len()).unwrap_or(0);
            let sort = d.sort.clone();
            let find_lower = d.find.to_lowercase();
            let mut events: Vec<GridEvent> = Vec::new();
            if ncols == 0 && !d.loading {
                ui.label("(empty result)");
            } else if let Some(grid) = &d.grid {
                // Client-side "find in data" over the displayed columns.
                let visible = compute_visible(grid, &find_lower);
                let mut sel = d.selected;
                let editing = d
                    .editing
                    .as_ref()
                    .map(|e| (display_row(&visible, e.row), e.col));
                let mut edit_value = d.edit_value.clone();
                let mut just_started = d.editing.as_ref().map(|e| e.just_started).unwrap_or(false);
                let evs = render_data_grid(
                    ui,
                    grid,
                    &visible,
                    &mut sel,
                    sort.as_ref(),
                    editing,
                    &mut edit_value,
                    &mut just_started,
                    &d.table,
                    &d.fks,
                );
                events.extend(evs);
                d.selected = sel;
                d.edit_value = edit_value;
                if let Some(e) = d.editing.as_mut() {
                    e.just_started = just_started;
                }
                if !find_lower.is_empty() {
                    ui.small(format!(
                        "find: {} of {} row(s) on this page match",
                        visible.len(),
                        grid.rows.len()
                    ));
                }
            }

            // Translate grid events into actions / state changes.
            let mut clipboard: Option<String> = None;
            for ev in events {
                match ev {
                    GridEvent::HeaderSort(col) => {
                        if let Some(grid) = &d.grid {
                            if let Some(meta) = grid.columns.get(col) {
                                let col_name = meta.name.clone();
                                let next_sort = match &sort {
                                    Some((c, true)) if *c == col_name => None,
                                    Some((c, false)) if *c == col_name => {
                                        Some((col_name, true))
                                    }
                                    _ => Some((col_name, false)),
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
                        }
                    }
                    GridEvent::Select(r, c) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                d.selected = Some((actual, c));
                            }
                        }
                    }
                    GridEvent::EditStart(r, c) => {
                        if let Some(grid) = &d.grid {
                            let visible = compute_visible(grid, &find_lower);
                            if let Some(&actual) = visible.get(r) {
                                d.edit_value = grid
                                    .rows
                                    .get(actual)
                                    .and_then(|row| row.get(c))
                                    .map(cell_text)
                                    .unwrap_or_default();
                                d.editing = Some(CellEdit {
                                    row: actual,
                                    col: c,
                                    just_started: true,
                                });
                            }
                        }
                    }
                    GridEvent::EditCommit(text) => {
                        if let Some(e) = d.editing.take() {
                            d.edit_value.clear();
                            actions.push(DataAction::ApplyEdit {
                                row: e.row,
                                col: e.col,
                                text,
                            });
                        }
                    }
                    GridEvent::EditCancel => {
                        d.editing = None;
                        d.edit_value.clear();
                    }
                    GridEvent::DeleteRow(r) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                d.confirm_delete_row = Some(actual);
                            }
                        }
                    }
                    GridEvent::SetNull(r, c) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                actions.push(DataAction::ApplyEdit {
                                    row: actual,
                                    col: c,
                                    text: "NULL".to_string(),
                                });
                            }
                        }
                    }
                    GridEvent::ViewCell(r, c) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                d.cell_view = Some((actual, c));
                            }
                        }
                    }
                    GridEvent::CopyRowTsv(r) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                let line = grid.rows[actual]
                                    .iter()
                                    .map(cell_text)
                                    .collect::<Vec<_>>()
                                    .join("\t");
                                clipboard = Some(line);
                            }
                        }
                    }
                    GridEvent::CopyRowInsert(r) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                let one = Grid {
                                    columns: grid.columns.clone(),
                                    rows: vec![grid.rows[actual].clone()],
                                };
                                let (stmts, _) = mff_core::sqlgen::insert_statements_into(
                                    &mff_core::sqlgen::quote_ident(&d.table),
                                    &one,
                                    1,
                                    true,
                                );
                                if let Some(stmt) = stmts.first() {
                                    clipboard = Some(stmt.clone());
                                }
                            }
                        }
                    }
                    GridEvent::FollowFk(r, c) => {
                        if let Some(grid) = &d.grid {
                            let visible = compute_visible(grid, &find_lower);
                            if let (Some(&actual), Some(fk)) = (
                                visible.get(r),
                                d.fks.iter().find(|f| f.0 == grid.columns[c].name),
                            ) {
                                let value = cell_text(&grid.rows[actual][c]);
                                if value == "NULL" {
                                    // nothing to follow
                                } else {
                                    actions.push(DataAction::FollowFk {
                                        table: fk.1.clone(),
                                        column: fk.2.clone(),
                                        value,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            if let Some(text) = clipboard {
                d_status = Some(format!("copied: {}", truncate(&text, 60)));
                ui.ctx().copy_text(text);
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

        if let Some(msg) = d_status {
            self.status = msg;
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
                DataAction::Import => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.open_import_dialog(conn_id, table);
                }
                DataAction::FollowFk {
                    table,
                    column,
                    value,
                } => {
                    let filter =
                        format!("{} = {}", mff_core::sqlgen::quote_ident(&column), mff_core::sqlgen::quote_string(&value));
                    session_view::open_data_filtered(self, conn_id, table.clone(), Some(filter));
                    self.status = format!("following {} → {}.{}", column, table, column);
                }
                DataAction::ApplyEdit { row, col, text } => {
                    self.apply_cell_edit(conn_id, row, col, &text);
                }
                DataAction::InsertRow => {
                    self.open_insert_dialog(conn_id);
                }
            }
        }
        if export_open {
            self.show_export_dialog(ctx, conn_id);
        }

        // Delete-row confirmation window.
        let confirm_row = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.data.confirm_delete_row);
        if let Some(r) = confirm_row {
            let mut open = true;
            let mut confirmed = false;
            egui::Window::new("Delete row")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Delete row {} of {}?", r + 1, {
                        self.sessions
                            .get(conn_id)
                            .and_then(|s| s.data.grid.as_ref())
                            .map(|g| g.row_count())
                            .unwrap_or(0)
                    }));
                    ui.weak("The row is removed from the server immediately.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("Delete").color(egui::Color32::LIGHT_RED))
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            if let Some(s) = self.sessions.get_mut(conn_id) {
                                s.data.confirm_delete_row = None;
                            }
                        }
                    });
                });
            if !open {
                if let Some(s) = self.sessions.get_mut(conn_id) {
                    s.data.confirm_delete_row = None;
                }
            }
            if confirmed {
                if let Some(s) = self.sessions.get_mut(conn_id) {
                    s.data.confirm_delete_row = None;
                }
                self.delete_grid_row(conn_id, r);
            }
        }

        // Cell value viewer window.
        let cell_view = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.data.cell_view);
        if let Some((r, c)) = cell_view {
            show_cell_viewer(self, ctx, conn_id, r, c);
        }

        // "Insert row" dialog (own window).
        if self
            .sessions
            .get(conn_id)
            .map(|s| s.data.insert_dlg.is_some())
            .unwrap_or(false)
        {
            self.show_insert_dialog(ctx, conn_id);
        }
    }

    /// Run the UPDATE for one edited cell (background job).
    fn apply_cell_edit(&mut self, conn_id: &str, row: usize, col: usize, text: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let d = &state.data;
        let Some(grid) = &d.grid else { return };
        let Some(meta) = grid.columns.get(col) else {
            return;
        };
        let meta = meta.clone();
        let m = match mff_core::edit::row_where(grid, row, &d.pk_cols) {
            Ok(m) => m,
            Err(e) => {
                self.status = format!("edit failed: {e}");
                return;
            }
        };
        let db = state.current_db.clone();
        let table = d.table.clone();
        let sql = mff_core::edit::update_sql(&db, &table, &meta, text, &m);
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = format!("saving edit… ({sql})");
        std::thread::spawn(move || {
            let result = session.run_statement(&sql).map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::DataEdited {
                conn_id: id,
                result,
            });
        });
    }

    /// Run the DELETE for one grid row (background job).
    fn delete_grid_row(&mut self, conn_id: &str, row: usize) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let d = &state.data;
        let Some(grid) = &d.grid else { return };
        let m = match mff_core::edit::row_where(grid, row, &d.pk_cols) {
            Ok(m) => m,
            Err(e) => {
                self.status = format!("delete failed: {e}");
                return;
            }
        };
        let db = state.current_db.clone();
        let table = d.table.clone();
        let sql = mff_core::edit::delete_sql(&db, &table, &m);
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = "deleting row…".into();
        std::thread::spawn(move || {
            let result = session.run_statement(&sql).map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(crate::jobs::UiMsg::DataEdited {
                conn_id: id,
                result,
            });
        });
    }

    /// Open the "insert row" dialog, loading the schema first if needed.
    fn open_insert_dialog(&mut self, conn_id: &str) {
        let (table, have_schema) = self
            .sessions
            .get(conn_id)
            .map(|s| {
                (
                    s.data.table.clone(),
                    s.structure.schema
                        .as_ref()
                        .map(|t| t.name.clone() == s.data.table)
                        .unwrap_or(false),
                )
            })
            .unwrap_or_default();
        if table.is_empty() {
            return;
        }
        if have_schema {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                let schema = state.structure.schema.as_ref().unwrap();
                state.data.insert_dlg =
                    Some(crate::insert_row::InsertRowDialog::from_schema(&table, schema));
            }
        } else {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.data.insert_pending = true;
            }
            self.load_schema(conn_id, table);
        }
    }
}

/// Map a display row (after find-filtering) back to its grid row index.
fn display_row(visible: &[usize], display: usize) -> usize {
    visible.get(display).copied().unwrap_or(display)
}

/// Rows to show: everything, or only rows where any cell (or a column
/// name) contains the find text.
fn compute_visible(grid: &Grid, find_lower: &str) -> Vec<usize> {
    if find_lower.is_empty() {
        (0..grid.rows.len()).collect()
    } else {
        grid.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.iter().zip(grid.columns.iter()).any(|(cell, col)| {
                    cell_text(cell).to_lowercase().contains(find_lower)
                        || col.name.to_lowercase().contains(find_lower)
                })
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// Events produced by the editable data grid (display coordinates).
enum GridEvent {
    HeaderSort(usize),
    Select(usize, usize),
    EditStart(usize, usize),
    EditCommit(String),
    EditCancel,
    DeleteRow(usize),
    SetNull(usize, usize),
    ViewCell(usize, usize),
    /// Row menu: copy the row (tab separated / as INSERT).
    CopyRowTsv(usize),
    CopyRowInsert(usize),
    /// Cell context menu: jump to the referenced row.
    FollowFk(usize, usize),
}

/// Render the editable data grid. Returns the events of this frame.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn render_data_grid(
    ui: &mut egui::Ui,
    grid: &Grid,
    visible: &[usize],
    selected: &mut Option<(usize, usize)>,
    sort: Option<&(String, bool)>,
    editing: Option<(usize, usize)>,
    edit_value: &mut String,
    just_started: &mut bool,
    table: &str,
    fks: &[(String, String, String)],
) -> Vec<GridEvent> {
    use egui_extras::{Column, TableBuilder};

    let mut events = Vec::new();
    let ncols = grid.columns.len();
    let nrows = visible.len();
    let selection = *selected;

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0);
    table = table.columns(Column::initial(64.0).resizable(false), 1);
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
                        events.push(GridEvent::HeaderSort(i));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(20.0, nrows, |mut row| {
                let di = row.index(); // display index
                let r = visible[di]; // actual grid row
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        ui.small((r + 1).to_string());
                        ui.add_space(2.0);
                        ui.menu_button("…", |ui| {
                            if ui.button("View selected cell").clicked() {
                                ui.close();
                                if let Some((_, c)) = selection {
                                    events.push(GridEvent::ViewCell(di, c));
                                }
                            }
                            if ui
                                .button("⧉ Copy row (TSV)")
                                .on_hover_text("Tab-separated cells — paste into a spreadsheet")
                                .clicked()
                            {
                                ui.close();
                                events.push(GridEvent::CopyRowTsv(di));
                            }
                            if ui
                                .button("⧉ Copy row as INSERT")
                                .on_hover_text("INSERT statement for this row")
                                .clicked()
                            {
                                ui.close();
                                events.push(GridEvent::CopyRowInsert(di));
                            }
                            // FK navigation for the selected cell of this row.
                            if let Some((sr, sc)) = selection {
                                if sr == r {
                                    if let Some((_, ref_table, ref_col)) =
                                        fks.iter().find(|f| f.0 == grid.columns[sc].name)
                                    {
                                        if ui
                                            .button(format!(
                                                "➜ Go to {} ({} …)",
                                                ref_table, ref_col
                                            ))
                                            .on_hover_text("Open the referenced row")
                                            .clicked()
                                        {
                                            ui.close();
                                            events.push(GridEvent::FollowFk(di, sc));
                                        }
                                    }
                                }
                            }
                            if ui
                                .button(
                                    egui::RichText::new("Delete row…")
                                        .color(egui::Color32::LIGHT_RED),
                                )
                                .clicked()
                            {
                                ui.close();
                                events.push(GridEvent::DeleteRow(di));
                            }
                        });
                    });
                });
                for c in 0..ncols {
                    row.col(|ui| {
                        let cell = &grid.rows[r][c];
                        let is_sel = selection == Some((r, c));
                        let is_editing = editing == Some((di, c));
                        if is_editing {
                            let resp = ui.add(
                                egui::TextEdit::singleline(edit_value)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(ui.available_width().min(220.0)),
                            );
                            if *just_started {
                                resp.request_focus();
                                *just_started = false;
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                events.push(GridEvent::EditCancel);
                            } else if ui
                                .input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                events.push(GridEvent::EditCommit(edit_value.clone()));
                            } else if resp.lost_focus() {
                                // Blur without Enter: treat as commit too
                                // (clicking another cell keeps the change).
                                events.push(GridEvent::EditCommit(edit_value.clone()));
                            }
                            return;
                        }
                        let text = cell_text(cell);
                        let rich = match cell {
                            Cell::Null => egui::RichText::new(text).weak().italics(),
                            Cell::Bytes(_) => egui::RichText::new(text)
                                .monospace()
                                .color(egui::Color32::from_rgb(170, 130, 255)),
                            Cell::Text(_) => egui::RichText::new(text),
                        };
                        let resp = ui
                            .add(egui::Label::new(rich).sense(egui::Sense::click()))
                            .on_hover_text("double-click to edit · right-click for more");
                        if resp.clicked() {
                            events.push(GridEvent::Select(di, c));
                        }
                        if resp.double_clicked() {
                            events.push(GridEvent::EditStart(di, c));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("⤢ View value").clicked() {
                                ui.close();
                                events.push(GridEvent::ViewCell(di, c));
                            }
                            // Jump to the row this FK points at.
                            if let Some((_, ref_table, ref_col)) =
                                fks.iter().find(|f| f.0 == grid.columns[c].name)
                            {
                                if ui
                                    .button(format!("➜ Go to {} ({} …)", ref_table, ref_col))
                                    .on_hover_text("Open the referenced row")
                                    .clicked()
                                {
                                    ui.close();
                                    events.push(GridEvent::FollowFk(di, c));
                                }
                            }
                            if ui.button("Set NULL").clicked() {
                                ui.close();
                                events.push(GridEvent::SetNull(di, c));
                            }
                            ui.separator();
                            if ui
                                .button(
                                    egui::RichText::new("Delete row…")
                                        .color(egui::Color32::LIGHT_RED),
                                )
                                .clicked()
                            {
                                ui.close();
                                events.push(GridEvent::DeleteRow(di));
                            }
                        });
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
    if !events.is_empty() {
        // Sorting changes the grid; drop a stale selection.
        if events
            .iter()
            .any(|e| matches!(e, GridEvent::HeaderSort(_)))
        {
            *selected = None;
        }
    }
    events
}

pub fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Null => "NULL".to_string(),
        Cell::Text(s) => s.clone(),
        Cell::Bytes(_) => cell.display("NULL"),
    }
}

/// Render a read-only result grid with selectable cells and clickable
/// (sortable) headers. Used by the query and structure views. Returns the
/// index of the header column that was clicked.
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

/// The "cell value" window: full text, pretty JSON, or byte inspection.
fn show_cell_viewer(app: &mut App, ctx: &Context, conn_id: &str, r: usize, c: usize) {
    let (title, text) = {
        let Some(state) = app.sessions.get(conn_id) else {
            return;
        };
        let Some(grid) = &state.data.grid else {
            return;
        };
        let Some(row) = grid.rows.get(r) else {
            return;
        };
        let Some(cell) = row.get(c) else {
            return;
        };
        let col_name = grid
            .columns
            .get(c)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let title = format!(
            "{} · {} (row {})",
            state.data.table, col_name,
            r + 1
        );
        let text = match cell {
            Cell::Null => "NULL".to_string(),
            Cell::Text(s) => match mff_core::edit::pretty_json(s) {
                Some(pretty) => pretty,
                None => s.clone(),
            },
            Cell::Bytes(b) => {
                let preview_len = b.len().min(256);
                let hex: String = b[..preview_len]
                    .iter()
                    .map(|x| format!("{x:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let utf8 = String::from_utf8_lossy(b).into_owned();
                format!(
                    "binary, {} byte(s)\n\nfirst {preview_len} byte(s) as hex:\n{hex}\n\nas text (lossy):\n{utf8}",
                    b.len()
                )
            }
        };
        (title, text)
    };

    let mut open = true;
    let mut copied = false;
    let mut close_clicked = false;
    egui::Window::new(title.clone())
        .open(&mut open)
        .default_width(560.0)
        .default_height(380.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut view = text.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut view)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(12)
                            .desired_width(f32::INFINITY),
                    );
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("⧉ Copy").clicked() {
                    copied = true;
                }
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        });
    if copied {
        ctx.copy_text(text.clone());
        app.status = format!("copied cell value ({} chars)", text.chars().count());
    }
    if !open || close_clicked {
        if let Some(state) = app.sessions.get_mut(conn_id) {
            state.data.cell_view = None;
        }
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
