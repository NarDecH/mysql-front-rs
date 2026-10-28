#!/usr/bin/env python3
"""ConnEditor: SSH tunnel section + OS-keychain checkbox; connect()
resolves the password from the keychain when flagged."""
import io

# ── dialogs.rs ──────────────────────────────────────────────────────────
p = "crates/mff-gui/src/dialogs.rs"
s = io.open(p, encoding="utf-8").read()

# keychain checkbox after Comments row
old = """                        ui.label("Comments");
                        ui.add(
                            egui::TextEdit::multiline(&mut editor.conn.comments)
                                .desired_width(240.0)
                                .desired_rows(2),
                        );
                        ui.end_row();
                    });"""
new = """                        ui.label("Comments");
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
                            "The password is kept in Windows Credential Manager \\
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
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.conn.ssh_password)
                                    .password(true)
                                    .desired_width(240.0),
                            )
                            .on_hover_text(
                                "SSH password, or the passphrase when a key file is set",
                            );
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
                    });"""
assert old in s, "comments row"
s = s.replace(old, new)

# keep ssh_key_tmp in sync with conn.ssh_key_path (reuse db_tmp pattern)
old = """                let mut db_tmp = editor.database_tmp.clone();
                let mut db_resp = None;"""
new = """                let mut db_tmp = editor.database_tmp.clone();
                let mut db_resp = None;
                editor.ssh_key_tmp = editor.conn.ssh_key_path.clone().unwrap_or_default();"""
assert old in s, "sync tmp"
s = s.replace(old, new)

old = """                let _ = db_resp;
                editor.database_tmp = db_tmp;"""
new = """                let _ = db_resp;
                editor.database_tmp = db_tmp;
                let k = editor.ssh_key_tmp.trim().to_string();
                editor.conn.ssh_key_path = if k.is_empty() { None } else { Some(k) };"""
assert old in s, "write back"
s = s.replace(old, new)

# save flow: keychain store/clear
old = """        if save_clicked {
            if let Some(mut editor) = self.conn_edit.take() {
                let mut conn = editor.conn.clone();
                if editor.editing_id.is_none() {
                    conn.id = Connection::new_id();
                }
                match save_connection(&mut self.registry, conn.clone()) {"""
new = """        if save_clicked {
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
                match save_connection(&mut self.registry, conn.clone()) {"""
assert old in s, "save flow"
s = s.replace(old, new)

# ConnEditor: ssh_key_tmp buffer
old = """    /// Editing buffer for the optional database name.
    pub database_tmp: String,"""
new = """    /// Editing buffer for the optional database name.
    pub database_tmp: String,
    /// Editing buffer for the optional SSH key file path.
    pub ssh_key_tmp: String,"""
assert old in s, "editor field"
s = s.replace(old, new)

old = """        Self {
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
    }"""
new = """        Self {
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
        Self {
            editing_id: Some(conn.id.clone()),
            conn,
            database_tmp,
            ssh_key_tmp: conn.ssh_key_path.clone().unwrap_or_default(),
            error: None,
            testing: false,
        }
    }"""
assert old in s, "ctor"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("dialogs.rs ok")

# ── session_view.rs: resolve keychain password on connect ───────────────
p = "crates/mff-gui/src/session_view.rs"
s = io.open(p, encoding="utf-8").read()
old = """    /// Open a session for the given connection (background job).
    pub fn connect(&mut self, conn: mff_core::Connection) {"""
new = """    /// Open a session for the given connection (background job).
    pub fn connect(&mut self, mut conn: mff_core::Connection) {
        // The registry file stores no password when the keychain flag is
        // set — resolve it here so the worker thread sees the secret.
        if conn.password_in_keychain && conn.password.is_empty() {
            if let Err(e) = conn.load_password_keychain() {
                self.status = format!("keychain: {e}");
                return;
            }
        }"""
assert old in s, "connect"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("session_view.rs ok")
