#!/usr/bin/env python3
"""GUI batch 3: app fields/routing for new dialogs, session menus
(edit definition, search, schema diff), designer index/FK UI, server
dashboard + grants wiring, autocomplete caret/Ctrl+Space, run selection
+ find/replace, multi-cell copy."""
import io

# ── app.rs: fields + routing (idempotent) ───────────────────────────────
p = "crates/mff-gui/src/app.rs"
s = io.open(p, encoding="utf-8").read()
if "schemadiff_dlg" not in s:
    old = """    pub compare_dlg: Option<crate::compare_dlg::CompareDlg>,
    pub compare_report: Option<crate::compare_dlg::CompareReportUi>,"""
    new = """    pub compare_dlg: Option<crate::compare_dlg::CompareDlg>,
    pub compare_report: Option<crate::compare_dlg::CompareReportUi>,
    pub schemadiff_dlg: Option<crate::schemadiff_dlg::SchemaDiffDlg>,
    pub schemadiff_result: Option<crate::schemadiff_dlg::SchemaDiffResultUi>,
    pub search_dlg: Option<crate::search_dlg::SearchDlg>,"""
    assert old in s, "struct"
    s = s.replace(old, new)
    old = """            compare_dlg: None,
            compare_report: None,
            app_state,"""
    new = """            compare_dlg: None,
            compare_report: None,
            schemadiff_dlg: None,
            schemadiff_result: None,
            search_dlg: None,
            app_state,"""
    assert old in s, "ctor"
    s = s.replace(old, new)
    old = """        // Compare dialog + report.
        if self.compare_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_compare_dialog(ctx, &id);
                } else {
                    self.compare_dlg = None;
                }
            }
        }
        if self.compare_report.is_some() {
            self.show_compare_report(ctx);
        }"""
    new = old + """

        // Schema diff + search.
        if self.schemadiff_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_schema_diff_dialog(ctx, &id);
                } else {
                    self.schemadiff_dlg = None;
                }
            }
        }
        if self.schemadiff_result.is_some() {
            self.show_schema_diff_result(ctx);
        }
        if self.search_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_search_dialog(ctx, &id);
                } else {
                    self.search_dlg = None;
                }
            }
        }

        // GRANT editor (server tab).
        if self
            .sessions
            .get(self.active_conn.as_deref().unwrap_or(""))
            .map(|s| s.server.grants_editor.is_some())
            .unwrap_or(false)
        {
            let id = self.active_conn.clone().unwrap();
            self.show_grants_editor(ctx, &id);
        }"""
    assert old in s, "routing"
    s = s.replace(old, new)
    io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("app.rs ok")

# ── session_view.rs: menu additions + Ctrl+F shortcut + dashboard ──────
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

# Ctrl+F opens search
if "open_search(&conn_id)" not in s:
    old = """        if ctx.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.ctrl) {"""
    new = """        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.ctrl) {
            self.open_search(&conn_id);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.ctrl) {"""
    assert old in s, "ctrl+f"
    s = s.replace(old, new)

# menu items: Diff schema + search button in tree
old = """                    if ui.button(t(lang, "Compare table with…")).clicked() {
                        ui.close();
                        self.open_compare_dialog(&conn_id);
                    }
                });"""
new = """                    if ui.button(t(lang, "Compare table with…")).clicked() {
                        ui.close();
                        self.open_compare_dialog(&conn_id);
                    }
                    if ui.button(t(lang, "Diff schema with…")).clicked() {
                        ui.close();
                        self.open_schema_diff(&conn_id);
                    }
                });"""
assert old in s, "menu diff"
s = s.replace(old, new)

# 🔍 button next to the tree filter
old = """                    ui.add(
                        egui::TextEdit::singleline(&mut state.tree_filter)
                            .hint_text("filter…")
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(4.0);"""
new = """                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.tree_filter)
                                .hint_text("filter…")
                                .desired_width(ui.available_width() - 32.0),
                        );
                        if ui
                            .small_button("🔍")
                            .on_hover_text("Search objects across all databases (Ctrl+F)")
                            .clicked()
                        {
                            search_clicked = true;
                        }
                    });
                    ui.add_space(4.0);"""
assert old in s, "tree filter"
s = s.replace(old, new)

# declare search_clicked before the panel + handle after
old = """        let mut actions: Vec<TreeAction> = Vec::new();
        let mut open_structure_table: Option<String> = None;
        let mut export_table: Option<String> = None;"""
new = """        let mut actions: Vec<TreeAction> = Vec::new();
        let mut open_structure_table: Option<String> = None;
        let mut export_table: Option<String> = None;
        let mut search_clicked = false;"""
assert old in s, "search decl"
s = s.replace(old, new)

old = """        if let Some(t) = open_structure_table {
            open_structure(self, &conn_id, t);
        }"""
new = """        if search_clicked {
            self.open_search(&conn_id);
        }
        if let Some(t) = open_structure_table {
            open_structure(self, &conn_id, t);
        }"""
assert old in s, "search handle"
s = s.replace(old, new)

# context menus: "Edit definition…" for views/routines/triggers/events
old = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::ShowDef(
                                        ObjKind::View,
                                        v.name.clone(),
                                        format!(
                                            "SHOW CREATE VIEW {}",
                                            mff_core::sqlgen::quote_qualified(&[&current_db, &v.name])
                                        ),
                                    ));
                                }"""
new = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::ShowDef(
                                        ObjKind::View,
                                        v.name.clone(),
                                        format!(
                                            "SHOW CREATE VIEW {}",
                                            mff_core::sqlgen::quote_qualified(&[&current_db, &v.name])
                                        ),
                                    ));
                                }
                                if ui.button("Edit definition…").clicked() {
                                    ui.close();
                                    edit_def = Some(("VIEW".into(), v.name.clone()));
                                }"""
assert old in s, "view menu"
s = s.replace(old, new)

old = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
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
                                }"""
new = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
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
                                if ui.button("Edit definition…").clicked() {
                                    ui.close();
                                    let kind = if r.kind.eq_ignore_ascii_case("FUNCTION") {
                                        "FUNCTION"
                                    } else {
                                        "PROCEDURE"
                                    };
                                    edit_def = Some((kind.into(), r.name.clone()));
                                }"""
assert old in s, "routine menu"
s = s.replace(old, new)

old = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
                                    let sql = format!(
                                        "SHOW CREATE TRIGGER {}",
                                        mff_core::sqlgen::quote_qualified(&[&current_db, &t.name])
                                    );
                                    actions.push(TreeAction::ShowDef(
                                        ObjKind::Trigger,
                                        t.name.clone(),
                                        sql,
                                    ));
                                }"""
new = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
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
                                if ui.button("Edit definition…").clicked() {
                                    ui.close();
                                    edit_def = Some(("TRIGGER".into(), t.name.clone()));
                                }"""
assert old in s, "trigger menu"
s = s.replace(old, new)

old = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
                                    let sql = format!(
                                        "SHOW CREATE EVENT {}",
                                        mff_core::sqlgen::quote_qualified(&[&current_db, &e.name])
                                    );
                                    actions.push(TreeAction::ShowDef(
                                        ObjKind::Event,
                                        e.name.clone(),
                                        sql,
                                    ));
                                }"""
new = """                            resp.context_menu(|ui| {
                                if ui.button("Open definition").clicked() {
                                    ui.close();
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
                                if ui.button("Edit definition…").clicked() {
                                    ui.close();
                                    edit_def = Some(("EVENT".into(), e.name.clone()));
                                }"""
assert old in s, "event menu"
s = s.replace(old, new)

# declare edit_def + handle
old = """        let mut search_clicked = false;"""
new = """        let mut search_clicked = false;
        let mut edit_def: Option<(String, String)> = None;"""
assert old in s
s = s.replace(old, new)

old = """        if search_clicked {
            self.open_search(&conn_id);
        }"""
new = """        if search_clicked {
            self.open_search(&conn_id);
        }
        if let Some((kind, name)) = edit_def {
            self.load_definition(&conn_id, kind, name);
        }"""
assert old in s, "edit_def handle"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("session_view ok")
