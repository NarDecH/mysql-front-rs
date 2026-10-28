#!/usr/bin/env python3
"""Fix remaining transfer wiring errors."""
import io

# 1. session_view.rs: move the flag next to open_db_rename (before top bar).
p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()
old = "        let mut open_transfer = false;\n        let mut actions: Vec<TreeAction> = Vec::new();"
if old in src:
    src = src.replace(old, "        let mut actions: Vec<TreeAction> = Vec::new();", 1)
anchor = "        let mut open_db_rename: Option<String> = None;"
assert anchor in src, "flag anchor"
src = src.replace(anchor, "        let mut open_transfer = false;\n" + anchor, 1)

# 2. apply_msg: TransferDone arm before Status.
old = "            UiMsg::Status(s) => self.status = s,"
assert old in src, "status arm"
if "UiMsg::TransferDone" not in src:
    src = src.replace(old, """            UiMsg::TransferDone {
                conn_id,
                text,
                ok,
                target_same_server,
                target_db,
            } => {
                let Some(state) = self.sessions.get_mut(&conn_id) else {
                    return;
                };
                state.busy = state.busy.saturating_sub(1);
                if let Some(d) = self.transfer_dlg.as_mut() {
                    d.running = false;
                    d.result = Some(text.clone());
                }
                self.status = text;
                if ok && target_same_server && state.current_db == target_db {
                    self.load_objects(&conn_id, target_db);
                }
            }
            UiMsg::Status(s) => self.status = s,""", 1)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("session_view fixed")

# 3. transfer.rs: import TransferOutcome, qualify the fn call.
p = r"crates\mff-gui\src\transfer.rs"
src = io.open(p, encoding="utf-8").read()
old = "use mff_core::transfer::{self, TransferItem, TransferOptions};"
assert old in src, "import"
src = src.replace(old, "use mff_core::transfer::{self, TransferItem, TransferOptions, TransferOutcome};", 1)
old = "                    let outcome = (|| -> mff_core::transfer::TransferOutcome {"
assert old in src, "closure sig"
src = src.replace(old, "                    let outcome = (|| -> TransferOutcome {", 1)
old = "                    transfer(\n                        &source_session,"
assert old in src, "fn call"
src = src.replace(old, "                    transfer::transfer(\n                        &source_session,", 1)
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("transfer.rs fixed")
