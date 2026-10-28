//! Query-result charting: one line/point series from a result grid using
//! egui_plot. Numeric cells are plotted as-is; non-numeric X values fall
//! back to the row number.

use std::sync::Arc;

use egui::Context;
use egui_plot::{Line, Plot, PlotPoints, Points};

use crate::app::App;
use crate::data_view::cell_text;
use mff_core::session::Grid;

pub struct ChartDlg {
    pub grid: Arc<Grid>,
    pub x_col: usize,
    pub y_col: usize,
    pub as_points: bool,
}

impl ChartDlg {
    pub fn new(grid: Arc<Grid>) -> Self {
        // Defaults: last column as Y (usually a measure), X = row index.
        let y_col = grid.columns.len().saturating_sub(1);
        Self {
            grid,
            x_col: usize::MAX, // MAX = row index
            y_col,
            as_points: false,
        }
    }

    fn numeric_value(cell: &mff_core::session::Cell) -> Option<f64> {
        let t = cell_text(cell);
        if t.is_empty() || t == "NULL" {
            return None;
        }
        t.replace(',', "").parse::<f64>().ok()
    }

    fn column_values(&self, col: usize) -> Vec<(usize, f64)> {
        // (row index, value) — only rows where the cell is numeric
        self.grid
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| row.get(col).and_then(Self::numeric_value).map(|v| (i, v)))
            .collect()
    }
}

impl App {
    pub fn show_chart_dialog(&mut self, ctx: &Context) {
        let Some(dlg) = self.chart.as_mut() else {
            return;
        };
        let mut open = true;
        let mut close_clicked = false;

        let ncols = dlg.grid.columns.len();
        let col_names: Vec<String> = dlg
            .grid
            .columns
            .iter()
            .map(|c| c.name.clone())
            .chain(std::iter::once("(row #)".to_string()))
            .collect();

        egui::Window::new("Chart")
            .open(&mut open)
            .default_width(680.0)
            .default_height(420.0)
            .show(ctx, |ui| {
                // Pickers.
                let x_names = col_names.clone();
                let y_names: Vec<String> = dlg
                    .grid
                    .columns
                    .iter()
                    .map(|c| c.name.clone())
                    .collect();
                ui.horizontal(|ui| {
                    ui.label("X:");
                    egui::ComboBox::from_id_salt("chart_x")
                        .selected_text(x_names[dlg.x_col.min(x_names.len() - 1)].clone())
                        .show_ui(ui, |ui| {
                            for (i, n) in x_names.iter().enumerate() {
                                ui.selectable_value(&mut dlg.x_col, i, n);
                            }
                        });
                    ui.label("Y:");
                    egui::ComboBox::from_id_salt("chart_y")
                        .selected_text(y_names[dlg.y_col.min(y_names.len() - 1)].clone())
                        .show_ui(ui, |ui| {
                            for (i, n) in y_names.iter().enumerate() {
                                ui.selectable_value(&mut dlg.y_col, i, n);
                            }
                        });
                    ui.checkbox(&mut dlg.as_points, "points");
                });

                let (title, mk_points) = {
                    let g = &dlg.grid;
                    let ys = dlg.column_values(dlg.y_col);
                    let label = g
                        .columns
                        .get(dlg.y_col)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "value".into());
                    // X: chosen column (numeric) or the row number.
                    let pts: PlotPoints = if dlg.x_col == usize::MAX {
                        ys.iter()
                            .map(|(i, v)| [*i as f64, *v])
                            .collect::<Vec<_>>()
                            .into()
                    } else {
                        let xs = dlg.column_values(dlg.x_col);
                        // Pair by row index.
                        ys.iter()
                            .filter_map(|(i, v)| {
                                xs.iter()
                                    .find(|(xi, _)| xi == i)
                                    .map(|(xi, xv)| [*xv, *v])
                            })
                            .collect::<Vec<_>>()
                            .into()
                    };
                    (label, pts)
                };
                let _ = &mk_points;

                Plot::new("result_chart")
                    .legend(egui_plot::Legend::default())
                    .height(320.0)
                    .show(ui, |plot_ui| {
                        if dlg.as_points {
                            plot_ui.points(Points::new(title, mk_points));
                        } else {
                            plot_ui.line(Line::new(title, mk_points));
                        }
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.weak(format!(
                        "{} point(s) plotted (non-numeric cells are skipped)",
                        dlg.column_values(dlg.y_col).len()
                    ));
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            });

        if !open || close_clicked {
            self.chart = None;
        }
    }
}

/// Open a chart for a result grid (query view button).
impl App {
    pub fn open_chart(&mut self, grid: Grid) {
        self.chart = Some(ChartDlg::new(Arc::new(grid)));
    }
}
