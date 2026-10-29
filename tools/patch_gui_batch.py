#!/usr/bin/env python3
"""GUI batch: wire schemadiff + search dialogs, jobs messages, GRANT
editor, server dashboard, edit-def menu, designer index/FK UI,
autocomplete at caret + Ctrl+Space, run selection + find/replace,
multi-cell copy."""
import io

# ── main.rs: modules ────────────────────────────────────────────────────
p = "crates/mff-gui/src/main.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("mod server_view;", "mod schema_diff_missing;\nmod schemadiff_dlg;\nmod search_dlg;\nmod server_view;")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
# remove the stray placeholder module
p = "crates/mff-gui/src/main.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("mod schema_diff_missing;\n", "")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── jobs.rs: new messages ───────────────────────────────────────────────
p = "crates/mff-gui/src/jobs.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// The table designer's ALTER statements finished.
    DesignApplied {"""
new = """    /// A schema diff finished.
    SchemaDiffDone {
        conn_id: String,
        result: std::result::Result<mff_core::schemadiff::SchemaDiff, String>,
    },
    /// Schema-diff ALTERs were applied to the target.
    SchemaDiffApplied {
        conn_id: String,
        target: String,
        result: std::result::Result<(), String>,
    },
    /// Global object search finished.
    SearchDone {
        conn_id: String,
        result: std::result::Result<Vec<(String, String, String, String)>, String>,
    },
    /// SHOW GRANTS for one user finished.
    GrantsLoaded {
        conn_id: String,
        user: String,
        host: String,
        result: std::result::Result<Vec<String>, String>,
    },
    /// GRANT/REVOKE statements finished.
    GrantsApplied {
        conn_id: String,
        result: std::result::Result<String, String>,
    },
    /// Server overview stats finished.
    ServerStatsLoaded {
        conn_id: String,
        result: std::result::Result<Vec<(String, String)>, String>,
    },
    /// An object definition (for editing) finished loading.
    DefinitionLoaded {
        conn_id: String,
        kind: String,
        name: String,
        result: std::result::Result<String, String>,
    },
    /// The table designer's ALTER statements finished.
    DesignApplied {"""
assert old in s, "jobs"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("jobs ok")
