#!/usr/bin/env python3
"""Finalize the schema-diff result flow: define SchemaDiffResultUi,
simplify the producer, and fix the result/apply functions."""
import io

# 1. schemadiff_dlg.rs: struct + rewritten functions
p = "crates/mff-gui/src/schemadiff_dlg.rs"
s = io.open(p, encoding="utf-8").read()

old = """impl App {
    pub fn open_schema_diff(&mut self, conn_id: &str) {"""
new = """/// A finished schema diff kept on `App` for the result window.
pub struct SchemaDiffResultUi {
    pub diff: mff_core::schemadiff::SchemaDiff,
}

impl App {
    pub fn open_schema_diff(&mut self, conn_id: &str) {"""
assert old in s, "struct"
s = s.replace(old, new)

# Rewrite show_schema_diff_result
start = s.find("    pub fn show_schema_diff_result(&mut self, ctx: &Context) {")
end = s.find("    fn apply_schema_diff(&mut self) {")
assert start > 0 and end > start, "fn bounds"
new_show = '''    pub fn show_schema_diff_result(&mut self, ctx: &Context) {
        let Some(res) = self.schemadiff_result.as_ref() else {
            return;
        };
        let diff = &res.diff;
        let mut open = true;
        let mut close = false;
        let mut apply = false;
        egui::Window::new("Schema diff result")
            .open(&mut open)
            .default_width(760.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                ui.weak(format!(
                    "source: {}.{} · target receives the ALTERs: {}.{}",
                    diff.source_db, diff.source_table, diff.target_db, diff.target_table
                ));
                ui.separator();
                if diff.alter.is_empty() {
                    ui.colored_label(
                        egui::Color32::from_rgb(120, 200, 120),
                        "✓ structures are identical",
                    );
                } else {
                    ui.strong(format!("{} ALTER statement(s):", diff.alter.len()));
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.monospace(diff.alter.join("\\n\\n"));
                        });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !diff.alter.is_empty(),
                            egui::Button::new(
                                egui::RichText::new("Apply to target")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(crate::theme::accent(ui.ctx()))
                            .min_size(egui::vec2(0.0, 24.0)),
                        )
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if close || !open {
            self.schemadiff_result = None;
        }
        if apply {
            self.apply_schema_diff();
        }
    }

'''
s = s[:start] + new_show + s[end:]

# Rewrite apply_schema_diff
start = s.find("    fn apply_schema_diff(&mut self) {")
end = s.find("}", s.find("std::thread::spawn", start))
# find the end of the function: the spawn block then closing braces; take up to the
# last "}\n}" after spawn — simpler: locate the closing of this impl by next "\n}\n" after spawn
tail_start = s.find("std::thread::spawn", start)
assert tail_start > 0
# find "\n    }\n}" pattern after spawn (end of fn + end of impl)
fn_end = s.find("\n    }\n}", tail_start)
assert fn_end > 0, "fn end"
new_apply = '''    fn apply_schema_diff(&mut self) {
        let Some(res) = self.schemadiff_result.clone() else {
            return;
        };
        let Some(state) = self.sessions.get_mut(&res.diff.target_conn) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = res.diff.target_conn.clone();
        let stmts = res.diff.alter.clone();
        let target_label = format!("{}.{}", res.diff.target_db, res.diff.target_table);
        state.busy += 1;
        self.status = "applying schema changes…".into();
        self.schemadiff_result = None;
        std::thread::spawn(move || {
            let mut result: std::result::Result<(), String> = Ok(());
            for r in session.run_script(&stmts, false) {
                if let Err(e) = r {
                    result = Err(e.to_string());
                    break;
                }
            }
            let _ = tx.send(crate::jobs::UiMsg::SchemaDiffApplied {
                conn_id: id,
                target: target_label,
                result,
            });
        });
    }
}'''
s = s[:start] + new_apply + s[fn_end + len("\n    }\n}"):] 
# ensure trailing newline
s = s.rstrip() + "\n"
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("schemadiff_dlg finalized")

# 2. session_view.rs: simplify SchemaDiffDone producer
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()
start = s.find("            UiMsg::SchemaDiffDone { conn_id, result } => {")
end = s.find("            UiMsg::SearchDone { conn_id, result } => {")
assert start > 0 and end > start, "handler bounds"
new_handler = '''            UiMsg::SchemaDiffDone { conn_id, result } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                if let Some(dlg) = self.schemadiff_dlg.as_mut() {
                    dlg.running = false;
                }
                match result {
                    Ok(diff) => {
                        self.status = format!("schema diff: {} ALTER(s)", diff.alter.len());
                        self.schemadiff_result =
                            Some(crate::schemadiff_dlg::SchemaDiffResultUi { diff });
                        self.schemadiff_dlg = None;
                    }
                    Err(e) => {
                        self.status = format!("schema diff failed: {e}");
                        if let Some(dlg) = self.schemadiff_dlg.as_mut() {
                            dlg.message = Some(format!("✗ {e}"));
                        }
                    }
                }
            }
'''
s = s[:start] + new_handler + s[end:]
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("producer simplified")
