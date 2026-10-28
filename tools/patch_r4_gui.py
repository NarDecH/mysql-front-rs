#!/usr/bin/env python3
"""Wire the table designer: jobs message, app field/routing, structure
view button, module declaration."""
import io

# ── jobs.rs ──────────────────────────────────────────────────────────────
p = "crates/mff-gui/src/jobs.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// A database restore finished.
    RestoreDone {
        conn_id: String,
        target: String,
        result: std::result::Result<usize, String>,
    },
    Status(String),
}"""
new = """    /// A database restore finished.
    RestoreDone {
        conn_id: String,
        target: String,
        result: std::result::Result<usize, String>,
    },
    /// The table designer's ALTER statements finished.
    DesignApplied {
        conn_id: String,
        old_name: String,
        new_name: String,
        result: std::result::Result<(), String>,
    },
    Status(String),
}"""
assert old in s, "jobs"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── main.rs ──────────────────────────────────────────────────────────────
p = "crates/mff-gui/src/main.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("mod structure_view;", "mod structure_view;\nmod table_designer;")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── app.rs ───────────────────────────────────────────────────────────────
p = "crates/mff-gui/src/app.rs"
s = io.open(p, encoding="utf-8").read()
old = """    pub restore_dlg: Option<crate::backup_dlg::RestoreDlg>,
    /// Persisted UI state (theme, last connection, query history, …).
    pub app_state: mff_core::AppState,
}"""
new = """    pub restore_dlg: Option<crate::backup_dlg::RestoreDlg>,
    pub designer: Option<crate::table_designer::DesignerState>,
    /// Persisted UI state (theme, last connection, query history, …).
    pub app_state: mff_core::AppState,
}"""
assert old in s, "app struct"
s = s.replace(old, new)

old = """            backup_dlg: None,
            restore_dlg: None,
            app_state,
        }
    }"""
new = """            backup_dlg: None,
            restore_dlg: None,
            designer: None,
            app_state,
        }
    }"""
assert old in s, "app init"
s = s.replace(old, new)

old = """        // "Restore database" modal.
        if self.restore_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_restore_dialog(ctx, &id);
                } else {
                    self.restore_dlg = None;
                }
            }
        }"""
new = """        // "Restore database" modal.
        if self.restore_dlg.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_restore_dialog(ctx, &id);
                } else {
                    self.restore_dlg = None;
                }
            }
        }

        // Table designer modal.
        if self.designer.is_some() {
            if let Some(id) = self.active_conn.clone() {
                if self.sessions.contains_key(&id) {
                    self.show_designer(ctx, &id);
                } else {
                    self.designer = None;
                }
            }
        }"""
assert old in s, "app routing"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── structure_view.rs: add the Edit structure button ────────────────────
p = "crates/mff-gui/src/structure_view.rs"
s = io.open(p, encoding="utf-8").read()
print("structure_view head:")
print(s[:1200])
