#!/usr/bin/env python3
"""R5 session tabs, R6 EXPLAIN, R7 copy rows, R8 FK navigation, R9
keyboard shortcuts (F5 / Ctrl+T)."""
import io

# ── jobs.rs: DataLoaded carries FK info ─────────────────────────────────
p = "crates/mff-gui/src/jobs.rs"
s = io.open(p, encoding="utf-8").read()
old = """    DataLoaded {
        conn_id: String,
        table: String,
        result: std::result::Result<(Grid, u64, Vec<String>), String>,
    },"""
new = """    DataLoaded {
        conn_id: String,
        table: String,
        result: std::result::Result<
            (Grid, u64, Vec<String>, Vec<(String, String, String)>),
            String,
        >,
    },"""
assert old in s, "jobs DataLoaded"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── session_view.rs ─────────────────────────────────────────────────────
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

# DataState: fk map
old = """    /// Primary-key columns of the browsed table (empty → no editing).
    pub pk_cols: Vec<String>,"""
new = """    /// Primary-key columns of the browsed table (empty → no editing).
    pub pk_cols: Vec<String>,
    /// Foreign keys of the browsed table: (column, ref_table, ref_column).
    pub fks: Vec<(String, String, String)>,"""
assert old in s, "DataState fks"
s = s.replace(old, new)

old = """            export_open: false,
            pk_cols: Vec::new(),"""
new = """            export_open: false,
            pk_cols: Vec::new(),
            fks: Vec::new(),"""
assert old in s, "DataState init"
s = s.replace(old, new)

# load_data: fetch FKs too
old = """            let pk = session.primary_key_cols(&db, &table).unwrap_or_default();
            let rows = session.table_rows(
                &db,
                &table,
                filter_opt,
                sort_ref,
                offset,
                page_size as u64,
            );
            let total = session
                .count_rows(&db, &table, Some(filter.as_str()).filter(|f| !f.is_empty()))
                .unwrap_or(0);
            let result = match rows {
                Ok(grid) => Ok((grid, total, pk)),
                Err(e) => Err(e.to_string()),
            };"""
new = """            let pk = session.primary_key_cols(&db, &table).unwrap_or_default();
            // Foreign keys for "go to referenced row" navigation.
            let fks: Vec<(String, String, String)> = session
                .table_schema(&db, &table)
                .map(|t| {
                    t.foreign_keys
                        .iter()
                        .flat_map(|fk| {
                            fk.columns
                                .iter()
                                .cloned()
                                .zip(fk.ref_columns.iter().cloned())
                                .map(move |(c, rc)| (c, fk.ref_table.clone(), rc))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let rows = session.table_rows(
                &db,
                &table,
                filter_opt,
                sort_ref,
                offset,
                page_size as u64,
            );
            let total = session
                .count_rows(&db, &table, Some(filter.as_str()).filter(|f| !f.is_empty()))
                .unwrap_or(0);
            let result = match rows {
                Ok(grid) => Ok((grid, total, pk, fks)),
                Err(e) => Err(e.to_string()),
            };"""
assert old in s, "load_data"
s = s.replace(old, new)

# apply_msg DataLoaded
old = """                        match result {
                            Ok((grid, total, pk)) => {
                                state.data.grid = Some(grid);
                                state.data.total = Some(total);
                                state.data.pk_cols = pk;
                                state.data.editing = None;
                                state.data.edit_value.clear();
                            }
                            Err(e) => state.data.error = Some(e),
                        }"""
new = """                        match result {
                            Ok((grid, total, pk, fks)) => {
                                state.data.grid = Some(grid);
                                state.data.total = Some(total);
                                state.data.pk_cols = pk;
                                state.data.fks = fks;
                                state.data.editing = None;
                                state.data.edit_value.clear();
                            }
                            Err(e) => state.data.error = Some(e),
                        }"""
assert old in s, "apply DataLoaded"
s = s.replace(old, new)

# open_data refactored to open_data_filtered
old = """/// Open the data view of a table and (re)load its rows.
pub fn open_data(app: &mut App, conn_id: &str, table: String) {
    {
        let Some(state) = app.sessions.get_mut(conn_id) else {
            return;
        };
        state.view = ActiveView::Data;
        state.data.offset = 0;
        state.data.sort = None;
        state.data.filter_applied = String::new();
        state.data.filter_input = String::new();
        state.data.selected = None;
    }
    app.load_data(conn_id, table);
}"""
new = """/// Open the data view of a table and (re)load its rows.
pub fn open_data(app: &mut App, conn_id: &str, table: String) {
    open_data_filtered(app, conn_id, table, None);
}

/// Open the data view with an initial WHERE filter (e.g. following a
/// foreign key to the referenced row).
pub fn open_data_filtered(
    app: &mut App,
    conn_id: &str,
    table: String,
    filter: Option<String>,
) {
    {
        let Some(state) = app.sessions.get_mut(conn_id) else {
            return;
        };
        state.view = ActiveView::Data;
        state.data.offset = 0;
        state.data.sort = None;
        let f = filter.unwrap_or_default();
        state.data.filter_applied = f.clone();
        state.data.filter_input = f;
        state.data.selected = None;
    }
    app.load_data(conn_id, table);
}"""
assert old in s, "open_data"
s = s.replace(old, new)

# R9 shortcuts at the top of show_session
old = """    /// Render the workspace of the active session.
    pub fn show_session(&mut self, ctx: &Context, conn_id: String) {
        let (conn_name, version, current_db) = {
            let Some(state) = self.sessions.get(&conn_id) else {
                return;
            };
            (
                state.conn_name.clone(),
                state.version.clone(),
                state.current_db.clone(),
            )
        };"""
new = """    /// Render the workspace of the active session.
    pub fn show_session(&mut self, ctx: &Context, conn_id: String) {
        let (conn_name, version, current_db) = {
            let Some(state) = self.sessions.get(&conn_id) else {
                return;
            };
            (
                state.conn_name.clone(),
                state.version.clone(),
                state.current_db.clone(),
            )
        };

        // ── Keyboard shortcuts ─────────────────────────────────────────
        // F5 refreshes whatever view is active; Ctrl+T opens a new query tab.
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            let view = self.sessions.get(&conn_id).map(|s| s.view);
            match view {
                Some(ActiveView::Data) => {
                    let table = self
                        .sessions
                        .get(&conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    if !table.is_empty() {
                        reopen_data_current(self, &conn_id, table);
                    }
                }
                Some(ActiveView::Structure) => {
                    let table = self
                        .sessions
                        .get(&conn_id)
                        .map(|s| s.structure.table.clone())
                        .unwrap_or_default();
                    if !table.is_empty() {
                        self.load_schema(&conn_id, table);
                    }
                }
                Some(ActiveView::Info) => self.load_variables(&conn_id),
                Some(ActiveView::Server) => {
                    let session = self.sessions.get(&conn_id).map(|s| s.session.clone());
                    if let Some(session) = session {
                        self.refresh_server_tab(&conn_id);
                        let _ = session;
                    }
                }
                _ => {}
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.ctrl) {
            if let Some(state) = self.sessions.get_mut(&conn_id) {
                state
                    .queries
                    .push(QueryTab::new(format!("Query {}", state.queries.len() + 1)));
                state.active_query = state.queries.len() - 1;
                state.view = ActiveView::Query;
            }
        }"""
assert old in s, "show_session shortcuts"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("session_view.rs ok")

# ── app.rs: session tabs ────────────────────────────────────────────────
p = "crates/mff-gui/src/app.rs"
s = io.open(p, encoding="utf-8").read()

old = """            self.show_session_top(ctx, &conn_id);
            self.show_session(ctx, conn_id);"""
new = """            self.show_session_tabs(ctx);
            self.show_session_top(ctx, &conn_id);
            self.show_session(ctx, conn_id);"""
assert old in s, "update"
s = s.replace(old, new)

old = """    /// Top bar of the session workspace (copy cell + back home).
    fn show_session_top(&mut self, ctx: &Context, _conn_id: &str) {"""
new = """    /// One tab per open connection; "＋" returns to the connections
    /// screen, ✕ disconnects.
    fn show_session_tabs(&mut self, ctx: &Context) {
        if self.sessions.is_empty() {
            return;
        }
        let active = self.active_conn.clone().unwrap_or_default();
        let mut tabs: Vec<(String, String)> = self
            .sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.conn_name.clone()))
            .collect();
        tabs.sort_by(|a, b| a.1.cmp(&b.1));

        let mut switch_to: Option<String> = None;
        let mut close_id: Option<String> = None;
        let mut go_home = false;
        egui::TopBottomPanel::top("session_tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (id, name) in &tabs {
                    let selected = *id == active;
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(selected, egui::RichText::new(name).strong())
                            .clicked()
                        {
                            switch_to = Some(id.clone());
                        }
                        if ui
                            .small_button("✕")
                            .on_hover_text(format!("disconnect {name}"))
                            .clicked()
                        {
                            close_id = Some(id.clone());
                        }
                    });
                }
                ui.separator();
                if ui
                    .button("＋")
                    .on_hover_text("Open another connection")
                    .clicked()
                {
                    go_home = true;
                }
            });
        });
        if let Some(id) = switch_to {
            self.active_conn = Some(id);
        }
        if let Some(id) = close_id {
            self.disconnect(&id);
        }
        if go_home {
            self.active_conn = None;
        }
    }

    /// Top bar of the session workspace (copy cell + back home).
    fn show_session_top(&mut self, ctx: &Context, _conn_id: &str) {"""
assert old in s, "tabs"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("app.rs ok")

# ── query_view.rs: EXPLAIN ──────────────────────────────────────────────
p = "crates/mff-gui/src/query_view.rs"
s = io.open(p, encoding="utf-8").read()

old = """        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let elapsed: Option<u128>;"""
new = """        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let mut explain_clicked = false;
        let elapsed: Option<u128>;"""
assert old in s, "flags"
s = s.replace(old, new)

old = """                if ui
                    .button("✨ Format")
                    .on_hover_text("Reformat the SQL (uppercase keywords, one clause per line)")
                    .clicked()
                {
                    format_clicked = true;
                }"""
new = """                if ui
                    .button("✨ Format")
                    .on_hover_text("Reformat the SQL (uppercase keywords, one clause per line)")
                    .clicked()
                {
                    format_clicked = true;
                }
                if ui
                    .button("🔍 Explain")
                    .on_hover_text(
                        "Run EXPLAIN on every SELECT/INSERT/UPDATE/DELETE/REPLACE statement",
                    )
                    .clicked()
                {
                    explain_clicked = true;
                }"""
assert old in s, "explain button"
s = s.replace(old, new)

old = """        let ctrl_enter = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);"""
new = """        if explain_clicked && !running {
            let stmts = mff_core::sqlsplit::split_statements(&code);
            let mut parts: Vec<String> = Vec::new();
            for st in &stmts {
                let first = st
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches('`')
                    .to_ascii_uppercase();
                if matches!(
                    first.as_str(),
                    "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "TABLE"
                ) {
                    parts.push(format!("EXPLAIN {}", st.trim()));
                }
            }
            if parts.is_empty() {
                self.status = "nothing to explain (needs SELECT/INSERT/UPDATE/DELETE/REPLACE)".into();
            } else {
                self.run_query(conn_id, active, parts.join(";\\n"), false);
            }
        }

        let ctrl_enter = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);"""
assert old in s, "explain handler"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("query_view.rs ok")
