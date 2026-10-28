//! SQL text generation: identifier/value quoting, `CREATE TABLE` DDL and
//! batched `INSERT` statements.
//!
//! The quoting rules follow the ones implemented by `SQLUtils.pas` in the
//! original Delphi project (backtick-doubling for identifiers, doubled
//! single quotes plus backslash escaping for string literals).

use crate::model::Table;
use crate::session::{Cell, ColMeta, Grid};
use mysql::Value;

/// Quote a SQL identifier with backticks (`name` → `` `name` ``),
/// doubling embedded backticks.
pub fn quote_ident(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('`');
    for c in name.chars() {
        if c == '`' {
            out.push('`');
        }
        out.push(c);
    }
    out.push('`');
    out
}

/// Quote a fully-qualified identifier such as `` `db`.`table` ``.
pub fn quote_qualified(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|p| quote_ident(p))
        .collect::<Vec<_>>()
        .join(".")
}

/// Quote a string literal: doubles single quotes and escapes backslashes
/// and control characters (server-side `NO_BACKSLASH_ESCAPES` is not set).
pub fn quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{1A}' => out.push_str("\\Z"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// SQL type categories used for literal rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    Numeric,
    Binary,
    Text,
    Temporal,
}

/// Classify a column type string (e.g. from `information_schema` or a
/// protocol-level type name).
pub fn type_class(col_type: &str) -> TypeClass {
    // Base token without modifiers/lengths: "int unsigned(10)" → "int".
    let base: String = col_type
        .to_ascii_uppercase()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '_')
        .collect();

    match base.as_str() {
        "TINY" | "SHORT" | "LONG" | "LONGLONG" | "INT24" | "INT" | "INTEGER" | "TINYINT"
        | "SMALLINT" | "MEDIUMINT" | "BIGINT" | "FLOAT" | "DOUBLE" | "PRECISION" | "DECIMAL"
        | "NEWDECIMAL" | "NUMERIC" | "YEAR" | "BIT" | "BOOL" | "BOOLEAN" | "SERIAL" => {
            TypeClass::Numeric
        }
        "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BLOB" | "BINARY" | "VARBINARY" | "GEOMETRY"
        | "POINT" | "LINESTRING" | "POLYGON" => TypeClass::Binary,
        "DATE" | "DATETIME" | "TIMESTAMP" | "TIME" => TypeClass::Temporal,
        _ => TypeClass::Text,
    }
}

/// Render one cell as a SQL literal.
///
/// * `NULL`        → `NULL`
/// * binary types  → `0x…` hex literal
/// * numeric types → raw text when parseable, quoted otherwise
/// * everything else → quoted string
pub fn cell_to_sql(col_type: &str, cell: &Cell) -> String {
    match cell {
        Cell::Null => "NULL".to_string(),
        Cell::Bytes(b) => {
            if type_class(col_type) == TypeClass::Numeric {
                // BIT columns arrive as bytes: render as hex.
                if b.is_empty() {
                    "b''".to_string()
                } else {
                    format!("0x{}", hex_encode(b))
                }
            } else {
                format!("0x{}", hex_encode(b))
            }
        }
        Cell::Text(s) => value_text_to_sql(col_type, s),
    }
}

/// Render a plain string value (e.g. from a CSV import) as a SQL literal
/// for a column of the given type.
pub fn value_text_to_sql(col_type: &str, s: &str) -> String {
    match type_class(col_type) {
        TypeClass::Numeric => {
            if is_number(s) {
                s.to_string()
            } else {
                quote_string(s)
            }
        }
        TypeClass::Binary => format!("0x{}", hex_encode(s.as_bytes())),
        TypeClass::Temporal => {
            // MySQL accepts quoted temporal literals everywhere.
            quote_string(s)
        }
        TypeClass::Text => quote_string(s),
    }
}

fn is_number(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    t.parse::<i128>().is_ok() || t.parse::<u128>().is_ok() || t.parse::<f64>().is_ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

/// Render a `mysql::Value` as a SQL literal (keeps the type information of
/// live result sets).
pub fn value_to_sql(col_type: &str, v: &Value) -> String {
    match v {
        Value::NULL => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => format_f64(*f as f64),
        Value::Double(f) => format_f64(*f),
        Value::Bytes(b) => match type_class(col_type) {
            // Binary columns and BIT values become hex literals.
            TypeClass::Text | TypeClass::Temporal => quote_string(&String::from_utf8_lossy(b)),
            _ => format!("0x{}", hex_encode(b)),
        },
        // Date/Time values: `as_sql` renders them as quoted literals.
        other => other.as_sql(false),
    }
}

fn format_f64(f: f64) -> String {
    if f.is_finite() {
        format!("{f}")
    } else {
        quote_string(&f.to_string())
    }
}

/// True when the column type should be quoted as a string even when it
/// looks numeric (used by importers that must not lose type fidelity).
pub fn needs_quoting(col_type: &str) -> bool {
    type_class(col_type) != TypeClass::Numeric
}

/// Generate a `CREATE TABLE` statement from a [`Table`] structure.
///
/// This mirrors the DDL the original application produced from its schema
/// model; when a verbatim `SHOW CREATE TABLE` output is available prefer
/// that instead (see [`Table::create_statement`]).
pub fn create_table_sql(t: &Table) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(t.columns.len() + t.keys.len() + 2);

    for c in &t.columns {
        let mut line = format!("  {} {}", quote_ident(&c.name), c.column_type);
        line.push(' ');
        line.push_str(c.null_clause());
        if c.is_auto_increment() {
            line.push_str(" AUTO_INCREMENT");
        }
        match (&c.default, c.extra.contains("DEFAULT_GENERATED")) {
            (Some(d), _) if d.is_empty() => { /* no default */ }
            (Some(d), true) => {
                line.push_str(" DEFAULT ");
                line.push_str(d);
            }
            (Some(d), false) => {
                line.push_str(" DEFAULT ");
                line.push_str(&default_literal(&c.column_type, d));
            }
            (None, _) => {
                if c.nullable {
                    line.push_str(" DEFAULT NULL");
                }
            }
        }
        if !c.comment.is_empty() {
            line.push_str(&format!(" COMMENT {}", quote_string(&c.comment)));
        }
        lines.push(line);
    }

    for k in &t.keys {
        let cols = key_columns_sql(&k.columns);
        // The primary key is unnamed in `SHOW CREATE TABLE` output.
        match k.kind {
            crate::model::KeyKind::Primary => lines.push(format!("  PRIMARY KEY {cols}")),
            _ => lines.push(format!(
                "  {} {} {}",
                k.kind.keyword(),
                quote_ident(&k.name),
                cols
            )),
        }
    }

    for fk in &t.foreign_keys {
        let cols = fk
            .columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let refs = fk
            .ref_columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let mut line = format!(
            "  CONSTRAINT {} FOREIGN KEY ({cols}) REFERENCES {} ({refs})",
            quote_ident(&fk.name),
            quote_ident(&fk.ref_table)
        );
        if !fk.on_delete.is_empty() {
            line.push_str(&format!(" ON DELETE {}", fk.on_delete));
        }
        if !fk.on_update.is_empty() {
            line.push_str(&format!(" ON UPDATE {}", fk.on_update));
        }
        lines.push(line);
    }

    let mut out = format!(
        "CREATE TABLE {} (\n{}\n)",
        quote_ident(&t.name),
        lines.join(",\n")
    );

    let mut options: Vec<String> = Vec::new();
    if let Some(e) = &t.engine {
        if !e.is_empty() {
            options.push(format!("ENGINE={e}"));
        }
    }
    if let Some(ai) = t.auto_increment {
        if ai > 0 {
            options.push(format!("AUTO_INCREMENT={ai}"));
        }
    }
    if let Some(cs) = &t.charset {
        if !cs.is_empty() {
            options.push(format!("DEFAULT CHARSET={cs}"));
        }
    }
    if let Some(co) = &t.collation {
        if !co.is_empty() {
            options.push(format!("COLLATE={co}"));
        }
    }
    if let Some(cm) = &t.comment {
        if !cm.is_empty() {
            options.push(format!("COMMENT={}", quote_string(cm)));
        }
    }
    if !options.is_empty() {
        out.push(' ');
        out.push_str(options.join(" ").as_str());
    }
    out.push(';');
    out
}

/// Render a default value. `information_schema` (MySQL 8) already returns
/// string defaults quoted; older servers / the mysql crate report them
/// unquoted — handle both.
pub fn default_literal(col_type: &str, d: &str) -> String {
    if d.eq_ignore_ascii_case("null") {
        return "NULL".to_string();
    }
    if type_class(col_type) == TypeClass::Numeric && is_number(d) {
        return d.to_string();
    }
    if d.starts_with('\'') && d.ends_with('\'') && d.len() >= 2 {
        // Already quoted by the server (MySQL 8 behaviour).
        return d.to_string();
    }
    let upper = d.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CURRENT_TIMESTAMP" | "NOW()" | "CURRENT_DATE" | "CURRENT_DATE()" | "CURRENT_TIME" | "CURRENT_TIME()"
    ) || upper.starts_with("CURRENT_TIMESTAMP(")
        || upper.starts_with("NOW(")
    {
        return d.to_string();
    }
    quote_string(d)
}

fn key_columns_sql(cols: &[(String, Option<u32>)]) -> String {
    let inner = cols
        .iter()
        .map(|(name, sub)| match sub {
            Some(n) if *n > 0 => format!("{}({})", quote_ident(name), n),
            _ => quote_ident(name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({inner})")
}

/// Rewrite a `SHOW CREATE VIEW` statement so the view lives in `new_db`
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
        out.push_str("`.`");
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
    let batch_size = batch_size.max(1);
    let mut out = Vec::new();
    let cols: Vec<&ColMeta> = grid.columns.iter().collect();
    if cols.is_empty() {
        return (out, 0);
    }

    let col_list = if include_column_list {
        format!(
            " ({})",
            cols.iter()
                .map(|c| quote_ident(&c.name))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        String::new()
    };
    let prefix = format!("INSERT INTO {qualified}{col_list} VALUES");

    let mut batch: Vec<String> = Vec::new();
    let mut count = 0usize;
    for row in &grid.rows {
        let values = row
            .iter()
            .zip(cols.iter())
            .map(|(v, c)| cell_to_sql(&c.type_str, v))
            .collect::<Vec<_>>()
            .join(", ");
        batch.push(format!("({values})"));
        count += 1;
        if batch.len() >= batch_size {
            out.push(format!("{prefix}\n{}\n;", batch.join(",\n")));
            batch.clear();
        }
    }
    if !batch.is_empty() {
        out.push(format!("{prefix}\n{}\n;", batch.join(",\n")));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents_are_backticked() {
        assert_eq!(quote_ident("id"), "`id`");
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
        assert_eq!(quote_qualified(&["db", "tbl"]), "`db`.`tbl`");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(quote_string("it's"), "'it''s'");
        assert_eq!(quote_string("back\\slash"), "'back\\\\slash'");
        assert_eq!(quote_string("line\nbreak"), "'line\\nbreak'");
        assert_eq!(quote_string(""), "''");
    }

    #[test]
    fn type_classification() {
        assert_eq!(type_class("int unsigned"), TypeClass::Numeric);
        assert_eq!(type_class("INT"), TypeClass::Numeric);
        assert_eq!(type_class("bigint(20)"), TypeClass::Numeric);
        assert_eq!(type_class("decimal(10,2)"), TypeClass::Numeric);
        assert_eq!(type_class("varchar(50)"), TypeClass::Text);
        assert_eq!(type_class("LONGTEXT"), TypeClass::Text);
        assert_eq!(type_class("MEDIUMTEXT"), TypeClass::Text);
        assert_eq!(type_class("LONGBLOB"), TypeClass::Binary);
        assert_eq!(type_class("VARBINARY(16)"), TypeClass::Binary);
        assert_eq!(type_class("datetime"), TypeClass::Temporal);
        assert_eq!(type_class("BIT(8)"), TypeClass::Numeric);
    }

    #[test]
    fn literals_by_type() {
        let g = Grid {
            columns: vec![
                ColMeta::new("id", "LONG"),
                ColMeta::new("name", "VARCHAR"),
                ColMeta::new("data", "BLOB"),
                ColMeta::new("ts", "DATETIME"),
            ],
            rows: vec![vec![
                Cell::Text("42".into()),
                Cell::Text("O'Brien".into()),
                Cell::Bytes(vec![0xDE, 0xAD]),
                Cell::Null,
            ]],
        };
        let (stmts, n) = insert_statements("t", &g, 100, true);
        assert_eq!(n, 1);
        assert_eq!(
            stmts[0],
            "INSERT INTO `t` (`id`, `name`, `data`, `ts`) VALUES\n(42, 'O''Brien', 0xDEAD, NULL)\n;"
        );
    }

    #[test]
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
        let shown = "CREATE TABLE `t1` (\n  `id` int NOT NULL\n) ENGINE=InnoDB";
        let out = qualify_create_table(shown, "dst3");
        assert!(out.contains("CREATE TABLE `dst3`.`t1` ("), "got: {out}");
    }

    #[test]
    fn batching_works() {
        let g = Grid {
            columns: vec![ColMeta::new("a", "LONG")],
            rows: (0..5).map(|i| vec![Cell::Text(i.to_string())]).collect(),
        };
        let (stmts, n) = insert_statements("t", &g, 2, false);
        assert_eq!(n, 5);
        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].starts_with("INSERT INTO `t` VALUES\n(0),\n(1)\n;"));
    }

    #[test]
    fn ddl_matches_show_create_shape() {
        let t = Table {
            name: "users".into(),
            engine: Some("InnoDB".into()),
            charset: Some("utf8mb4".into()),
            collation: Some("utf8mb4_0900_ai_ci".into()),
            comment: Some("People".into()),
            columns: vec![
                crate::model::Column {
                    name: "id".into(),
                    column_type: "int unsigned".into(),
                    nullable: false,
                    extra: "auto_increment".into(),
                    ..Default::default()
                },
                crate::model::Column {
                    name: "email".into(),
                    column_type: "varchar(190)".into(),
                    nullable: false,
                    default: Some("''".into()),
                    ..Default::default()
                },
            ],
            keys: vec![
                crate::model::IndexKey {
                    name: "PRIMARY".into(),
                    kind: crate::model::KeyKind::Primary,
                    columns: vec![("id".into(), None)],
                },
                crate::model::IndexKey {
                    name: "email".into(),
                    kind: crate::model::KeyKind::Unique,
                    columns: vec![("email".into(), Some(50))],
                },
            ],
            foreign_keys: vec![],
            ..Default::default()
        };
        let ddl = create_table_sql(&t);
        assert!(ddl.contains("CREATE TABLE `users` ("));
        assert!(ddl.contains("`id` int unsigned NOT NULL AUTO_INCREMENT"));
        assert!(ddl.contains("`email` varchar(190) NOT NULL DEFAULT ''"));
        assert!(ddl.contains("PRIMARY KEY (`id`)"));
        assert!(ddl.contains("UNIQUE KEY `email` (`email`(50))"));
        assert!(ddl.ends_with("COMMENT='People';"));
    }
}
