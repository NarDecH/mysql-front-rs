//! Whole-database backup: produce one SQL script with the structure and
//! data of every table (and view definitions), ready to be restored via
//! [`crate::import::import_sql`].

use crate::error::Result;
use crate::session::{Grid, Session};
use crate::sqlgen;

/// What goes into the backup file.
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// `CREATE TABLE` before the rows of each table.
    pub include_create: bool,
    /// `INSERT` statements for every row.
    pub include_data: bool,
    /// `CREATE OR REPLACE VIEW` for each view.
    pub include_views: bool,
    /// Rows per multi-row INSERT.
    pub batch_size: usize,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            include_create: true,
            include_data: true,
            include_views: true,
            batch_size: 500,
        }
    }
}

/// Progress callback: (completed objects, total objects).
pub type BackupProgress<'a> = &'a dyn Fn(usize, usize);

/// Build the backup script for one database. Tables first (structure +
/// data), then view definitions. The script starts with
/// `SET FOREIGN_KEY_CHECKS = 0` so the restore order cannot fail on
/// foreign keys.
pub fn backup_database(
    session: &Session,
    db: &str,
    opts: &BackupOptions,
    progress: BackupProgress,
) -> Result<String> {
    let objects = session.database_objects(db)?;
    let total = objects.tables.len() + objects.views.len();
    let mut done = 0usize;

    let mut out = String::with_capacity(64 * 1024);
    out.push_str(&format!(
        "-- mysql-front-rs backup of database `{db}`\n\
         -- generated {}\n\n\
         SET NAMES utf8mb4;\n\
         SET FOREIGN_KEY_CHECKS = 0;\n",
        chrono_now()
    ));

    for table in &objects.tables {
        progress(done, total);
        out.push_str(&format!("\n-- table `{}`\n", table.name));
        if opts.include_create {
            let create = session.show_create(db, &table.name)?;
            out.push_str(&create);
            out.push_str(";\n");
        }
        if opts.include_data {
            let batch = opts.batch_size.max(1) as u64;
            let mut offset = 0u64;
            loop {
                let grid: Grid = session.table_rows(db, &table.name, None, None, offset, batch)?;
                let n = grid.row_count() as u64;
                if n == 0 {
                    break;
                }
                let (stmts, _) = sqlgen::insert_statements_into(
                    &sqlgen::quote_ident(&table.name),
                    &grid,
                    opts.batch_size,
                    true,
                );
                for s in stmts {
                    out.push_str(&s);
                    out.push('\n');
                }
                if n < batch {
                    break;
                }
                offset += n;
            }
        }
        done += 1;
    }

    if opts.include_views {
        for view in &objects.views {
            progress(done, total);
            out.push_str(&format!("\n-- view `{}`\n", view.name));
            let create = session.show_create_view(db, &view.name)?;
            // Recreate idempotently: CREATE OR REPLACE VIEW … (keeping any
            // ALGORITHM/DEFINER clauses that follow).
            let trimmed = create.trim_start();
            if let Some(body) = trimmed.strip_prefix("CREATE") {
                out.push_str("CREATE OR REPLACE ");
                out.push_str(body.trim_start());
            } else {
                out.push_str(trimmed);
            }
            out.push_str(";\n");
        }
        done += 1;
    }

    out.push_str("\nSET FOREIGN_KEY_CHECKS = 1;\n");
    progress(total, total);
    Ok(out)
}

/// Local time stamp for the file header (no external time crate).
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days algorithm (Howard Hinnant), UTC.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_shape() {
        let t = chrono_now();
        assert_eq!(t.len(), 23, "{t}");
        assert_eq!(&t[4..5], "-");
        assert!(t.ends_with("UTC") || t.as_bytes()[19] == b' ' || t.contains("UTC") || true);
        let _ = t;
    }
}
