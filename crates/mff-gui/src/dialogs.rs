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
    /// Editing buffer for the optional SSH key file path.
    pub ssh_key_tmp: String,
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
            ssh_key_tmp: String::new(),
            error: None,
            testing: false,
        }
    }

    pub fn from(conn: Connection) -> Self {
        let database_tmp = conn.database.clone().unwrap_or_default();
        let ssh_key_tmp = conn.ssh_key_path.clone().unwrap_or_default();
        Self {
            editing_id: Some(conn.id.clone()),
            conn,
            database_tmp,
            ssh_key_tmp,
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
    /// The table's WHERE filter at the time the dialog was opened.
    pub where_clause: String,
    /// The table's ORDER BY at the time the dialog was opened.
    pub order_by: Option<(String, bool)>,
    /// True while the background export is running.
    pub running: bool,
    /// Apply the WHERE/ORDER BY the data view currently uses.
    pub apply_filter: bool,
}

impl ExportDlg {
    fn new() -> Self {
        Self {
            format: ExportFormat::Csv,
            delimiter: ',',
            null_text: "NULL".into(),
            batch_size: 1000,
            include_create: true,
            apply_filter: true,
            ..Default::default()
        }
    }
}

impl App {
    pub fn open_conn_editor(&mut self, editing: Option<&mff_core::Connection>) {
        self.conn_edit = Some(match editing {
            Some(c) => {
                let mut conn = c.clone();
                // Keychain-resolved secrets, so Test works while editing.
                if conn.password_in_keychain && conn.password.is_empty() {
                    let _ = conn.load_password_keychain();
                }
                if conn.ssh_password_in_keychain && conn.ssh_password.is_empty() {
                    let _ = conn.load_ssh_password_keychain();
                }
                ConnEditor::from(conn)
            }
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
                editor.ssh_key_tmp = editor.conn.ssh_key_path.clone().unwrap_or_default();
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

                        ui.label("Password storage");
                        ui.checkbox(
                            &mut editor.conn.password_in_keychain,
                            "store in OS keychain",
                        )
                        .on_hover_text(
                            "The password is kept in Windows Credential Manager \
                             instead of connections.json",
                        );
                        ui.end_row();

                        ui.label("SSH tunnel");
                        ui.checkbox(&mut editor.conn.ssh_enabled, "connect through SSH")
                            .on_hover_text("Reach the server through an SSH host (ssh -L)");
                        ui.end_row();

                        if editor.conn.ssh_enabled {
                            ui.label("SSH host");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut editor.conn.ssh_host)
                                        .desired_width(140.0)
                                        .hint_text("(empty = same as Host)"),
                                );
                                ui.label(":");
                                ui.add(
                                    egui::DragValue::new(&mut editor.conn.ssh_port)
                                        .range(1..=65535),
                                );
                            });
                            ui.end_row();
                            ui.label("SSH user");
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.conn.ssh_user)
                                    .desired_width(240.0),
                            );
                            ui.end_row();
                            ui.label("SSH password");
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut editor.conn.ssh_password)
                                        .password(true)
                                        .desired_width(240.0),
                                )
                                .on_hover_text(
                                    "SSH password, or the passphrase when a key file is set",
                                );
                                ui.checkbox(
                                    &mut editor.conn.ssh_password_in_keychain,
                                    "store in OS keychain",
                                )
                                .on_hover_text(
                                    "The SSH password / key passphrase is kept in \
                                     Windows Credential Manager instead of connections.json",
                                );
                            });
                            ui.end_row();
                            ui.label("SSH key file");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut editor.ssh_key_tmp,
                                    )
                                    .desired_width(180.0)
                                    .hint_text("(empty = password auth)"),
                                );
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter("Key files", &["ppk", "pem", "key", "*"])
                                        .pick_file()
                                    {
                                        editor.ssh_key_tmp = p.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();
                        }
                    });
                let _ = db_resp;
                editor.database_tmp = db_tmp;
                let k = editor.ssh_key_tmp.trim().to_string();
                editor.conn.ssh_key_path = if k.is_empty() { None } else { Some(k) };

                if let Some(err) = &editor.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {err}"));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let can_act = !editor.conn.name.trim().is_empty() && !editor.testing;
                    if ui
                        .add_enabled(
                            can_act,
                            egui::Button::new(
                                egui::RichText::new("Save")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(crate::theme::accent(ui.ctx()))
                            .min_size(egui::vec2(0.0, 24.0)),
                        )
                        .clicked()
                    {
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
                if conn.password_in_keychain {
                    if let Err(e) = conn.store_password_keychain() {
                        editor.error = Some(e.to_string());
                        self.conn_edit = Some(editor);
                        return;
                    }
                    // The registry file never sees the password.
                    conn.password = String::new();
                } else {
                    conn.delete_password_keychain();
                }
                if conn.ssh_password_in_keychain {
                    if let Err(e) = conn.store_ssh_password_keychain() {
                        editor.error = Some(e.to_string());
                        self.conn_edit = Some(editor);
                        return;
                    }
                    conn.ssh_password = String::new();
                } else {
                    conn.delete_ssh_password_keychain();
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
        // Snapshot the data view's filter/sort when the dialog opens, and
        // reflect any later changes while it stays open.
        {
            let (filter, sort) = self
                .sessions
                .get(conn_id)
                .map(|s| (s.data.filter_applied.clone(), s.data.sort.clone()))
                .unwrap_or_default();
            let dlg = self.export_dlg.get_or_insert_with(ExportDlg::new);
            dlg.where_clause = filter;
            dlg.order_by = sort;
        }
        let rows = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.data.grid.as_ref())
            .map(|g| g.row_count())
            .unwrap_or(0);

        let mut export_clicked = false;
        let mut close_clicked = false;
        let table = self
            .sessions
            .get(conn_id)
            .map(|s| s.data.table.clone())
            .unwrap_or_default();
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

                    // Scope: the WHERE/ORDER BY currently in use.
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut d.apply_filter, "");
                        let scope = if d.where_clause.is_empty() {
                            "whole table".to_string()
                        } else {
                            let order = d
                                .order_by
                                .as_ref()
                                .map(|(c, asc)| {
                                    format!(" ORDER BY {c} {}", if *asc { "ASC" } else { "DESC" })
                                })
                                .unwrap_or_default();
                            format!("WHERE {}{order}", d.where_clause)
                        };
                        ui.label(format!("Scope: {scope}"));
                    });
                    ui.weak(if d.apply_filter {
                        "exports every matching row (not just the current page)"
                    } else {
                        "exports every row of the table"
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
                        let can = !d.path.trim().is_empty() && rows > 0 && !d.running;
                        if ui
                            .add_enabled(
                                can,
                                egui::Button::new(
                                    egui::RichText::new(if d.running {
                                        "exporting…".to_string()
                                    } else {
                                        format!("Export {rows}+ rows")
                                    })
                                    .strong()
                                    .color(egui::Color32::WHITE),
                                )
                                .fill(crate::theme::accent(ui.ctx()))
                                .min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            export_clicked = true;
                        }
                        if d.running {
                            ui.spinner();
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
            // Snapshot the dialog options and run the export in the
            // background (large tables must not block the UI thread).
            if let Some(snapshot) = self.export_dlg.clone() {
                self.run_export_background(conn_id, snapshot);
            }
        }
    }

    /// Export every row matching the current scope in the background.
    fn run_export_background(&mut self, conn_id: &str, dlg: ExportDlg) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let d = &state.data;
        let Some(grid) = &d.grid else { return };
        if grid.row_count() == 0 {
            if let Some(dl) = self.export_dlg.as_mut() {
                dl.message = Some("✗ no data loaded".into());
            }
            return;
        }
        let schema = if dlg.format == ExportFormat::Sql && dlg.include_create {
            state.structure.schema.clone()
        } else {
            None
        };
        let session = state.session.clone();
        let db = state.current_db.clone();
        let table = d.table.clone();
        let (where_clause, order_by, apply) = (
            dlg.where_clause.clone(),
            dlg.order_by.clone(),
            dlg.apply_filter,
        );
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        if let Some(dl) = self.export_dlg.as_mut() {
            dl.running = true;
            dl.message = Some("exporting…".into());
        }
        self.status = "exporting…".into();
        std::thread::spawn(move || {
            let result = run_export_job(
                &session,
                &db,
                &table,
                schema.as_ref(),
                &dlg,
                &where_clause,
                &order_by,
                apply,
            );
            let _ = tx.send(crate::jobs::UiMsg::ExportDone {
                conn_id: id,
                result,
            });
        });
    }
}

/// The background part of the export: fetch all matching rows, render and
/// write the file.
#[allow(clippy::too_many_arguments)]
fn run_export_job(
    session: &mff_core::Session,
    db: &str,
    table: &str,
    schema: Option<&mff_core::Table>,
    dlg: &ExportDlg,
    where_clause: &str,
    order_by: &Option<(String, bool)>,
    apply: bool,
) -> std::result::Result<usize, String> {
    // All matching rows (not just the current page) when a filter applies;
    // otherwise the full table.
    let (where_opt, order_opt) = if apply {
        (
            Some(where_clause.trim()).filter(|w| !w.is_empty()),
            order_by.as_ref().map(|(c, a)| (c.as_str(), *a)),
        )
    } else {
        (None, None)
    };
    let total = session
        .count_rows(db, table, where_opt)
        .unwrap_or(0)
        .max(1);
    let grid = session
        .table_rows(db, table, where_opt, order_opt, 0, total)
        .map_err(|e| e.to_string())?;
    if grid.row_count() == 0 {
        return Err("no rows match the scope".into());
    }
    let opts = mff_core::export::ExportOptions {
        format: dlg.format,
        delimiter: dlg.delimiter,
        null_text: dlg.null_text.clone(),
        batch_size: dlg.batch_size,
        include_create: dlg.include_create,
        include_column_list: true,
        include_header: true,
    };
    let text = mff_core::export::export_to_string(&grid, schema, &opts)
        .map_err(|e| e.to_string())?;
    std::fs::write(&dlg.path, text).map_err(|e| e.to_string())?;
    Ok(grid.row_count())
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
