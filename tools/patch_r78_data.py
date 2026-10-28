#!/usr/bin/env python3
"""R7 copy-row actions and R8 FK navigation in the data view."""
import io

p = "crates/mff-gui/src/data_view.rs"
s = io.open(p, encoding="utf-8").read()

# 1. DataAction::FollowFk
old = """    Structure(String),
    Export,
    Import,"""
new = """    Structure(String),
    Export,
    Import,
    /// Follow a foreign-key cell to the referenced row.
    FollowFk {
        table: String,
        column: String,
        value: String,
    },"""
assert old in s, "enum"
s = s.replace(old, new)

# 2. GridEvent variants
old = """enum GridEvent {
    HeaderSort(usize),
    Select(usize, usize),
    EditStart(usize, usize),
    EditCommit(String),
    EditCancel,
    DeleteRow(usize),
    SetNull(usize, usize),
    ViewCell(usize, usize),
}"""
new = """enum GridEvent {
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
}"""
assert old in s, "GridEvent"
s = s.replace(old, new)

# 3. render_data_grid signature gains fks
old = """fn render_data_grid(
    ui: &mut egui::Ui,
    grid: &Grid,
    visible: &[usize],
    selected: &mut Option<(usize, usize)>,
    sort: Option<&(String, bool)>,
    editing: Option<(usize, usize)>,
    edit_value: &mut String,
    just_started: &mut bool,
) -> Vec<GridEvent> {"""
new = """#[allow(clippy::too_many_arguments)]
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
) -> Vec<GridEvent> {"""
assert old in s, "render signature"
s = s.replace(old, new)

# 4. call site passes fks
old = """                let evs = render_data_grid(
                    ui,
                    grid,
                    &visible,
                    &mut sel,
                    sort.as_ref(),
                    editing,
                    &mut edit_value,
                    &mut just_started,
                );"""
new = """                let evs = render_data_grid(
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
                );"""
assert old in s, "render call"
s = s.replace(old, new)

# 5. row menu: copy actions (extend the ⋯ menu)
old = """                        ui.menu_button("⋯", |ui| {
                            if ui.button("View selected cell").clicked() {
                                ui.close();
                                if let Some((_, c)) = selection {
                                    events.push(GridEvent::ViewCell(di, c));
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
                        });"""
new = """                        ui.menu_button("⋯", |ui| {
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
                        });"""
assert old in s, "row menu"
s = s.replace(old, new)

# 6. cell context menu: FK navigation
old = """                        resp.context_menu(|ui| {
                            if ui.button("⤢ View value").clicked() {
                                ui.close();
                                events.push(GridEvent::ViewCell(di, c));
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
                        });"""
new = """                        resp.context_menu(|ui| {
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
                        });"""
assert old in s, "cell menu"
s = s.replace(old, new)

# 7. event handlers (state block) — copy + follow-fk; clipboard buffer
old = """            // Translate grid events into actions / state changes.
            for ev in events {
                match ev {"""
new = """            // Translate grid events into actions / state changes.
            let mut clipboard: Option<String> = None;
            for ev in events {
                match ev {"""
assert old in s, "event loop"
s = s.replace(old, new)

old = """                    GridEvent::ViewCell(r, c) => {
                        if let Some(grid) = &d.grid {
                            if let Some(&actual) = compute_visible(grid, &find_lower).get(r) {
                                d.cell_view = Some((actual, c));
                            }
                        }
                    }
                }
            }"""
new = """                    GridEvent::ViewCell(r, c) => {
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
                                    .join("\\t");
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
                                fks.iter().find(|f| f.0 == grid.columns[c].name),
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
                ui.ctx().copy_text(text);
                d_status = Some(format!("copied: {}", truncate(&text, 60)));
            }"""
assert old in s, "event handlers"
s = s.replace(old, new)

# d_status declaration + use: status is on App (not reachable inside state
# borrow) — collect it and set after the block.
old = """    pub fn show_data_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let mut actions: Vec<DataAction> = Vec::new();
        let export_open;"""
new = """    pub fn show_data_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        let mut actions: Vec<DataAction> = Vec::new();
        let export_open;
        let mut d_status: Option<String> = None;"""
assert old in s, "d_status decl"
s = s.replace(old, new)

# set status after the state block ends (before deferred actions)
old = """        // ── Deferred actions (need &mut self) ──────────────────────────
        for action in actions {"""
new = """        if let Some(msg) = d_status {
            self.status = msg;
        }

        // ── Deferred actions (need &mut self) ──────────────────────────
        for action in actions {"""
assert old in s, "status set"
s = s.replace(old, new)

# 8. FollowFk deferred handler
old = """                DataAction::Import => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.open_import_dialog(conn_id, table);
                }"""
new = """                DataAction::Import => {
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
                }"""
assert old in s, "followfk handler"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("data_view.rs ok")
