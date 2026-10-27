#!/usr/bin/env python3
"""Make Routines/Triggers/Events selectable in the object browser: single
click selects, double click opens a query tab with their SHOW CREATE …
definition (auto-run)."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()

# 1. ObjKind enum next to ActiveView.
anchor = """#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {"""
assert anchor in src
src = src.replace(anchor, """/// Kind of object selected in the tree (drives the highlight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjKind {
    Table,
    View,
    Routine,
    Trigger,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {""", 1)

# 2. tree_selected type.
src = src.replace(
    "    /// Explorer-style tree selection: (is_view, name).\n    pub tree_selected: Option<(bool, String)>,",
    "    /// Explorer-style tree selection.\n    pub tree_selected: Option<(ObjKind, String)>,",
    1,
)

# 3. TreeAction variants: Open/Select carry ObjKind, add ShowDef.
old_actions = """enum TreeAction {
    Select(bool, String),
    Open(bool, String),
    StartRename(bool, String),
    RenameValue(String),
    RenameCommit(bool, String, String),
    RenameCancel,
    RenameFocusTaken,
}"""
assert old_actions in src
src = src.replace(old_actions, """enum TreeAction {
    Select(ObjKind, String),
    Open(ObjKind, String),
    ShowDef(ObjKind, String, String),
    StartRename(bool, String),
    RenameValue(String),
    RenameCommit(bool, String, String),
    RenameCancel,
    RenameFocusTaken,
}""", 1)

# F2 handler: tree_selected now carries ObjKind.
old_f2 = """                if tree_rename.is_none() {
                    if let Some((is_view, name)) = &tree_selected {
                        if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                            actions.push(TreeAction::StartRename(*is_view, name.clone()));
                        }
                    }
                }"""
assert old_f2 in src
src = src.replace(old_f2, """                if tree_rename.is_none() {
                    if let Some((kind, name)) = &tree_selected {
                        if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                            let is_view = matches!(kind, ObjKind::View);
                            actions.push(TreeAction::StartRename(is_view, name.clone()));
                        }
                    }
                }""", 1)

# 4. is_sel closure signature.
old_sel = """                let is_sel = |is_view: bool, name: &str| -> bool {
                    match tree_selected.as_ref() {
                        Some((v, n)) => *v == is_view && n == name,
                        None => browsed == name,
                    }
                };"""
assert old_sel in src
src = src.replace(old_sel, """                let is_sel = |kind: ObjKind, name: &str| -> bool {
                    match tree_selected.as_ref() {
                        Some((k, n)) => *k == kind && n == name,
                        None => matches!(kind, ObjKind::Table | ObjKind::View) && browsed == name,
                    }
                };""", 1)

# 5. Tables loop actions.
old_tbl = """                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(false, t.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(false, &t.name) {
                                    actions.push(TreeAction::StartRename(false, t.name.clone()));
                                } else {
                                    actions.push(TreeAction::Select(false, t.name.clone()));
                                }
                            }"""
assert old_tbl in src
src = src.replace(old_tbl, """                            if resp.double_clicked() {
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
                            }""", 1)

old_menu = """                                if ui.button("Show data").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::Open(false, t.name.clone()));
                                }"""
assert old_menu in src
src = src.replace(old_menu, """                                if ui.button("Show data").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::Open(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                }""", 1)

old_menu2 = """                                if ui.button("Rename").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::StartRename(false, t.name.clone()));
                                }"""
assert old_menu2 in src
src = src.replace(old_menu2, """                                if ui.button("Rename").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::StartRename(
                                        ObjKind::Table,
                                        t.name.clone(),
                                    ));
                                }""", 1)

old_tbl2 = """                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::table_icon(),
                                &t.name,
                                is_sel(false, &t.name),
                            );"""
assert old_tbl2 in src
src = src.replace(old_tbl2, """                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::table_icon(),
                                &t.name,
                                is_sel(ObjKind::Table, &t.name),
                            );""", 1)

# 6. Views loop.
old_view = """                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(true, v.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(true, &v.name) {
                                    actions.push(TreeAction::StartRename(true, v.name.clone()));
                                } else {
                                    actions.push(TreeAction::Select(true, v.name.clone()));
                                }
                            }"""
assert old_view in src
src = src.replace(old_view, """                            if resp.double_clicked() {
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
                            }""", 1)

old_view2 = """                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::view_icon(),
                                &v.name,
                                is_sel(true, &v.name),
                            );"""
assert old_view2 in src
src = src.replace(old_view2, """                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::view_icon(),
                                &v.name,
                                is_sel(ObjKind::View, &v.name),
                            );""", 1)

# 7. Routines loop: selectable + double-click shows definition.
old_routines = """                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {
                        for r in &objects.routines {
                            if !matches(&r.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::routine_icon(),
                                &r.name,
                            );
                        }
                    });"""
assert old_routines in src
src = src.replace(old_routines, """                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {
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
                            if resp.double_clicked() {
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
                            }
                        }
                    });""", 1)

# 8. Triggers loop.
old_triggers = """                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {
                        for t in &objects.triggers {
                            if !matches(&t.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::trigger_icon(),
                                &t.name,
                            );
                        }
                    });"""
assert old_triggers in src
src = src.replace(old_triggers, """                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {
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
                            if resp.double_clicked() {
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
                            }
                        }
                    });""", 1)

# 9. Events loop.
old_events = """                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {
                        for e in &objects.events {
                            if !matches(&e.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::event_icon(),
                                &e.name,
                            );
                        }
                    });"""
assert old_events in src
src = src.replace(old_events, """                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {
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
                            if resp.double_clicked() {
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
                            }
                        }
                    });""", 1)

# 10. Deferred actions: Open/Select carry ObjKind, StartRename maps to
#     is_view, add ShowDef handling.
old_def = """                TreeAction::Select(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((is_view, name));
                    }
                }
                TreeAction::Open(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((is_view, name.clone()));
                    }
                    open_data(self, &conn_id, name);
                }
                TreeAction::StartRename(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if state.tree_rename.is_none() {
                            state.tree_selected = Some((is_view, name.clone()));
                            state.tree_rename = Some(TreeRename {
                                is_view,
                                old: name.clone(),
                                value: name,
                                just_started: true,
                                error: None,
                            });
                        }
                    }
                }"""
assert old_def in src
src = src.replace(old_def, """                TreeAction::Select(kind, name) => {
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
                        let idx = state.queries.len();
                        state.queries.push(QueryTab::new(format!("Def: {name}")));
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
                }""", 1)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("patched routines/triggers/events interactions")
