//! Import files into a MySQL table: CSV / JSON / SQL scripts.
//!
//! The counterpart of the original `TTImportSQL` and `TTImportText`
//! tool classes. Rows are inserted in batched multi-row `INSERT`s inside
//! transactions; column values are converted to SQL literals according to
//! the target column's declared type.

use std::io::Read;
use std::path::Path;

use serde_json::Value as Json;

use crate::error::{MffError, Result};
use crate::model::Table;
use crate::session::Session;
use crate::sqlgen;

/// Options for a CSV import.
#[derive(Debug, Clone)]
pub struct CsvImportOptions {
    pub delimiter: char,
    /// First row contains column names and should be mapped by name.
    pub has_header: bool,
    /// Rows per multi-row INSERT / transaction chunk.
    pub batch_size: usize,
    /// Skip empty CSV fields (insert NULL) instead of empty strings.
    pub empty_as_null: bool,
    /// Cell text that means SQL NULL (matches the exporter's default).
    pub null_text: String,
}

impl Default for CsvImportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            has_header: true,
            batch_size: 500,
            empty_as_null: true,
            null_text: "NULL".to_string(),
        }
    }
}

/// Progress callback: rows completed so far.
pub type Progress<'a> = &'a dyn Fn(usize);

/// Import a CSV file into an existing table; returns the inserted row count.
///
/// With `has_header` the file's header names are matched (case-insensitive)
/// against the table columns; otherwise the file order is used positionally.
pub fn import_csv(
    session: &Session,
    db: &str,
    table: &str,
    file: &Path,
    opts: &CsvImportOptions,
    progress: Option<Progress>,
) -> Result<usize> {
    let (headers, records) = read_csv(file, opts.delimiter)?;
    if records.is_empty() {
        return Ok(0);
    }

    let target = session.table_schema(db, table)?;
    let column_names: Vec<String> = if opts.has_header {
        headers
    } else {
        target.columns.iter().take(headers.len()).map(|c| c.name.clone()).collect()
    };

    // Resolve file columns to table columns.
    let mut mapping: Vec<Option<usize>> = Vec::with_capacity(column_names.len());
    for name in &column_names {
        mapping.push(
            target
                .columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name)),
        );
    }
    if opts.has_header && mapping.iter().all(|m| m.is_none()) {
        return Err(MffError::Invalid(format!(
            "none of the CSV columns ({}) match table {}",
            column_names.join(", "),
            table
        )));
    }
    let used: Vec<&crate::model::Column> = mapping
        .iter()
        .filter_map(|m| m.map(|i| &target.columns[i]))
        .collect();
    if used.is_empty() {
        return Err(MffError::Invalid("no mappable columns found".into()));
    }

    let mut inserted = 0usize;
    let mut batch: Vec<String> = Vec::with_capacity(opts.batch_size);
    for record in &records {
        let values = build_row_literals(&target, &mapping, record, opts);
        batch.push(format!("({})", values.join(", ")));
        if batch.len() >= opts.batch_size {
            flush_batch(session, db, table, &used, &batch)?;
            inserted += batch.len();
            batch.clear();
            if let Some(cb) = progress {
                cb(inserted);
            }
        }
    }
    if !batch.is_empty() {
        flush_batch(session, db, table, &used, &batch)?;
        inserted += batch.len();
    }
    if let Some(cb) = progress {
        cb(inserted);
    }
    Ok(inserted)
}

fn flush_batch(
    session: &Session,
    db: &str,
    table: &str,
    used: &[&crate::model::Column],
    rows: &[String],
) -> Result<()> {
    let cols = used
        .iter()
        .map(|c| sqlgen::quote_ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES\n{}\n",
        sqlgen::quote_qualified(&[db, table]),
        cols,
        rows.join(",\n")
    );
    session.run_statement(&sql).map(|_| ())?;
    Ok(())
}

fn build_row_literals(
    target: &Table,
    mapping: &[Option<usize>],
    record: &[Option<String>],
    opts: &CsvImportOptions,
) -> Vec<String> {
    let mut out = Vec::with_capacity(mapping.len());
    for (file_idx, table_idx) in mapping.iter().enumerate() {
        let value = record.get(file_idx).cloned().flatten();
        match (table_idx, value) {
            (Some(ti), Some(v)) => {
                let col = &target.columns[*ti];
                if v == opts.null_text {
                    // The exporter's NULL marker.
                    out.push("NULL".into());
                } else if is_binary_column(col) && is_hex_prefixed(&v) {
                    // Round-trip of the exporter's `0x…` binary encoding.
                    out.push(v);
                } else if v.is_empty() {
                    if opts.empty_as_null && col.nullable {
                        out.push("NULL".into());
                    } else {
                        out.push(sqlgen::value_text_to_sql(&col.column_type, ""));
                    }
                } else {
                    out.push(sqlgen::value_text_to_sql(&col.column_type, &v));
                }
            }
            (Some(_), None) => out.push("NULL".into()),
            (None, _) => out.push("NULL".into()), // unmapped CSV column
        }
    }
    out
}

/// True for BLOB/BINARY columns (by declared type or missing charset).
fn is_binary_column(col: &crate::model::Column) -> bool {
    if col.charset.is_none() {
        return sqlgen::type_class(&col.column_type) == sqlgen::TypeClass::Binary
            || col.column_type.to_ascii_lowercase().contains("blob");
    }
    sqlgen::type_class(&col.column_type) == sqlgen::TypeClass::Binary
}

/// `0xABCD` / `0xabcd` shape check.
fn is_hex_prefixed(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit())
}

/// Read a CSV file; returns the header names (if present in the file) and
/// one record per data row, each cell `None` for empty fields.
pub fn read_csv(file: &Path, delimiter: char) -> Result<(Vec<String>, Vec<Vec<Option<String>>>)> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::None)
        .from_path(file)?;

    let mut all: Vec<Vec<Option<String>>> = Vec::new();
    for result in reader.records() {
        let record = result?;
        let mut row = Vec::with_capacity(record.len());
        for field in record.iter() {
            if field.is_empty() {
                row.push(None);
            } else {
                row.push(Some(field.to_string()));
            }
        }
        all.push(row);
    }
    if all.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let headers = all.remove(0).into_iter().map(|c| c.unwrap_or_default()).collect();
    Ok((headers, all))
}

/// Import a JSON file. Two shapes are accepted:
///
/// 1. an array of objects — `[{"id": 1, "name": "Ann"}, …]`
/// 2. the grid shape produced by the matching exporter —
///    `{"columns": ["id", "name"], "rows": [[1, "Ann"], …]}`
pub fn import_json(
    session: &Session,
    db: &str,
    table: &str,
    file: &Path,
    progress: Option<Progress>,
) -> Result<usize> {
    let text = std::fs::read_to_string(file)?;
    let doc: Json = serde_json::from_str(&text)?;
    let rows: Vec<Json> = match doc {
        Json::Array(items) => items,
        Json::Object(map) => match (map.get("columns"), map.get("rows")) {
            (Some(Json::Array(columns)), Some(Json::Array(items))) => {
                // Grid shape: zip each row array with the column names to
                // build one object per row.
                let names: Vec<String> = columns
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect();
                items
                    .iter()
                    .filter_map(|row| match row {
                        Json::Array(values) => {
                            let obj: serde_json::Map<String, Json> = names
                                .iter()
                                .cloned()
                                .zip(values.iter().cloned())
                                .collect();
                            Some(Json::Object(obj))
                        }
                        _ => None,
                    })
                    .collect()
            }
            _ => {
                return Err(MffError::Invalid(
                    "JSON import expects an array of objects or {\"columns\": […], \"rows\": [[…]]}".into(),
                ))
            }
        },
        _ => return Err(MffError::Invalid("JSON import expects an array".into())),
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let target = session.table_schema(db, table)?;
    // Use the union of all keys, in the target table's declaration order,
    // so every batch shares one column list. Missing keys become NULL.
    let mut wanted: Vec<&str> = Vec::new();
    for item in &rows {
        if let Json::Object(map) = item {
            for key in map.keys() {
                if !wanted.iter().any(|w| w.eq_ignore_ascii_case(key)) {
                    wanted.push(key);
                }
            }
        }
    }
    let mapped: Vec<&crate::model::Column> = target
        .columns
        .iter()
        .filter(|c| wanted.iter().any(|w| w.eq_ignore_ascii_case(&c.name)))
        .collect();
    if mapped.is_empty() {
        return Err(MffError::Invalid(format!(
            "no JSON keys match the columns of table {db}.{table}"
        )));
    }

    let mut inserted = 0usize;
    let batch_size = 500;
    let mut batch: Vec<String> = Vec::with_capacity(batch_size);

    for item in &rows {
        let obj = match item {
            Json::Object(map) => map,
            _ => continue,
        };
        let mut vals: Vec<String> = Vec::with_capacity(mapped.len());
        for col in &mapped {
            match obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(&col.name)) {
                Some((_, v)) => vals.push(json_literal(&col.column_type, v)),
                None => vals.push("NULL".into()),
            }
        }
        batch.push(format!("({})", vals.join(", ")));
        if batch.len() >= batch_size {
            flush_json_batch(session, db, table, &mapped, &batch)?;
            inserted += batch.len();
            batch.clear();
            if let Some(cb) = progress {
                cb(inserted);
            }
        }
    }
    if !batch.is_empty() {
        flush_json_batch(session, db, table, &mapped, &batch)?;
        inserted += batch.len();
    }
    if let Some(cb) = progress {
        cb(inserted);
    }
    Ok(inserted)
}

fn flush_json_batch(
    session: &Session,
    db: &str,
    table: &str,
    mapped: &[&crate::model::Column],
    rows: &[String],
) -> Result<()> {
    let cols = mapped
        .iter()
        .map(|c| sqlgen::quote_ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES\n{}\n",
        sqlgen::quote_qualified(&[db, table]),
        cols,
        rows.join(",\n")
    );
    session.run_statement(&sql).map(|_| ())
}

fn json_literal(col_type: &str, v: &Json) -> String {
    match v {
        Json::Null => "NULL".to_string(),
        Json::Bool(b) => sqlgen::value_text_to_sql(col_type, if *b { "1" } else { "0" }),
        Json::Number(n) => n.to_string(),
        Json::String(s) => {
            if sqlgen::type_class(col_type) == sqlgen::TypeClass::Binary && is_hex_prefixed(s) {
                s.clone() // exporter round-trip of binary data
            } else {
                sqlgen::value_text_to_sql(col_type, s)
            }
        }
        other => sqlgen::quote_string(&other.to_string()),
    }
}

/// Import (execute) a SQL script file; returns the number of statements run.
pub fn import_sql(session: &Session, file: &Path, continue_on_error: bool) -> Result<usize> {
    let mut text = String::new();
    std::fs::File::open(file)?.read_to_string(&mut text)?;
    let statements = crate::sqlsplit::split_statements(&text);
    let results = session.run_script(&statements, continue_on_error);
    let failed = results.iter().filter(|r| r.is_err()).count();
    if failed > 0 && !continue_on_error {
        // run_script stops at the first error; surface it.
        for r in &results {
            if let Err(e) = r {
                return Err(MffError::Other(format!("script failed: {e}")));
            }
        }
    }
    Ok(results.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn tmp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mff-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(content).unwrap();
        p
    }

    #[test]
    fn csv_reading_basic() {
        let p = tmp_file(
            "basic.csv",
            b"id,name,note\n1,Ann,\"with, comma\"\n2,,\n",
        );
        let (headers, rows) = read_csv(&p, ',').unwrap();
        assert_eq!(headers, vec!["id", "name", "note"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][2].as_deref(), Some("with, comma"));
        assert_eq!(rows[1][1], None);
    }

    #[test]
    fn csv_reading_semicolon() {
        let p = tmp_file("semi.csv", "a;b\n1;x\n".as_bytes());
        let (headers, rows) = read_csv(&p, ';').unwrap();
        assert_eq!(headers, vec!["a", "b"]);
        assert_eq!(rows[0][1].as_deref(), Some("x"));
    }

    #[test]
    fn row_literals_handle_null_and_hex() {
        let target = Table {
            name: "t".into(),
            columns: vec![
                crate::model::Column {
                    name: "bio".into(),
                    column_type: "text".into(),
                    nullable: true,
                    ..Default::default()
                },
                crate::model::Column {
                    name: "avatar".into(),
                    column_type: "blob".into(),
                    nullable: true,
                    ..Default::default()
                },
                crate::model::Column {
                    name: "email".into(),
                    column_type: "varchar(10)".into(),
                    nullable: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mapping = vec![Some(0), Some(1), Some(2)];
        let opts = CsvImportOptions::default();
        // NULL marker → SQL NULL; hex in binary column → raw hex;
        // "NULL" in a *text* column is also the NULL marker.
        let row = vec![
            Some("NULL".to_string()),
            Some("0xDEADBEEF".to_string()),
            Some("NULL".to_string()),
        ];
        let out = build_row_literals(&target, &mapping, &row, &opts);
        assert_eq!(out, vec!["NULL", "0xDEADBEEF", "NULL"]);
        // A real hex-looking value in a *text* column stays a string.
        let row2 = vec![
            Some("0xDEAD".to_string()),
            Some("NULL".to_string()),
            Some("0xDEAD".to_string()),
        ];
        let out2 = build_row_literals(&target, &mapping, &row2, &opts);
        assert_eq!(out2[0], "'0xDEAD'");
        assert_eq!(out2[2], "'0xDEAD'");
    }

    #[test]
    fn hex_prefixed_check() {
        assert!(is_hex_prefixed("0xDEADBEEF"));
        assert!(is_hex_prefixed("0x00FF"));
        assert!(!is_hex_prefixed("0xGG"));
        assert!(!is_hex_prefixed("NULL"));
        assert!(!is_hex_prefixed("0x"));
    }
}
