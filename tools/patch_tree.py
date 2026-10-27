#!/usr/bin/env python3
"""Replace the object-browser tree block in session_view.rs with the
Explorer-style interaction version (select / double-click open / inline
rename)."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()

start = src.index("        // ── Left: object browser")
end_marker = "        if let Some(t) = open_structure_table {"
end = src.index(end_marker)
old_len = end - start

new_block = '''        // ── Left: object browser ───────────────────────────────────────
        // The tree renders from a snapshot of the object lists; interactions
        // are collected and executed afterwards (borrow checker friendly).
        //
        // Explorer-like behaviour:
        //   * single click                       → select
        //   * double click                       → open the data view
        //   * click the selected item / F2 / context menu → inline rename
        //   * Enter or click elsewhere           → commit (RENAME TABLE)
        //   * Esc                                → cancel
        enum TreeAction {
            Select(bool, String),
            Open(bool, String),
            StartRename(bool, String),
            RenameCommit(bool, String, String),
            RenameCancel,
            RenameFocusTaken,
        }
        let mut actions: Vec<TreeAction> = Vec::new();

        egui::SidePanel::left("object_browser")
            .resizable(true)
            .default_width(250.0)
            .show(ctx, |ui| {
                ui.heading("Objects");
                ui.add_space(4.0);
                let (objects, objects_loading, tree_selected, tree_rename) = {
                    let Some(state) = self.sessions.get_mut(&conn_id) else {
                        return;
                    };
                    ui.add(
                        egui::TextEdit::singleline(&mut state.tree_filter)
                            .hint_text("filter…")
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(4.0);
                    if state.objects_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.small("loading…");
                        });
                    }
                    (
                        state.objects.clone(),
                        state.objects_loading,
                        state.tree_selected.clone(),
                        state.tree_rename.clone(),
                    )
                };

                let Some(objects) = objects else {
                    if !objects_loading {
                        ui.label("no database selected");
                    }
                    return;
                };
                let filter = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.tree_filter.to_lowercase())
                    .unwrap_or_default();
                // The table currently being browsed is highlighted until the
                // user makes an explicit tree selection.
                let browsed = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.data.table.clone())
                    .unwrap_or_default();
                let matches =
                    |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
                let is_sel = |is_view: bool, name: &str| -> bool {
                    match tree_selected.as_ref() {
                        Some((v, n)) => *v == is_view && n == name,
                        None => browsed == name,
                    }
                };
                let renaming = |is_view: bool, name: &str| -> bool {
                    tree_rename
                        .as_ref()
                        .map(|r| r.is_view == is_view && r.old == name)
                        .unwrap_or(false)
                };

                // F2 renames the selected entry.
                if tree_rename.is_none() {
                    if let Some((is_view, name)) = &tree_selected {
                        if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                            actions.push(TreeAction::StartRename(*is_view, name.clone()));
                        }
                    }
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.collapsing(format!("Tables ({})", objects.tables.len()), |ui| {
                        for t in &objects.tables {
                            if !matches(&t.name) {
                                continue;
                            }
                            if renaming(false, &t.name) {
                                render_rename_edit(
                                    ui,
                                    tree_rename.as_ref().unwrap(),
                                    false,
                                    &mut actions,
                                );
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::table_icon(),
                                &t.name,
                                is_sel(false, &t.name),
                            );
                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(false, t.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(false, &t.name) {
                                    actions.push(TreeAction::StartRename(false, t.name.clone()));
                                } else {
                                    actions.push(TreeAction::Select(false, t.name.clone()));
                                }
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Show data").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::Open(false, t.name.clone()));
                                }
                                if ui.button("Show structure").clicked() {
                                    ui.close();
                                    open_structure_table = Some(t.name.clone());
                                }
                                if ui.button("Rename").clicked() {
                                    ui.close();
                                    actions.push(TreeAction::StartRename(false, t.name.clone()));
                                }
                                if ui.button("Export…").clicked() {
                                    ui.close();
                                    export_table = Some(t.name.clone());
                                }
                            });
                        }
                    });
                    ui.collapsing(format!("Views ({})", objects.views.len()), |ui| {
                        for v in &objects.views {
                            if !matches(&v.name) {
                                continue;
                            }
                            if renaming(true, &v.name) {
                                render_rename_edit(
                                    ui,
                                    tree_rename.as_ref().unwrap(),
                                    true,
                                    &mut actions,
                                );
                                continue;
                            }
                            let resp = crate::theme::icon_label_sel(
                                ui,
                                &crate::theme::view_icon(),
                                &v.name,
                                is_sel(true, &v.name),
                            );
                            if resp.double_clicked() {
                                actions.push(TreeAction::Open(true, v.name.clone()));
                            } else if resp.clicked() {
                                if is_sel(true, &v.name) {
                                    actions.push(TreeAction::StartRename(true, v.name.clone()));
                                } else {
                                    actions.push(TreeAction::Select(true, v.name.clone()));
                                }
                            }
                        }
                    });
                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {
                        for r in &objects.routines {
                            if !matches(&r.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::routine_icon(),
                                &r.name,
                            );
                        }
                    });
                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {
                        for t in &objects.triggers {
                            if !matches(&t.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::trigger_icon(),
                                &t.name,
                            );
                        }
                    });
                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {
                        for e in &objects.events {
                            if !matches(&e.name) {
                                continue;
                            }
                            crate::theme::icon_label(
                                ui,
                                &crate::theme::event_icon(),
                                &e.name,
                            );
                        }
                    });
                });
            });

        // Deferred tree actions.
        for action in actions {
            match action {
                TreeAction::Select(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((is_view, name));
                    }
                }
                TreeAction::Open(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_selected = Some((is_view, name.clone()));
                    }
                    open_data(self, &conn_id, name);
                }
                TreeAction::StartRename(is_view, name) => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if state.tree_rename.is_none() {
                            state.tree_selected = Some((is_view, name.clone()));
                            state.tree_rename = Some(TreeRename {
                                is_view,
                                old: name.clone(),
                                value: name,
                                just_started: true,
                                error: None,
                            });
                        }
                    }
                }
                TreeAction::RenameCancel => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        state.tree_rename = None;
                    }
                }
                TreeAction::RenameFocusTaken => {
                    if let Some(state) = self.sessions.get_mut(&conn_id) {
                        if let Some(r) = state.tree_rename.as_mut() {
                            r.just_started = false;
                        }
                    }
                }
                TreeAction::RenameCommit(is_view, old, new) => {
                    commit_rename(self, &conn_id, is_view, old, new);
                }
            }
        }

'''

src = src[:start] + new_block + src[end:]

helpers = '''
/// Inline rename editor for one tree entry (Explorer-style). Emits
/// commit/cancel actions; the edit keeps focus until Enter, Escape or a
/// click elsewhere.
fn render_rename_edit(
    ui: &mut egui::Ui,
    rename: &TreeRename,
    is_view: bool,
    actions: &mut Vec<TreeAction>,
) {
    let mut value = rename.value.clone();
    let resp = ui.add(
        egui::TextEdit::singleline(&mut value)
            .desired_width(ui.available_width().min(220.0)),
    );
    if rename.just_started {
        resp.request_focus();
        actions.push(TreeAction::RenameFocusTaken);
    }
    if let Some(err) = &rename.error {
        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(TreeAction::RenameCancel);
    } else if resp.lost_focus() {
        // Enter blurs a single-line TextEdit, so a blur is either Enter or a
        // click elsewhere — both commit the rename (Explorer behaviour).
        actions.push(TreeAction::RenameCommit(is_view, rename.old.clone(), value));
    }
}

/// Validate and run `RENAME TABLE` for an inline rename.
fn commit_rename(app: &mut App, conn_id: &str, is_view: bool, old: String, new: String) {
    let new = new.trim().to_string();
    let Some(state) = app.sessions.get_mut(conn_id) else {
        return;
    };
    if new.is_empty() || new == old {
        state.tree_rename = None; // unchanged or empty → just leave rename mode
        return;
    }
    if new.contains('`') {
        if let Some(r) = state.tree_rename.as_mut() {
            r.error = Some("name must not contain a backtick".into());
            r.just_started = false;
        }
        return;
    }
    let session = state.session.clone();
    let db = state.current_db.clone();
    let tx = app.jobs.sender();
    let id = conn_id.to_string();
    state.busy += 1;
    std::thread::spawn(move || {
        let sql = format!(
            "RENAME TABLE {} TO {}",
            mff_core::sqlgen::quote_qualified(&[&db, &old]),
            mff_core::sqlgen::quote_qualified(&[&db, &new])
        );
        let result = session
            .run_statement(&sql)
            .map(|_| ())
            .map_err(|e| e.to_string());
        let _ = tx.send(UiMsg::Renamed {
            conn_id: id,
            is_view,
            old,
            new,
            result,
        });
    });
}
'''

# Append the helpers at the end of the file.
src = src.rstrip() + "\n" + helpers
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("replaced tree block; old bytes:", old_len)
