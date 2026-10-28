#!/usr/bin/env python3
"""Add the SQL autocomplete UI to query_view.rs."""
import io

p = "crates/mff-gui/src/query_view.rs"
s = io.open(p, encoding="utf-8").read()

# 1. imports
s = s.replace(
    """use egui::Context;
use mff_core::session::Grid;""",
    """use egui::{Context, Key, Modifiers};
use mff_core::session::Grid;
use mff_core::suggest::{SuggestCtx, SuggestKind};""",
)

# 2. SuggestState type at the end of file
s += '''
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
'''

# 3. rewrite the editor block with autocomplete
old = """        // ── Editor + toolbar (mutate only the tab) ─────────────────────
        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;
        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let elapsed: Option<u128>;
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
            let mut edited = code.clone();
            let editor = egui::TextEdit::multiline(&mut edited)
                .hint_text("type SQL here — multiple statements allowed")
                .font(egui::TextStyle::Monospace)
                .desired_rows(8)
                .desired_width(f32::INFINITY)
                .code_editor();
            ui.add(editor);
            if edited != tab.code {
                tab.code = edited;
            }
            code = tab.code.clone();
        }"""

new = """        // ── Editor + toolbar (mutate only the tab) ─────────────────────
        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;
        let mut run_clicked = false;
        let mut clear_clicked = false;
        let mut format_clicked = false;
        let elapsed: Option<u128>;
        // Suggestion chosen via Enter/Tab (set before the editor renders).
        let mut key_accept = false;
        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            // Make sure the completion index for the current db is loaded.
            if state.completion.db.as_deref() != Some(current_db)
                && !state.completion.loading
                && !current_db.is_empty()
            {
                let db = current_db.to_string();
                self.load_completion(conn_id, db);
            }
            let index = state.completion.index.clone();

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
                        if let Some(ix) = &index {
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
                    if let Some(s) = tab.suggest.items.get(tab.suggest.selected) {
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
                } else if focused
                    && !items.is_empty()
                    && word.chars().count() >= 2
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
        }"""

assert old in s, "editor block not found"
s = s.replace(old, new)

# 4. helpers at end of file
s += '''
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
        && !rest.starts_with('\\n')
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
'''

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("query_view.rs patched")
