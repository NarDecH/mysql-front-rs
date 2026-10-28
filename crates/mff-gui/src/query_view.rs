//! Query view: SQL editor with multiple tabs, statement splitting and
//! stacked result sections — the workbench of the original application.

use egui::{Context, Key, Modifiers};
use mff_core::session::Grid;
use mff_core::suggest::{SuggestCtx, SuggestKind};

use crate::app::App;
use crate::data_view::render_grid;
use crate::jobs::QuerySection;
use crate::session_view::QueryTab;

impl App {
    pub fn show_query_view(&mut self, ui: &mut egui::Ui, ctx: &Context, conn_id: &str) {
        // ── Tab bar ────────────────────────────────────────────────────
        let (active, ntabs, running) = {
            let Some(state) = self.sessions.get(conn_id) else {
                return;
            };
            (
                state.active_query,
                state.queries.len(),
                state
                    .queries
                    .get(state.active_query)
                    .map(|t| t.running)
                    .unwrap_or(false),
            )
        };
        let mut switch_to: Option<usize> = None;
        let mut add_tab = false;
        let mut close_tab = false;
        ui.horizontal(|ui| {
            for i in 0..ntabs {
                let busy = self
                    .sessions
                    .get(conn_id)
                    .and_then(|s| s.queries.get(i))
                    .map(|t| t.running)
                    .unwrap_or(false);
                let label = if busy {
                    format!("⏳ Tab {}", i + 1)
                } else {
                    format!("Tab {}", i + 1)
                };
                if ui.selectable_label(i == active, label).clicked() {
                    switch_to = Some(i);
                }
            }
            if ui.button("+").clicked() {
                add_tab = true;
            }
            if ntabs > 1 && ui.button("✕ close").clicked() {
                close_tab = true;
            }
        });
        if let Some(i) = switch_to {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                state.active_query = i;
            }
        }
        if add_tab {
            let state = self.sessions.get_mut(conn_id).unwrap();
            state
                .queries
                .push(QueryTab::new(format!("Query {}", state.queries.len() + 1)));
            state.active_query = state.queries.len() - 1;
        }
        if close_tab && !running {
            let state = self.sessions.get_mut(conn_id).unwrap();
            state.queries.remove(active);
            state.active_query = state.active_query.saturating_sub(1).min(state.queries.len() - 1);
        }
        ui.separator();

        // ── Editor + toolbar (mutate only the tab) ─────────────────────
        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;
        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let mut explain_clicked = false;
        let elapsed: Option<u128>;
        // Suggestion chosen via Enter/Tab (set before the editor renders).
        let mut key_accept = false;
        // Make sure the completion index for the current db is loaded.
        let completion = {
            let st = self.sessions.get(conn_id);
            let (db, have, loading) = st
                .map(|s| {
                    (
                        s.current_db.clone(),
                        s.completion.db.clone(),
                        s.completion.loading,
                    )
                })
                .unwrap_or_default();
            if !db.is_empty() && have.as_deref() != Some(db.as_str()) && !loading {
                self.load_completion(conn_id, db);
            }
            self.sessions
                .get(conn_id)
                .and_then(|s| s.completion.index.clone())
                .filter(|_| {
                    self.sessions
                        .get(conn_id)
                        .map(|s| s.completion.db.as_deref() == Some(s.current_db.as_str()))
                        .unwrap_or(false)
                })
        };
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };

            let Some(tab) = state.queries.get_mut(active) else {
                return;
            };
            ui.horizontal(|ui| {
                let c = crate::theme::accent(ui.ctx());
                let run = ui.add_enabled(
                    !tab.running,
                    egui::Button::new(
                        egui::RichText::new("▶ Run (Ctrl+Enter)")
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(c)
                    .min_size(egui::vec2(0.0, 24.0)),
                );
                if run.clicked() {
                    run_clicked = true;
                }
                if ui
                    .button("✨ Format")
                    .on_hover_text("Reformat the SQL (uppercase keywords, one clause per line)")
                    .clicked()
                {
                    format_clicked = true;
                }
                if ui
                    .button("🔍 Explain")
                    .on_hover_text(
                        "Run EXPLAIN on every SELECT/INSERT/UPDATE/DELETE/REPLACE statement",
                    )
                    .clicked()
                {
                    explain_clicked = true;
                }
                if ui.button("✕ Clear results").clicked() {
                    clear_clicked = true;
                }
                if ui
                    .selectable_label(state.history_open, "🕘 History")
                    .clicked()
                {
                    state.history_open = !state.history_open;
                }
                if let Some(ms) = tab.elapsed_ms {
                    ui.label(format!("last run: {ms} ms"));
                }
                if tab.running {
                    ui.spinner();
                    ui.small("executing…");
                }
            });
            elapsed = tab.elapsed_ms;
            code = tab.code.clone();

            // While the popup is open, capture the navigation keys before
            // the editor turns them into caret moves / newlines.
            if tab.suggest.open && tab.suggest.had_focus {
                ui.ctx().input_mut(|i| {
                    if i.consume_key(Modifiers::NONE, Key::Escape) {
                        tab.suggest.open = false;
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        let last = tab.suggest.items.len().saturating_sub(1);
                        tab.suggest.selected = (tab.suggest.selected + 1).min(last);
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        tab.suggest.selected = tab.suggest.selected.saturating_sub(1);
                    }
                    if i.consume_key(Modifiers::NONE, Key::Tab)
                        || i.consume_key(Modifiers::NONE, Key::Enter)
                    {
                        key_accept = true;
                    }
                });
            }

            let mut edited = code.clone();
            let editor = egui::TextEdit::multiline(&mut edited)
                .hint_text("type SQL here — multiple statements allowed")
                .font(egui::TextStyle::Monospace)
                .desired_rows(8)
                .desired_width(f32::INFINITY)
                .code_editor();
            let editor_resp = ui.add(editor);
            if edited != tab.code {
                tab.code = edited;
            }
            code = tab.code.clone();

            // ── Suggestion logic ───────────────────────────────────────
            let focused = editor_resp.has_focus();
            tab.suggest.had_focus = focused;
            let mut ctx_and_items: Option<(SuggestCtx, Vec<mff_core::suggest::Suggestion>)> = None;
            if focused {
                if let Some(st) = egui::TextEdit::load_state(ui.ctx(), editor_resp.id) {
                    if let Some(range) = st.cursor.char_range() {
                        let caret = range.primary.index;
                        if let Some(ix) = &completion {
                            let sctx = mff_core::suggest::context(ix, &tab.code, caret);
                            let items = mff_core::suggest::candidates(&sctx, ix, 12);
                            ctx_and_items = Some((sctx, items));
                        }
                    }
                }
            }

            if let Some((sctx, items)) = ctx_and_items {
                let word = sctx.hit.text.clone();
                let word_lower = word.to_lowercase();

                if key_accept && tab.suggest.open {
                    let chosen = tab
                        .suggest
                        .items
                        .get(tab.suggest.selected)
                        .cloned();
                    if let Some(s) = chosen {
                        apply_suggestion(
                            tab,
                            ui.ctx(),
                            editor_resp.id,
                            &sctx.hit,
                            &s.text,
                            s.kind,
                        );
                        tab.suggest.open = false;
                        tab.suggest.dismissed_word = Some(s.text.to_lowercase());
                        code = tab.code.clone();
                    }
                } else if tab.suggest.open {
                    // Keep the list fresh; close when nothing matches or the
                    // editor lost focus.
                    tab.suggest.items = items;
                    tab.suggest.selected = tab
                        .suggest
                        .selected
                        .min(tab.suggest.items.len().saturating_sub(1));
                    if !focused || tab.suggest.items.is_empty() {
                        tab.suggest.open = false;
                    }
                // Open while typing (2+ chars) or right after a dot, where
                // every column of the table applies.
                } else if focused
                    && !items.is_empty()
                    && (word.chars().count() >= 2 || sctx.dotted)
                    && tab.suggest.dismissed_word.as_deref() != Some(word_lower.as_str())
                {
                    tab.suggest.open = true;
                    tab.suggest.items = items;
                    tab.suggest.selected = 0;
                }

                if tab.suggest.open && !tab.suggest.items.is_empty() {
                    if let Some(picked) = render_suggest_popup(ui, &editor_resp, &tab.suggest) {
                        let s = tab.suggest.items.get(picked).cloned();
                        if let Some(s) = s {
                            apply_suggestion(
                                tab,
                                ui.ctx(),
                                editor_resp.id,
                                &sctx.hit,
                                &s.text,
                                s.kind,
                            );
                            tab.suggest.open = false;
                            tab.suggest.dismissed_word = Some(s.text.to_lowercase());
                            code = tab.code.clone();
                        }
                    }
                }
            } else if tab.suggest.open {
                tab.suggest.open = false;
            }
        }
        if format_clicked {
            if let Some(state) = self.sessions.get_mut(conn_id) {
                if let Some(tab) = state.queries.get_mut(active) {
                    tab.code = mff_core::sqlfmt::format(&tab.code);
                    code = tab.code.clone();
                }
            }
        }

        // ── History panel ──────────────────────────────────────────────
        if self
            .sessions
            .get(conn_id)
            .map(|s| s.history_open)
            .unwrap_or(false)
        {
            let history = self.sessions.get(conn_id).map(|s| s.history.clone()).unwrap_or_default();
            ui.separator();
            ui.small(format!("query history ({} most recent)", history.len()));
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if history.is_empty() {
                        ui.weak("no queries executed yet");
                    }
                    let mut picked: Option<String> = None;
                    for (i, h) in history.iter().enumerate() {
                        let preview = h
                            .lines()
                            .next()
                            .unwrap_or("")
                            .chars()
                            .take(90)
                            .collect::<String>();
                        if ui
                            .selectable_label(false, format!("{}. {}", i + 1, preview))
                            .clicked()
                        {
                            picked = Some(h.clone());
                        }
                    }
                    if let Some(sql) = picked {
                        if let Some(state) = self.sessions.get_mut(conn_id) {
                            if let Some(tab) = state.queries.get_mut(active) {
                                tab.code = sql;
                            }
                        }
                    }
                });
            ui.separator();
        }

        if explain_clicked && !running {
            let stmts = mff_core::sqlsplit::split_statements(&code);
            let mut parts: Vec<String> = Vec::new();
            for st in &stmts {
                let first = st
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches('`')
                    .to_ascii_uppercase();
                if matches!(
                    first.as_str(),
                    "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "TABLE"
                ) {
                    parts.push(format!("EXPLAIN {}", st.trim()));
                }
            }
            if parts.is_empty() {
                self.status = "nothing to explain (needs SELECT/INSERT/UPDATE/DELETE/REPLACE)".into();
            } else {
                self.run_query(conn_id, active, parts.join(";\n"), false);
            }
        }

        let ctrl_enter = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);
        if clear_clicked {
            if let Some(tab) = self
                .sessions
                .get_mut(conn_id)
                .and_then(|s| s.queries.get_mut(active))
            {
                tab.sections.clear();
                tab.elapsed_ms = None;
            }
        }
        if (run_clicked || ctrl_enter) && !running {
            self.run_query(conn_id, active, code, false);
        }

        // ── Results ────────────────────────────────────────────────────
        ui.add_space(6.0);
        ui.separator();
        let sections: Vec<QuerySection> = self
            .sessions
            .get(conn_id)
            .and_then(|s| s.queries.get(active))
            .map(|t| t.sections.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if sections.is_empty() {
                    ui.small("no results yet");
                }
                for (i, section) in sections.iter().enumerate() {
                    match section {
                        QuerySection::Grid(grid) => {
                            ui.horizontal(|ui| {
                                ui.strong(format!(
                                    "Result {} — {} row(s)",
                                    i + 1,
                                    grid.row_count()
                                ));
                                if ui.small_button("⭳ Export…").clicked() {
                                    export_grid_interactive(grid);
                                }
                            });
                            let mut sel = None;
                            render_grid(ui, grid, &mut sel, None);
                            ui.add_space(8.0);
                        }
                        QuerySection::Affected {
                            affected,
                            last_insert_id,
                            info,
                        } => {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 200, 120),
                                    format!(
                                        "✓ Result {}: {affected} row(s) affected, last_insert_id={last_insert_id}",
                                        i + 1
                                    ),
                                );
                                if let Some(info) = info {
                                    ui.small(info);
                                }
                            });
                        }
                        QuerySection::Error(e) => {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!("✗ Result {}: {e}", i + 1),
                            );
                        }
                    }
                }
                if let Some(ms) = elapsed {
                    ui.small(format!("total: {ms} ms"));
                }
            });
        let _ = ctx;
    }
}

/// Ask for a file name and write one grid to it (format from extension).
fn export_grid_interactive(grid: &Grid) {
    let file = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .add_filter("JSON", &["json"])
        .add_filter("SQL inserts", &["sql"])
        .add_filter("HTML", &["html"])
        .add_filter("XML", &["xml"])
        .set_file_name("result.csv")
        .save_file();
    let Some(path) = file else { return };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("csv")
        .to_string();
    let Some(format) = mff_core::ExportFormat::parse(&ext) else {
        return;
    };
    let opts = mff_core::export::ExportOptions {
        format,
        ..Default::default()
    };
    if let Ok(text) = mff_core::export::export_to_string(grid, None, &opts) {
        let _ = std::fs::write(&path, text);
    }
}

/// Autocomplete popup state of one query tab's editor.
#[derive(Default)]
pub struct SuggestState {
    pub open: bool,
    /// Ranked suggestions for the current word (refreshed every frame).
    pub items: Vec<mff_core::suggest::Suggestion>,
    /// Highlighted entry.
    pub selected: usize,
    /// Word at the time the popup was dismissed with Esc — auto-open
    /// stays off until the word changes.
    pub dismissed_word: Option<String>,
    /// Whether the editor had keyboard focus on the previous frame.
    pub had_focus: bool,
}

/// Replace the word under the caret with the accepted suggestion and
/// restore the caret after the inserted text.
fn apply_suggestion(
    tab: &mut crate::session_view::QueryTab,
    ctx: &egui::Context,
    editor_id: egui::Id,
    hit: &mff_core::suggest::WordHit,
    text: &str,
    kind: SuggestKind,
) {
    let chars: Vec<char> = tab.code.chars().collect();
    let end = hit.end.min(chars.len());
    let mut out: String = chars[..hit.start].iter().collect();
    out.push_str(text);
    let mut new_caret = hit.start + text.chars().count();
    let rest: String = chars[end..].iter().collect();
    // Keywords and tables read better with a space after them.
    if matches!(kind, SuggestKind::Keyword | SuggestKind::Table)
        && !rest.starts_with(' ')
        && !rest.starts_with('\n')
        && !rest.starts_with('(')
        && !rest.starts_with(';')
    {
        out.push(' ');
        new_caret += 1;
    }
    out.push_str(&rest);
    tab.code = out;

    let mut st = egui::TextEdit::load_state(ctx, editor_id).unwrap_or_default();
    st.cursor
        .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
            new_caret,
        ))));
    egui::TextEdit::store_state(ctx, editor_id, st);
}

/// The floating suggestion list under the SQL editor. Returns the index
/// of a clicked entry, if any.
fn render_suggest_popup(
    ui: &mut egui::Ui,
    editor_resp: &egui::Response,
    suggest: &SuggestState,
) -> Option<usize> {
    let mut picked = None;
    let pos = editor_resp.rect.left_bottom() + egui::vec2(0.0, 4.0);
    let area_id = egui::Id::new(("suggest_popup", editor_resp.id));
    egui::Area::new(area_id)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .movable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(240.0);
                for (i, s) in suggest.items.iter().enumerate() {
                    let mut rich = match s.kind {
                        SuggestKind::Table => egui::RichText::new(&s.text).strong(),
                        SuggestKind::Column => egui::RichText::new(&s.text),
                        SuggestKind::Keyword => egui::RichText::new(&s.text).weak().italics(),
                    };
                    if s.kind != SuggestKind::Keyword {
                        rich = rich.monospace();
                    }
                    let label = ui
                        .selectable_label(i == suggest.selected, rich)
                        .on_hover_text(match s.kind {
                            SuggestKind::Table => "table / view — Enter or Tab to insert",
                            SuggestKind::Column => "column — Enter or Tab to insert",
                            SuggestKind::Keyword => "keyword — Enter or Tab to insert",
                        });
                    if label.clicked() {
                        picked = Some(i);
                    }
                }
            });
        });
    picked
}
