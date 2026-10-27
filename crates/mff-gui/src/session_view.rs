//! Session workspace: the per-connection state and its UI
//! (object browser + data / structure / query / info tabs).

use std::time::Instant;

use egui::Context;
use mff_core::model::{DatabaseObjects, Table};
use mff_core::session::{Grid, Session};

use crate::jobs::{QuerySection, UiMsg};
use crate::app::App;

/// Everything the UI keeps for one open connection.
pub struct SessionState {
    pub session: Session,
    pub conn_name: String,
    pub version: String,
    pub databases: Vec<String>,
    pub current_db: String,
    pub objects: Option<DatabaseObjects>,
    pub objects_loading: bool,
    pub view: ActiveView,
    pub data: DataState,
    pub structure: StructureState,
    pub queries: Vec<QueryTab>,
    pub active_query: usize,
    pub info: InfoState,
    pub tree_filter: String,
    pub busy: usize,
    pub error: Option<String>,
    /// Explorer-style tree selection.
    pub tree_selected: Option<(ObjKind, String)>,
    /// Active inline rename, if any.
    pub tree_rename: Option<TreeRename>,
}

/// Kind of object selected in the tree (drives the highlight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjKind {
    Table,
    View,
    Routine,
    Trigger,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Data,
    Structure,
    Query,
    Info,
}

pub struct DataState {
    pub table: String,
    pub grid: Option<Grid>,
    pub total: Option<u64>,
    pub offset: u64,
    pub page_size: usize,
    pub filter_input: String,
    pub filter_applied: String,
    pub sort: Option<(String, bool)>,
    pub selected: Option<(usize, usize)>,
    pub loading: bool,
    pub error: Option<String>,
    pub export_open: bool,
}

impl Default for DataState {
    fn default() -> Self {
        Self {
            table: String::new(),
            grid: None,
            total: None,
            offset: 0,
            page_size: 100,
            filter_input: String::new(),
            filter_applied: String::new(),
            sort: None,
            selected: None,
            loading: false,
            error: None,
            export_open: false,
        }
    }
}

pub struct StructureState {
    pub table: String,
    pub schema: Option<Table>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Default for StructureState {
    fn default() -> Self {
        Self {
            table: String::new(),
            schema: None,
            loading: false,
            error: None,
        }
    }
}

pub struct QueryTab {
    #[allow(dead_code)]
    pub title: String,
    pub code: String,
    pub sections: Vec<QuerySection>,
    pub running: bool,
    pub elapsed_ms: Option<u128>,
}

impl QueryTab {
    pub fn new(title: String) -> Self {
        Self {
            title,
            code: String::new(),
            sections: Vec::new(),
            running: false,
            elapsed_ms: None,
        }
    }
}

#[derive(Default)]
pub struct InfoState {
    pub variables: Option<Vec<(String, String)>>,
    pub filter: String,
    pub loading: bool,
}

/// Inline rename state for one tree entry (Explorer-style).
#[derive(Debug, Clone)]
pub struct TreeRename {
    pub is_view: bool,
    pub old: String,
    pub value: String,
    /// Request focus on the edit for the frame(s) after entering rename mode.
    pub just_started: bool,
    pub error: Option<String>,
}

impl SessionState {
    pub fn new(
        session: Session,
        conn_name: String,
        version: String,
        current_db: String,
    ) -> Self {
        Self {
            session,
            conn_name,
            version,
            databases: Vec::new(),
            current_db,
            objects: None,
            objects_loading: false,
            view: ActiveView::Query,
            data: Default::default(),
            structure: Default::default(),
            queries: vec![QueryTab::new("Query 1".into())],
            active_query: 0,
            info: Default::default(),
            tree_filter: String::new(),
            busy: 0,
            error: None,
            tree_selected: None,
            tree_rename: None,
        }
    }
}

impl App {
    /// Open a session for the given connection (background job).
    pub fn connect(&mut self, conn: mff_core::Connection) {
        let tx = self.jobs.sender();
        let conn_id = conn.id.clone();
        self.status = format!("connecting to {} …", conn.host_port());
        std::thread::spawn(move || {
            let result = Session::connect(&conn).map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::Connected { conn_id, result });
        });
    }

    pub fn disconnect(&mut self, conn_id: &str) {
        self.sessions.remove(conn_id);
        if self.active_conn.as_deref() == Some(conn_id) {
            self.active_conn = None;
        }
        self.status = "disconnected".into();
    }

    /// Load the database list for a session (background job).
    pub fn load_databases(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session.databases().map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::DatabasesLoaded { conn_id: id, result });
        });
    }

    /// Load the object lists of one database (background job).
    pub fn load_objects(&mut self, conn_id: &str, db: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.objects_loading = true;
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session.database_objects(&db).map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::ObjectsLoaded { conn_id: id, db, result });
        });
    }

    /// Browse table data with the current page/filter/sort (background job).
    pub fn load_data(&mut self, conn_id: &str, table: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let db = state.current_db.clone();
        let (offset, page_size, sort, filter) = {
            let d = &mut state.data;
            d.table = table.clone();
            d.loading = true;
            d.error = None;
            (d.offset, d.page_size, d.sort.clone(), d.filter_applied.clone())
        };
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let filter_opt = Some(filter.as_str()).filter(|f| !f.is_empty());
            let sort_ref = sort.as_ref().map(|(c, a)| (c.as_str(), *a));
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
                Ok(grid) => Ok((grid, total)),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(UiMsg::DataLoaded {
                conn_id: id,
                table,
                result,
            });
        });
    }

    /// Load the structure of one table (background job).
    pub fn load_schema(&mut self, conn_id: &str, table: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let db = state.current_db.clone();
        state.structure.table = table.clone();
        state.structure.loading = true;
        state.structure.error = None;
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session.table_schema(&db, &table).map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::SchemaLoaded {
                conn_id: id,
                table,
                result,
            });
        });
    }

    /// Run the statements of one query tab (background job).
    pub fn run_query(&mut self, conn_id: &str, tab_index: usize, code: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(tab) = state.queries.get_mut(tab_index) else {
            return;
        };
        let statements = mff_core::sqlsplit::split_statements(&code);
        if statements.is_empty() {
            tab.sections.clear();
            return;
        }
        tab.running = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut sections = Vec::new();
            for stmt in &statements {
                match session.run_statement(stmt) {
                    Ok(mff_core::session::ExecOutcome::Grid(grid)) => {
                        sections.push(QuerySection::Grid(grid))
                    }
                    Ok(mff_core::session::ExecOutcome::Affected {
                        affected,
                        last_insert_id,
                        info,
                    }) => sections.push(QuerySection::Affected {
                        affected,
                        last_insert_id,
                        info,
                    }),
                    Err(e) => {
                        sections.push(QuerySection::Error(e.to_string()));
                        break;
                    }
                }
            }
            let _ = tx.send(UiMsg::QueryDone {
                conn_id: id,
                tab: tab_index,
                sections,
                elapsed_ms: started.elapsed().as_millis(),
            });
        });
    }

    /// Load SHOW VARIABLES (background job).
    pub fn load_variables(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        if state.info.loading {
            return;
        }
        state.info.loading = true;
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session.variables().map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::VariablesLoaded { conn_id: id, result });
        });
    }

    /// Apply one drained job message to the matching session.
    pub fn apply_msg(&mut self, msg: UiMsg) {
        match msg {
            UiMsg::Status(s) => self.status = s,
            UiMsg::Connected { conn_id, result } => match result {
                Ok(session) => {
                    let version = session.info().version.clone();
                    let conn_name = self
                        .registry
                        .get(&conn_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "connection".into());
                    let current_db = self
                        .registry
                        .get(&conn_id)
                        .and_then(|c| c.database.clone())
                        .unwrap_or_default();
                    self.status = format!("connected to {conn_name}");
                    let state = SessionState::new(session, conn_name, version, current_db);
                    self.sessions.insert(conn_id.clone(), state);
                    self.active_conn = Some(conn_id.clone());
                    self.load_databases(&conn_id);
                }
                Err(e) => {
                    self.status = format!("connect failed: {e}");
                    self.connect_error = Some(e);
                }
            },
            UiMsg::DatabasesLoaded { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    match result {
                        Ok(dbs) => {
                            state.databases = dbs;
                            if state.current_db.is_empty() {
                                // Prefer a database the user can actually see.
                                state.current_db = state
                                    .databases
                                    .iter()
                                    .find(|d| {
                                        !matches!(
                                            d.to_lowercase().as_str(),
                                            "information_schema"
                                                | "mysql"
                                                | "performance_schema"
                                                | "sys"
                                        )
                                    })
                                    .cloned()
                                    .unwrap_or_default();
                            }
                            if !state.current_db.is_empty() {
                                let db = state.current_db.clone();
                                self.load_objects(&conn_id, db);
                            }
                        }
                        Err(e) => state.error = Some(format!("loading databases: {e}")),
                    }
                }
            }
            UiMsg::ObjectsLoaded { conn_id, db, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    state.objects_loading = false;
                    // A fresh object list invalidates any pending selection or
                    // inline rename.
                    state.tree_selected = None;
                    state.tree_rename = None;
                    if state.current_db == db {
                        match result {
                            Ok(objects) => state.objects = Some(objects),
                            Err(e) => state.error = Some(format!("loading objects: {e}")),
                        }
                    }
                }
            }
            UiMsg::DataLoaded {
                conn_id,
                table,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    if state.data.table == table {
                        state.data.loading = false;
                        match result {
                            Ok((grid, total)) => {
                                state.data.grid = Some(grid);
                                state.data.total = Some(total);
                            }
                            Err(e) => state.data.error = Some(e),
                        }
                    }
                }
            }
            UiMsg::SchemaLoaded {
                conn_id,
                table,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    if state.structure.table == table {
                        state.structure.loading = false;
                        match result {
                            Ok(schema) => state.structure.schema = Some(schema),
                            Err(e) => state.structure.error = Some(e),
                        }
                    }
                }
            }
            UiMsg::QueryDone {
                conn_id,
                tab,
                sections,
                elapsed_ms,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    if let Some(t) = state.queries.get_mut(tab) {
                        t.running = false;
                        t.sections = sections;
                        t.elapsed_ms = Some(elapsed_ms);
                    }
                }
            }
            UiMsg::VariablesLoaded { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    state.info.loading = false;
                    match result {
                        Ok(vars) => state.info.variables = Some(vars),
                        Err(e) => state.error = Some(format!("loading variables: {e}")),
                    }
                }
            }
            UiMsg::Renamed {
                conn_id,
                is_view: _,
                old,
                new,
                result,
            } => {
                let mut reload: Option<(String, String, Option<(String, bool)>)> = None;
                let mut status = String::new();
                let mut success = false;
                {
                    let Some(state) = self.sessions.get_mut(&conn_id) else {
                        return;
                    };
                    state.busy = state.busy.saturating_sub(1);
                    match result {
                        Ok(()) => {
                            success = true;
                            state.tree_rename = None;
                            // Follow the renamed object if it is open somewhere.
                            if state.data.table == old {
                                state.data.table = new.clone();
                                if state.view == ActiveView::Data {
                                    let filter = state.data.filter_applied.clone();
                                    let sort = state.data.sort.clone();
                                    reload = Some((new.clone(), filter, sort));
                                }
                            }
                            if state.structure.table == old {
                                state.structure.table = new.clone();
                                state.structure.schema = None;
                            }
                            status = format!("renamed {old} -> {new}");
                        }
                        Err(e) => {
                            // Keep the inline editor open so the name can be fixed.
                            if let Some(r) = state.tree_rename.as_mut() {
                                r.error = Some(e.clone());
                                r.just_started = false;
                            }
                            status = format!("rename failed: {e}");
                        }
                    }
                }
                self.status = status;
                if let Some((table, filter, sort)) = reload {
                    reopen_data(self, &conn_id, table, filter, sort);
                } else if success {
                    let db = self
                        .sessions
                        .get(&conn_id)
                        .map(|s| s.current_db.clone())
                        .unwrap_or_default();
                    self.load_objects(&conn_id, db);
                }
            }
        }
    }

    /// Render the workspace of the active session.
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

        // ── Top bar ────────────────────────────────────────────────────
        egui::TopBottomPanel::top("session_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("⏻ Disconnect").clicked() {
                    self.disconnect(&conn_id);
                    return;
                }
                ui.separator();
                ui.strong(&conn_name);
                if !version.is_empty() {
                    ui.label(format!("(server {version})"));
                }
                ui.separator();
                ui.label("database:");
                let dbs = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.databases.clone())
                    .unwrap_or_default();
                let mut db = current_db.clone();
                // egui's ComboBox response does not report `.changed()` for
                // clicks on the selectable items inside the popup, so detect
                // the selection by comparing the value before/after show_ui.
                let before = db.clone();
                egui::ComboBox::from_id_salt("db_combo")
                    .selected_text(if db.is_empty() { "—" } else { &db })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut db, String::new(), "—");
                        for d in &dbs {
                            ui.selectable_value(&mut db, d.clone(), d);
                        }
                    });
                if db != before {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.current_db = db.clone();
                        state.objects = None;
                    }
                    if !db.is_empty() {
                        self.load_objects(&conn_id, db);
                    }
                }
                if ui.button("⟳ Refresh").clicked() {
                    self.load_databases(&conn_id);
                }
            });
        });

        // ── Left: object browser ───────────────────────────────────────
        // The tree renders from a snapshot of the object lists; interactions
        // are collected and executed afterwards (borrow checker friendly).
        //
        // Explorer-like behaviour:
        //   * single click                       → select
        //   * double click                       → open the data view
        //   * click the selected item / F2 / context menu → inline rename
        //   * Enter or click elsewhere           → commit (RENAME TABLE)
        //   * Esc                                → cancel
        let mut actions: Vec<TreeAction> = Vec::new();
        let mut open_structure_table: Option<String> = None;
        let mut export_table: Option<String> = None;

        egui::SidePanel::left("object_browser")
            .resizable(true)
            .default_width(250.0)
            .show(ctx, |ui| {
                ui.heading("Objects");
                ui.add_space(4.0);
                let (objects, objects_loading, tree_selected, tree_rename) = {
                    let Some(state) = self.sessions.get_mut(&conn_id) else {
                        return;
                    };
                    ui.add(
                        egui::TextEdit::singleline(&mut state.tree_filter)
                            .hint_text("filter…")
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(4.0);
                    if state.objects_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.small("loading…");
                        });
                    }
                    (
                        state.objects.clone(),
                        state.objects_loading,
                        state.tree_selected.clone(),
                        state.tree_rename.clone(),
                    )
                };

                let Some(objects) = objects else {
                    if !objects_loading {
                        ui.label("no database selected");
                    }
                    return;
                };
                let filter = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.tree_filter.to_lowercase())
                    .unwrap_or_default();
                // The table currently being browsed is highlighted until the
                // user makes an explicit tree selection.
                let browsed = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.data.table.clone())
                    .unwrap_or_default();
                let matches =
                    |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
                let is_sel = |kind: ObjKind, name: &str| -> bool {
                    match tree_selected.as_ref() {
                        Some((k, n)) => *k == kind && n == name,
                        None => matches!(kind, ObjKind::Table | ObjKind::View) && browsed == name,
                    }
                };
                let renaming = |is_view: bool, name: &str| -> bool {
                    tree_rename
                        .as_ref()
                        .map(|r| r.is_view == is_view && r.old == name)
                        .unwrap_or(false)
                };

                // F2 renames the selected entry.
                if tree_rename.is_none() {
                    if let Some((kind, name)) = &tree_selected {
                        if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                            actions.push(TreeAction::StartRename(*kind, name.clone()));
                        }
                    }
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.collapsing(format!("Tables ({})", objects.tables.len()), |ui| {
                        for t in &objects.tables {
                            if !matches(&t.name) {
                                continue;
                            }
                            if renaming(false, &t.name) {
                                render_rename_edit(
                                    ui,
                                    tree_rename.as_ref().unwrap(),
                                    false,
                                    &mut actions,
                                );
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::table_icon(),
                                &t.name,
                                is_sel(ObjKind::Table, &t.name),
                            );
                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::Table, t.name.clone()));
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Show data").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::Open(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                }
                                if ui.button("Show structure").clicked() {
                                    ui.close();
                                    open_structure_table = Some(t.name.clone());
                                }
                                if ui.button("Rename").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::StartRename(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                }
                                if ui.button("Export…").clicked() {
                                    ui.close();
                                    export_table = Some(t.name.clone());
                                }
                            });
                        }
                    });
                    ui.collapsing(format!("Views ({})", objects.views.len()), |ui| {
                        for v in &objects.views {
                            if !matches(&v.name) {
                                continue;
                            }
                            if renaming(true, &v.name) {
                                render_rename_edit(
                                    ui,
                                    tree_rename.as_ref().unwrap(),
                                    true,
                                    &mut actions,
                                );
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::view_icon(),
                                &v.name,
                                is_sel(ObjKind::View, &v.name),
                            );
                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::View, v.name.clone()));
                            }
                        }
                    });
                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {
                        for r in &objects.routines {
                            if !matches(&r.name) {
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::routine_icon(),
                                &r.name,
                                is_sel(ObjKind::Routine, &r.name),
                            );
                            if resp.clicked() || resp.double_clicked() {
                                let kw = if r.kind.eq_ignore_ascii_case("FUNCTION") {
                                    "FUNCTION"
                                } else {
                                    "PROCEDURE"
                                };
                                let sql = format!(
                                    "SHOW CREATE {} {}",
                                    kw,
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &r.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Routine,
                                    r.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {
                        for t in &objects.triggers {
                            if !matches(&t.name) {
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::trigger_icon(),
                                &t.name,
                                is_sel(ObjKind::Trigger, &t.name),
                            );
                            if resp.clicked() || resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE TRIGGER {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &t.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Trigger,
                                    t.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {
                        for e in &objects.events {
                            if !matches(&e.name) {
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::event_icon(),
                                &e.name,
                                is_sel(ObjKind::Event, &e.name),
                            );
                            if resp.clicked() || resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE EVENT {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &e.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Event,
                                    e.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                });
            });

        // Deferred tree actions.
        for action in actions {
            match action {
                TreeAction::Select(kind, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((kind, name));
                    }
                }
                TreeAction::Open(kind, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((kind, name.clone()));
                    }
                    open_data(self, &conn_id, name);
                }
                TreeAction::ShowDef(kind, name, sql) => {
                    let idx = {
                        let Some(state) = self.sessions.get_mut(&conn_id) else {
                            continue;
                        };
                        state.tree_selected = Some((kind, name.clone()));
                        state.view = ActiveView::Query;
                        let title = format!("Def: {name}");
                        let idx = match state.queries.iter().position(|q| q.title == title) {
                            Some(i) => i,
                            None => {
                                state.queries.push(QueryTab::new(title));
                                state.queries.len() - 1
                            }
                        };
                        state.active_query = idx;
                        idx
                    };
                    self.run_query(&conn_id, idx, sql);
                }
                TreeAction::StartRename(kind, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if state.tree_rename.is_none() {
                            state.tree_selected = Some((kind, name.clone()));
                            state.tree_rename = Some(TreeRename {
                                is_view: kind == ObjKind::View,
                                old: name.clone(),
                                value: name,
                                just_started: true,
                                error: None,
                            });
                        }
                    }
                }
                TreeAction::RenameCancel => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_rename = None;
                    }
                }
                TreeAction::RenameValue(value) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if let Some(r) = state.tree_rename.as_mut() {
                            r.value = value;
                        }
                    }
                }
                TreeAction::RenameFocusTaken => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if let Some(r) = state.tree_rename.as_mut() {
                            r.just_started = false;
                        }
                    }
                }
                TreeAction::RenameCommit(is_view, old, new) => {
                    commit_rename(self, &conn_id, is_view, old, new);
                }
            }
        }

        if let Some(t) = open_structure_table {
            open_structure(self, &conn_id, t);
        }
        if let Some(t) = export_table {
            open_data(self, &conn_id, t.clone());
            if let Some(s) = self.sessions.get_mut(&conn_id) {
                s.data.export_open = true;
            }
        }

        // ── Center: tab area ───────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            let (view, busy, error) = {
                let state = self.sessions.get(&conn_id).unwrap();
                (state.view, state.busy, state.error.clone())
            };
            if let Some(err) = error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
            }
            ui.horizontal(|ui| {
                tab_button(ui, "Data", ActiveView::Data, view, &conn_id, self);
                tab_button(ui, "Structure", ActiveView::Structure, view, &conn_id, self);
                tab_button(ui, "Query", ActiveView::Query, view, &conn_id, self);
                tab_button(ui, "Info", ActiveView::Info, view, &conn_id, self);
                if busy > 0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spinner();
                        ui.small("working…");
                    });
                }
            });
            ui.separator();
            ui.add_space(4.0);

            match view {
                ActiveView::Data => self.show_data_view(ui, ctx, &conn_id),
                ActiveView::Structure => self.show_structure_view(ui, ctx, &conn_id),
                ActiveView::Query => self.show_query_view(ui, ctx, &conn_id),
                ActiveView::Info => self.show_info_view(ui, &conn_id),
            }
        });
    }
}

fn tab_button(
    ui: &mut egui::Ui,
    label: &str,
    this: ActiveView,
    current: ActiveView,
    conn_id: &str,
    app: &mut App,
) {
    let selected = this == current;
    if ui
        .selectable_label(selected, egui::RichText::new(label).strong())
        .clicked()
    {
        if let Some(state) = app.sessions.get_mut(conn_id) {
            state.view = this;
        }
        // Switching to Structure follows the table being browsed in the
        // data view, loading its schema if not already shown.
        if this == ActiveView::Structure {
            let (browsed, loaded) = app
                .sessions
                .get(conn_id)
                .map(|s| (s.data.table.clone(), s.structure.table.clone()))
                .unwrap_or_default();
            if !browsed.is_empty() && browsed != loaded {
                app.load_schema(conn_id, browsed);
            }
        }
    }
}

/// Open the data view of a table and (re)load its rows.
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
}

/// (Re)load the data view keeping the current filter and sort.
pub fn reopen_data(
    app: &mut App,
    conn_id: &str,
    table: String,
    filter: String,
    sort: Option<(String, bool)>,
) {
    let Some(state) = app.sessions.get_mut(conn_id) else {
        return;
    };
    state.view = ActiveView::Data;
    state.data.table = table.clone();
    state.data.filter_applied = filter;
    state.data.sort = sort;
    state.data.loading = true;
    state.data.error = None;
    app.load_data(conn_id, table);
}

/// Open the structure view of a table.
pub fn open_structure(app: &mut App, conn_id: &str, table: String) {
    {
        let Some(state) = app.sessions.get_mut(conn_id) else {
            return;
        };
        state.view = ActiveView::Structure;
    }
    app.load_schema(conn_id, table);
}

/// Deferred interactions collected while rendering the object browser.
enum TreeAction {
    Select(ObjKind, String),
    Open(ObjKind, String),
    ShowDef(ObjKind, String, String),
    StartRename(ObjKind, String),
    RenameValue(String),
    RenameCommit(bool, String, String),
    RenameCancel,
    RenameFocusTaken,
}

/// Inline rename editor for one tree entry (Explorer-style). Emits
/// commit/cancel actions; the edit keeps focus until Enter, Escape or a
/// click elsewhere.
fn render_rename_edit(
    ui: &mut egui::Ui,
    rename: &TreeRename,
    is_view: bool,
    actions: &mut Vec<TreeAction>,
) {
    let mut value = rename.value.clone();
    let resp = ui.add(
        egui::TextEdit::singleline(&mut value)
            .desired_width(ui.available_width().min(220.0)),
    );
    // Sync the typed text back, otherwise the edit is reset from the stale
    // state snapshot on the next frame and keystrokes are lost.
    if value != rename.value {
        actions.push(TreeAction::RenameValue(value.clone()));
    }
    if rename.just_started {
        resp.request_focus();
        actions.push(TreeAction::RenameFocusTaken);
    }
    if let Some(err) = &rename.error {
        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(TreeAction::RenameCancel);
    } else if resp.lost_focus() {
        // Enter blurs a single-line TextEdit, so a blur is either Enter or a
        // click elsewhere — both commit the rename (Explorer behaviour).
        actions.push(TreeAction::RenameCommit(is_view, rename.old.clone(), value));
    }
}

/// Validate and run `RENAME TABLE` for an inline rename.
fn commit_rename(app: &mut App, conn_id: &str, is_view: bool, old: String, new: String) {
    let new = new.trim().to_string();
    let Some(state) = app.sessions.get_mut(conn_id) else {
        return;
    };
    if new.is_empty() || new == old {
        state.tree_rename = None; // unchanged or empty → just leave rename mode
        return;
    }
    if new.contains('`') {
        if let Some(r) = state.tree_rename.as_mut() {
            r.error = Some("name must not contain a backtick".into());
            r.just_started = false;
        }
        return;
    }
    let session = state.session.clone();
    let db = state.current_db.clone();
    let tx = app.jobs.sender();
    let id = conn_id.to_string();
    state.busy += 1;
    std::thread::spawn(move || {
        let sql = format!(
            "RENAME TABLE {} TO {}",
            mff_core::sqlgen::quote_qualified(&[&db, &old]),
            mff_core::sqlgen::quote_qualified(&[&db, &new])
        );
        let result = session
            .run_statement(&sql)
            .map(|_| ())
            .map_err(|e| e.to_string());
        let _ = tx.send(UiMsg::Renamed {
            conn_id: id,
            is_view,
            old,
            new,
            result,
        });
    });
}
