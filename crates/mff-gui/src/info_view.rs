//! Info view: server variables with a filter — a light version of the
//! original variables browser.

use crate::app::App;

impl App {
    pub fn show_info_view(&mut self, ui: &mut egui::Ui, conn_id: &str) {
        let mut load_clicked = false;
        let filter;
        let loading;
        let variables: Option<Vec<(String, String)>> = {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            ui.horizontal(|ui| {
                ui.strong("Server information");
                if state.info.loading {
                    ui.spinner();
                } else if ui.button("⟳ Load variables").clicked() {
                    load_clicked = true;
                }
                ui.separator();
                ui.label("filter:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.info.filter)
                        .hint_text("e.g. innodb")
                        .desired_width(240.0),
                );
            });
            loading = state.info.loading;
            filter = state.info.filter.clone();
            state.info.variables.clone()
        };

        if load_clicked && !loading {
            self.load_variables(conn_id);
        }

        ui.add_space(6.0);
        let Some(vars) = variables else {
            if !loading {
                ui.label("press “Load variables” to fetch SHOW VARIABLES");
            }
            return;
        };

        let filter = filter.to_lowercase();
        let shown: Vec<&(String, String)> = vars
            .iter()
            .filter(|(k, _)| filter.is_empty() || k.to_lowercase().contains(&filter))
            .collect();

        ui.label(format!("{} variable(s)", shown.len()));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("vars_grid")
                    .num_columns(2)
                    .striped(true)
                    .min_col_width(120.0)
                    .show(ui, |ui| {
                        ui.strong("Variable");
                        ui.strong("Value");
                        ui.end_row();
                        for (k, v) in shown {
                            ui.label(k);
                            ui.monospace(v);
                            ui.end_row();
                        }
                    });
            });
    }
}
