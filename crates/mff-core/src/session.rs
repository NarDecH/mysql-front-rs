//! Live sessions against a MySQL server, built on the `mysql` crate.
//!
//! This replaces the raw wire-protocol implementation of
//! `MySQLClient.pas`/`MySQLDB.pas` in the original project with a
//! well-tested async-capable client library, keeping a thin, grid-oriented
//! API that the GUI and CLI can share.

use std::time::Duration;

use mysql::{OptsBuilder, Pool, Value};
use mysql::prelude::Queryable;

use crate::account::Connection;
use crate::error::{MffError, Result};
use crate::model::{
    Column, DatabaseObjects, Event, ForeignKey, IndexKey, KeyKind, Routine, Table, TableBrief,
    Trigger,
};

/// One grid column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColMeta {
    pub name: String,
    /// MySQL type name as reported by the client library
    /// (e.g. `LONG`, `VARCHAR`, `DATETIME`, `LONGBLOB`).
    pub type_str: String,
}

impl ColMeta {
    pub fn new(name: impl Into<String>, type_str: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_str: type_str.into(),
        }
    }
}

/// A single result cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Text(String),
    Bytes(Vec<u8>),
}

impl Cell {
    /// Text as shown in a data grid (NULL cells render as the given text).
    pub fn display(&self, null_text: &str) -> String {
        match self {
            Cell::Null => null_text.to_string(),
            Cell::Text(s) => s.clone(),
            Cell::Bytes(_) => {
                if let Ok(s) = std::str::from_utf8(self.as_bytes().unwrap()) {
                    s.to_string()
                } else {
                    format!("[{} bytes]", self.as_bytes().unwrap().len())
                }
            }
        }
    }

    fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Cell::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// A result grid — columns plus rows of raw values.
#[derive(Debug, Clone, Default)]
pub struct Grid {
    pub columns: Vec<ColMeta>,
    pub rows: Vec<Vec<Cell>>,
}

impl Grid {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn is_binary(&self, col: usize) -> bool {
        self.columns
            .get(col)
            .map(|c| crate::sqlgen::type_class(&c.type_str) == crate::sqlgen::TypeClass::Binary)
            .unwrap_or(false)
    }
}

/// Result of executing one statement.
#[derive(Debug, Clone)]
pub enum ExecOutcome {
    /// The statement returned a result set.
    Grid(Grid),
    /// The statement changed data / returned no result set.
    Affected {
        affected: u64,
        last_insert_id: u64,
        info: Option<String>,
    },
}

impl ExecOutcome {
    pub fn summary(&self) -> String {
        match self {
            ExecOutcome::Grid(g) => format!("{} row(s) returned", g.row_count()),
            ExecOutcome::Affected { affected, .. } => format!("{affected} row(s) affected"),
        }
    }
}

/// Static server information captured at connect time.
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    pub version: String,
    pub user: String,
    pub host: String,
    pub port: u16,
}

/// A live session — a thin wrapper around a connection pool.
///
/// `Session` is cheap to clone; the underlying pool is shared.
#[derive(Clone)]
pub struct Session {
    pool: Pool,
    info: ServerInfo,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("info", &self.info)
            .finish()
    }
}

impl Session {
    /// Open a session from a saved [`Connection`].
    ///
    /// Connects immediately so that errors surface at a well-defined point.
    pub fn connect(c: &Connection) -> Result<Session> {
        if c.host.trim().is_empty() {
            return Err(MffError::Invalid("host must not be empty".into()));
        }
        let builder = OptsBuilder::default()
            .ip_or_hostname(Some(c.host.trim().to_string()))
            .tcp_port(c.port)
            .user(Some(c.user.trim().to_string()))
            .pass(Some(c.password.clone()))
            .db_name(c.database.clone().filter(|d| !d.trim().is_empty()))
            .prefer_socket(false)
            .tcp_connect_timeout(Some(Duration::from_secs(c.timeout_secs.max(1))))
            .ssl_opts(ssl_opts(c.ssl_mode))
            .pool_opts(
                mysql::PoolOpts::default().with_constraints(
                    mysql::PoolConstraints::new(1, 8).unwrap_or_default(),
                ),
            );

        let pool = Pool::new(builder)?;
        let mut conn = pool.get_conn()?;

        let charset = if c.charset.trim().is_empty() {
            "utf8mb4"
        } else {
            c.charset.trim()
        };
        // SET NAMES on the pooled connection; the pool keeps it alive.
        let _ = conn.query_drop(format!("SET NAMES {charset}"));

        let version: String = conn
            .query_first("SELECT VERSION()")
            .map_err(MffError::from)?
            .unwrap_or_default();

        Ok(Session {
            pool,
            info: ServerInfo {
                version,
                user: c.user.clone(),
                host: c.host.clone(),
                port: c.port,
            },
        })
    }

    /// Open a session that uses a single dedicated connection whose session
    /// state (`USE db`, `SET FOREIGN_KEY_CHECKS`, …) persists across
    /// statements. Used for multi-step operations like data transfer.
    pub fn connect_dedicated(c: &Connection) -> Result<Session> {
        if c.host.trim().is_empty() {
            return Err(MffError::Invalid("host must not be empty".into()));
        }
        let builder = OptsBuilder::default()
            .ip_or_hostname(Some(c.host.trim().to_string()))
            .tcp_port(c.port)
            .user(Some(c.user.trim().to_string()))
            .pass(Some(c.password.clone()))
            .db_name(c.database.clone().filter(|d| !d.trim().is_empty()))
            .prefer_socket(false)
            .tcp_connect_timeout(Some(Duration::from_secs(c.timeout_secs.max(1))))
            .ssl_opts(ssl_opts(c.ssl_mode))
            .pool_opts(
                mysql::PoolOpts::default()
                    .with_constraints(mysql::PoolConstraints::new(1, 1).unwrap_or_default())
                    .with_reset_connection(false),
            );

        let pool = Pool::new(builder)?;
        let mut conn = pool.get_conn()?;

        let charset = if c.charset.trim().is_empty() {
            "utf8mb4"
        } else {
            c.charset.trim()
        };
        let _ = conn.query_drop(format!("SET NAMES {charset}"));

        let version: String = conn
            .query_first("SELECT VERSION()")
            .map_err(MffError::from)?
            .unwrap_or_default();

        Ok(Session {
            pool,
            info: ServerInfo {
                version,
                user: c.user.clone(),
                host: c.host.clone(),
                port: c.port,
            },
        })
    }

    /// Static information gathered at connect time.
    pub fn info(&self) -> &ServerInfo {
        &self.info
    }

    /// Run exactly one SQL statement.
    pub fn run_statement(&self, sql: &str) -> Result<ExecOutcome> {
        let mut conn = self.pool.get_conn()?;
        let mut res = conn.query_iter(sql)?;

        if res.columns().as_ref().is_empty() {
            let outcome = ExecOutcome::Affected {
                affected: res.affected_rows(),
                last_insert_id: res.last_insert_id().unwrap_or(0),
                info: {
                    let info = res.info_str().to_string();
                    if info.is_empty() {
                        None
                    } else {
                        Some(info)
                    }
                },
            };
            while res.iter().is_some() {} // drain any further result sets
            Ok(outcome)
        } else {
            let grid = collect_current_set(&mut res)?;
            while res.iter().is_some() {}
            Ok(ExecOutcome::Grid(grid))
        }
    }

    /// Execute a script containing several statements (already split via
    /// [`crate::sqlsplit`]), returning one outcome per statement.
    /// Stops at the first error unless `continue_on_error` is set.
    pub fn run_script(
        &self,
        statements: &[String],
        continue_on_error: bool,
    ) -> Vec<Result<ExecOutcome>> {
        let mut out = Vec::with_capacity(statements.len());
        for stmt in statements {
            match self.run_statement(stmt) {
                Ok(o) => out.push(Ok(o)),
                Err(e) => {
                    out.push(Err(e));
                    if !continue_on_error {
                        break;
                    }
                }
            }
        }
        out
    }

    /// Run a query and collect it into a [`Grid`].
    pub fn query(&self, sql: &str) -> Result<Grid> {
        let mut conn = self.pool.get_conn()?;
        let mut res = conn.query_iter(sql)?;
        let grid = collect_current_set(&mut res)?;
        while res.iter().is_some() {}
        Ok(grid)
    }

    /// List all visible databases (system schema kept for completeness).
    pub fn databases(&self) -> Result<Vec<String>> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<String> = conn.query("SHOW DATABASES")?;
        Ok(rows)
    }

    /// Databases a human should browse (system schemas filtered out).
    pub fn user_databases(&self) -> Result<Vec<String>> {
        Ok(self
            .databases()?
            .into_iter()
            .filter(|d| !is_system_db(d))
            .collect())
    }

    /// Load the object lists of one database for the object browser.
    pub fn database_objects(&self, db: &str) -> Result<DatabaseObjects> {
        let mut conn = self.pool.get_conn()?;

        let tables: Vec<(String, Option<String>, Option<u64>, Option<u64>, Option<String>, Option<String>)> = conn.exec(
            "SELECT TABLE_NAME, ENGINE, TABLE_ROWS, DATA_LENGTH, TABLE_COMMENT, TABLE_TYPE \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (db,),
        )?;

        let mut objects = DatabaseObjects {
            name: db.to_string(),
            ..Default::default()
        };
        for (name, engine, rows, size, comment, table_type) in tables {
            let brief = TableBrief {
                name,
                engine,
                comment: comment.filter(|c| !c.is_empty()),
                row_count: rows,
                data_length: size,
            };
            if table_type.as_deref() == Some("VIEW") {
                objects.views.push(brief);
            } else {
                objects.tables.push(brief);
            }
        }

        let routines: Vec<(String, String)> = conn.exec(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_TYPE, ROUTINE_NAME",
            (db,),
        )?;
        objects.routines = routines
            .into_iter()
            .map(|(name, kind)| Routine { name, kind, create_statement: None })
            .collect();

        let triggers: Vec<(String, String, String, String)> = conn.exec(
            "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, EVENT_OBJECT_TABLE \
             FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? ORDER BY TRIGGER_NAME",
            (db,),
        )?;
        objects.triggers = triggers
            .into_iter()
            .map(|(name, timing, event, table)| Trigger {
                name,
                timing,
                event,
                table,
                create_statement: None,
            })
            .collect();

        let events: Vec<(String, String)> = conn.exec(
            "SELECT EVENT_NAME, STATUS FROM information_schema.EVENTS \
             WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME",
            (db,),
        )?;
        objects.events = events
            .into_iter()
            .map(|(name, status)| Event { name, status, create_statement: None })
            .collect();

        Ok(objects)
    }

    /// Load the full structure of one table from `information_schema`.
    pub fn table_schema(&self, db: &str, table: &str) -> Result<Table> {
        let mut conn = self.pool.get_conn()?;

        let meta: Vec<(Option<String>, Option<String>, Option<String>, Option<u64>, Option<u64>)> =
            conn.exec(
                "SELECT ENGINE, TABLE_COLLATION, TABLE_COMMENT, TABLE_ROWS, DATA_LENGTH \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (db, table),
            )?;
        let (engine, collation, comment, row_count, data_length) =
            meta.into_iter().next().unwrap_or_default();

        let cols: Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
        )> = conn.exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, \
                    EXTRA, COLUMN_COMMENT, CHARACTER_SET_NAME, COLLATION_NAME \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (db, table),
        )?;

        if cols.is_empty() {
            return Err(MffError::Invalid(format!(
                "table {db}.{table} not found or not readable"
            )));
        }

        let columns: Vec<Column> = cols
            .into_iter()
            .map(|(name, column_type, is_nullable, key, default, extra, comment, charset, collation)| {
                Column {
                    name,
                    column_type,
                    nullable: is_nullable == "YES",
                    key,
                    default,
                    extra,
                    comment,
                    charset,
                    collation,
                }
            })
            .collect();

        // The collation reported for the table implies its charset.
        let charset = collation
            .as_ref()
            .map(|c| c.split('_').next().unwrap_or(c).to_string());

        let mut t = Table {
            name: table.to_string(),
            engine,
            charset,
            collation,
            comment: comment.filter(|c| !c.is_empty()),
            auto_increment: None,
            row_count,
            data_length,
            columns,
            keys: Vec::new(),
            foreign_keys: Vec::new(),
            create_statement: None,
        };

        // Indexes.
        let idx: Vec<(String, u64, String, String, Option<u64>, u64)> = conn.exec(
            "SELECT INDEX_NAME, NON_UNIQUE, INDEX_TYPE, COLUMN_NAME, SUB_PART, SEQ_IN_INDEX \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
             ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (db, table),
        )?;
        for (name, non_unique, idx_type, column, sub_part, _seq) in idx {
            let entry = match t.keys.last_mut() {
                Some(k) if k.name == name => k,
                _ => {
                    let kind = if name == "PRIMARY" {
                        KeyKind::Primary
                    } else if idx_type == "FULLTEXT" {
                        KeyKind::Fulltext
                    } else if idx_type == "SPATIAL" {
                        KeyKind::Spatial
                    } else if non_unique == 0 {
                        KeyKind::Unique
                    } else {
                        KeyKind::Index
                    };
                    t.keys.push(IndexKey {
                        name: name.clone(),
                        kind,
                        columns: Vec::new(),
                    });
                    t.keys.last_mut().unwrap()
                }
            };
            entry.columns.push((column, sub_part.map(|n| n as u32)));
        }

        // Foreign keys.
        let fks: Vec<(String, String, String, u64)> = conn.exec(
            "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, ORDINAL_POSITION \
             FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
            (db, table),
        )?;
        if !fks.is_empty() {
            let rules: Vec<(String, Option<String>, Option<String>)> = conn.exec(
                "SELECT CONSTRAINT_NAME, DELETE_RULE, UPDATE_RULE \
                 FROM information_schema.REFERENTIAL_CONSTRAINTS \
                 WHERE CONSTRAINT_SCHEMA = ? AND TABLE_NAME = ?",
                (db, table),
            )?;
            for (name, column, ref_table, _pos) in fks {
                let (on_delete, on_update) = rules
                    .iter()
                    .find(|(n, _, _)| *n == name)
                    .map(|(_, d, u)| {
                        (
                            d.clone().unwrap_or_else(|| "RESTRICT".into()),
                            u.clone().unwrap_or_else(|| "RESTRICT".into()),
                        )
                    })
                    .unwrap_or_else(|| ("RESTRICT".into(), "RESTRICT".into()));
                let fk = match t.foreign_keys.last_mut() {
                    Some(f) if f.name == name => f,
                    _ => {
                        t.foreign_keys.push(ForeignKey {
                            name: name.clone(),
                            columns: Vec::new(),
                            ref_table: ref_table.clone(),
                            ref_columns: Vec::new(),
                            on_delete,
                            on_update,
                        });
                        t.foreign_keys.last_mut().unwrap()
                    }
                };
                fk.columns.push(column);
            }
            // Fill referenced columns.
            let ref_cols: Vec<(String, String, u64)> = conn.exec(
                "SELECT CONSTRAINT_NAME, REFERENCED_COLUMN_NAME, ORDINAL_POSITION \
                 FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
                 ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
                (db, table),
            )?;
            for (name, ref_col, _pos) in ref_cols {
                if let Some(fk) = t.foreign_keys.iter_mut().find(|f| f.name == name) {
                    fk.ref_columns.push(ref_col);
                }
            }
        }

        t.create_statement = Some(self.show_create(db, table)?);
        Ok(t)
    }

    /// `SHOW CREATE TABLE|VIEW|…` for the given object.
    pub fn show_create(&self, db: &str, name: &str) -> Result<String> {
        let sql = format!(
            "SHOW CREATE TABLE {}",
            crate::sqlgen::quote_qualified(&[db, name])
        );
        let g = self.query(&sql)?;
        if g.row_count() >= 1 && g.columns.len() >= 2 {
            match &g.rows[0][1] {
                Cell::Text(s) => return Ok(s.clone()),
                _ => {}
            }
        }
        Err(MffError::Invalid(format!(
            "no CREATE statement returned for {db}.{name}"
        )))
    }

    /// Browse table data with pagination, optional WHERE filter and ORDER BY.
    pub fn table_rows(
        &self,
        db: &str,
        table: &str,
        where_clause: Option<&str>,
        order_by: Option<(&str, bool)>, // (column, ascending)
        offset: u64,
        limit: u64,
    ) -> Result<Grid> {
        let mut sql = format!(
            "SELECT * FROM {}",
            crate::sqlgen::quote_qualified(&[db, table])
        );
        if let Some(w) = where_clause.map(str::trim).filter(|w| !w.is_empty()) {
            sql.push_str(" WHERE ");
            sql.push_str(w);
        }
        if let Some((col, asc)) = order_by {
            sql.push_str(&format!(
                " ORDER BY {} {}",
                crate::sqlgen::quote_ident(col),
                if asc { "ASC" } else { "DESC" }
            ));
        }
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
        self.query(&sql)
    }

    /// Count rows of a table, honouring the same WHERE filter.
    pub fn count_rows(&self, db: &str, table: &str, where_clause: Option<&str>) -> Result<u64> {
        let mut conn = self.pool.get_conn()?;
        let mut sql = format!(
            "SELECT COUNT(*) FROM {}",
            crate::sqlgen::quote_qualified(&[db, table])
        );
        if let Some(w) = where_clause.map(str::trim).filter(|w| !w.is_empty()) {
            sql.push_str(" WHERE ");
            sql.push_str(w);
        }
        let n: Option<u64> = conn.query_first(sql)?;
        Ok(n.unwrap_or(0))
    }

    /// `SHOW VARIABLES` as name/value pairs.
    pub fn variables(&self) -> Result<Vec<(String, String)>> {
        let g = self.query("SHOW VARIABLES")?;
        Ok(grid_to_pairs(&g))
    }

    /// `SHOW ENGINES` as (engine, support, comment) rows.
    pub fn engines(&self) -> Result<Vec<(String, String, String)>> {
        let g = self.query("SHOW ENGINES")?;
        let mut out = Vec::new();
        for r in &g.rows {
            out.push((
                cell_text(&r, 0),
                cell_text(&r, 1),
                cell_text(&r, 2),
            ));
        }
        Ok(out)
    }

    /// List users from `mysql.user` (requires privileges).
    pub fn users(&self) -> Result<Vec<(String, String)>> {
        let g = self.query("SELECT user, host FROM mysql.user ORDER BY user, host")?;
        let mut out = Vec::new();
        for r in &g.rows {
            out.push((cell_text(&r, 0), cell_text(&r, 1)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mysql::consts::ColumnType;

    #[test]
    fn text_columns_are_not_binary() {
        // A TEXT column carries a real character set (utf8mb4 = 45).
        let text_col = mysql::Column::new(ColumnType::MYSQL_TYPE_BLOB)
            .with_name(b"bio")
            .with_character_set(45);
        assert_eq!(col_meta_from_column(&text_col).type_str, "TEXT");

        // A true BLOB uses the binary charset id 63.
        let blob_col = mysql::Column::new(ColumnType::MYSQL_TYPE_BLOB)
            .with_name(b"avatar")
            .with_character_set(63);
        assert_eq!(col_meta_from_column(&blob_col).type_str, "BLOB");

        let varchar = mysql::Column::new(ColumnType::MYSQL_TYPE_VAR_STRING)
            .with_name(b"name")
            .with_character_set(45);
        assert_eq!(col_meta_from_column(&varchar).type_str, "VARCHAR");
    }

    #[test]
    fn value_display_formats_dates_and_times() {
        assert_eq!(
            value_display(&Value::Date(2024, 1, 15, 9, 30, 5, 0)),
            "2024-01-15 09:30:05"
        );
        assert_eq!(
            value_display(&Value::Date(2024, 1, 15, 9, 30, 5, 123000)),
            "2024-01-15 09:30:05.123000"
        );
        assert_eq!(
            value_display(&Value::Time(false, 1, 2, 3, 4, 0)),
            "26:03:04" // 1 day + 2 hours
        );
        assert_eq!(value_display(&Value::Time(true, 0, 1, 0, 0, 0)), "-01:00:00");
        assert_eq!(value_display(&Value::NULL), "NULL");
    }
}

fn ssl_opts(m: crate::account::SslMode) -> Option<mysql::SslOpts> {
    use mysql::SslOpts;
    match m {
        // The mysql crate has no "try TLS, fall back to plain" mode;
        // `Preferred` therefore behaves like `Disabled`.
        crate::account::SslMode::Disabled | crate::account::SslMode::Preferred => None,
        crate::account::SslMode::Required => Some(
            SslOpts::default()
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true),
        ),
        // Full verification against the OS trust store.
        crate::account::SslMode::VerifyIdentity => Some(SslOpts::default()),
    }
}

fn is_system_db(db: &str) -> bool {
    matches!(
        db.to_ascii_lowercase().as_str(),
        "information_schema" | "mysql" | "performance_schema" | "sys"
    )
}

fn cell_text(row: &[Cell], i: usize) -> String {
    row.get(i).map(|c| c.display("NULL")).unwrap_or_default()
}

fn grid_to_pairs(g: &Grid) -> Vec<(String, String)> {
    g.rows.iter().map(|r| (cell_text(r, 0), cell_text(r, 1))).collect()
}

/// Collect the *current* result set into a [`Grid`].
fn collect_current_set(
    res: &mut mysql::QueryResult<'_, '_, '_, impl mysql::prelude::Protocol>,
) -> Result<Grid> {
    let mut set = res
        .iter()
        .ok_or_else(|| MffError::Other("statement returned no result set".into()))?;
    let columns: Vec<ColMeta> = set
        .columns()
        .as_ref()
        .iter()
        .map(col_meta_from_column)
        .collect();

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for row in &mut set {
        let row = row?;
        let values: Vec<Value> = row.unwrap();
        let cells = values
            .into_iter()
            .zip(columns.iter())
            .map(|(v, col)| value_to_cell(v, &col.type_str))
            .collect();
        rows.push(cells);
    }
    Ok(Grid { columns, rows })
}

/// Map the protocol-level `ColumnType` to a stable SQL-ish type name
/// (the mysql 26 crate no longer provides `Column::type_str()`).
fn column_type_str(t: mysql::consts::ColumnType) -> &'static str {
    use mysql::consts::ColumnType::*;
    match t {
        MYSQL_TYPE_DECIMAL => "DECIMAL",
        MYSQL_TYPE_TINY => "TINYINT",
        MYSQL_TYPE_SHORT => "SMALLINT",
        MYSQL_TYPE_LONG => "INT",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_NULL => "NULL",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        MYSQL_TYPE_LONGLONG => "BIGINT",
        MYSQL_TYPE_INT24 => "MEDIUMINT",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "DATE",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "TIME",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "DATETIME",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => "VARCHAR",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_TINY_BLOB => "TINYBLOB",
        MYSQL_TYPE_MEDIUM_BLOB => "MEDIUMBLOB",
        MYSQL_TYPE_LONG_BLOB => "LONGBLOB",
        MYSQL_TYPE_BLOB => "BLOB",
        MYSQL_TYPE_STRING => "CHAR",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_VECTOR => "VECTOR",
        MYSQL_TYPE_TYPED_ARRAY => "ARRAY",
        MYSQL_TYPE_UNKNOWN => "UNKNOWN",
    }
}

/// Build the grid metadata for one result column. The protocol reports
/// `TEXT` columns under the BLOB family too — they carry a character set
/// (binary columns use charset id 63), so use that to name them `*TEXT`.
fn col_meta_from_column(c: &mysql::Column) -> ColMeta {
    let mut type_str = column_type_str(c.column_type());
    if c.character_set() != 63 {
        type_str = match type_str {
            "TINYBLOB" => "TINYTEXT",
            "MEDIUMBLOB" => "MEDIUMTEXT",
            "LONGBLOB" => "LONGTEXT",
            "BLOB" => "TEXT",
            other => other,
        };
    }
    ColMeta::new(c.name_str().to_string(), type_str)
}

/// Human-readable rendering of a `mysql::Value` (the crate no longer
/// implements `Display` for it).
pub fn value_display(v: &Value) -> String {
    match v {
        Value::NULL => "NULL".to_string(),
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => {
            if *us > 0 {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
            } else {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            }
        }
        Value::Time(neg, days, h, mi, s, us) => {
            let sign = if *neg { "-" } else { "" };
            let hours = *days * 24 + *h as u32;
            if *us > 0 {
                format!("{sign}{hours:02}:{mi:02}:{s:02}.{us:06}")
            } else {
                format!("{sign}{hours:02}:{mi:02}:{s:02}")
            }
        }
    }
}

fn value_to_cell(v: Value, type_str: &str) -> Cell {
    match v {
        Value::NULL => Cell::Null,
        Value::Bytes(b) => {
            let binary = crate::sqlgen::type_class(type_str) == crate::sqlgen::TypeClass::Binary;
            if binary {
                Cell::Bytes(b)
            } else {
                Cell::Text(String::from_utf8_lossy(&b).into_owned())
            }
        }
        other => Cell::Text(value_display(&other)),
    }
}
