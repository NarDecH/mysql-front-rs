# Changelog

## 0.3.0 — 2026-10-29

Server administration, structural editing and editor quality-of-life.

### Added
- **Server dashboard** — version, max connections, threads, query count and
  uptime on the Server tab (works without performance_schema).
- **GRANT editor** — per-user privilege grid that applies GRANT/REVOKE on
  `*.*`; current grants are shown from `SHOW GRANTS`.
- **Edit definition** — views, procedures, functions, triggers and events
  open in a query tab as re-runnable SQL (`CREATE OR REPLACE`).
- **Global object search** (Ctrl+Shift+F; Ctrl+F outside the editor) — finds
  tables, views and columns across every database; click a result to jump.
- **Find/Replace** in the SQL editor (Ctrl+F) with Next / Replace / Replace
  all, case-sensitive toggle.
- **Run selection** — Run (and Ctrl+Enter) executes only the selected SQL
  when the editor has a selection.
- **Ctrl+Space** force-opens the SQL autocomplete at the caret.
- **Multi-cell selection** in the data grid — click a cell, Shift+click
  another to select a rectangle; copies as TSV **with headers**.
- **Table designer: indexes and foreign keys** — add/remove
  INDEX/UNIQUE/FULLTEXT/SPATIAL indexes (with sub-part lengths) and foreign
  keys with ON DELETE/ON UPDATE actions; live SQL preview; unchanged
  objects emit no clauses.
- **Schema diff** — compare two tables (across connections) and apply the
  generated ALTER statements to the target.
- **SSH hardening** — SSH password / key passphrase can be stored in the OS
  keychain; host keys are verified trust-on-first-use against
  `~/.ssh/known_hosts` and a changed key refuses the connection.
- **CI** — GitHub Actions runs `cargo test` on push and builds the Windows
  exe on tags.

### Fixed
- Process/user lists no longer panic when the server reports more columns
  than pre-allocated (e.g. MariaDB's 9-column process list).
- Background job results (e.g. the initial connect) are picked up even
  without user input — the UI repaints periodically.
- Copying the object-browser panel can no longer swallow the window
  (width clamped to a sane range).

## 0.2.0 — 2026-10-29

- Data editing in the grid (edit cell, insert row, delete row), guarded
  UPDATE/DELETE without WHERE, import dialog, database backup/restore,
  session tabs, EXPLAIN button, row copy as TSV/INSERT, FK navigation,
  F5/Ctrl+T shortcuts, SSH tunnels, OS-keychain passwords, charts,
  data comparison, English/Thai UI.

## 0.1.0 — 2026-10-27

- First working rewrite: connections, object browser, data browser,
  structure viewer, query editor, export/import, CLI, `mff-core` library.
