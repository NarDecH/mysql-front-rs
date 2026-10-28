//! Data transfer between sessions/databases: copy table structures, data
//! and view definitions from a source database to a target database —
//! possibly on a completely different server.

use crate::error::Result;
use crate::session::{Cell, Grid, Session};
use crate::sqlgen;

/// One object to transfer.
#[derive(Debug, Clone)]
pub struct TransferItem {
    pub name: String,
    pub is_view: bool,
}

#[derive(Debug, Clone)]
pub struct TransferOptions {
    /// Copy row data (tables only; views are definition-only).
    pub include_data: bool,
    /// `DROP TABLE/VIEW IF EXISTS` on the target before recreating.
    pub drop_existing: bool,
    /// `CREATE DATABASE IF NOT EXISTS` on the target with the source charset.
    pub create_target_db: bool,
    /// Rows fetched/copied per batch.
    pub batch_rows: usize,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            include_data: true,
            drop_existing: false,
            create_target_db: true,
            batch_rows: 1000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TransferSummary {
    pub tables: usize,
    pub views: usize,
    pub rows: u64,
    /// Set when the transfer stopped early.
    pub error: Option<String>,
}

/// Summary plus the error that stopped the transfer (if any). The summary
/// reflects everything completed before the failure.
#[derive(Debug)]
pub struct TransferOutcome {
    pub summary: TransferSummary,
    pub error: Option<String>,
}

impl TransferOutcome {
    fn fail(mut summary: TransferSummary, error: String) -> Self {
        summary.error = Some(error);
        Self { summary, error: None }
    }

    pub fn is_ok(&self) -> bool {
        self.summary.error.is_none()
    }

    pub fn describe(&self) -> String {
        let s = &self.summary;
        let mut text = format!(
            "{} table(s), {} view(s), {} row(s) transferred",
            s.tables, s.views, s.rows
        );
        if let Some(e) = &s.error {
            text.push_str(&format!(" — failed: {e}"));
        }
        text
    }
}

fn text_col(grid: &Grid, contains: &str) -> Option<String> {
    let idx = grid.columns.iter().position(|c| c.name.contains(contains))?;
    match grid.rows.first()?.get(idx) {
        Some(Cell::Text(s)) => Some(s.clone()),
        Some(other) => Some(other.display("")),
        None => None,
    }
}

/// Run the transfer. `progress` receives human-readable step descriptions
/// suitable for a status bar.
pub fn transfer(
    source: &Session,
    source_db: &str,
    target: &Session,
    target_db: &str,
    items: &[TransferItem],
    opts: &TransferOptions,
    progress: &dyn Fn(&str),
) -> TransferOutcome {
    let mut summary = TransferSummary::default();
    let fail = |mut s: TransferSummary, e: String| {
        s.error = Some(e);
        TransferOutcome { summary: s, error: None }
    };

    let q_target = |name: &str| sqlgen::quote_qualified(&[target_db, name]);
    let q_source = |name: &str| sqlgen::quote_qualified(&[source_db, name]);

    // 1. Target database (with the source charset) + session context.
    if opts.create_target_db {
        progress(&format!("creating database {target_db}"));
        let g = match source.query(&format!(
            "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = {}",
            sqlgen::quote_string(source_db)
        )) {
            Ok(g) => g,
            Err(e) => return fail(summary, e.to_string()),
        };
        let mut create =
            format!("CREATE DATABASE IF NOT EXISTS {}", sqlgen::quote_ident(target_db));
        if let Some(cs) = text_col(&g, "CHARACTER_SET") {
            create.push_str(&format!(" CHARACTER SET {cs}"));
            if let Some(co) = text_col(&g, "COLLATION") {
                create.push_str(&format!(" COLLATE {co}"));
            }
        }
        if let Err(e) = target.run_statement(&create) {
            return fail(summary, e.to_string());
        }
    }
    if let Err(e) = target.run_statement(&format!("USE {}", sqlgen::quote_ident(target_db))) {
        return fail(summary, e.to_string());
    }
    // Dump/restore style: create/insert order may violate FK constraints.
    if let Err(e) = target.run_statement("SET FOREIGN_KEY_CHECKS = 0") {
        return fail(summary, e.to_string());
    }

    // 2. Objects.
    for item in items {
        if item.is_view {
            progress(&format!("view {}: definition", item.name));
            if opts.drop_existing {
                let _ = target.run_statement(&format!(
                    "DROP VIEW IF EXISTS {}",
                    q_target(&item.name)
                ));
            }
            let sql = match source.show_create(source_db, &item.name) {
                Ok(s) => s,
                Err(e) => return fail(summary, e.to_string()),
            };
            let sql = sqlgen::qualify_view_definition(&sql, source_db, target_db);
            if let Err(e) = target.run_statement(&sql) {
                return fail(summary, e.to_string());
            }
            summary.views += 1;
            continue;
        }

        progress(&format!("table {}: structure", item.name));
        // Re-assert before every CREATE: the pool may hand out a different
        // connection (or reset session state) between statements.
        if let Err(e) = target.run_statement("SET FOREIGN_KEY_CHECKS = 0") {
            return fail(summary, e.to_string());
        }
        if opts.drop_existing {
            let _ = target.run_statement(&format!(
                "DROP TABLE IF EXISTS {}",
                q_target(&item.name)
            ));
        }
        let create = match source.show_create(source_db, &item.name) {
            Ok(s) => s,
            Err(e) => return fail(summary, e.to_string()),
        };
        let create = sqlgen::qualify_create_table(&create, target_db);
        if let Err(e) = target.run_statement(&create) {
            return fail(summary, e.to_string());
        }
        summary.tables += 1;

        if !opts.include_data {
            continue;
        }
        let count = source.count_rows(source_db, &item.name, None).unwrap_or(0);
        let batch = opts.batch_rows.max(1) as u64;
        let mut offset = 0u64;
        let mut rows = 0u64;
        loop {
            let grid = match source.table_rows(
                source_db,
                &item.name,
                None,
                None,
                offset,
                batch,
            ) {
                Ok(g) => g,
                Err(e) => return fail(summary, e.to_string()),
            };
            let n = grid.rows.len() as u64;
            if n == 0 {
                break;
            }
            let (stmts, _) = sqlgen::insert_statements_into(
                &q_target(&item.name),
                &grid,
                opts.batch_rows,
                true,
            );
            for stmt in &stmts {
                if let Err(e) = target.run_statement(stmt) {
                    return fail(summary, e.to_string());
                }
            }
            rows += n;
            offset += n;
            progress(&format!("table {}: data {rows}/{count} rows", item.name));
            if n < batch {
                break;
            }
        }
        summary.rows += rows;
    }

    let _ = target.run_statement("SET FOREIGN_KEY_CHECKS = 1");
    progress("transfer finished");
    TransferOutcome {
        summary,
        error: None,
    }
}
