#!/usr/bin/env python3
"""Add qualify helpers + insert_statements_into to sqlgen, register the
transfer module, and move the view-qualify logic out of the GUI."""
import io

# --- sqlgen.rs ---
p = r"crates\mff-core\src\sqlgen.rs"
src = io.open(p, encoding="utf-8").read()

old = """/// Generate batched multi-row `INSERT` statements for the given grid.
///
/// Returns the (possibly empty) list of statements plus the number of data
/// rows covered.
pub fn insert_statements(
    table: &str,
    grid: &Grid,
    batch_size: usize,
    include_column_list: bool,
) -> (Vec<String>, usize) {
    let batch_size = batch_size.max(1);"""
assert old in src, "sqlgen: insert_statements doc+sig"
src = src.replace(old, """/// Rewrite a `SHOW CREATE VIEW` statement so the view lives in `new_db`
/// and its body references tables from `new_db` instead of `old_db`.
/// MariaDB already qualifies the view name with its own database, in which
/// case the rewrite above covers it; the explicit qualifier is only added
/// when the definition came back unqualified.
pub fn qualify_view_definition(sql: &str, old_db: &str, new_db: &str) -> String {
    let mut out = sql.replace(&format!("`{}`.", old_db), &format!("`{}`.", new_db));
    if let Some(pos) = out.find(" VIEW ") {
        let after = &out[pos + " VIEW ".len()..];
        if !after.starts_with(&format!("`{}`.", new_db)) {
            let insert = format!(" VIEW {}.", quote_ident(new_db));
            out.replace_range(pos..pos + " VIEW ".len(), &insert);
        }
    }
    out
}

/// Qualify the table name of a `SHOW CREATE TABLE` statement with a
/// database: "CREATE TABLE `t` (...)" becomes
/// "CREATE TABLE `db`.`t` (...)".
pub fn qualify_create_table(sql: &str, db: &str) -> String {
    if let Some(pos) = sql.find("CREATE TABLE `") {
        let split = pos + "CREATE TABLE `".len();
        let mut out = String::with_capacity(sql.len() + db.len() + 2);
        out.push_str(&sql[..split]);
        out.push_str(db);
        out.push_str("`.");
        out.push_str(&sql[split..]);
        out
    } else {
        sql.to_string()
    }
}

/// Like [`insert_statements`], but the destination is an already-quoted
/// (possibly database-qualified) table reference.
pub fn insert_statements_into(
    qualified: &str,
    grid: &Grid,
    batch_size: usize,
    include_column_list: bool,
) -> (Vec<String>, usize) {
    let batch_size = batch_size.max(1);""", 1)

old2 = '''    let prefix = format!(
        "INSERT INTO {}{col_list} VALUES",
        quote_ident(table)
    );'''
assert old2 in src, "sqlgen: prefix"
src = src.replace(old2, '''    let prefix = format!("INSERT INTO {qualified}{col_list} VALUES");''', 1)

old3 = '''    if !batch.is_empty() {
        out.push(format!("{prefix}\\n{}\\n;", batch.join(",\\n")));
    }
    (out, count)
}'''
assert old3 in src, "sqlgen: tail"
src = src.replace(old3, '''    if !batch.is_empty() {
        out.push(format!("{prefix}\\n{}\\n;", batch.join(",\\n")));
    }
    (out, count)
}

/// Generate batched multi-row `INSERT` statements for the given grid.
///
/// Returns the (possibly empty) list of statements plus the number of data
/// rows covered.
pub fn insert_statements(
    table: &str,
    grid: &Grid,
    batch_size: usize,
    include_column_list: bool,
) -> (Vec<String>, usize) {
    insert_statements_into(&quote_ident(table), grid, batch_size, include_column_list)
}''', 1)

# Tests for the new helpers (inserted before batching_works).
old_t = """    #[test]
    fn batching_works() {"""
assert old_t in src, "sqlgen: tests anchor"
src = src.replace(old_t, """    #[test]
    fn view_definition_is_qualified() {
        let shown = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `db_rename_src`.`v_labels` AS select `db_rename_src`.`t1`.`id` AS `id` from `db_rename_src`.`t1`";
        let out = qualify_view_definition(shown, "db_rename_src", "dst3");
        assert!(out.contains("VIEW `dst3`.`v_labels` AS"), "got: {out}");
        assert!(out.contains("`dst3`.`t1`"));
        assert!(!out.contains("`db_rename_src`."), "old db still referenced");
        assert!(!out.contains("`dst3`.`dst3`"), "double qualifier");
        // Unqualified definitions get the qualifier injected.
        let unq = "CREATE ALGORITHM=UNDEFINED VIEW `v_labels` AS select * from `users`";
        let out2 = qualify_view_definition(unq, "db_rename_src", "dst3");
        assert!(out2.contains("VIEW `dst3`.`v_labels` AS"), "got: {out2}");
    }

    #[test]
    fn create_table_is_qualified() {
        let shown = "CREATE TABLE `t1` (\\n  `id` int NOT NULL\\n) ENGINE=InnoDB";
        let out = qualify_create_table(shown, "dst3");
        assert!(out.contains("CREATE TABLE `dst3`.`t1` ("), "got: {out}");
    }

    #[test]
    fn batching_works() {""", 1)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("sqlgen patched")

# --- lib.rs ---
p = r"crates\mff-core\src\lib.rs"
src = io.open(p, encoding="utf-8").read()
old = "pub mod sqlgen;\npub mod sqlsplit;"
assert old in src, "lib: mods"
src = src.replace(old, "pub mod sqlgen;\npub mod sqlsplit;\npub mod transfer;", 1)
old = "pub use export::ExportFormat;"
assert old in src, "lib: reexport"
src = src.replace(old, "pub use export::ExportFormat;\npub use transfer::{TransferItem, TransferOptions, TransferSummary};", 1)
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("lib patched")

# --- gui db_rename.rs: use core helper, drop local copy/tests ---
p = r"crates\mff-gui\src\db_rename.rs"
src = io.open(p, encoding="utf-8").read()
old = "            sql = qualify_view_definition(&sql, old, new);"
assert old in src, "db_rename: call"
src = src.replace(old, "            sql = mff_core::sqlgen::qualify_view_definition(&sql, old, new);", 1)
marker = "/// Rewrite a `SHOW CREATE VIEW` statement so the view lives in `new_db`"
assert marker in src, "db_rename: local fn"
src = src[:src.index(marker)].rstrip() + "\n"
io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("db_rename patched")
