#!/usr/bin/env python3
"""R13 i18n: language field on App (+ persistence), toggle button, and
translated chrome in session_view / data_view / query_view / app.rs."""
import io

# ── mff-core appstate: language field ───────────────────────────────────
p = "crates/mff-core/src/appstate.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// "dark" or "light".
    #[serde(default)]
    pub theme: Option<String>,"""
new = """    /// "dark" or "light".
    #[serde(default)]
    pub theme: Option<String>,
    /// UI language: "en" (default) or "th".
    #[serde(default)]
    pub language: Option<String>,"""
assert old in s, "appstate"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── main.rs: module ─────────────────────────────────────────────────────
p = "crates/mff-gui/src/main.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("mod info_view;", "mod i18n;\nmod info_view;")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── app.rs: lang field, init, persist, toggle ───────────────────────────
p = "crates/mff-gui/src/app.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// Persisted UI state (theme, last connection, query history, …).
    pub app_state: mff_core::AppState,
}"""
new = """    /// Persisted UI state (theme, last connection, query history, …).
    pub app_state: mff_core::AppState,
    /// UI language (persisted in app_state).
    pub lang: crate::i18n::Lang,
}"""
assert old in s, "struct"
s = s.replace(old, new)

old = """        let theme = match app_state.theme.as_deref() {
            Some("dark") => egui::Theme::Dark,
            _ => egui::Theme::Light,
        };
        cc.egui_ctx.set_theme(theme);"""
new = """        let theme = match app_state.theme.as_deref() {
            Some("dark") => egui::Theme::Dark,
            _ => egui::Theme::Light,
        };
        cc.egui_ctx.set_theme(theme);
        let lang = match app_state.language.as_deref() {
            Some("th") => crate::i18n::Lang::Th,
            _ => crate::i18n::Lang::En,
        };"""
assert old in s, "init"
s = s.replace(old, new)

old = """            compare_dlg: None,
            compare_report: None,
            app_state,
        }
    }"""
new = """            compare_dlg: None,
            compare_report: None,
            app_state,
            lang,
        }
    }"""
assert old in s, "ctor"
s = s.replace(old, new)

old = """        self.app_state.last_conn = self.active_conn.clone();"""
new = """        self.app_state.last_conn = self.active_conn.clone();
        self.app_state.language = Some(
            match self.lang {
                crate::i18n::Lang::En => "en",
                crate::i18n::Lang::Th => "th",
            }
            .to_string(),
        );"""
assert old in s, "persist"
s = s.replace(old, new)

old = """    /// The theme toggle button (shows the theme you would switch to).
    pub fn theme_button(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let label = match self.theme {
            egui::Theme::Dark => "☀",
            egui::Theme::Light => "🌙",
        };
        if ui
            .button(label)
            .on_hover_text("Toggle light/dark theme")
            .clicked()
        {
            self.toggle_theme(ctx);
        }
    }"""
new = """    /// The theme toggle button (shows the theme you would switch to).
    pub fn theme_button(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let label = match self.theme {
            egui::Theme::Dark => "☀",
            egui::Theme::Light => "🌙",
        };
        if ui
            .button(label)
            .on_hover_text("Toggle light/dark theme")
            .clicked()
        {
            self.toggle_theme(ctx);
        }
        if ui
            .button(self.lang.toggle_label())
            .on_hover_text("Switch UI language / เปลี่ยนภาษา")
            .clicked()
        {
            self.lang = self.lang.other();
            self.persist_state();
        }
    }"""
assert old in s, "toggle"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("app.rs ok")

# ── session_view.rs: tabs + top bar + menu ──────────────────────────────
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()

old = """        // ── Top bar ────────────────────────────────────────────────────
        let mut open_transfer = false;
        let mut open_db_rename: Option<String> = None;
        egui::TopBottomPanel::top("session_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("⏻ Disconnect").clicked() {"""
new = """        let lang = self.lang;
        use crate::i18n::t;

        // ── Top bar ────────────────────────────────────────────────────
        let mut open_transfer = false;
        let mut open_db_rename: Option<String> = None;
        egui::TopBottomPanel::top("session_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(t(lang, "⏻ Disconnect")).clicked() {"""
assert old in s, "top bar"
s = s.replace(old, new)

old = """                ui.strong(&conn_name);
                if !version.is_empty() {
                    ui.label(format!("(server {version})"));
                }
                ui.separator();
                ui.label("database:");"""
new = """                ui.strong(&conn_name);
                if !version.is_empty() {
                    ui.label(format!("(server {version})"));
                }
                ui.separator();
                ui.label(t(lang, "database:"));"""
assert old in s, "db label"
s = s.replace(old, new)

old = """                ui.menu_button("Menu ▾", |ui| {
                    if ui.button("Rename database…").clicked() {
                        ui.close();
                        open_db_rename = Some(current_db.clone());
                    }
                    if ui.button("Transfer data…").clicked() {
                        ui.close();
                        open_transfer = true;
                    }
                    if ui.button("Processes & users").clicked() {
                        ui.close();
                        if let Some(state) = self.sessions.get_mut(&conn_id) {
                            state.view = ActiveView::Server;
                        }
                    }
                    ui.separator();
                    if ui.button("Backup database…").clicked() {
                        ui.close();
                        self.backup_dlg = Some(Default::default());
                    }
                    if ui.button("Restore database…").clicked() {
                        ui.close();
                        self.restore_dlg = Some(Default::default());
                    }
                    if ui.button("Compare table with…").clicked() {
                        ui.close();
                        self.open_compare_dialog(&conn_id);
                    }
                });
                if ui.button("⟳ Refresh").clicked() {
                    self.load_databases(&conn_id);
                }"""
new = """                ui.menu_button(t(lang, "Menu ▾"), |ui| {
                    if ui.button(t(lang, "Rename database…")).clicked() {
                        ui.close();
                        open_db_rename = Some(current_db.clone());
                    }
                    if ui.button(t(lang, "Transfer data…")).clicked() {
                        ui.close();
                        open_transfer = true;
                    }
                    if ui.button(t(lang, "Processes & users")).clicked() {
                        ui.close();
                        if let Some(state) = self.sessions.get_mut(&conn_id) {
                            state.view = ActiveView::Server;
                        }
                    }
                    ui.separator();
                    if ui.button(t(lang, "Backup database…")).clicked() {
                        ui.close();
                        self.backup_dlg = Some(Default::default());
                    }
                    if ui.button(t(lang, "Restore database…")).clicked() {
                        ui.close();
                        self.restore_dlg = Some(Default::default());
                    }
                    if ui.button(t(lang, "Compare table with…")).clicked() {
                        ui.close();
                        self.open_compare_dialog(&conn_id);
                    }
                });
                if ui.button(t(lang, "⟳ Refresh")).clicked() {
                    self.load_databases(&conn_id);
                }"""
assert old in s, "menu"
s = s.replace(old, new)

old = """            ui.horizontal(|ui| {
                tab_button(ui, "Data", ActiveView::Data, view, &conn_id, self);
                tab_button(ui, "Structure", ActiveView::Structure, view, &conn_id, self);
                tab_button(ui, "Query", ActiveView::Query, view, &conn_id, self);
                tab_button(ui, "Info", ActiveView::Info, view, &conn_id, self);
                tab_button(ui, "Server", ActiveView::Server, view, &conn_id, self);"""
new = """            ui.horizontal(|ui| {
                tab_button(ui, t(lang, "Data"), ActiveView::Data, view, &conn_id, self);
                tab_button(ui, t(lang, "Structure"), ActiveView::Structure, view, &conn_id, self);
                tab_button(ui, t(lang, "Query"), ActiveView::Query, view, &conn_id, self);
                tab_button(ui, t(lang, "Info"), ActiveView::Info, view, &conn_id, self);
                tab_button(ui, t(lang, "Server"), ActiveView::Server, view, &conn_id, self);"""
assert old in s, "tabs"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("session_view.rs ok")

# ── data_view.rs toolbar ────────────────────────────────────────────────
p = "crates/mff-gui/src/data_view.rs"
s = io.open(p, encoding="utf-8").read()
old = """        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let d = &mut state.data;

            if d.table.is_empty() {"""
new = """        let lang = self.lang;
        use crate::i18n::t;

        {
            let Some(state) = self.sessions.get_mut(conn_id) else {
                return;
            };
            let d = &mut state.data;

            if d.table.is_empty() {"""
assert old in s, "lang decl"
s = s.replace(old, new)

pairs = [
    ('} else if ui.button("⟳ Refresh").clicked() {', '} else if ui.button(t(lang, "⟳ Refresh")).clicked() {'),
    ('if ui.button("⚒ Structure").clicked() {', 'if ui.button(t(lang, "⚒ Structure")).clicked() {'),
    ('if ui.button("⭳ Export…").clicked() {', 'if ui.button(t(lang, "⭳ Export…")).clicked() {'),
    ('.button("⇪ Import…")', '.button(t(lang, "⇪ Import…"))'),
    ('.button("＋ Row")', '.button(t(lang, "＋ Row"))'),
    ('.button("⤢ View cell")', '.button(t(lang, "⤢ View cell"))'),
    ('ui.label("filter (WHERE):");', 'ui.label(t(lang, "filter (WHERE):"));'),
    ('apply_filter |= ui.button("Apply").clicked();', 'apply_filter |= ui.button(t(lang, "Apply")).clicked();'),
    ('if !d.filter_applied.is_empty() && ui.button("✕ Clear").clicked() {', 'if !d.filter_applied.is_empty() && ui.button(t(lang, "✕ Clear")).clicked() {'),
    ('ui.label("find:");', 'ui.label(t(lang, "find:"));'),
    ('ui.label("page size:");', 'ui.label(t(lang, "page size:"));'),
    ('if ui.button("go").clicked() {', 'if ui.button(t(lang, "go")).clicked() {'),
]
for old, new in pairs:
    assert old in s, old
    s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("data_view.rs ok")

# ── query_view.rs toolbar ───────────────────────────────────────────────
p = "crates/mff-gui/src/query_view.rs"
s = io.open(p, encoding="utf-8").read()
old = """        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;"""
new = """        let lang = self.lang;
        use crate::i18n::t;

        let active = self.sessions.get(conn_id).map(|s| s.active_query).unwrap_or(0);
        let mut code;"""
assert old in s, "lang decl"
s = s.replace(old, new)

old = """                    egui::RichText::new("▶ Run (Ctrl+Enter)")"""
new = """                    egui::RichText::new(t(lang, "▶ Run (Ctrl+Enter)"))"""
assert old in s
s = s.replace(old, new)
old = """                if ui
                    .button("✨ Format")"""
new = """                if ui
                    .button(t(lang, "✨ Format"))"""
assert old in s
s = s.replace(old, new)
old = """                if ui
                    .button("🔍 Explain")"""
new = """                if ui
                    .button(t(lang, "🔍 Explain"))"""
assert old in s
s = s.replace(old, new)
old = """                if ui.button("✕ Clear results").clicked() {"""
new = """                if ui.button(t(lang, "✕ Clear results")).clicked() {"""
assert old in s
s = s.replace(old, new)
old = """                if ui
                    .selectable_label(state.history_open, "🕘 History")"""
new = """                if ui
                    .selectable_label(state.history_open, t(lang, "🕘 History"))"""
assert old in s
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("query_view.rs ok")

# ── app.rs: home screen + status bar + session tabs ────────────────────
p = "crates/mff-gui/src/app.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// The connections home screen.
    fn show_connections_screen(&mut self, ctx: &Context) {"""
new = """    /// The connections home screen.
    fn show_connections_screen(&mut self, ctx: &Context) {
        let lang = self.lang;
        use crate::i18n::t;
"""
assert old in s, "home"
s = s.replace(old, new)

pairs = [
    ('if ui.button("＋ New connection").clicked() {', 'if ui.button(t(lang, "＋ New connection")).clicked() {'),
    ('if crate::theme::primary_button(ui, "Connect").clicked() {', 'if crate::theme::primary_button(ui, t(lang, "Connect")).clicked() {'),
    ('if ui.small_button("Edit").clicked() {', 'if ui.small_button(t(lang, "Edit")).clicked() {'),
    ('if ui.small_button("Duplicate").clicked() {', 'if ui.small_button(t(lang, "Duplicate")).clicked() {'),
    ('if ui.small_button("Delete").clicked() {', 'if ui.small_button(t(lang, "Delete")).clicked() {'),
    ('for t in ["Name", "Host:port", "User", "Database", "SSL", ""] {', 'for lbl in [t(lang, "Name"), t(lang, "Host:port"), t(lang, "User"), t(lang, "Database"), "SSL", ""] {'),
    ('if ui.small_button("⌂ Connections").clicked() {', 'if ui.small_button(t(lang, "⌂ Connections")).clicked() {'),
    ('if ui\n                        .small_button("⧉ Copy cell")', 'if ui\n                        .small_button(t(lang, "⧉ Copy cell"))'),
]
for old, new in pairs:
    assert old in s, old
    s = s.replace(old, new)
# the header loop body used `t` as variable name — fix references
old = """                        for lbl in [t(lang, "Name"), t(lang, "Host:port"), t(lang, "User"), t(lang, "Database"), "SSL", ""] {
                            h.col(|ui| {
                                ui.strong(t);
                            });
                        }"""
new = """                        for lbl in [t(lang, "Name"), t(lang, "Host:port"), t(lang, "User"), t(lang, "Database"), "SSL", ""] {
                            h.col(|ui| {
                                ui.strong(lbl);
                            });
                        }"""
assert old in s, "header loop"
s = s.replace(old, new)

# session tabs strip: "＋" hover text
old = """                if ui
                    .button("＋")
                    .on_hover_text("Open another connection")
                    .clicked()
                {"""
new = """                if ui
                    .button(crate::i18n::t(self.lang, "＋"))
                    .on_hover_text("Open another connection")
                    .clicked()
                {"""
assert old in s, "tabs strip"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("app.rs chrome ok")
