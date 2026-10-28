#!/usr/bin/env python3
"""Add the Import button to the data view toolbar."""
import io

p = "crates/mff-gui/src/data_view.rs"
s = io.open(p, encoding="utf-8").read()

old = """    Structure(String),
    Export,
    /// UPDATE one cell (`text` is the user-typed value; `NULL` → SQL NULL)."""
new = """    Structure(String),
    Export,
    Import,
    /// UPDATE one cell (`text` is the user-typed value; `NULL` → SQL NULL)."""
assert old in s, "enum"
s = s.replace(old, new)

old = """                if ui.button("⭳ Export…").clicked() {
                    actions.push(DataAction::Export);
                }
                ui.separator();"""
new = """                if ui.button("⭳ Export…").clicked() {
                    actions.push(DataAction::Export);
                }
                if ui
                    .button("⇪ Import…")
                    .on_hover_text("Load a CSV / JSON / SQL file into a table")
                    .clicked()
                {
                    actions.push(DataAction::Import);
                }
                ui.separator();"""
assert old in s, "toolbar"
s = s.replace(old, new)

old = """                DataAction::Export => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.ensure_schema(conn_id, table);
                    if let Some(s) = self.sessions.get_mut(conn_id) {
                        s.data.export_open = true;
                    }
                }"""
new = """                DataAction::Export => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.ensure_schema(conn_id, table);
                    if let Some(s) = self.sessions.get_mut(conn_id) {
                        s.data.export_open = true;
                    }
                }
                DataAction::Import => {
                    let table = self
                        .sessions
                        .get(conn_id)
                        .map(|s| s.data.table.clone())
                        .unwrap_or_default();
                    self.open_import_dialog(conn_id, table);
                }"""
assert old in s, "action"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("data_view.rs patched")
