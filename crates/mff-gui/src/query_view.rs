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
        let lang = self.lang;
        use crate::i18n::t;

        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;
        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let mut explain_clicked = false;
        let mut chart_grid: Option<Grid> = None;
        // Non-empty editor selection → Run executes only the selected SQL.
        let mut selected_sql: Option<String> = None;
        let elapsed: Option<u128>;
        // Suggestion chosen via Enter/Tab (set before the editor renders).
        let mut key_accept = false;
        // Find bar events collected while rendering it.
        let mut close_find = false;
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
                        egui::RichText::new(t(lang, "▶ Run (Ctrl+Enter)"))
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
                    .button(t(lang, "✨ Format"))
                    .on_hover_text("Reformat the SQL (uppercase keywords, one clause per line)")
                    .clicked()
                {
                    format_clicked = true;
                }
                if ui
                    .button(t(lang, "🔍 Explain"))
                    .on_hover_text(
                        "Run EXPLAIN on every SELECT/INSERT/UPDATE/DELETE/REPLACE statement",
                    )
                    .clicked()
                {
                    explain_clicked = true;
                }
                if ui.button(t(lang, "✕ Clear results")).clicked() {
                    clear_clicked = true;
                }
                if ui
                    .selectable_label(state.history_open, t(lang, "🕘 History"))
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

            // Ctrl+F toggles the find/replace bar (query view owns Ctrl+F;
            // the global object search is Ctrl+Shift+F here).
            let ctrl_f = ui
                .ctx()
                .input(|i| i.key_pressed(Key::F) && i.modifiers.ctrl && !i.modifiers.shift);
            if ctrl_f {
                tab.find = match tab.find {
                    Some(_) => None,
                    None => Some(FindState::default()),
                };
            }

            // ── Find/Replace bar ───────────────────────────────────────
            let mut find_next_clicked = false;
            let mut replace_clicked = false;
            let mut replace_all_clicked = false;
            if let Some(f) = tab.find.as_mut() {
                ui.horizontal(|ui| {
                    ui.weak("Find:");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut f.what)
                            .desired_width(180.0)
                            .hint_text("search…"),
                    );
                    if resp.changed() {
                        f.from_byte = 0;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        find_next_clicked = true;
                    }
                    ui.checkbox(&mut f.case_sensitive, "Aa")
                        .on_hover_text("case sensitive");
                    if ui.button("⤵ Next").clicked() {
                        find_next_clicked = true;
                    }
                    ui.separator();
                    ui.weak("Replace with:");
                    ui.add(
                        egui::TextEdit::singleline(&mut f.replacement).desired_width(140.0),
                    );
                    if ui.button("Replace").clicked() {
                        replace_clicked = true;
                    }
                    if ui.button("All").clicked() {
                        replace_all_clicked = true;
                    }
                    if ui.button("✕").on_hover_text("close find bar").clicked() {
                        close_find = true;
                    }
                });
            }

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
            // Ctrl+Space force-opens the popup even for a short prefix and
            // cancels a previous Escape dismissal.
            let ctrl_space = ui.ctx().input(|i| {
                i.key_pressed(egui::Key::Space) && i.modifiers.ctrl && !i.modifiers.shift && !i.modifiers.alt
            });
            if ctrl_space {
                tab.suggest.dismissed_word = None;
            }

            // Capture the selection for "run selection" (char indices).
            if let Some(st) = egui::TextEdit::load_state(ui.ctx(), editor_resp.id) {
                if let Some(range) = st.cursor.char_range() {
                    let a = range.secondary.index.min(range.primary.index);
                    let b = range.primary.index.max(range.secondary.index);
                    if a != b {
                        let (ba, bb) = char_span(&tab.code, a, b);
                        let s = tab.code[ba..bb].trim().to_string();
                        if !s.is_empty() {
                            selected_sql = Some(s);
                        }
                    }
                }
            }

            // ── Find/Replace actions (need the editor id) ──────────────
            let find_params = tab
                .find
                .as_ref()
                .map(|f| (f.what.clone(), f.replacement.clone(), f.case_sensitive));
            if let Some((what, repl, case)) = find_params {
                if find_next_clicked || replace_clicked {
                    let mut from = tab.find.as_ref().map(|f| f.from_byte).unwrap_or(0);
                    if replace_clicked {
                        // Replace the current selection when it is a match.
                        if let Some(st) = egui::TextEdit::load_state(ui.ctx(), editor_resp.id) {
                            if let Some(range) = st.cursor.char_range() {
                                let a = range.secondary.index.min(range.primary.index);
                                let b = range.primary.index.max(range.secondary.index);
                                let (ba, bb) = char_span(&tab.code, a, b);
                                let sel = &tab.code[ba..bb];
                                let is_match = if case {
                                    sel == what
                                } else {
                                    sel.eq_ignore_ascii_case(&what)
                                };
                                if is_match {
                                    tab.code.replace_range(ba..bb, &repl);
                                    from = ba + repl.len();
                                }
                            }
                        }
                    }
                    match find_match(&tab.code, from, &what, case) {
                        Some((s, e)) => {
                            let ca = tab.code[..s].chars().count();
                            let ce = tab.code[..e].chars().count();
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), editor_resp.id)
                            {
                                st.cursor.set_char_range(Some(
                                    egui::text::CCursorRange::two(
                                        egui::text::CCursor::new(ca),
                                        egui::text::CCursor::new(ce),
                                    ),
                                ));
                                egui::TextEdit::store_state(ui.ctx(), editor_resp.id, st);
                            }
                            let next_from = if e < tab.code.len() { e } else { 0 };
                            if let Some(f) = tab.find.as_mut() {
                                f.from_byte = next_from;
                            }
                        }
                        None => {
                            if let Some(f) = tab.find.as_mut() {
                                f.from_byte = 0;
                            }
                            self.status = format!("\"{what}\" not found");
                        }
                    }
                }
                if replace_all_clicked {
                    let (new_code, n) = replace_all(&tab.code, &what, &repl, case);
                    tab.code = new_code;
                    if let Some(f) = tab.find.as_mut() {
                        f.from_byte = 0;
                    }
                    self.status = if n == 0 {
                        format!("\"{what}\" not found")
                    } else {
                        format!("replaced {n} occurrence(s)")
                    };
                }
            }
            if close_find {
                tab.find = None;
            }

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
                // Open while typing (2+ chars), right after a dot (every
                // column of the table applies) or via Ctrl+Space.
                } else if focused
                    && !items.is_empty()
                    && (word.chars().count() >= 2 || sctx.dotted || ctrl_space)
                    && (ctrl_space
                        || tab.suggest.dismissed_word.as_deref() != Some(word_lower.as_str()))
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
            // With a non-empty selection only that SQL is executed.
            let sql = selected_sql.unwrap_or_else(|| code.clone());
            self.run_query(conn_id, active, sql, false);
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
                                if ui
                                    .small_button("📈 Chart…")
                                    .on_hover_text("Plot a numeric column")
                                    .clicked()
                                {
                                    chart_grid = Some(grid.clone());
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
        // The Chart button sets the flag while rendering results (above).
        if let Some(grid) = chart_grid.take() {
            self.open_chart(grid);
        }
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

/// Find/Replace bar state of one query tab's editor.
#[derive(Default)]
pub struct FindState {
    pub what: String,
    pub replacement: String,
    pub case_sensitive: bool,
    /// Byte offset the next search starts from (code length on wrap).
    pub from_byte: usize,
}

/// Convert a character-index span (a, b) into a byte-index span over
/// `code`, clamped to the text.
fn char_span(code: &str, a: usize, b: usize) -> (usize, usize) {
    let mut byte_a = code.len();
    let mut byte_b = code.len();
    for (i, (bi, _)) in code.char_indices().enumerate() {
        if i == a {
            byte_a = bi;
        }
        if i == b {
            byte_b = bi;
            break;
        }
    }
    (byte_a.min(byte_b), byte_b.max(byte_a))
}

/// Find the next occurrence of `what` in `code` starting at byte offset
/// `from`, wrapping once; returns the byte range of the match.
fn find_match(code: &str, from: usize, what: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if what.is_empty() {
        return None;
    }
    let (hay, needle) = if case_sensitive {
        (code.to_string(), what.to_string())
    } else {
        (code.to_lowercase(), what.to_lowercase())
    };
    let from = from.min(hay.len());
    if let Some(pos) = hay[from..].find(&needle) {
        return Some((from + pos, from + pos + needle.len()));
    }
    // wrap around
    hay[..from].find(&needle).map(|pos| (pos, pos + needle.len()))
}

/// Replace every occurrence; returns (new code, count).
fn replace_all(code: &str, what: &str, with: &str, case_sensitive: bool) -> (String, usize) {
    if what.is_empty() {
        return (code.to_string(), 0);
    }
    let (hay, needle) = if case_sensitive {
        (code.to_string(), what.to_string())
    } else {
        (code.to_lowercase(), what.to_lowercase())
    };
    let mut out = String::with_capacity(code.len());
    let mut last = 0;
    let mut count = 0;
    while let Some(pos) = hay[last..].find(&needle) {
        let start = last + pos;
        let end = start + needle.len();
        out.push_str(&code[last..start]);
        out.push_str(with);
        last = end;
        count += 1;
    }
    if count == 0 {
        return (code.to_string(), 0);
    }
    out.push_str(&code[last..]);
    (out, count)
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
