#!/usr/bin/env python3
"""Wire import/backup/restore + no-WHERE guard into session_view.rs."""
import io

p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

# 1. SessionState: guard state
old = """    /// SQL completion index for the current database (lazily loaded).
    pub completion: CompletionUi,
}"""
new = """    /// SQL completion index for the current database (lazily loaded).
    pub completion: CompletionUi,
    /// UPDATE/DELETE-without-WHERE confirmation: the flagged statements.
    pub confirm_no_where: Option<Vec<String>>,
    /// The query that is waiting behind that confirmation.
    pub pending_run: Option<(usize, String)>,
}"""
assert old in s, "state struct"
s = s.replace(old, new)

old = """            history_open: false,
            completion: Default::default(),
        }
    }
}"""
new = """            history_open: false,
            completion: Default::default(),
            confirm_no_where: None,
            pending_run: None,
        }
    }
}"""
assert old in s, "state init"
s = s.replace(old, new)

# 2. run_query: guard before spawning
old = """        let statements = mff_core::sqlsplit::split_statements(&code);
        if statements.is_empty() {
            tab.sections.clear();
            return;
        }
        tab.running = true;"""
new = """        let statements = mff_core::sqlsplit::split_statements(&code);
        if statements.is_empty() {
            tab.sections.clear();
            return;
        }
        // Safety net: UPDATE/DELETE without WHERE rewrite or remove every
        // row — ask before running them.
        let flagged: Vec<String> = statements
            .iter()
            .filter(|s| mff_core::sqlsplit::needs_confirm_no_where(s))
            .cloned()
            .collect();
        if !flagged.is_empty() {
            state.confirm_no_where = Some(flagged);
            state.pending_run = Some((tab_index, code));
            return;
        }
        tab.running = true;"""
assert old in s, "run_query"
s = s.replace(old, new)

# 3. apply_msg handlers before Status
old = """            UiMsg::Status(s) => self.status = s,"""
new = """            UiMsg::ImportDone {
                conn_id,
                table,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                match &result {
                    Ok(n) => self.status = format!("imported {n} row(s) into {table}"),
                    Err(e) => self.status = format!("import failed: {e}"),
                }
                if let Some(dlg) = self.import_dlg.as_mut() {
                    dlg.running = false;
                    dlg.message = Some(match &result {
                        Ok(n) => format!("✓ imported {n} row(s)"),
                        Err(e) => format!("✗ {e}"),
                    });
                }
                // Refresh the data view when it shows the target table.
                let needs_reload = self
                    .sessions
                    .get(&conn_id)
                    .map(|s| s.data.table == table)
                    .unwrap_or(false);
                if needs_reload {
                    let (filter, sort) = self
                        .sessions
                        .get(&conn_id)
                        .map(|s| (s.data.filter_applied.clone(), s.data.sort.clone()))
                        .unwrap_or_default();
                    reopen_data(self, &conn_id, table, filter, sort);
                }
            }
            UiMsg::BackupDone {
                conn_id,
                path,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                if let Some(dlg) = self.backup_dlg.as_mut() {
                    dlg.running = false;
                    dlg.message = Some(match &result {
                        Ok(()) => format!("✓ wrote {}", path),
                        Err(e) => format!("✗ {e}"),
                    });
                }
                self.status = match &result {
                    Ok(()) => format!("backup written to {path}"),
                    Err(e) => format!("backup failed: {e}"),
                };
            }
            UiMsg::RestoreDone {
                conn_id,
                target,
                result,
            } => {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.busy = state.busy.saturating_sub(1);
                }
                if let Some(dlg) = self.restore_dlg.as_mut() {
                    dlg.running = false;
                    dlg.message = Some(match &result {
                        Ok(n) => format!("✓ ran {n} statement(s) into {target}"),
                        Err(e) => format!("✗ {e}"),
                    });
                }
                self.status = match &result {
                    Ok(n) => format!("restored {n} statement(s) into {target}"),
                    Err(e) => format!("restore failed: {e}"),
                };
                // The target database may have been created just now.
                self.load_databases(&conn_id);
            }
            UiMsg::Status(s) => self.status = s,"""
assert old in s, "apply_msg"
s = s.replace(old, new)

# 4. Menu ▾: backup/restore entries
old = """                    if ui.button("Processes & users").clicked() {
                        ui.close();
                        if let Some(state) = self.sessions.get_mut(&conn_id) {
                            state.view = ActiveView::Server;
                        }
                    }
                });"""
new = """                    if ui.button("Processes & users").clicked() {
                        ui.close();
                        if let Some(state) = self.sessions.get_mut(&conn_id) {
                            state.view = ActiveView::Server;
                        }
                    }
                    ui.separator();
                    if ui.button("Backup database…").clicked() {
                        ui.close();
                        self.backup_dlg = Some(Default::default());
                    }
                    if ui.button("Restore database…").clicked() {
                        ui.close();
                        self.restore_dlg = Some(Default::default());
                    }
                });"""
assert old in s, "menu"
s = s.replace(old, new)

# 5. WHERE confirmation window after the drop-object window
old = """            if confirmed {
                self.drop_object(&conn_id, kind, name);
            }
        }
    }
}"""
new = """            if confirmed {
                self.drop_object(&conn_id, kind, name);
            }
        }

        // "UPDATE/DELETE without WHERE" confirmation window.
        let guard = self
            .sessions
            .get(&conn_id)
            .map(|s| s.confirm_no_where.clone())
            .unwrap_or(None);
        if let Some(flagged) = guard {
            let mut open = true;
            let mut run_anyway = false;
            let mut cancel = false;
            egui::Window::new("Run without WHERE?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 170, 120),
                        "⚠ These statements have no WHERE clause and will affect EVERY row:",
                    );
                    egui::ScrollArea::vertical()
                        .max_height(120.0)
                        .show(ui, |ui| {
                            for stmt in &flagged {
                                ui.monospace(format!(
                                    "· {}",
                                    stmt.trim().chars().take(90).collect::<String>()
                                ));
                            }
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                egui::RichText::new("Run anyway")
                                    .color(egui::Color32::LIGHT_RED),
                            )
                            .clicked()
                        {
                            run_anyway = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if !open || cancel {
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.confirm_no_where = None;
                    state.pending_run = None;
                }
            }
            if run_anyway {
                let pending = self
                    .sessions
                    .get_mut(&conn_id)
                    .and_then(|s| s.pending_run.take());
                if let Some(state) = self.sessions.get_mut(&conn_id) {
                    state.confirm_no_where = None;
                }
                if let Some((tab_index, code)) = pending {
                    self.run_query(&conn_id, tab_index, code);
                }
            }
        }
    }
}"""
assert old in s, "guard window"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("session_view.rs patched")
