#!/usr/bin/env python3
"""Remaining session_view.rs wiring for the database-rename dialog."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()


def rep(old, new, tag):
    global src
    assert old in src, f"pattern not found: {tag}"
    src = src.replace(old, new, 1)


# 1. Capture the combo response.
rep(
    """                let before = db.clone();
                egui::ComboBox::from_id_salt("db_combo")
                    .selected_text(if db.is_empty() { "—" } else { &db })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut db, String::new(), "—");
                        for d in &dbs {
                            ui.selectable_value(&mut db, d.clone(), d);
                        }
                    });""",
    """                let before = db.clone();
                let combo_resp = egui::ComboBox::from_id_salt("db_combo")
                    .selected_text(if db.is_empty() { "—" } else { &db })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut db, String::new(), "—");
                        for d in &dbs {
                            ui.selectable_value(&mut db, d.clone(), d);
                        }
                    });""",
    "combo-resp",
)

# 2. Context menu on the combo.
rep(
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
)

# 3. Local flag next to the deferred-actions vector.
rep(
    "        let mut actions: Vec<TreeAction> = Vec::new();",
    "        let mut open_db_rename: Option<String> = None;\n        let mut actions: Vec<TreeAction> = Vec::new();",
    "flag",
)

# 4. Open the dialog after the top bar.
rep(
    "        // ── Left: object browser ───────────────────────────────────────",
    """        if let Some(old) = open_db_rename {
            self.open_db_rename(old);
        }

        // ── Left: object browser ───────────────────────────────────────""",
    "open-dialog",
)

# 5. apply_msg: DbRenamed arm before Status.
rep(
    "            UiMsg::Status(s) => self.status = s,",
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
    "dbrenamed-arm",
)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("session_view wired")
