//! "Import data" dialog: load a CSV / JSON / SQL file into a table of the
//! current database. The heavy lifting is `mff_core::import`; this is the
//! UI around it, running the import in the background.

use egui::Context;
use std::path::PathBuf;

use crate::app::App;

/// Which file format the import will use (derived from the extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Csv,
    Json,
    Sql,
    Unknown,
}

impl ImportFormat {
    pub fn from_path(path: &str) -> Self {
        match PathBuf::from(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("csv") | Some("tsv") | Some("txt") => Self::Csv,
            Some("json") => Self::Json,
            Some("sql") => Self::Sql,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Sql => "SQL script",
            Self::Unknown => "?",
        }
    }
}

/// Dialog state (lives on `App`).
pub struct ImportDlg {
    pub path: String,
    pub table: String,
    pub delimiter: char,
    pub has_header: bool,
    pub empty_as_null: bool,
    pub null_text: String,
    pub continue_on_error: bool,
    pub message: Option<String>,
    pub running: bool,
}

impl ImportDlg {
    fn new(table: String) -> Self {
        Self {
            path: String::new(),
            table,
            delimiter: ',',
            has_header: true,
            empty_as_null: true,
            null_text: "NULL".into(),
            continue_on_error: false,
            message: None,
            running: false,
        }
    }
}

impl App {
    pub fn open_import_dialog(&mut self, conn_id: &str, table: String) {
        self.import_dlg = Some(ImportDlg::new(table));
        let _ = conn_id;
    }

    pub fn show_import_dialog(&mut self, ctx: &Context, conn_id: &str) {
        let format;
        let tables;
        let current_db;
        {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            format = self
                .import_dlg
                .as_ref()
                .map(|d| ImportFormat::from_path(&d.path))
                .unwrap_or(ImportFormat::Unknown);
            tables = state
                .objects
                .as_ref()
                .map(|o| {
                    o.tables
                        .iter()
                        .map(|t| t.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            current_db = state.current_db.clone();
        }

        let mut import_clicked = false;
        let mut close_clicked = false;
        {
            let Some(dlg) = self.import_dlg.as_mut() else {
                return;
            };
            let mut open = true;
            egui::Window::new("Import data")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::Grid::new("import_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("File");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut dlg.path)
                                        .desired_width(250.0)
                                        .hint_text("file.csv / .json / .sql"),
                                );
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter("Importable", &["csv", "tsv", "txt", "json", "sql"])
                                        .pick_file()
                                    {
                                        dlg.path = p.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Format");
                            ui.label(format.label());
                            ui.end_row();

                            if format != ImportFormat::Sql {
                                ui.label("Target table");
                                egui::ComboBox::from_id_salt("import_table")
                                    .selected_text(dlg.table.as_str())
                                    .show_ui(ui, |ui| {
                                        for t in &tables {
                                            ui.selectable_value(&mut dlg.table, t.clone(), t);
                                        }
                                    });
                                ui.end_row();
                            }

                            if format == ImportFormat::Csv {
                                ui.label("Delimiter");
                                egui::ComboBox::from_id_salt("import_delim")
                                    .selected_text(match dlg.delimiter {
                                        '\t' => "\\t".to_string(),
                                        other => other.to_string(),
                                    })
                                    .show_ui(ui, |ui| {
                                        for c in [',', ';', '\t', '|'] {
                                            let label = if c == '\t' {
                                                "\\t"
                                            } else {
                                                &c.to_string()
                                            };
                                            ui.selectable_value(&mut dlg.delimiter, c, label);
                                        }
                                    });
                                ui.end_row();
                                ui.label("First row = column names");
                                ui.checkbox(&mut dlg.has_header, "");
                                ui.end_row();
                                ui.label("Empty field = NULL");
                                ui.checkbox(&mut dlg.empty_as_null, "");
                                ui.end_row();
                                ui.label("NULL text");
                                ui.add(
                                    egui::TextEdit::singleline(&mut dlg.null_text)
                                        .desired_width(80.0),
                                );
                                ui.end_row();
                            }
                            if format == ImportFormat::Sql {
                                ui.label("Continue on error");
                                ui.checkbox(&mut dlg.continue_on_error, "");
                                ui.end_row();
                            }
                        });

                    if format == ImportFormat::Unknown && !dlg.path.trim().is_empty() {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            "⚠ unknown file extension — use .csv, .json or .sql",
                        );
                    }

                    // Fixed-height message area (same trick as export).
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(44.0)
                        .show(ui, |ui| {
                            if let Some(msg) = &dlg.message {
                                ui.label(msg);
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can = !dlg.path.trim().is_empty()
                            && format != ImportFormat::Unknown
                            && !dlg.running
                            && (format == ImportFormat::Sql || !dlg.table.is_empty());
                        let label = if dlg.running {
                            "importing…".to_string()
                        } else {
                            "Import".to_string()
                        };
                        if ui
                            .add_enabled(
                                can,
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
                            import_clicked = true;
                        }
                        if dlg.running {
                            ui.spinner();
                        }
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                    ui.weak(format!(
                        "into database “{current_db}” — rows are appended, nothing is deleted"
                    ));
                });
            if !open {
                close_clicked = true;
            }
        }

        if close_clicked {
            self.import_dlg = None;
        } else if import_clicked {
            self.run_import(conn_id);
        }
    }

    /// Start the import on a background thread.
    fn run_import(&mut self, conn_id: &str) {
        let Some(state) = self.sessions.get_mut(conn_id) else {
            return;
        };
        let Some(dlg) = self.import_dlg.as_mut() else {
            return;
        };
        if ImportFormat::from_path(&dlg.path) == ImportFormat::Unknown {
            dlg.message = Some("✗ unknown file type".into());
            return;
        }
        dlg.message = Some("importing…".into());
        dlg.running = true;
        let session = state.session.clone();
        let db = state.current_db.clone();
        let path = PathBuf::from(dlg.path.trim().to_string());
        let (table, delim, has_header, empty_as_null, null_text, cont) = {
            let d = dlg;
            (
                d.table.clone(),
                d.delimiter,
                d.has_header,
                d.empty_as_null,
                d.null_text.clone(),
                d.continue_on_error,
            )
        };
        let tx = self.jobs.sender();
        let id = conn_id.to_string();
        state.busy += 1;
        self.status = "importing…".into();
        std::thread::spawn(move || {
            let result: Result<usize, String> = (|| match ImportFormat::from_path(
                &path.to_string_lossy(),
            ) {
                ImportFormat::Csv => mff_core::import::import_csv(
                    &session,
                    &db,
                    &table,
                    &path,
                    &mff_core::import::CsvImportOptions {
                        delimiter: delim,
                        has_header,
                        empty_as_null,
                        null_text,
                        ..Default::default()
                    },
                    None,
                )
                .map_err(|e| e.to_string()),
                ImportFormat::Json => {
                    mff_core::import::import_json(&session, &db, &table, &path, None)
                        .map_err(|e| e.to_string())
                }
                ImportFormat::Sql => {
                    mff_core::import::import_sql(&session, &path, cont).map_err(|e| e.to_string())
                }
                ImportFormat::Unknown => Err("unknown file type".into()),
            })();
            let _ = tx.send(crate::jobs::UiMsg::ImportDone {
                conn_id: id,
                table,
                result,
            });
        });
    }
}

