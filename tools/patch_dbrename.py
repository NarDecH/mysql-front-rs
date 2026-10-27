#!/usr/bin/env python3
"""Wire the database-rename dialog: module, app field, combo context menu,
apply_msg arm, UiMsg variant."""
import io


def patch(path, pairs):
    src = io.open(path, encoding="utf-8").read()
    for old, new, tag in pairs:
        assert old in src, f"{path}: pattern not found: {tag}"
        src = src.replace(old, new, 1)
    io.open(path, "w", encoding="utf-8", newline="\n").write(src)
    print(f"patched {path}")


patch(
    r"crates\mff-gui\src\main.rs",
    [("mod data_view;\n", "mod data_view;\nmod db_rename;\n", "mod")],
)

patch(
    r"crates\mff-gui\src\app.rs",
    [
        (
            "    pub theme: egui::Theme,\n    pub new_dlg: Option<crate::new_dialog::NewDialog>,\n}",
            "    pub theme: egui::Theme,\n    pub new_dlg: Option<crate::new_dialog::NewDialog>,\n    pub db_rename_dlg: Option<crate::db_rename::DbRenameDialog>,\n}",
            "field",
        ),
        (
            "            theme: egui::Theme::Light,\n            new_dlg: None,\n        }",
            "            theme: egui::Theme::Light,\n            new_dlg: None,\n            db_rename_dlg: None,\n        }",
            "init",
        ),
        (
            """        // "New object" modal (needs the session it belongs to).
        if self.new_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_new_dialog(ctx, &id);
                } else {
                    self.new_dlg = None;
                }
            }
        }""",
            """        // "New object" modal (needs the session it belongs to).
        if self.new_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_new_dialog(ctx, &id);
                } else {
                    self.new_dlg = None;
                }
            }
        }

        // "Rename database" modal.
        if self.db_rename_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_db_rename_dialog(ctx, &id);
                } else {
                    self.db_rename_dlg = None;
                }
            }
        }""",
            "routing",
        ),
    ],
)

patch(
    r"crates\mff-gui\src\jobs.rs",
    [
        (
            """    Created {
        conn_id: String,
        name: String,
        result: std::result::Result<(), String>,
    },""",
            """    Created {
        conn_id: String,
        name: String,
        result: std::result::Result<(), String>,
    },
    DbRenamed {
        conn_id: String,
        new_db: String,
        result: std::result::Result<(), String>,
    },""",
            "variant",
        )
    ],
)

patch(
    r"crates\mff-gui\src\session_view.rs",
    [
        # Combo response: attach a context menu with Rename database…
        (
            """                if db != before {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.current_db = db.clone();
                        state.objects = None;
                    }
                    if !db.is_empty() {
                        self.load_objects(&conn_id, db);
                    }
                }""",
            """                if db != before {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.current_db = db.clone();
                        state.objects = None;
                    }
                    if !db.is_empty() {
                        self.load_objects(&conn_id, db);
                    }
                }
                combo_resp.context_menu(|ui| {
                    if ui.button("Rename database…").clicked() {
                        ui.close();
                        open_db_rename = Some(current_db.clone());
                    }
                });""",
            "combo-menu",
        ),
        # Capture the combo response + a local flag for opening the dialog.
        (
            """                let mut db = current_db.clone();
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
                    });""",
            """                let mut db = current_db.clone();
                // egui's ComboBox response does not report `.changed()` for
                // clicks on the selectable items inside the popup, so detect
                // the selection by comparing the value before/after show_ui.
                let before = db.clone();
                let combo_resp = egui::ComboBox::from_id_salt("db_combo")
                    .selected_text(if db.is_empty() { "—" } else { &db })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut db, String::new(), "—");
                        for d in &dbs {
                            ui.selectable_value(&mut db, d.clone(), d);
                        }
                    });""",
            "combo-resp",
        ),
        # Local flag declaration next to the other tree locals.
        (
            """        let mut open_table: Option<String> = None;
            let mut open_structure_table: Option<String> = None;
            let mut export_table: Option<String> = None;""",
            """        let mut open_table: Option<String> = None;
            let mut open_structure_table: Option<String> = None;
            let mut export_table: Option<String> = None;""",
            "noop",  # placeholder replaced below if indentation differs
        ),
    ],
)

# The tree locals may use different indentation; add the flag regardless.
src = io.open(r"crates\mff-gui\src\session_view.rs", encoding="utf-8").read()
anchor = "        let mut actions: Vec<TreeAction> = Vec::new();"
assert anchor in src
src = src.replace(
    anchor,
    "        let mut open_db_rename: Option<String> = None;\n" + anchor,
    1,
)
io.open(r"crates\mff-gui\src\session_view.rs", "w", encoding="utf-8", newline="\n").write(src)
print("flag added")

patch(
    r"crates\mff-gui\src\session_view.rs",
    [
        # After the top-bar panel: open the dialog if the menu was used.
        (
            """        // ── Left: object browser ───────────────────────────────────────""",
            """        if let Some(old) = open_db_rename {
            self.open_db_rename(old);
        }

        // ── Left: object browser ───────────────────────────────────────""",
            "open-dialog",
        ),
        # apply_msg: DbRenamed arm.
        (
            """            UiMsg::Status(s) => self.status = s,""",
            """            UiMsg::DbRenamed {
                conn_id,
                new_db,
                result,
            } => {
                let Some(state) = self.sessions.get_mut(&conn_id) else {
                    return;
                };
                state.busy = state.busy.saturating_sub(1);
                match result {
                    Ok(()) => {
                        state.current_db = new_db.clone();
                        state.objects = None;
                        state.tree_selected = None;
                        state.tree_rename = None;
                        self.status = format!("database renamed to {new_db}");
                        self.load_objects(&conn_id, new_db);
                    }
                    Err(e) => {
                        state.error = Some(format!("rename database failed: {e}"));
                        self.status = format!("rename database failed: {e}");
                    }
                }
            }
            UiMsg::Status(s) => self.status = s,""",
            "created-arm",
        ),
    ],
)
print("all wired")
