#!/usr/bin/env python3
"""Rewrite apply_msg in session_view.rs cleanly (duplicate arms had crept in
from successive patches)."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()

start = src.index("    /// Apply one drained job message to the matching session.")
end = src.index("    /// Render the workspace of the active session.")

new_fn = r'''    /// Apply one drained job message to the matching session.
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

'''

src = src[:start] + new_fn + src[end:]
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("apply_msg rewritten")
