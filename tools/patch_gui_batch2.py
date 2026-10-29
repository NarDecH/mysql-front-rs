#!/usr/bin/env python3
"""GUI batch 2: session_view apply_msg handlers + menu wiring + designer
index/FK UI + search/schemadiff app fields."""
import io

# ── session_view.rs: apply_msg handlers ─────────────────────────────────
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

old = """            UiMsg::Status(s) => self.status = s,"""
new = """            UiMsg::SchemaDiffDone { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                if let Some(dlg) = self.schemadiff_dlg.as_mut() {
                    dlg.running = false;
                }
                match result {
                    Ok(diff) => {
                        self.status = format!(
                            "{} ALTER statement(s) for {}.{}",
                            diff.alter.len(),
                            {
                                let _ = &diff;
                                "target"
                            },
                            "table"
                        );
                        self.status = format!("schema diff: {} ALTER(s)", diff.alter.len());
                        self.schemadiff_result =
                            Some(crate::schemadiff_dlg::SchemaDiffResultUi {
                                diff,
                                source_db: String::new(),
                                target_db: String::new(),
                                source_table: String::new(),
                                target_table: String::new(),
                                target_conn: String::new(),
                            });
                        // Fill display labels from the diff itself.
                        if let Some(res) = self.schemadiff_result.as_mut() {
                            // SchemaDiff has no db fields; the caller stored
                            // them via the dialog. We reconstruct below.
                            res.source_db = self
                                .schemadiff_dlg
                                .as_ref()
                                .map(|d| d.source_db.clone())
                                .unwrap_or_default();
                            res.target_db = self
                                .schemadiff_dlg
                                .as_ref()
                                .map(|d| d.target_db.clone())
                                .unwrap_or_default();
                            res.source_table = self
                                .schemadiff_dlg
                                .as_ref()
                                .map(|d| d.source_table.clone())
                                .unwrap_or_default();
                            res.target_table = self
                                .schemadiff_dlg
                                .as_ref()
                                .map(|d| d.target_table.clone())
                                .unwrap_or_default();
                            res.target_conn = self
                                .schemadiff_dlg
                                .as_ref()
                                .map(|d| d.target_conn.clone())
                                .unwrap_or_default();
                        }
                        self.schemadiff_dlg = None;
                    }
                    Err(e) => {
                        self.status = format!("schema diff failed: {e}");
                        if let Some(dlg) = self.schemadiff_dlg.as_mut() {
                            dlg.message = Some(format!("✗ {e}"));
                        }
                    }
                }
            }
            UiMsg::SchemaDiffApplied {
                conn_id,
                target,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                match &result {
                    Ok(()) => self.status = format!("schema applied to {target}"),
                    Err(e) => self.status = format!("schema apply failed: {e}"),
                }
            }
            UiMsg::SearchDone { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                if let Some(dlg) = self.search_dlg.as_mut() {
                    dlg.running = false;
                    dlg.done = true;
                    match result {
                        Ok(rows) => dlg.results = rows,
                        Err(e) => dlg.error = Some(e),
                    }
                }
            }
            UiMsg::GrantsLoaded {
                conn_id,
                user,
                host,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    match result {
                        Ok(grants) => {
                            let editor = crate::session_view::GrantsEditor {
                                user,
                                host,
                                selected: std::collections::HashSet::new(),
                                current_grants: grants,
                                running: false,
                                error: None,
                            };
                            state.server.grants_editor = Some(editor);
                        }
                        Err(e) => self.status = format!("show grants failed: {e}"),
                    }
                }
            }
            UiMsg::GrantsApplied { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    if let Some(ed) = state.server.grants_editor.as_mut() {
                        ed.running = false;
                        match &result {
                            Ok(msg) => {
                                ed.error = None;
                                self.status = msg.clone();
                                let user = ed.user.clone();
                                let host = ed.host.clone();
                                drop(state);
                                self.load_grants(&conn_id, user, host);
                            }
                            Err(e) => ed.error = Some(e.clone()),
                        }
                    }
                }
            }
            UiMsg::ServerStatsLoaded { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    state.server.stats.loading = false;
                    match result {
                        Ok(rows) => state.server.stats.rows = Some(rows),
                        Err(e) => state.server.error = Some(format!("stats: {e}")),
                    }
                }
            }
            UiMsg::DefinitionLoaded {
                conn_id,
                kind,
                name,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                    match result {
                        Ok(sql) => {
                            let title = format!("Edit: {name}");
                            let idx = match state.queries.iter().position(|q| q.title == title) {
                                Some(i) => i,
                                None => {
                                    state.queries.push(QueryTab::new(title));
                                    state.queries.len() - 1
                                }
                            };
                            state.active_query = idx;
                            state.view = ActiveView::Query;
                            if let Some(tab) = state.queries.get_mut(idx) {
                                tab.code = sql;
                            }
                            self.status = format!("{kind} {name} loaded for editing");
                        }
                        Err(e) => self.status = format!("load definition failed: {e}"),
                    }
                }
            }
            UiMsg::Status(s) => self.status = s,"""
assert old in s, "apply_msg"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("apply_msg ok")
