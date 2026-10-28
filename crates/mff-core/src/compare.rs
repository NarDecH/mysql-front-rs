//! Table data comparison: diff two tables (possibly on different servers)
//! by their primary key, or row-by-row when there is none.

use std::collections::HashMap;

use crate::error::{MffError, Result};
use crate::session::{Cell, ColMeta, Grid, Session};
use crate::sqlgen;

/// What one side contributes to the diff: rows keyed by their PK tuple.
struct Side {
    grid: Grid,
    keys: Vec<String>,
}

/// The comparison result. `only_source` / `only_target` hold full rows;
/// `different` holds (source row, target row) pairs whose keys matched but
/// whose values differ.
#[derive(Debug, Clone, Default)]
pub struct CompareReport {
    pub src_table: String,
    pub tgt_table: String,
    pub columns: Vec<ColMeta>,
    /// Columns that make up the compared key (primary key, or all).
    pub key_columns: Vec<String>,
    pub same: u64,
    pub only_source: Vec<Vec<Cell>>,
    pub only_target: Vec<Vec<Cell>>,
    pub different: Vec<(Vec<Cell>, Vec<Cell>)>,
    /// True when results were cut at `max_rows` per bucket.
    pub truncated: bool,
}

impl CompareReport {
    pub fn summary(&self) -> String {
        format!(
            "same: {} · only in {}: {} · only in {}: {} · different: {}",
            self.same,
            self.src_table,
            self.only_source.len(),
            self.tgt_table,
            self.only_target.len(),
            self.different.len()
        )
    }
}

fn row_key(row: &[Cell], cols: &[ColMeta], key_cols: &[String]) -> String {
    let mut parts = Vec::with_capacity(key_cols.len());
    for kc in key_cols {
        let idx = cols
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(kc))
            .unwrap_or(usize::MAX);
        let text = row
            .get(idx)
            .map(|c| c.display("∅"))
            .unwrap_or_else(|| "∅".into());
        parts.push(format!("{}={}", kc, text));
    }
    parts.join("§")
}

fn load_side(
    session: &Session,
    db: &str,
    table: &str,
    max_rows: u64,
) -> Result<Side> {
    let count = session.count_rows(db, table, None)?;
    if count > max_rows {
        return Err(MffError::Invalid(format!(
            "{db}.{table} has {count} rows — comparison is capped at {max_rows}"
        )));
    }
    let grid = if count == 0 {
        Grid::default()
    } else {
        session.table_rows(db, table, None, None, 0, count.max(1))?
    };
    let pk = session.primary_key_cols(db, table)?;
    let keys: Vec<String> = if pk.is_empty() {
        grid.columns.iter().map(|c| c.name.clone()).collect()
    } else {
        pk
    };
    Ok(Side { grid, keys })
}

/// Compare two tables of the same shape. Column names are matched by
/// name; columns missing on one side are ignored in the value check but
/// the report carries the source side's column list for display.
pub fn compare_tables(
    src: &Session,
    src_db: &str,
    src_table: &str,
    tgt: &Session,
    tgt_db: &str,
    tgt_table: &str,
    max_rows: u64,
) -> Result<CompareReport> {
    let s = load_side(src, src_db, src_table, max_rows)?;
    let t = load_side(tgt, tgt_db, tgt_table, max_rows)?;

    let mut report = CompareReport {
        src_table: format!("{src_db}.{src_table}"),
        tgt_table: format!("{tgt_db}.{tgt_table}"),
        columns: s.grid.columns.clone(),
        key_columns: s.keys.clone(),
        ..Default::default()
    };

    // Key → row for both sides.
    let mut src_map: HashMap<String, Vec<Cell>> = HashMap::new();
    for row in &s.grid.rows {
        src_map.insert(row_key(row, &s.grid.columns, &s.keys), row.clone());
    }
    let mut tgt_map: HashMap<String, Vec<Cell>> = HashMap::new();
    for row in &t.grid.rows {
        tgt_map.insert(row_key(row, &t.grid.columns, &t.keys), row.clone());
    }

    // Rows only in source / differing pairs.
    for (key, srow) in &src_map {
        match tgt_map.get(key) {
            None => report.only_source.push(srow.clone()),
            Some(trow) => {
                // Compare by shared column names.
                let differs = s.grid.columns.iter().any(|col| {
                    if !t.grid.columns.iter().any(|tc| tc.name == col.name) {
                        return false;
                    }
                    let si = s.grid.columns.iter().position(|c| c.name == col.name).unwrap();
                    let ti = t.grid.columns.iter().position(|c| c.name == col.name).unwrap();
                    let sv = srow.get(si).map(|c| c.display("∅"));
                    let tv = trow.get(ti).map(|c| c.display("∅"));
                    sv != tv
                });
                if differs {
                    report.different.push((srow.clone(), trow.clone()));
                } else {
                    report.same += 1;
                }
            }
        }
    }
    for (key, trow) in &tgt_map {
        if !src_map.contains_key(key) {
            report.only_target.push(trow.clone());
        }
    }

    report.truncated = report.only_source.len() > 500
        || report.only_target.len() > 500
        || report.different.len() > 500;
    let cap = 500;
    report.only_source.truncate(cap);
    report.only_target.truncate(cap);
    report.different.truncate(cap);
    let _ = sqlgen::quote_ident(""); // keep the import meaningful for future DDL output
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ColMeta;

    fn grid(cols: &[&str], rows: &[Vec<&str>]) -> Grid {
        Grid {
            columns: cols.iter().map(|c| ColMeta::new(*c, "INT")).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|v| Cell::Text(v.to_string())).collect())
                .collect(),
        }
    }

    #[test]
    fn row_key_uses_key_columns() {
        let g = grid(&["id", "name"], &[vec!["1", "Ann"]]);
        let k = row_key(&g.rows[0], &g.columns, &["id".to_string()]);
        assert_eq!(k, "id=1");
    }

    #[test]
    fn null_marker_in_key() {
        let g = Grid {
            columns: vec![ColMeta::new("id", "INT")],
            rows: vec![vec![Cell::Null]],
        };
        let k = row_key(&g.rows[0], &g.columns, &["id".to_string()]);
        assert_eq!(k, "id=∅");
    }
}
