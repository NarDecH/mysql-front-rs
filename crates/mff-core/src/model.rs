//! Schema domain model.
//!
//! Mirrors the object hierarchy of the original `uSession.pas`
//! (`TSDatabase`, `TSTable`, `TSField`, `TSKey`, `TSForeignKey`, …) in a
//! plain-data, serde-friendly form.

use serde::{Deserialize, Serialize};

/// One column of a table — equivalent to `TSBaseField`/`TSTableField`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// Full MySQL type, e.g. `int unsigned`, `varchar(50)`, `decimal(10,2)`.
    pub column_type: String,
    /// `true` when the column may be NULL.
    pub nullable: bool,
    /// `PRI`, `UNI` or `MUL` (as reported by `information_schema.COLUMNS`).
    #[serde(default)]
    pub key: String,
    /// Raw `COLUMN_DEFAULT` text; `None` when the column has no default.
    #[serde(default)]
    pub default: Option<String>,
    /// `EXTRA` field, e.g. `auto_increment`,
    /// `DEFAULT_GENERATED on update CURRENT_TIMESTAMP`.
    #[serde(default)]
    pub extra: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
}

impl Column {
    /// True when the column is auto-incremented.
    pub fn is_auto_increment(&self) -> bool {
        self.extra.contains("auto_increment")
    }

    /// `NOT NULL` fragment for DDL.
    pub fn null_clause(&self) -> &'static str {
        if self.nullable {
            "NULL"
        } else {
            "NOT NULL"
        }
    }
}

/// Index / key kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyKind {
    Primary,
    Unique,
    Index,
    Fulltext,
    Spatial,
}

impl KeyKind {
    /// The SQL keyword used in DDL.
    pub fn keyword(&self) -> &'static str {
        match self {
            KeyKind::Primary => "PRIMARY KEY",
            KeyKind::Unique => "UNIQUE KEY",
            KeyKind::Index => "KEY",
            KeyKind::Fulltext => "FULLTEXT KEY",
            KeyKind::Spatial => "SPATIAL KEY",
        }
    }
}

/// One index (or the primary key) — equivalent to `TSKey`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexKey {
    pub name: String,
    pub kind: KeyKind,
    /// Column names with optional sub-part length.
    pub columns: Vec<(String, Option<u32>)>,
}

impl Default for KeyKind {
    fn default() -> Self {
        KeyKind::Index
    }
}

/// A foreign key constraint — equivalent to `TSForeignKey`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    /// e.g. `CASCADE`, `RESTRICT`, `NO ACTION`, `SET NULL`.
    #[serde(default = "default_ref_action")]
    pub on_delete: String,
    #[serde(default = "default_ref_action")]
    pub on_update: String,
}

fn default_ref_action() -> String {
    "RESTRICT".to_string()
}

/// A table — equivalent to `TSTable`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: Option<u64>,
    #[serde(default)]
    pub row_count: Option<u64>,
    #[serde(default)]
    pub data_length: Option<u64>,
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub keys: Vec<IndexKey>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    /// Verbatim `SHOW CREATE TABLE` output, when available.
    #[serde(default)]
    pub create_statement: Option<String>,
}

impl Table {
    /// Find a column by (case-insensitive) name.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }
}

/// Brief table/view entry as listed in the object browser.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableBrief {
    pub name: String,
    pub engine: Option<String>,
    pub comment: Option<String>,
    pub row_count: Option<u64>,
    pub data_length: Option<u64>,
}

/// A view — equivalent to `TSView`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    /// `SELECT …` body (without the `AS`).
    pub definition: Option<String>,
    pub create_statement: Option<String>,
}

/// A stored procedure or function — equivalent to `TSRoutine`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    /// `PROCEDURE` or `FUNCTION`.
    pub kind: String,
    pub create_statement: Option<String>,
}

/// A trigger — equivalent to `TSTrigger`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// `BEFORE` / `AFTER`.
    pub timing: String,
    /// `INSERT` / `UPDATE` / `DELETE`.
    pub event: String,
    pub table: String,
    pub create_statement: Option<String>,
}

/// A scheduled event — equivalent to `TSEvent`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub status: String,
    pub create_statement: Option<String>,
}

/// All named objects of one database — what the object browser shows for
/// a selected database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseObjects {
    pub name: String,
    pub tables: Vec<TableBrief>,
    pub views: Vec<TableBrief>,
    pub routines: Vec<Routine>,
    pub triggers: Vec<Trigger>,
    pub events: Vec<Event>,
}
