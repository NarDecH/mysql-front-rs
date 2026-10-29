#!/usr/bin/env python3
"""Clean up the SchemaDiff result flow: the diff carries its own labels,
and the dialog stores source/target for the apply step."""
import io

# schemadiff core: add db/table fields to SchemaDiff
p = "crates/mff-core/src/schemadiff.rs"
s = io.open(p, encoding="utf-8").read()
old = """pub struct SchemaDiff {
    pub source: Table,
    pub target: Table,
    /// ALTER statements (possibly empty when structures match).
    pub alter: Vec<String>,
}"""
new = """pub struct SchemaDiff {
    pub source_db: String,
    pub source_table: String,
    pub target_db: String,
    pub target_table: String,
    pub source: Table,
    pub target: Table,
    /// ALTER statements (possibly empty when structures match).
    pub alter: Vec<String>,
}"""
assert old in s
s = s.replace(old, new)
old = """    let alter = build_alter(&target, &design)?;
    Ok(SchemaDiff {
        source,
        target,
        alter,
    })
}"""
new = """    let alter = build_alter(&target, &design)?;
    Ok(SchemaDiff {
        source_db: src_db.to_string(),
        source_table: src_table.to_string(),
        target_db: tgt_db.to_string(),
        target_table: tgt_table.to_string(),
        source,
        target,
        alter,
    })
}"""
assert old in s
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# schemadiff_dlg.rs: fix result UI struct + labels
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
assert old in s
s = s.replace(old, new)

old = """        std::thread::spawn(move || {
            let result = (|| {
                let tgt_session = target_session.ok_or("target session is gone")?;
                mff_core::schemadiff::diff_tables(
                    &session,
                    &src_db,
                    &src_table,
                    &tgt_session,
                    &tgt_db,
                    &tgt_table,
                )
                .map_err(|e| e.to_string())
            })();
            let _ = tx.send(crate::jobs::UiMsg::SchemaDiffDone {
                conn_id: id,
                result,
            });
        });"""
new = """        std::thread::spawn(move || {
            let result = (|| {
                let tgt_session = target_session.ok_or("target session is gone")?;
                mff_core::schemadiff::diff_tables(
                    &session,
                    &src_db,
                    &src_table,
                    &tgt_session,
                    &tgt_db,
                    &tgt_table,
                )
                .map_err(|e| e.to_string())
            })();
            let _ = tx.send(crate::jobs::UiMsg::SchemaDiffDone {
                conn_id: id,
                result,
            });
        });"""
assert old in s, "spawn indent"
s = s.replace(old, new)

# Result window: rewrite
start = s.find("    pub fn show_schema_diff_result(&mut self, ctx: &Context) {")
end = s.find("    fn apply_schema_diff(&mut self) {")
assert start > 0 and end > start
new_fn = '''    pub fn show_schema_diff_result(&mut self, ctx: &Context) {
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
s = s[:start] + new_fn + s[end:]

# apply_schema_diff: use res.diff fields
old = """    fn apply_schema_diff(&mut self) {
        let Some(diff) = self.schemadiff_result.clone() else {
            return;
        };
        let Some(state) = self.sessions.get_mut(&diff.target_conn) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = diff.target_conn.clone();
        let stmts = diff.alter.clone();
        let target_label = format!("{}.{}", diff.target_db, diff.target_table);
        state.busy += 1;
        self.status = "applying schema changes…".into();
        self.schemadiff_result = None;"""
new = """    fn apply_schema_diff(&mut self) {
        let Some(res) = self.schemadiff_result.clone() else {
            return;
        };
        let Some(state) = self.sessions.get_mut(&res.diff.target_conn.as_str()) else {
            return;
        };
        let session = state.session.clone();
        let tx = self.jobs.sender();
        let id = res.diff.target_conn.clone();
        let stmts = res.diff.alter.clone();
        let target_label = format!("{}.{}", res.diff.target_db, res.diff.target_table);
        state.busy += 1;
        self.status = "applying schema changes…".into();
        self.schemadiff_result = None;"""
assert old in s, "apply"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("schemadiff_dlg ok")
