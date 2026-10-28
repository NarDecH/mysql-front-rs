#!/usr/bin/env python3
"""Remove duplicate lines/blocks introduced by re-running an idempotent-ish
patch (it matched partially different 'new' texts)."""
import io

# main.rs: keep a single "mod transfer;"
p = r"crates\mff-gui\src\main.rs"
src = io.open(p, encoding="utf-8").read()
src = src.replace("mod transfer;\nmod transfer;\n", "mod transfer;\n")
src = src.replace("mod transfer;\nmod transfer;\n", "mod transfer;\n")
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("main:", src.count("mod transfer;"), "mod transfer line(s)")

# jobs.rs: keep the FIRST TransferDone block only.
p = r"crates\mff-gui\src\jobs.rs"
src = io.open(p, encoding="utf-8").read()
block = """    TransferDone {
        conn_id: String,
        text: String,
        ok: bool,
        target_same_server: bool,
        target_db: String,
    },
"""
n = src.count(block)
while src.count(block) > 1:
    idx = src.rindex(block)
    src = src[:idx] + src[idx + len(block):]
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("jobs:", src.count("TransferDone {"), "TransferDone block(s)")

# session_view.rs: dedupe the three additions.
p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()
line = "        let mut open_transfer = false;\n"
n = src.count(line)
while src.count(line) > 1:
    idx = src.rindex(line)
    src = src[:idx] + src[idx + len(line):]
print("session_view flags:", src.count("let mut open_transfer = false;"))

menu = """                    if ui.button("Transfer data…").clicked() {
                        ui.close();
                        open_transfer = true;
                    }
"""
n = src.count(menu)
while src.count(menu) > 1:
    idx = src.rindex(menu)
    src = src[:idx] + src[idx + len(menu):]
print("session_view menu items:", src.count('ui.button("Transfer data…")'))

hook = """        if open_transfer {
            self.open_transfer(&conn_id);
        }
"""
n = src.count(hook)
while src.count(hook) > 1:
    idx = src.rindex(hook)
    src = src[:idx] + src[idx + len(hook):]
print("session_view hooks:", src.count("self.open_transfer(&conn_id);"))

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("dedupe done")
