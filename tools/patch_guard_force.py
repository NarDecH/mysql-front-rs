#!/usr/bin/env python3
"""Add a `force` flag to run_query so 'Run anyway' bypasses the re-check
(previously it re-flagged the same statement and looped forever)."""
import io

p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

old = """    /// Run the statements of one query tab (background job).
    pub fn run_query(&mut self, conn_id: &str, tab_index: usize, code: String) {"""
new = """    /// Run the statements of one query tab (background job). `force`
    /// skips the missing-WHERE confirmation (used by "Run anyway").
    pub fn run_query(&mut self, conn_id: &str, tab_index: usize, code: String, force: bool) {"""
assert old in s, "signature"
s = s.replace(old, new)

old = """        if !flagged.is_empty() {
            state.confirm_no_where = Some(flagged);
            state.pending_run = Some((tab_index, code));
            return;
        }
        tab.running = true;"""
new = """        if !flagged.is_empty() && !force {
            state.confirm_no_where = Some(flagged);
            state.pending_run = Some((tab_index, code));
            return;
        }
        state.confirm_no_where = None;
        state.pending_run = None;
        tab.running = true;"""
assert old in s, "guard"
s = s.replace(old, new)

# run_anyway call site
old = """                if let Some((tab_index, code)) = pending {
                    self.run_query(&conn_id, tab_index, code);
                }"""
new = """                if let Some((tab_index, code)) = pending {
                    self.run_query(&conn_id, tab_index, code, true);
                }"""
assert old in s, "run_anyway"
s = s.replace(old, new)

# ShowDef call site
old = """                    self.run_query(&conn_id, idx, sql);"""
new = """                    self.run_query(&conn_id, idx, sql, false);"""
assert old in s, "showdef"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# query_view.rs call site
p = "crates/mff-gui/src/query_view.rs"
s = io.open(p, encoding="utf-8").read()
old = """        if (run_clicked || ctrl_enter) && !running {
            self.run_query(conn_id, active, code);
        }"""
new = """        if (run_clicked || ctrl_enter) && !running {
            self.run_query(conn_id, active, code, false);
        }"""
assert old in s, "query_view"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("force flag wired")
