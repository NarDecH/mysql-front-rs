//! Table designer window: edit columns / table options with a live
//! ALTER TABLE preview, then apply in the background.

use egui::Context;
use mff_core::designer::{build_alter, is_destructive, ColSpec, Design};
use mff_core::model::Table;

use crate::app::App;

/// Designer state bound to one session.
pub struct DesignerState {
    pub original: Table,
    pub design: Design,
    pub running: bool,
    pub error: Option<String>,
    /// Set when the next Apply needs an extra confirmation (column drops).
    pub confirm_destructive: bool,
}

impl DesignerState {
    pub fn from_schema(schema: &Table) -> Self {
        Self {
            original: schema.clone(),
            design: Design::from_table(schema),
            running: false,
            error: None,
            confirm_destructive: false,
        }
    }

    /// The live ALTER preview (empty = no changes).
    fn preview(&self) -> Result<Vec<String>, String> {
        build_alter(&self.original, &self.design).map_err(|e| e.to_string())
    }
}

const ENGINES: &[&str] = &["InnoDB", "MyISAM", "Aria", "MEMORY", "CSV"];
const CHARSETS: &[&str] = &["utf8mb4", "utf8", "latin1", "tis620"];

impl App {
    /// Open the designer from the structure view (schema must be loaded).
    pub fn open_designer(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get(conn_id) else {
            return;
        };
        match state.structure.schema.as_ref() {
            Some(schema) if schema.name == state.structure.table => {
                self.designer = Some(crate::table_designer::DesignerState::from_schema(schema));
            }
            _ => {
                self.status = "load a table's structure first".into();
            }
        }
    }

    pub fn show_designer(&mut self, ctx: &Context, conn_id: &str) {
        let mut apply_clicked = false;
        let mut cancel_clicked = false;
        {
            let Some(d) = self.designer.as_mut() else {
                return;
            };

            let mut open = true;
            egui::Window::new(format!(
                "Edit structure — {}.{}",
                {
                    // best-effort db label
                    self.sessions
                        .get(conn_id)
                        .map(|s| s.current_db.clone())
                        .unwrap_or_default()
                },
                d.original.name
            ))
            .open(&mut open)
            .resizable(true)
            .default_width(880.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                // ── Table options ──────────────────────────────────────
                egui::Grid::new("designer_opts")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("Table name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.design.table_name)
                                .desired_width(200.0),
                        );
                        ui.label("Engine:");
                        egui::ComboBox::from_id_salt("designer_engine")
                            .selected_text(d.design.engine.as_str())
                            .show_ui(ui, |ui| {
                                for e in ENGINES {
                                    ui.selectable_value(&mut d.design.engine, e.to_string(), *e);
                                }
                            });
                        ui.end_row();
                        ui.label("Charset:");
                        egui::ComboBox::from_id_salt("designer_charset")
                            .selected_text(d.design.charset.as_str())
                            .show_ui(ui, |ui| {
                                for c in CHARSETS {
                                    ui.selectable_value(
                                        &mut d.design.charset,
                                        c.to_string(),
                                        *c,
                                    );
                                }
                            });
                        ui.label("Comment:");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.design.comment)
                                .desired_width(320.0),
                        );
                        ui.end_row();
                    });
                ui.separator();

                // ── Columns ────────────────────────────────────────────
                ui.strong(format!("Columns ({})", d.design.cols.len()));
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let n = d.design.cols.len();
                        let mut move_up: Option<usize> = None;
                        let mut move_down: Option<usize> = None;
                        let mut remove: Option<usize> = None;
                        let mut add_after: Option<usize> = None;
                        for i in 0..n {
                            let is_new = d.design.cols[i].orig_name.is_none();
                            let tag = if is_new { "＋" } else { "" };
                            ui.horizontal(|ui| {
                                if !tag.is_empty() {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(120, 200, 120),
                                        tag,
                                    );
                                }
                                let mut c = d.design.cols[i].clone();
                                ui.add(
                                    egui::TextEdit::singleline(&mut c.name)
                                        .desired_width(110.0)
                                        .hint_text("name"),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut c.col_type)
                                        .desired_width(140.0)
                                        .hint_text("type"),
                                );
                                ui.checkbox(&mut c.nullable, "NULL")
                                    .on_hover_text("allow NULL values");
                                ui.checkbox(&mut c.auto_inc, "AI")
                                    .on_hover_text("auto increment");
                                match &mut c.default {
                                    None => {
                                        if ui
                                            .small_button("default…")
                                            .on_hover_text("add a DEFAULT clause")
                                            .clicked()
                                        {
                                            c.default = Some(Some(String::new()));
                                        }
                                    }
                                    Some(Some(text)) => {
                                        ui.add(
                                            egui::TextEdit::singleline(text)
                                                .desired_width(80.0)
                                                .hint_text("default"),
                                        );
                                        if ui
                                            .small_button("✕")
                                            .on_hover_text("remove the DEFAULT clause")
                                            .clicked()
                                        {
                                            c.default = None;
                                        }
                                        if ui
                                            .small_button("NULL")
                                            .on_hover_text("default NULL")
                                            .clicked()
                                        {
                                            c.default = Some(None);
                                        }
                                    }
                                    Some(None) => {
                                        ui.weak("DEFAULT NULL");
                                        if ui
                                            .small_button("value…")
                                            .on_hover_text("set a default value")
                                            .clicked()
                                        {
                                            c.default = Some(Some(String::new()));
                                        }
                                        if ui
                                            .small_button("✕")
                                            .on_hover_text("remove the DEFAULT clause")
                                            .clicked()
                                        {
                                            c.default = None;
                                        }
                                    }
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut c.comment)
                                        .desired_width(110.0)
                                        .hint_text("comment"),
                                );
                                d.design.cols[i] = c;
                                if i > 0 && ui.small_button("↑").clicked() {
                                    move_up = Some(i);
                                }
                                if i + 1 < n && ui.small_button("↓").clicked() {
                                    move_down = Some(i);
                                }
                                if ui
                                    .small_button(
                                        egui::RichText::new("✕")
                                            .color(egui::Color32::LIGHT_RED),
                                    )
                                    .on_hover_text(if is_new {
                                        "remove this new column"
                                    } else {
                                        "DROP this column"
                                    })
                                    .clicked()
                                {
                                    remove = Some(i);
                                }
                                if ui
                                    .small_button("＋")
                                    .on_hover_text("insert a column after this one")
                                    .clicked()
                                {
                                    add_after = Some(i);
                                }
                            });
                            ui.end_row();
                        }
                        if let Some(i) = move_up {
                            d.design.cols.swap(i - 1, i);
                        }
                        if let Some(i) = move_down {
                            d.design.cols.swap(i, i + 1);
                        }
                        if let Some(i) = remove {
                            d.design.cols.remove(i);
                        }
                        if let Some(i) = add_after {
                            d.design.cols.insert(
                                i + 1,
                                ColSpec {
                                    orig_name: None,
                                    name: String::new(),
                                    col_type: "varchar(100)".into(),
                                    nullable: true,
                                    ..Default::default()
                                },
                            );
                        }
                    });
                if ui.button("＋ Add column").clicked() {
                    d.design.cols.push(ColSpec {
                        orig_name: None,
                        name: String::new(),
                        col_type: "varchar(100)".into(),
                        nullable: true,
                        ..Default::default()
                    });
                }
                ui.separator();

                // ── Indexes ────────────────────────────────────────────
                ui.strong(format!("Indexes ({})", d.design.indexes.len()));
                ui.add_space(2.0);
                {
                    const IDX_KINDS: &[&str] = &["INDEX", "UNIQUE", "FULLTEXT", "SPATIAL"];
                    let n = d.design.indexes.len();
                    let mut remove: Option<usize> = None;
                    for i in 0..n {
                        let is_new = d.design.indexes[i].orig_name.is_none();
                        ui.horizontal(|ui| {
                            if is_new {
                                ui.colored_label(egui::Color32::from_rgb(120, 200, 120), "＋");
                            }
                            let mut idx = d.design.indexes[i].clone();
                            ui.add(
                                egui::TextEdit::singleline(&mut idx.name)
                                    .desired_width(120.0)
                                    .hint_text("index name"),
                            );
                            egui::ComboBox::from_id_salt(("designer_idx_kind", i))
                                .selected_text(idx.kind.as_str())
                                .show_ui(ui, |ui| {
                                    for k in IDX_KINDS {
                                        ui.selectable_value(&mut idx.kind, k.to_string(), *k);
                                    }
                                });
                            ui.add(
                                egui::TextEdit::singleline(&mut idx.columns)
                                    .desired_width(220.0)
                                    .hint_text("col1, col2(191)"),
                            );
                            d.design.indexes[i] = idx;
                            if ui
                                .small_button(
                                    egui::RichText::new("✕").color(egui::Color32::LIGHT_RED),
                                )
                                .on_hover_text(if is_new {
                                    "remove this new index"
                                } else {
                                    "DROP this index"
                                })
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        d.design.indexes.remove(i);
                    }
                }
                if ui.button("＋ Add index").clicked() {
                    d.design.indexes.push(mff_core::designer::IdxSpec {
                        orig_name: None,
                        name: String::new(),
                        kind: "INDEX".into(),
                        columns: String::new(),
                    });
                }
                ui.separator();

                // ── Foreign keys ───────────────────────────────────────
                ui.strong(format!("Foreign keys ({})", d.design.foreign_keys.len()));
                ui.add_space(2.0);
                {
                    const ACTIONS: &[&str] =
                        &["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"];
                    let n = d.design.foreign_keys.len();
                    let mut remove: Option<usize> = None;
                    for i in 0..n {
                        let is_new = d.design.foreign_keys[i].orig_name.is_none();
                        ui.horizontal(|ui| {
                            if is_new {
                                ui.colored_label(egui::Color32::from_rgb(120, 200, 120), "＋");
                            }
                            let mut fk = d.design.foreign_keys[i].clone();
                            ui.add(
                                egui::TextEdit::singleline(&mut fk.name)
                                    .desired_width(110.0)
                                    .hint_text("constraint name"),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut fk.columns)
                                    .desired_width(90.0)
                                    .hint_text("columns"),
                            );
                            ui.weak("→");
                            ui.add(
                                egui::TextEdit::singleline(&mut fk.ref_table)
                                    .desired_width(90.0)
                                    .hint_text("ref table"),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut fk.ref_columns)
                                    .desired_width(70.0)
                                    .hint_text("ref cols"),
                            );
                            ui.weak("ON DELETE");
                            egui::ComboBox::from_id_salt(("designer_fk_del", i))
                                .selected_text(fk.on_delete.as_str())
                                .show_ui(ui, |ui| {
                                    for a in ACTIONS {
                                        ui.selectable_value(
                                            &mut fk.on_delete,
                                            a.to_string(),
                                            *a,
                                        );
                                    }
                                });
                            ui.weak("ON UPDATE");
                            egui::ComboBox::from_id_salt(("designer_fk_upd", i))
                                .selected_text(fk.on_update.as_str())
                                .show_ui(ui, |ui| {
                                    for a in ACTIONS {
                                        ui.selectable_value(
                                            &mut fk.on_update,
                                            a.to_string(),
                                            *a,
                                        );
                                    }
                                });
                            d.design.foreign_keys[i] = fk;
                            if ui
                                .small_button(
                                    egui::RichText::new("✕").color(egui::Color32::LIGHT_RED),
                                )
                                .on_hover_text(if is_new {
                                    "remove this new foreign key"
                                } else {
                                    "DROP this foreign key"
                                })
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        d.design.foreign_keys.remove(i);
                    }
                }
                if ui.button("＋ Add foreign key").clicked() {
                    d.design.foreign_keys.push(mff_core::designer::FkSpec {
                        orig_name: None,
                        name: String::new(),
                        columns: String::new(),
                        ref_table: String::new(),
                        ref_columns: String::new(),
                        on_delete: "RESTRICT".into(),
                        on_update: "RESTRICT".into(),
                    });
                }
                ui.separator();

                // ── Live ALTER preview ─────────────────────────────────
                match d.preview() {
                    Ok(stmts) => {
                        if stmts.is_empty() {
                            ui.weak("no changes");
                        } else {
                            ui.strong("SQL preview:");
                            if is_destructive(&d.original, &d.design) {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "⚠ this drops column(s) — data in them is lost",
                                );
                            }
                            egui::ScrollArea::vertical()
                                .id_salt("designer_sql")
                                .auto_shrink([false, false])
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    ui.monospace(stmts.join("\n\n"));
                                });
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {e}"));
                    }
                }

                if let Some(err) = &d.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let valid = !d.running
                        && d.preview().map(|s| !s.is_empty()).unwrap_or(false);
                    let label = if d.running {
                        "applying…".to_string()
                    } else {
                        "Apply".to_string()
                    };
                    if ui
                        .add_enabled(
                            valid,
                            egui::Button::new(
                                egui::RichText::new(label)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(crate::theme::accent(ui.ctx()))
                            .min_size(egui::vec2(0.0, 24.0)),
                        )
                        .clicked()
                    {
                        apply_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                    if d.running {
                        ui.spinner();
                        ui.small("applying…");
                    }
                });
            });
            if !open {
                cancel_clicked = true;
            }
        }

        if cancel_clicked {
            self.designer = None;
            return;
        }
        if apply_clicked {
            self.apply_design(conn_id);
        }
    }

    /// Run the ALTER statements (background job).
    fn apply_design(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(d) = self.designer.as_mut() else {
            return;
        };
        // Column drops need one extra confirmation.
        if !d.confirm_destructive && is_destructive(&d.original, &d.design) {
            d.confirm_destructive = true;
            return;
        }
        let stmts = match build_alter(&d.original, &d.design) {
            Ok(stmts) => stmts,
            Err(e) => {
                d.error = Some(e.to_string());
                return;
            }
        };
        d.error = None;
        d.running = true;
        d.confirm_destructive = false;
        let session = state.session.clone();
        let old_name = d.original.name.clone();
        let new_name = d.design.table_name.trim().to_string();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = "applying structure changes…".into();
        std::thread::spawn(move || {
            let mut result: std::result::Result<(), String> = Ok(());
            for r in session.run_script(&stmts, false) {
                if let Err(e) = r {
                    result = Err(e.to_string());
                    break;
                }
            }
            let _ = tx.send(crate::jobs::UiMsg::DesignApplied {
                conn_id: id,
                old_name,
                new_name,
                result,
            });
        });
    }
}

