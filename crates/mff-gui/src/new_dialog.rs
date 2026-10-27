//! "New object" dialogs — form-based creation for tables, views, routines,
//! triggers and events, with a live SQL preview. The generated statement is
//! executed in the session's current database.

use egui::Context;
use mff_core::sqlgen::{default_literal, quote_ident, quote_qualified, quote_string};

use crate::app::App;
use crate::jobs::UiMsg;

/// Which object the dialog creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    Table,
    View,
    Procedure,
    Function,
    Trigger,
    Event,
}

impl NewKind {
    pub fn label(&self) -> &'static str {
        match self {
            NewKind::Table => "table",
            NewKind::View => "view",
            NewKind::Procedure => "procedure",
            NewKind::Function => "function",
            NewKind::Trigger => "trigger",
            NewKind::Event => "event",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ColumnDraft {
    pub name: String,
    pub dtype: String,
    pub nullable: bool,
    pub auto_inc: bool,
    pub pk: bool,
    pub default: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParamDraft {
    pub name: String,
    pub ptype: String,
    pub direction: usize, // 0 = IN, 1 = OUT, 2 = INOUT (procedures only)
}

#[derive(Debug, Clone)]
pub enum NewDialogData {
    Table {
        name: String,
        engine: usize,
        charset: usize,
        comment: String,
        columns: Vec<ColumnDraft>,
    },
    View {
        name: String,
        select_sql: String,
    },
    Routine {
        is_function: bool,
        name: String,
        params: Vec<ParamDraft>,
        returns: String,
        deterministic: bool,
        body: String,
    },
    Trigger {
        name: String,
        table: String,
        timing: usize, // 0 = BEFORE, 1 = AFTER
        event: usize,  // 0 = INSERT, 1 = UPDATE, 2 = DELETE
        body: String,
    },
    Event {
        name: String,
        every: bool, // true = EVERY n unit, false = AT datetime
        interval: i64,
        unit: usize,
        at: String,
        body: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone)]
pub struct NewDialog {
    pub kind: NewKind,
    pub data: NewDialogData,
    pub error: Option<String>,
}

const ENGINES: [&str; 4] = ["InnoDB", "MyISAM", "MEMORY", "Aria"];
const CHARSETS: [&str; 4] = ["utf8mb4", "utf8", "latin1", "tis620"];
const UNITS: [&str; 5] = ["MINUTE", "HOUR", "DAY", "WEEK", "MONTH"];
const DIRECTIONS: [&str; 3] = ["IN", "OUT", "INOUT"];
const TIMINGS: [&str; 2] = ["BEFORE", "AFTER"];
const EVENTS: [&str; 3] = ["INSERT", "UPDATE", "DELETE"];

impl NewDialog {
    pub fn new(kind: NewKind) -> Self {
        let data = match kind {
            NewKind::Table => NewDialogData::Table {
                name: "new_table".into(),
                engine: 0,
                charset: 0,
                comment: String::new(),
                columns: vec![
                    ColumnDraft {
                        name: "id".into(),
                        dtype: "INT UNSIGNED".into(),
                        nullable: false,
                        auto_inc: true,
                        pk: true,
                        default: String::new(),
                    },
                    ColumnDraft {
                        name: "name".into(),
                        dtype: "VARCHAR(100)".into(),
                        nullable: false,
                        auto_inc: false,
                        pk: false,
                        default: String::new(),
                    },
                ],
            },
            NewKind::View => NewDialogData::View {
                name: "new_view".into(),
                select_sql: "SELECT *\nFROM `users`\nWHERE 1 = 1".into(),
            },
            NewKind::Procedure => NewDialogData::Routine {
                is_function: false,
                name: "new_procedure".into(),
                params: vec![ParamDraft {
                    name: "p_id".into(),
                    ptype: "INT UNSIGNED".into(),
                    direction: 0,
                }],
                returns: String::new(),
                deterministic: false,
                body: "SELECT * FROM `users` WHERE `id` = p_id;".into(),
            },
            NewKind::Function => NewDialogData::Routine {
                is_function: true,
                name: "new_function".into(),
                params: vec![ParamDraft {
                    name: "p_score".into(),
                    ptype: "DECIMAL(5,2)".into(),
                    direction: 0,
                }],
                returns: "VARCHAR(20)".into(),
                deterministic: true,
                body: "IF p_score IS NULL THEN RETURN 'unknown';\nELSE RETURN 'ok';\nEND IF;".into(),
            },
            NewKind::Trigger => NewDialogData::Trigger {
                name: "new_trigger".into(),
                table: String::new(),
                timing: 0,
                event: 0,
                body: "SET NEW.`created_at` = NOW();".into(),
            },
            NewKind::Event => NewDialogData::Event {
                name: "new_event".into(),
                every: true,
                interval: 1,
                unit: 2, // DAY
                at: String::new(),
                body: "DELETE FROM `orders` WHERE `amount` < 0;".into(),
                enabled: true,
            },
        };
        Self {
            kind,
            data,
            error: None,
        }
    }
}

/// Validate and generate the CREATE statement for the dialog.
pub fn generate_sql(dlg: &NewDialog, db: &str) -> Result<String, String> {
    let check_name = |name: &str| -> Result<String, String> {
        let n = name.trim();
        if n.is_empty() {
            return Err("name must not be empty".into());
        }
        if n.contains('`') {
            return Err("name must not contain a backtick".into());
        }
        Ok(n.to_string())
    };

    match &dlg.data {
        NewDialogData::Table {
            name,
            engine,
            charset,
            comment,
            columns,
        } => {
            let table = check_name(name)?;
            if columns.is_empty() {
                return Err("a table needs at least one column".into());
            }
            let mut seen: Vec<&str> = Vec::new();
            for c in columns {
                let cn = c.name.trim();
                if cn.is_empty() {
                    return Err("every column needs a name".into());
                }
                if cn.contains('`') {
                    return Err("column names must not contain a backtick".into());
                }
                if seen.iter().any(|s| s.eq_ignore_ascii_case(cn)) {
                    return Err(format!("duplicate column name: {cn}"));
                }
                seen.push(cn);
                if c.dtype.trim().is_empty() {
                    return Err(format!("column {cn} needs a data type"));
                }
            }
            let mut lines: Vec<String> = Vec::new();
            for c in columns {
                let dtype = c.dtype.trim();
                let mut line = format!(
                    "  {} {} {}",
                    quote_ident(c.name.trim()),
                    dtype,
                    if c.nullable { "NULL" } else { "NOT NULL" }
                );
                if c.auto_inc {
                    line.push_str(" AUTO_INCREMENT");
                }
                let d = c.default.trim();
                if !d.is_empty() {
                    line.push_str(&format!(
                        " DEFAULT {}",
                        default_literal(dtype, d)
                    ));
                }
                lines.push(line);
            }
            let pk: Vec<String> = columns
                .iter()
                .filter(|c| c.pk)
                .map(|c| quote_ident(c.name.trim()))
                .collect();
            if !pk.is_empty() {
                lines.push(format!("  PRIMARY KEY ({})", pk.join(", ")));
            }
            let mut out = format!(
                "CREATE TABLE {} (\n{}\n) ENGINE={} DEFAULT CHARSET={}",
                quote_qualified(&[db, &table]),
                lines.join(",\n"),
                ENGINES.get(*engine).copied().unwrap_or("InnoDB"),
                CHARSETS.get(*charset).copied().unwrap_or("utf8mb4"),
            );
            let cm = comment.trim();
            if !cm.is_empty() {
                out.push_str(&format!(" COMMENT={}", quote_string(cm)));
            }
            out.push(';');
            Ok(out)
        }
        NewDialogData::View { name, select_sql } => {
            let view = check_name(name)?;
            let body = select_sql.trim().trim_end_matches(';').trim();
            if body.is_empty() {
                return Err("the view needs a SELECT statement".into());
            }
            Ok(format!(
                "CREATE OR REPLACE VIEW {} AS\n{};",
                quote_qualified(&[db, &view]),
                body
            ))
        }
        NewDialogData::Routine {
            is_function,
            name,
            params,
            returns,
            deterministic,
            body,
        } => {
            let routine = check_name(name)?;
            let mut ps: Vec<String> = Vec::new();
            for p in params {
                let pn = p.name.trim();
                if pn.is_empty() || p.ptype.trim().is_empty() {
                    return Err("every parameter needs a name and a type".into());
                }
                if *is_function {
                    ps.push(format!("{} {}", quote_ident(pn), p.ptype.trim()));
                } else {
                    let dir = DIRECTIONS.get(p.direction).copied().unwrap_or("IN");
                    ps.push(format!(
                        "{} {} {}",
                        dir,
                        quote_ident(pn),
                        p.ptype.trim()
                    ));
                }
            }
            let kw = if *is_function { "FUNCTION" } else { "PROCEDURE" };
            let head = if ps.is_empty() {
                format!("CREATE {} {}", kw, quote_qualified(&[db, &routine]))
            } else {
                format!(
                    "CREATE {} {}({})",
                    kw,
                    quote_qualified(&[db, &routine]),
                    ps.join(", ")
                )
            };
            let mut out = head;
            if *is_function {
                let ret = returns.trim();
                if ret.is_empty() {
                    return Err("a function needs a RETURNS type".into());
                }
                out.push_str(&format!(" RETURNS {}", ret));
                if *deterministic {
                    out.push_str(" DETERMINISTIC");
                }
            }
            let b = body.trim().trim_end_matches(';').trim();
            if b.is_empty() {
                return Err("the body must not be empty".into());
            }
            out.push_str(&format!("\nBEGIN\n{}\nEND;", b));
            Ok(out)
        }
        NewDialogData::Trigger {
            name,
            table,
            timing,
            event,
            body,
        } => {
            let trigger = check_name(name)?;
            let tbl = table.trim();
            if tbl.is_empty() {
                return Err("pick the table the trigger acts on".into());
            }
            let b = body.trim().trim_end_matches(';').trim();
            if b.is_empty() {
                return Err("the trigger body must not be empty".into());
            }
            Ok(format!(
                "CREATE TRIGGER {} {} {} ON {} FOR EACH ROW\n{};",
                quote_qualified(&[db, &trigger]),
                TIMINGS.get(*timing).copied().unwrap_or("BEFORE"),
                EVENTS.get(*event).copied().unwrap_or("INSERT"),
                quote_qualified(&[db, tbl]),
                b
            ))
        }
        NewDialogData::Event {
            name,
            every,
            interval,
            unit,
            at,
            body,
            enabled,
        } => {
            let event = check_name(name)?;
            let b = body.trim().trim_end_matches(';').trim();
            if b.is_empty() {
                return Err("the event body must not be empty".into());
            }
            let schedule = if *every {
                if *interval <= 0 {
                    return Err("the interval must be a positive number".into());
                }
                format!(
                    "EVERY {interval} {}",
                    UNITS.get(*unit).copied().unwrap_or("DAY")
                )
            } else {
                let t = at.trim();
                if t.is_empty() {
                    return Err("the start time must not be empty".into());
                }
                format!("AT {}", quote_string(t))
            };
            let state = if *enabled { "ENABLE" } else { "DISABLE" };
            Ok(format!(
                "CREATE EVENT {} ON SCHEDULE {schedule} {state}\nDO\n{};",
                quote_qualified(&[db, &event]),
                b
            ))
        }
    }
}

impl App {
    pub fn open_new_dialog(&mut self, kind: NewKind) {
        self.new_dlg = Some(NewDialog::new(kind));
    }

    /// The "New object" window: fields + live SQL preview + Create.
    pub fn show_new_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let db = self.current_db_for(conn_id);
        let trigger_tables: Vec<String> = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.objects.as_ref())
            .map(|o| o.tables.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default();

        let Some(dlg) = self.new_dlg.as_mut() else {
            return;
        };
        let title = format!("New {}", dlg.kind.label());
        let mut open = true;
        let mut create_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    match &mut dlg.data {
                        NewDialogData::Table {
                            name,
                            engine,
                            charset,
                            comment,
                            columns,
                        } => {
                            egui::Grid::new("tbl_meta")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label("Table name");
                                    ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                                    ui.end_row();
                                    ui.label("Engine");
                                    egui::ComboBox::from_id_salt("dlg_engine")
                                        .selected_text(ENGINES.get(*engine).copied().unwrap_or("InnoDB"))
                                        .show_ui(ui, |ui| {
                                            for (i, e) in ENGINES.iter().enumerate() {
                                                ui.selectable_value(engine, i, *e);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("Charset");
                                    egui::ComboBox::from_id_salt("dlg_charset")
                                        .selected_text(CHARSETS.get(*charset).copied().unwrap_or("utf8mb4"))
                                        .show_ui(ui, |ui| {
                                            for (i, c) in CHARSETS.iter().enumerate() {
                                                ui.selectable_value(charset, i, *c);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("Comment");
                                    ui.add(
                                        egui::TextEdit::singleline(comment).desired_width(220.0),
                                    );
                                    ui.end_row();
                                });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.strong("Columns");
                                if ui.small_button("+ Add column").clicked() {
                                    columns.push(ColumnDraft {
                                        name: format!("col{}", columns.len() + 1),
                                        dtype: "VARCHAR(100)".into(),
                                        nullable: true,
                                        auto_inc: false,
                                        pk: false,
                                        default: String::new(),
                                    });
                                }
                            });
                            let mut remove: Option<usize> = None;
                            egui::Grid::new("tbl_cols")
                                .num_columns(8)
                                .striped(true)
                                .spacing([6.0, 3.0])
                                .show(ui, |ui| {
                                    ui.strong("Name");
                                    ui.strong("Type");
                                    ui.strong("Null");
                                    ui.strong("A_I");
                                    ui.strong("PK");
                                    ui.strong("Default");
                                    ui.strong("");
                                    ui.end_row();
                                    let ncols = columns.len();
                                    for (i, c) in columns.iter_mut().enumerate() {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut c.name)
                                                .desired_width(110.0),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut c.dtype)
                                                .desired_width(140.0),
                                        );
                                        ui.checkbox(&mut c.nullable, "");
                                        ui.checkbox(&mut c.auto_inc, "");
                                        ui.checkbox(&mut c.pk, "");
                                        ui.add(
                                            egui::TextEdit::singleline(&mut c.default)
                                                .desired_width(110.0),
                                        );
                                        let clicked = ui.small_button("✕").clicked();
                                        if clicked && ncols > 1 {
                                            remove = Some(i);
                                        }
                                        ui.end_row();
                                    }
                                });
                            if let Some(i) = remove {
                                columns.remove(i);
                            }
                        }
                        NewDialogData::View { name, select_sql } => {
                            egui::Grid::new("view_meta")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label("View name");
                                    ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                                    ui.end_row();
                                });
                            ui.label("SELECT definition");
                            ui.add(
                                egui::TextEdit::multiline(select_sql)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(6)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        }
                        NewDialogData::Routine {
                            is_function,
                            name,
                            params,
                            returns,
                            deterministic,
                            body,
                        } => {
                            egui::Grid::new("rt_meta")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label(if *is_function { "Function name" } else { "Procedure name" });
                                    ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                                    ui.end_row();
                                    if *is_function {
                                        ui.label("Returns");
                                        ui.add(
                                            egui::TextEdit::singleline(returns)
                                                .desired_width(220.0),
                                        );
                                        ui.end_row();
                                        ui.label("Deterministic");
                                        ui.checkbox(deterministic, "");
                                        ui.end_row();
                                    }
                                });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.strong("Parameters");
                                if ui.small_button("+ Add parameter").clicked() {
                                    params.push(ParamDraft {
                                        name: format!("p{}", params.len() + 1),
                                        ptype: "INT UNSIGNED".into(),
                                        direction: 0,
                                    });
                                }
                            });
                            let mut remove: Option<usize> = None;
                            egui::Grid::new("rt_params")
                                .num_columns(4)
                                .striped(true)
                                .spacing([6.0, 3.0])
                                .show(ui, |ui| {
                                    ui.strong("Direction");
                                    ui.strong("Name");
                                    ui.strong("Type");
                                    ui.strong("");
                                    ui.end_row();
                                    for (i, prm) in params.iter_mut().enumerate() {
                                        if *is_function {
                                            ui.label("IN");
                                        } else {
                                            egui::ComboBox::from_id_salt(("dir", i))
                                                .selected_text(
                                                    DIRECTIONS.get(prm.direction).copied().unwrap_or("IN"),
                                                )
                                                .show_ui(ui, |ui| {
                                                    for (j, d) in DIRECTIONS.iter().enumerate() {
                                                        ui.selectable_value(
                                                            &mut prm.direction,
                                                            j,
                                                            *d,
                                                        );
                                                    }
                                                });
                                        }
                                        ui.add(
                                            egui::TextEdit::singleline(&mut prm.name)
                                                .desired_width(110.0),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut prm.ptype)
                                                .desired_width(150.0),
                                        );
                                        if ui.small_button("✕").clicked() {
                                            remove = Some(i);
                                        }
                                        ui.end_row();
                                    }
                                });
                            if let Some(i) = remove {
                                params.remove(i);
                            }
                            ui.add_space(4.0);
                            ui.label("Body (between BEGIN … END)");
                            ui.add(
                                egui::TextEdit::multiline(body)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(5)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        }
                        NewDialogData::Trigger {
                            name,
                            table,
                            timing,
                            event,
                            body,
                        } => {
                            let tables = &trigger_tables;
                            egui::Grid::new("trg_meta")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label("Trigger name");
                                    ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                                    ui.end_row();
                                    ui.label("Table");
                                    if tables.is_empty() {
                                        ui.add(
                                            egui::TextEdit::singleline(table)
                                                .desired_width(220.0),
                                        );
                                    } else {
                                        let sel = tables
                                            .iter()
                                            .position(|t| t == table)
                                            .unwrap_or(0);
                                        egui::ComboBox::from_id_salt("trg_table")
                                            .selected_text(
                                                tables.get(sel).cloned().unwrap_or_default(),
                                            )
                                            .show_ui(ui, |ui| {
                                                for (i, t) in tables.iter().enumerate() {
                                                    ui.selectable_value(table, t.clone(), t);
                                                }
                                            });
                                    }
                                    ui.end_row();
                                    ui.label("Timing");
                                    egui::ComboBox::from_id_salt("trg_timing")
                                        .selected_text(TIMINGS.get(*timing).copied().unwrap_or("BEFORE"))
                                        .show_ui(ui, |ui| {
                                            for (i, t) in TIMINGS.iter().enumerate() {
                                                ui.selectable_value(timing, i, *t);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("Event");
                                    egui::ComboBox::from_id_salt("trg_event")
                                        .selected_text(EVENTS.get(*event).copied().unwrap_or("INSERT"))
                                        .show_ui(ui, |ui| {
                                            for (i, e) in EVENTS.iter().enumerate() {
                                                ui.selectable_value(event, i, *e);
                                            }
                                        });
                                    ui.end_row();
                                });
                            ui.add_space(4.0);
                            ui.label("Body (one statement per row event)");
                            ui.add(
                                egui::TextEdit::multiline(body)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        }
                        NewDialogData::Event {
                            name,
                            every,
                            interval,
                            unit,
                            at,
                            body,
                            enabled,
                        } => {
                            egui::Grid::new("evt_meta")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label("Event name");
                                    ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                                    ui.end_row();
                                    ui.label("Schedule");
                                    ui.horizontal(|ui| {
                                        ui.selectable_value(every, true, "Every");
                                        ui.selectable_value(every, false, "At time");
                                    });
                                    ui.end_row();
                                    ui.label("When");
                                    ui.horizontal(|ui| {
                                        if *every {
                                            ui.add(
                                                egui::DragValue::new(interval).range(1..=100000),
                                            );
                                            egui::ComboBox::from_id_salt("evt_unit")
                                                .selected_text(UNITS.get(*unit).copied().unwrap_or("DAY"))
                                                .show_ui(ui, |ui| {
                                                    for (i, u) in UNITS.iter().enumerate() {
                                                        ui.selectable_value(unit, i, *u);
                                                    }
                                                });
                                        } else {
                                            ui.add(
                                                egui::TextEdit::singleline(at)
                                                    .hint_text("YYYY-MM-DD HH:MM:SS")
                                                    .desired_width(220.0),
                                            );
                                        }
                                    });
                                    ui.end_row();
                                    ui.label("Enabled");
                                    ui.checkbox(enabled, "");
                                    ui.end_row();
                                });
                            ui.add_space(4.0);
                            ui.label("Event body");
                            ui.add(
                                egui::TextEdit::multiline(body)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        }
                    }

                    ui.add_space(8.0);
                    ui.strong("SQL preview");
                    let sql = generate_sql(dlg, &db);
                    match &sql {
                        Ok(text) => {
                            egui::ScrollArea::vertical()
                                .max_height(140.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut text.as_str())
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY),
                                    );
                                });
                        }
                        Err(e) => {
                            ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {e}"));
                        }
                    }

                    if let Some(err) = &dlg.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if sql.is_ok() {
                            let c = crate::theme::accent(ui.ctx());
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Create")
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(c)
                                    .min_size(egui::vec2(0.0, 24.0)),
                                )
                                .clicked()
                            {
                                create_clicked = true;
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new("Create"));
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_clicked = true;
                        }
                    });
                });
            });

        if !open || cancel_clicked {
            self.new_dlg = None;
            return;
        }

        if create_clicked {
            if let Some(snapshot) = self.new_dlg.clone() {
                match generate_sql(&snapshot, &self.current_db_for(conn_id)) {
                    Ok(sql) => {
                        let name = snapshot.object_name();
                        self.create_object(conn_id, sql, name);
                    }
                    Err(e) => {
                        if let Some(d) = self.new_dlg.as_mut() {
                            d.error = Some(e);
                        }
                    }
                }
            }
        }
    }
}

impl NewDialog {
    /// The object name being created (for status messages).
    pub fn object_name(&self) -> String {
        match &self.data {
            NewDialogData::Table { name, .. }
            | NewDialogData::View { name, .. }
            | NewDialogData::Routine { name, .. }
            | NewDialogData::Trigger { name, .. }
            | NewDialogData::Event { name, .. } => name.trim().to_string(),
        }
    }
}

impl App {
    /// Current database of the session (fallback empty string).
    pub fn current_db_for(&self, conn_id: &str) -> String {
        self.sessions
            .get(conn_id)
            .map(|s| s.current_db.clone())
            .unwrap_or_default()
    }

    /// Run `USE db` + the generated statement on a worker thread; the
    /// result comes back as UiMsg::Created.
    pub fn create_object(&mut self, conn_id: &str, sql: String, name: String) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let session = state.session.clone();
        let db = state.current_db.clone();
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        std::thread::spawn(move || {
            let result = session
                .run_statement(&format!(
                    "USE {}",
                    mff_core::sqlgen::quote_ident(&db)
                ))
                .and_then(|_| session.run_statement(&sql).map(|_| ()))
                .map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::Created {
                conn_id: id,
                name,
                result,
            });
        });
    }
}
