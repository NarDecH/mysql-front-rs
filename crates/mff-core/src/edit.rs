//! Row-level editing: build UPDATE / DELETE / INSERT statements for the
//! data grid from the values currently shown.
//!
//! The WHERE condition of an edit is taken from the table's primary key
//! columns; when the table has none, every column is matched exactly and
//! the statement carries `LIMIT 1` so it can never touch more than one row.

use crate::error::{MffError, Result};
use crate::session::{Cell, ColMeta, Grid};
use crate::sqlgen::{self, TypeClass};

/// One equality term of a WHERE condition (`column = <literal>` or
/// `column IS NULL`).
#[derive(Debug, Clone)]
pub struct WhereTerm {
    pub column: String,
    pub sql: String,
}

impl WhereTerm {
    fn render(&self) -> String {
        format!("{} {}", sqlgen::quote_ident(&self.column), self.sql)
    }
}

/// The WHERE terms of one row plus whether they match by *all* columns
/// (the table has no usable primary key) — statements then carry `LIMIT 1`.
#[derive(Debug, Clone)]
pub struct RowMatch {
    pub terms: Vec<WhereTerm>,
    pub match_all: bool,
}

/// Build the WHERE terms identifying one row of a grid.
///
/// `pk_cols` lists the primary-key column names; when it is empty every
/// column of the row is used instead and [`RowMatch::match_all`] is set.
pub fn row_where(grid: &Grid, row: usize, pk_cols: &[String]) -> Result<RowMatch> {
    let Some(cells) = grid.rows.get(row) else {
        return Err(MffError::Invalid(format!("row {row} out of range")));
    };
    // Fall back to all columns when there is no primary key.
    let names: Vec<String> = if pk_cols.is_empty() {
        grid.columns.iter().map(|c| c.name.clone()).collect()
    } else {
        pk_cols.to_vec()
    };
    let mut terms = Vec::with_capacity(names.len());
    for name in names {
        let Some(idx) = grid.columns.iter().position(|c| c.name == name) else {
            return Err(MffError::Invalid(format!(
                "key column {name} is not part of the result set"
            )));
        };
        let Some(meta) = grid.columns.get(idx) else {
            continue;
        };
        let Some(cell) = cells.get(idx) else {
            continue;
        };
        let sql = match cell {
            Cell::Null => "IS NULL".to_string(),
            other => format!("= {}", sqlgen::cell_to_sql(&meta.type_str, other)),
        };
        terms.push(WhereTerm {
            column: name,
            sql,
        });
    }
    if terms.is_empty() {
        return Err(MffError::Invalid("no columns to match the row".into()));
    }
    Ok(RowMatch {
        terms,
        match_all: pk_cols.is_empty(),
    })
}

fn where_clause(terms: &[WhereTerm]) -> String {
    terms
        .iter()
        .map(|t| t.render())
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `UPDATE … SET <column> = <literal> WHERE …` for one cell of one row.
///
/// `text` is what the user typed: the exact word `NULL` sets SQL NULL and
/// for binary columns a `0x…` prefix is taken as a hex literal.
pub fn update_sql(db: &str, table: &str, meta: &ColMeta, text: &str, m: &RowMatch) -> String {
    let literal = text_literal(&meta.type_str, text);
    let limit = if m.match_all { " LIMIT 1" } else { "" };
    format!(
        "UPDATE {} SET {} = {} WHERE {}{}",
        sqlgen::quote_qualified(&[db, table]),
        sqlgen::quote_ident(&meta.name),
        literal,
        where_clause(&m.terms),
        limit
    )
}

/// `DELETE FROM … WHERE …` (with `LIMIT 1` when matching by all columns).
pub fn delete_sql(db: &str, table: &str, m: &RowMatch) -> String {
    let limit = if m.match_all { " LIMIT 1" } else { "" };
    format!(
        "DELETE FROM {} WHERE {}{}",
        sqlgen::quote_qualified(&[db, table]),
        where_clause(&m.terms),
        limit
    )
}

/// One column of an INSERT built from the data grid.
#[derive(Debug, Clone)]
pub struct InsertCol {
    pub name: String,
    pub col_type: String,
}

/// `INSERT INTO … (cols…) VALUES (…)` from user-typed texts.
///
/// An empty text omits the column (server default applies), the exact word
/// `NULL` becomes SQL NULL.
pub fn insert_sql(db: &str, table: &str, cols: &[InsertCol], texts: &[String]) -> Result<String> {
    let mut names = Vec::new();
    let mut values = Vec::new();
    for (col, text) in cols.iter().zip(texts.iter()) {
        if text.trim().is_empty() {
            continue; // no value → use the column default
        }
        names.push(sqlgen::quote_ident(&col.name));
        values.push(text_literal(&col.col_type, text));
    }
    if names.is_empty() {
        return Err(MffError::Invalid(
            "no values given (leave fields empty to use defaults)".into(),
        ));
    }
    Ok(format!(
        "INSERT INTO {} ({}) VALUES ({})",
        sqlgen::quote_qualified(&[db, table]),
        names.join(", "),
        values.join(", ")
    ))
}

/// Render user-typed text as a SQL literal.
pub fn text_literal(col_type: &str, text: &str) -> String {
    let t = text.trim();
    if t.eq_ignore_ascii_case("NULL") {
        return "NULL".to_string();
    }
    if sqlgen::type_class(col_type) == TypeClass::Binary {
        // Accept an explicit 0x… hex literal, otherwise encode the text.
        if t.len() > 2
            && (t.starts_with("0x") || t.starts_with("0X"))
            && t[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            return t.to_string();
        }
    }
    sqlgen::value_text_to_sql(col_type, t)
}

/// Pretty-print a JSON document (`null` when `s` is not valid JSON).
pub fn pretty_json(s: &str) -> Option<String> {
    let t = s.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(t).ok()?;
    serde_json::to_string_pretty(&v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ColMeta;

    fn grid() -> Grid {
        Grid {
            columns: vec![
                ColMeta::new("id", "INT"),
                ColMeta::new("name", "VARCHAR"),
                ColMeta::new("note", "TEXT"),
                ColMeta::new("blob", "BLOB"),
            ],
            rows: vec![vec![
                Cell::Text("7".into()),
                Cell::Text("Ann".into()),
                Cell::Null,
                Cell::Bytes(vec![0xDE, 0xAD]),
            ]],
        }
    }

    #[test]
    fn update_by_primary_key() {
        let m = row_where(&grid(), 0, &["id".to_string()]).unwrap();
        assert!(!m.match_all);
        let sql = update_sql("db", "t", &ColMeta::new("name", "VARCHAR"), "Bob", &m);
        assert_eq!(sql, "UPDATE `db`.`t` SET `name` = 'Bob' WHERE `id` = 7");
    }

    #[test]
    fn update_without_pk_matches_all_columns() {
        let m = row_where(&grid(), 0, &[]).unwrap();
        assert!(m.match_all);
        let sql = update_sql("db", "t", &ColMeta::new("note", "TEXT"), "x", &m);
        assert!(
            sql.ends_with("LIMIT 1"),
            "no-PK update must carry LIMIT 1: {sql}"
        );
        assert!(sql.contains("`note` IS NULL"));
    }

    #[test]
    fn null_literal_and_hex_binary() {
        assert_eq!(text_literal("VARCHAR", "null"), "NULL");
        assert_eq!(text_literal("INT", "NULL"), "NULL");
        assert_eq!(text_literal("VARCHAR", "NULL"), "NULL");
        assert_eq!(text_literal("BLOB", "0xDEADBEEF"), "0xDEADBEEF");
        assert_eq!(text_literal("BLOB", "hi"), "0x6869");
        assert_eq!(text_literal("VARCHAR", "NULL-ish"), "'NULL-ish'");
    }

    #[test]
    fn insert_skips_empty_fields() {
        let cols = vec![
            InsertCol {
                name: "id".into(),
                col_type: "INT".into(),
            },
            InsertCol {
                name: "name".into(),
                col_type: "VARCHAR".into(),
            },
            InsertCol {
                name: "note".into(),
                col_type: "TEXT".into(),
            },
        ];
        let texts = vec![
            String::new(),
            "Ann".into(),
            "NULL".to_string(),
        ];
        let sql = insert_sql("db", "t", &cols, &texts).unwrap();
        assert_eq!(sql, "INSERT INTO `db`.`t` (`name`, `note`) VALUES ('Ann', NULL)");
    }

    #[test]
    fn insert_rejects_all_empty() {
        let cols = vec![InsertCol {
            name: "id".into(),
            col_type: "INT".into(),
        }];
        assert!(insert_sql("db", "t", &cols, &[String::new()]).is_err());
    }

    #[test]
    fn delete_uses_pk() {
        let m = row_where(&grid(), 0, &["id".to_string()]).unwrap();
        assert_eq!(
            delete_sql("db", "t", &m),
            "DELETE FROM `db`.`t` WHERE `id` = 7"
        );
    }

    #[test]
    fn json_pretty() {
        assert_eq!(
            pretty_json("{\"a\":1}").as_deref(),
            Some("{\n  \"a\": 1\n}")
        );
        assert_eq!(pretty_json("plain text"), None);
    }
}
