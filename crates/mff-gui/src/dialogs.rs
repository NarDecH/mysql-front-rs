//! Modal dialogs: connection editor (add/edit/test) and the export dialog
//! for the data view.

use egui::Context;
use mff_core::{Connection, ExportFormat, SslMode};

use crate::app::App;

/// In-progress connection editor state.
pub struct ConnEditor {
    /// None → new connection.
    pub editing_id: Option<String>,
    pub conn: Connection,
    /// Editing buffer for the optional database name.
    pub database_tmp: String,
    pub error: Option<String>,
    pub testing: bool,
}

impl ConnEditor {
    pub fn new() -> Self {
        let conn = Connection::default();
        let database_tmp = conn.database.clone().unwrap_or_default();
        Self {
            editing_id: None,
            conn,
            database_tmp,
            error: None,
            testing: false,
        }
    }

    pub fn from(conn: Connection) -> Self {
        let database_tmp = conn.database.clone().unwrap_or_default();
        Self {
            editing_id: Some(conn.id.clone()),
            conn,
            database_tmp,
            error: None,
            testing: false,
        }
    }
}

/// Export dialog state for the data view.
#[derive(Default, Clone)]
pub struct ExportDlg {
    pub format: ExportFormat,
    pub path: String,
    pub delimiter: char,
    pub null_text: String,
    pub include_create: bool,
    pub batch_size: usize,
    pub message: Option<String>,
}

impl ExportDlg {
    fn new() -> Self {
        Self {
            format: ExportFormat::Csv,
            delimiter: ',',
            null_text: "NULL".into(),
            batch_size: 1000,
            include_create: true,
            ..Default::default()
        }
    }
}

impl App {
    pub fn open_conn_editor(&mut self, editing: Option<&mff_core::Connection>) {
        self.conn_edit = Some(match editing {
            Some(c) => ConnEditor::from(c.clone()),
            None => ConnEditor::new(),
        });
    }

    pub fn show_conn_editor(&mut self, ctx: &Context) {
        let Some(editor) = self.conn_edit.as_mut() else {
            return;
        };
        let mut open = true;
        let mut save_clicked = false;
        let mut test_clicked = false;
        let mut cancel_clicked = false;
        egui::Window::new("Connection")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let mut db_tmp = editor.database_tmp.clone();
                let mut db_resp = None;
                egui::Grid::new("conn_form")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.conn.name)
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label("Host");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.conn.host)
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label("Port");
                        ui.add(egui::DragValue::new(&mut editor.conn.port).range(1..=65535));
                        ui.end_row();

                        ui.label("User");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.conn.user)
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label("Password");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.conn.password)
                                .password(true)
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label("Database (optional)");
                        db_resp = Some(
                            ui.add(
                                egui::TextEdit::singleline(&mut db_tmp)
                                    .desired_width(240.0)
                                    .hint_text("(leave empty for none)"),
                            ),
                        );
                        ui.end_row();

                        ui.label("Charset");
                        egui::ComboBox::from_id_salt("charset")
                            .selected_text(&editor.conn.charset)
                            .show_ui(ui, |ui| {
                                for cs in ["utf8mb4", "utf8", "latin1", "tis620"] {
                                    ui.selectable_value(
                                        &mut editor.conn.charset,
                                        cs.to_string(),
                                        cs,
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label("SSL/TLS");
                        egui::ComboBox::from_id_salt("ssl")
                            .selected_text(editor.conn.ssl_mode.to_string())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut editor.conn.ssl_mode,
                                    SslMode::Disabled,
                                    "disabled",
                                );
                                ui.selectable_value(
                                    &mut editor.conn.ssl_mode,
                                    SslMode::Preferred,
                                    "preferred",
                                );
                                ui.selectable_value(
                                    &mut editor.conn.ssl_mode,
                                    SslMode::Required,
                                    "required",
                                );
                                ui.selectable_value(
                                    &mut editor.conn.ssl_mode,
                                    SslMode::VerifyIdentity,
                                    "verify identity",
                                );
                            });
                        ui.end_row();

                        ui.label("Timeout (s)");
                        ui.add(
                            egui::DragValue::new(&mut editor.conn.timeout_secs)
                                .range(1..=600),
                        );
                        ui.end_row();

                        ui.label("Comments");
                        ui.add(
                            egui::TextEdit::multiline(&mut editor.conn.comments)
                                .desired_width(240.0)
                                .desired_rows(2),
                        );
                        ui.end_row();
                    });
                let _ = db_resp;
                editor.database_tmp = db_tmp;

                if let Some(err) = &editor.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let can_act = !editor.conn.name.trim().is_empty() && !editor.testing;
                    if ui.add_enabled(can_act, egui::Button::new("Save")).clicked() {
                        save_clicked = true;
                    }
                    if ui
                        .add_enabled(can_act, egui::Button::new("Test connection"))
                        .clicked()
                    {
                        test_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });
        if !open || cancel_clicked {
            self.conn_edit = None;
            return;
        }

        // database_tmp → Option<String> mapping.
        if let Some(editor) = self.conn_edit.as_mut() {
            let t = editor.database_tmp.trim().to_string();
            editor.conn.database = if t.is_empty() { None } else { Some(t) };
        }

        if save_clicked {
            if let Some(mut editor) = self.conn_edit.take() {
                let mut conn = editor.conn.clone();
                if editor.editing_id.is_none() {
                    conn.id = Connection::new_id();
                }
                match save_connection(&mut self.registry, conn.clone()) {
                    Ok(()) => self.status = format!("connection “{}” saved", conn.name),
                    Err(e) => {
                        editor.error = Some(e.to_string());
                        self.conn_edit = Some(editor);
                    }
                }
            }
        }
        if test_clicked {
            if let Some(editor) = self.conn_edit.as_mut() {
                editor.testing = true;
                editor.error = Some("testing…".into());
                let conn = editor.conn.clone();
                let tx = self.jobs.sender();
                std::thread::spawn(move || {
                    let msg = match mff_core::Session::connect(&conn) {
                        Ok(s) => format!("✓ connected — server version {}", s.info().version),
                        Err(e) => format!("✗ connect failed: {e}"),
                    };
                    let _ = tx.send(crate::jobs::UiMsg::Status(msg));
                });
            }
        }
    }

    /// Show the export dialog bound to the data view of one session.
    pub fn show_export_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let (table, rows) = self
            .sessions
            .get(conn_id)
            .map(|s| {
                (
                    s.data.table.clone(),
                    s.data.grid.as_ref().map(|g| g.row_count()).unwrap_or(0),
                )
            })
            .unwrap_or_default();

        let mut export_clicked = false;
        let mut close_clicked = false;
        {
            let dlg = self.export_dlg.get_or_insert_with(ExportDlg::new);
            let mut open = true;
            egui::Window::new(format!("Export “{table}”"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let d = dlg;
                    egui::Grid::new("export_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Format");
                            egui::ComboBox::from_id_salt("export_format")
                                .selected_text(d.format.extension())
                                .show_ui(ui, |ui| {
                                    for name in ExportFormat::all() {
                                        let f = ExportFormat::parse(name).unwrap();
                                        ui.selectable_value(&mut d.format, f, *name);
                                    }
                                });
                            ui.end_row();

                            ui.label("File");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut d.path)
                                        .desired_width(260.0)
                                        .hint_text("output file…"),
                                );
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .set_file_name(format!(
                                            "export.{}",
                                            d.format.extension()
                                        ))
                                        .save_file()
                                    {
                                        d.path = p.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            if d.format == ExportFormat::Csv {
                                ui.label("Delimiter");
                                egui::ComboBox::from_id_salt("csv_delim")
                                    .selected_text(match d.delimiter {
                                        '\t' => "\\t".to_string(),
                                        other => other.to_string(),
                                    })
                                    .show_ui(ui, |ui| {
                                        for c in [',', ';', '\t', '|'] {
                                            let label =
                                                if c == '\t' { "\\t" } else { &c.to_string() };
                                            ui.selectable_value(&mut d.delimiter, c, label);
                                        }
                                    });
                                ui.end_row();
                            }
                            if d.format == ExportFormat::Sql {
                                ui.label("Include CREATE TABLE");
                                ui.checkbox(&mut d.include_create, "");
                                ui.end_row();
                                ui.label("Rows per INSERT");
                                ui.add(
                                    egui::DragValue::new(&mut d.batch_size).range(1..=100_000),
                                );
                                ui.end_row();
                            }
                            ui.label("NULL text");
                            ui.add(
                                egui::TextEdit::singleline(&mut d.null_text)
                                    .desired_width(80.0),
                            );
                            ui.end_row();
                        });

                    // Fixed-height message area: long paths wrap/scroll
                    // instead of pushing the buttons down.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(44.0)
                        .show(ui, |ui| {
                            if let Some(msg) = &d.message {
                                ui.label(msg);
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can = !d.path.trim().is_empty() && rows > 0;
                        if ui
                            .add_enabled(
                                can,
                                egui::Button::new(format!("Export {rows} rows")),
                            )
                            .clicked()
                        {
                            export_clicked = true;
                        }
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            if !open {
                close_clicked = true;
            }
        }

        if close_clicked {
            if let Some(s) = self.sessions.get_mut(conn_id) {
                s.data.export_open = false;
            }
            self.export_dlg = None;
        } else if export_clicked {
            // Snapshot the dialog options, then run the export.
            if let Some(snapshot) = self.export_dlg.clone() {
                let result = run_export(self, conn_id, &snapshot);
                if let Some(dlg) = self.export_dlg.as_mut() {
                    dlg.message = Some(match result {
                        Ok(n) => format!("✓ exported {n} row(s) to {}", dlg.path),
                        Err(e) => format!("✗ {e}"),
                    });
                }
            }
        }
    }
}

fn save_connection(reg: &mut mff_core::Registry, conn: Connection) -> Result<(), mff_core::MffError> {
    reg.upsert(conn)?;
    reg.save_default()
}

impl App {
    /// Make sure the structure view has the schema of `table` (for SQL
    /// exports that embed CREATE TABLE).
    pub fn ensure_schema(&mut self, conn_id: &str, table: String) {
        let need = match self.sessions.get(conn_id) {
            Some(s) => s.structure.schema.is_none() || s.structure.table != table,
            None => false,
        };
        if need {
            self.load_schema(conn_id, table);
        }
    }
}

fn run_export(
    app: &mut App,
    conn_id: &str,
    dlg: &ExportDlg,
) -> std::result::Result<usize, String> {
    let Some(state) = app.sessions.get(conn_id) else {
        return Err("no session".into());
    };
    let Some(grid) = &state.data.grid else {
        return Err("no data loaded".into());
    };
    let schema = if dlg.format == ExportFormat::Sql && dlg.include_create {
        state.structure.schema.as_ref()
    } else {
        None
    };
    let opts = mff_core::export::ExportOptions {
        format: dlg.format,
        delimiter: dlg.delimiter,
        null_text: dlg.null_text.clone(),
        batch_size: dlg.batch_size,
        include_create: dlg.include_create,
        include_column_list: true,
        include_header: true,
    };
    let text =
        mff_core::export::export_to_string(grid, schema, &opts).map_err(|e| e.to_string())?;
    std::fs::write(&dlg.path, text).map_err(|e| e.to_string())?;
    Ok(grid.row_count())
}
