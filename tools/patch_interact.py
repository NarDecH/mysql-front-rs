#!/usr/bin/env python3
"""Simplify tree interactions: single click opens (data for tables/views,
definition for routines/triggers/events); rename only via F2/context menu;
ShowDef reuses an existing 'Def: name' tab."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()

def rep(old, new, tag):
    global src
    assert old in src, f"pattern not found: {tag}"
    src = src.replace(old, new, 1)

# 1. Tables: single click opens data.
rep("""                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::Table, t.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(ObjKind::Table, &t.name) {
                                    actions.push(TreeAction::StartRename(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                } else {
                                    actions.push(TreeAction::Select(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                }
                            }""",
    """                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::Table, t.name.clone()));
                            }""",
    "tables")

# 2. Views: single click opens data.
rep("""                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::View, v.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(ObjKind::View, &v.name) {
                                    actions.push(TreeAction::StartRename(
                                        ObjKind::View,
                                        v.name.clone(),
                                    ));
                                } else {
                                    actions.push(TreeAction::Select(
                                        ObjKind::View,
                                        v.name.clone(),
                                    ));
                                }
                            }""",
    """                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::View, v.name.clone()));
                            }""",
    "views")

# 3. Routines: single click shows definition.
rep("""                            if resp.double_clicked() {
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
                            } else if resp.clicked() {
                                actions.push(TreeAction::Select(ObjKind::Routine, r.name.clone()));
                            }""",
    """                            if resp.clicked() || resp.double_clicked() {
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
                            }""",
    "routines")

# 4. Triggers: single click shows definition.
rep("""                            if resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE TRIGGER {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &t.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Trigger,
                                    t.name.clone(),
                                    sql,
                                ));
                            } else if resp.clicked() {
                                actions.push(TreeAction::Select(ObjKind::Trigger, t.name.clone()));
                            }""",
    """                            if resp.clicked() || resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE TRIGGER {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &t.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Trigger,
                                    t.name.clone(),
                                    sql,
                                ));
                            }""",
    "triggers")

# 5. Events: single click shows definition.
rep("""                            if resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE EVENT {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &e.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Event,
                                    e.name.clone(),
                                    sql,
                                ));
                            } else if resp.clicked() {
                                actions.push(TreeAction::Select(ObjKind::Event, e.name.clone()));
                            }""",
    """                            if resp.clicked() || resp.double_clicked() {
                                let sql = format!(
                                    "SHOW CREATE EVENT {}",
                                    mff_core::sqlgen::quote_qualified(&[&current_db, &e.name])
                                );
                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Event,
                                    e.name.clone(),
                                    sql,
                                ));
                            }""",
    "events")

# 6. ShowDef: reuse an existing "Def: name" tab.
rep("""                TreeAction::ShowDef(kind, name, sql) => {
                    let idx = {
                        let Some(state) = self.sessions.get_mut(&conn_id) else {
                            continue;
                        };
                        state.tree_selected = Some((kind, name.clone()));
                        state.view = ActiveView::Query;
                        let idx = state.queries.len();
                        state.queries.push(QueryTab::new(format!("Def: {name}")));
                        state.active_query = idx;
                        idx
                    };
                    self.run_query(&conn_id, idx, sql);
                }""",
    """                TreeAction::ShowDef(kind, name, sql) => {
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
                }""",
    "showdef")

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("interaction simplification applied")
