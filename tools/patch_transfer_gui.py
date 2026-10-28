#!/usr/bin/env python3
"""Wire the transfer dialog: main module, app field/routing, jobs variant,
theme helper."""
import io


def patch(path, pairs):
    src = io.open(path, encoding="utf-8").read()
    changed = False
    for old, new, tag in pairs:
        if new in src:
            print(f"{path}: {tag} already applied")
            continue
        assert old in src, f"{path}: pattern not found: {tag}"
        src = src.replace(old, new, 1)
        changed = True
    if changed:
        io.open(path, "w", encoding="utf-8", newline="\n").write(src)
        print(f"patched {path}")


patch(
    r"crates\mff-gui\src\main.rs",
    [("mod session_view;\n", "mod session_view;\nmod transfer;\n", "mod")],
)

patch(
    r"crates\mff-gui\src\jobs.rs",
    [
        (
            """    DbRenamed {
        conn_id: String,
        new_db: String,
        result: std::result::Result<(), String>,
    },""",
            """    DbRenamed {
        conn_id: String,
        new_db: String,
        result: std::result::Result<(), String>,
    },
    TransferDone {
        conn_id: String,
        text: String,
        ok: bool,
        target_same_server: bool,
        target_db: String,
    },""",
            "variant",
        )
    ],
)

patch(
    r"crates\mff-gui\src\app.rs",
    [
        (
            "    pub db_rename_dlg: Option<crate::db_rename::DbRenameDialog>,\n}",
            "    pub db_rename_dlg: Option<crate::db_rename::DbRenameDialog>,\n    pub transfer_dlg: Option<crate::transfer::TransferDialog>,\n}",
            "field",
        ),
        (
            "            new_dlg: None,\n            db_rename_dlg: None,\n        }",
            "            new_dlg: None,\n            db_rename_dlg: None,\n            transfer_dlg: None,\n        }",
            "init",
        ),
        (
            """        // "Rename database" modal.
        if self.db_rename_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_db_rename_dialog(ctx, &id);
                } else {
                    self.db_rename_dlg = None;
                }
            }
        }""",
            """        // "Rename database" modal.
        if self.db_rename_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_db_rename_dialog(ctx, &id);
                } else {
                    self.db_rename_dlg = None;
                }
            }
        }

        // "Data transfer" modal.
        if self.transfer_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_transfer_dialog(ctx, &id);
                } else {
                    self.transfer_dlg = None;
                }
            }
        }""",
            "routing",
        ),
    ],
)

patch(
    r"crates\mff-gui\src\theme.rs",
    [
        (
            """pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let c = accent(ui.ctx());
    ui.add(
        egui::Button::new(egui::RichText::new(label).strong().color(Color32::WHITE))
            .fill(c)
            .min_size(egui::vec2(0.0, 24.0)),
    )
}""",
            """pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(primary_button_widget(ui, label))
}

/// A primary-styled egui Button widget (for add_enabled call sites).
pub fn primary_button_widget(ui: &egui::Ui, label: &str) -> egui::Button<'static> {
    let c = accent(ui.ctx());
    egui::Button::new(egui::RichText::new(label.to_string()).strong().color(Color32::WHITE))
        .fill(c)
        .min_size(egui::vec2(0.0, 24.0))
}""",
            "theme-helper",
        )
    ],
)

# 5. session_view.rs: add "Transfer data…" to the ⋯ menu.
p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()
old = """                ui.menu_button("⋯", |ui| {
                    if ui.button("Rename database…").clicked() {
                        ui.close();
                        open_db_rename = Some(current_db.clone());
                    }
                });"""
assert old in src, "session_view: menu"
src = src.replace(old, """                ui.menu_button("⋯", |ui| {
                    if ui.button("Rename database…").clicked() {
                        ui.close();
                        open_db_rename = Some(current_db.clone());
                    }
                    if ui.button("Transfer data…").clicked() {
                        ui.close();
                        open_transfer = true;
                    }
                });""", 1)
old2 = "        let mut actions: Vec<TreeAction> = Vec::new();"
assert old2 in src, "session_view: flags"
src = src.replace(old2, "        let mut open_transfer = false;\n" + old2, 1)
old3 = """        if let Some(old) = open_db_rename {
            self.open_db_rename(old);
        }

        // ── Left: object browser ───────────────────────────────────────"""
assert old3 in src, "session_view: open"
src = src.replace(old3, """        if let Some(old) = open_db_rename {
            self.open_db_rename(old);
        }
        if open_transfer {
            self.open_transfer(&conn_id);
        }

        // ── Left: object browser ───────────────────────────────────────""", 1)
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("session_view patched")
