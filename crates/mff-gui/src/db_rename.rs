//! "Rename database" dialog + worker job.
//!
//! MySQL/MariaDB has no `RENAME DATABASE`; a rename is performed by
//! creating a new database (with the source charset), moving every table
//! with `RENAME TABLE`, recreating views/routines/triggers/events from
//! their definitions in the new database, dropping the old copies and —
//! optionally — dropping the source database.

use egui::Context;
use mff_core::sqlgen::{quote_ident, quote_qualified, quote_string};
use mff_core::session::{Grid, Session};

use crate::app::App;
use crate::jobs::UiMsg;

#[derive(Debug, Clone)]
pub struct DbRenameDialog {
    pub old: String,
    pub new: String,
    pub recreate_objects: bool,
    pub drop_source: bool,
    pub error: Option<String>,
}

impl App {
    pub fn open_db_rename(&mut self, old: String) {
        self.db_rename_dlg = Some(DbRenameDialog {
            old,
            new: String::new(),
            recreate_objects: true,
            drop_source: false,
            error: None,
        });
    }

    pub fn show_db_rename_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let Some(dlg) = self.db_rename_dlg.as_mut() else {
            return;
        };
        let counts = {
            let db = dlg.old.clone();
            self.sessions
                .get(conn_id)
                .and_then(|s| s.objects.as_ref())
                .filter(|o| o.name == db)
                .map(|o| {
                    (
                        o.tables.len(),
                        o.views.len(),
                        o.routines.len(),
                        o.triggers.len(),
                        o.events.len(),
                    )
                })
        };

        let mut open = true;
        let mut create_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new(format!("Rename database “{}”", dlg.old))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                let d = &mut *dlg;
                egui::Grid::new("dbren_meta")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("Current name");
                        ui.label(egui::RichText::new(&d.old).strong());
                        ui.end_row();
                        ui.label("New name");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.new)
                                .hint_text("new database name")
                                .desired_width(220.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.checkbox(&mut d.recreate_objects, "Recreate routines, triggers and events in the new database");
                ui.checkbox(&mut d.drop_source, "Drop the source database when finished");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Tables and views are moved with RENAME TABLE. If a step fails the \
                         database may be left half-moved; the failing step is reported below.",
                    )
                    .weak()
                    .small(),
                );
                if let Some((t, v, r, tr, e)) = counts {
                    ui.label(format!(
                        "to move: {t} table(s), {v} view(s), {r} routine(s), {tr} trigger(s), {e} event(s)"
                    ));
                }
                ui.label("source charset: copied automatically during the move");

                if let Some(err) = &d.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let valid = {
                        let n = d.new.trim();
                        !n.is_empty()
                            && !n.contains('`')
                            && !n.eq_ignore_ascii_case(&d.old)
                    };
                    if ui.add_enabled(
                        valid,
                        egui::Button::new(
                            egui::RichText::new("Rename database")
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(crate::theme::accent(ui.ctx()))
                        .min_size(egui::vec2(0.0, 24.0)),
                    ).clicked() {
                        create_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if !open || cancel_clicked {
            self.db_rename_dlg = None;
            return;
        }

        if create_clicked {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            let session = state.session.clone();
            let snapshot = self.db_rename_dlg.clone().unwrap();
            let new = snapshot.new.trim().to_string();
            let tx = self.jobs.sender();
            let id = conn_id.to_string();
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.busy += 1;
            }
            self.db_rename_dlg = None;
            std::thread::spawn(move || {
                let result = run_db_rename(
                    &session,
                    &tx,
                    &id,
                    &snapshot.old,
                    &new,
                    snapshot.recreate_objects,
                    snapshot.drop_source,
                );
                let _ = tx.send(UiMsg::DbRenamed {
                    conn_id: id,
                    new_db: new,
                    result,
                });
            });
        }
    }
}

/// Take one text column out of a single-row grid by (partial) column name.
fn grid_value(grid: &Grid, contains: &str) -> Option<String> {
    let idx = grid
        .columns
        .iter()
        .position(|c| c.name.contains(contains))?;
    match grid.rows.first()?.get(idx) {
        Some(mff_core::session::Cell::Text(s)) => Some(s.clone()),
        Some(other) => Some(other.display("")),
        None => None,
    }
}

/// Execute the multi-step database rename. Progress is reported through
/// `UiMsg::Status`; the final result through `UiMsg::DbRenamed`.
fn run_db_rename(
    session: &Session,
    tx: &std::sync::mpsc::Sender<UiMsg>,
    conn_id: &str,
    old: &str,
    new: &str,
    recreate_objects: bool,
    drop_source: bool,
) -> Result<(), String> {
    let step = |msg: &str| {
        let _ = tx.send(UiMsg::Status(format!("rename database: {msg}")));
    };
    let run = |sql: &str| -> Result<(), String> {
        session
            .run_statement(sql)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let q_old_table = |name: &str| quote_qualified(&[old, name]);
    let q_new_table = |name: &str| quote_qualified(&[new, name]);

    (|| -> Result<(), String> {
        let q = |sql: &str| -> Result<Grid, String> {
            session.query(sql).map_err(|e| e.to_string())
        };
        // 1. Create the target database with the source charset.
        step("creating target database");
        let g = q(&format!(
            "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = {}",
            quote_string(old)
        ))?;
        let mut create = format!("CREATE DATABASE {}", quote_ident(new));
        if let Some(cs) = grid_value(&g, "CHARACTER_SET") {
            create.push_str(&format!(" CHARACTER SET {}", cs));
            if let Some(co) = grid_value(&g, "COLLATION") {
                create.push_str(&format!(" COLLATE {}", co));
            }
        }
        run(&create)?;

        // Select the new database so unqualified names land there.
        run(&format!("USE {}", quote_ident(new)))?;

        // 2. Move tables (RENAME TABLE also carries indexes/data instantly).
        let objects = session.database_objects(old).map_err(|e| e.to_string())?;
        let total = objects.tables.len() + objects.views.len();
        for (i, t) in objects.tables.iter().enumerate() {
            step(&format!(
                "moving table {}/{}: {}",
                i + 1,
                total,
                t.name
            ));
            run(&format!(
                "RENAME TABLE {} TO {}",
                q_old_table(&t.name),
                q_new_table(&t.name)
            ))?;
        }

        // 3. Move views: recreate from SHOW CREATE VIEW (rewriting the old
        //    database qualifier in the body), then drop the old view.
        for (i, v) in objects.views.iter().enumerate() {
            step(&format!(
                "moving view {}/{}: {}",
                objects.tables.len() + i + 1,
                total,
                v.name
            ));
            let g = q(&format!(
                "SHOW CREATE VIEW {}",
                q_old_table(&v.name)
            ))?;
            let mut sql = grid_value(&g, "Create View")
                .ok_or_else(|| format!("no definition returned for view {}", v.name))?;
            sql = qualify_view_definition(&sql, old, new);
            run(&sql)?;
            run(&format!("DROP VIEW {}", q_old_table(&v.name)))?;
        }

        // 4. Routines, triggers, events: recreate from definitions.
        if recreate_objects {
            for r in &objects.routines {
                let kw = if r.kind.eq_ignore_ascii_case("FUNCTION") {
                    "FUNCTION"
                } else {
                    "PROCEDURE"
                };
                step(&format!("recreating {} {}", kw.to_lowercase(), r.name));
                let g = q(&format!(
                    "SHOW CREATE {} {}",
                    kw,
                    q_old_table(&r.name)
                ))?;
                let sql = grid_value(&g, "Create")
                    .ok_or_else(|| format!("no definition returned for {} {}", kw, r.name))?;
                run(&sql)?;
                run(&format!(
                    "DROP {} {}",
                    kw,
                    q_old_table(&r.name)
                ))?;
            }
            for t in &objects.triggers {
                step(&format!("recreating trigger {}", t.name));
                let g = q(&format!(
                    "SHOW CREATE TRIGGER {}",
                    q_old_table(&t.name)
                ))?;
                let sql = grid_value(&g, "SQL Original")
                    .ok_or_else(|| format!("no definition returned for trigger {}", t.name))?;
                run(&sql)?;
                run(&format!("DROP TRIGGER {}", q_old_table(&t.name)))?;
            }
            for e in &objects.events {
                step(&format!("recreating event {}", e.name));
                let g = q(&format!(
                    "SHOW CREATE EVENT {}",
                    q_old_table(&e.name)
                ))?;
                let sql = grid_value(&g, "Create Event")
                    .ok_or_else(|| format!("no definition returned for event {}", e.name))?;
                run(&sql)?;
                run(&format!("DROP EVENT {}", q_old_table(&e.name)))?;
            }
        }

        // 5. Optionally drop the source database.
        if drop_source {
            step("dropping source database");
            run(&format!("DROP DATABASE {}", quote_ident(old)))?;
        }
        Ok(())
    })()
}

/// Rewrite a `SHOW CREATE VIEW` statement so the view lives in `new_db`
/// and its body references tables from `new_db` instead of `old_db`.
pub fn qualify_view_definition(sql: &str, old_db: &str, new_db: &str) -> String {
    let mut out = sql.replace(
        &format!("`{}`.", old_db),
        &format!("`{}`.", new_db),
    );
    // MariaDB already qualifies the view name with the database it lives
    // in (which the rewrite above retargets). Inject the qualifier only
    // when the definition came back unqualified.
    if let Some(pos) = out.find(" VIEW ") {
        let after = &out[pos + " VIEW ".len()..];
        if !after.starts_with(&format!("`{}`.", new_db)) {
            let insert = format!(" VIEW {}.", quote_ident(new_db));
            out.replace_range(pos..pos + " VIEW ".len(), &insert);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::qualify_view_definition;

    #[test]
    fn view_definition_is_fully_qualified() {
        // MariaDB qualifies the view name with its database.
        let shown = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `db_rename_src`.`v_labels` AS select `db_rename_src`.`t1`.`id` AS `id`,`db_rename_src`.`t1`.`label` AS `label` from `db_rename_src`.`t1`";
        let out = qualify_view_definition(shown, "db_rename_src", "dbrenamedst3");
        assert!(out.contains("VIEW `dbrenamedst3`.`v_labels` AS"), "got: {out}");
        assert!(out.contains("`dbrenamedst3`.`t1`"));
        assert!(!out.contains("`db_rename_src`."), "old db still referenced");
        assert!(!out.contains("`dbrenamedst3`.`dbrenamedst3`"), "double qualifier");
    }

    #[test]
    fn unqualified_view_name_gets_qualified() {
        let shown = "CREATE ALGORITHM=UNDEFINED VIEW `v_labels` AS select * from `users`";
        let out = qualify_view_definition(shown, "db_rename_src", "dbrenamedst3");
        assert!(out.contains("VIEW `dbrenamedst3`.`v_labels` AS"), "got: {out}");
    }
}
