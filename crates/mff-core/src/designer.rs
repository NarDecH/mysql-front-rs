//! Table designer: turn edits of a table's columns/options into a
//! sequence of `ALTER TABLE` / `RENAME TABLE` statements.
//!
//! Pure logic — the GUI keeps a [`ColSpec`] list and calls [`build_alter`]
//! for the live preview and the final execution.

use crate::error::{MffError, Result};
use crate::model::{Column, Table};
use crate::sqlgen;

/// One column as edited in the designer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColSpec {
    /// Original name when this column already exists on the server
    /// (`None` = brand new).
    pub orig_name: Option<String>,
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub auto_inc: bool,
    /// `None` → no DEFAULT clause; `Some(None)` → `DEFAULT NULL`;
    /// `Some(Some(text))` → literal (quoted/typed by [`default_literal`]).
    pub default: Option<Option<String>>,
    pub comment: String,
}

impl ColSpec {
    fn from_column(c: &Column) -> Self {
        let default = match (&c.default, c.extra.contains("DEFAULT_GENERATED")) {
            (Some(d), _) => Some(Some(d.clone())),
            (None, _) => None,
        };
        Self {
            orig_name: Some(c.name.clone()),
            name: c.name.clone(),
            col_type: c.column_type.clone(),
            nullable: c.nullable,
            auto_inc: c.is_auto_increment(),
            default,
            comment: c.comment.clone(),
        }
    }

    /// Build the column definition body (`name type NULL DEFAULT …`).
    fn definition(&self) -> Result<String> {
        let name = self.name.trim();
        let col_type = self.col_type.trim();
        if name.is_empty() {
            return Err(MffError::Invalid("column name must not be empty".into()));
        }
        if name.contains('`') {
            return Err(MffError::Invalid(format!(
                "column name {name} must not contain a backtick"
            )));
        }
        if col_type.is_empty() {
            return Err(MffError::Invalid(format!(
                "column {name} needs a data type"
            )));
        }
        let mut out = format!("{} {}", sqlgen::quote_ident(name), col_type);
        out.push_str(if self.nullable { " NULL" } else { " NOT NULL" });
        if self.auto_inc {
            out.push_str(" AUTO_INCREMENT");
        }
        match &self.default {
            None => {}
            Some(None) => out.push_str(" DEFAULT NULL"),
            Some(Some(text)) if text.is_empty() => {
                out.push_str(" DEFAULT ''");
            }
            Some(Some(text)) => {
                out.push_str(" DEFAULT ");
                out.push_str(&sqlgen::default_literal(col_type, text));
            }
        }
        if !self.comment.trim().is_empty() {
            out.push_str(&format!(
                " COMMENT {}",
                sqlgen::quote_string(self.comment.trim())
            ));
        }
        Ok(out)
    }
}

/// The whole design as edited by the user.
#[derive(Debug, Clone, Default)]
pub struct Design {
    pub table_name: String,
    pub engine: String,
    pub charset: String,
    pub comment: String,
    pub cols: Vec<ColSpec>,
}

impl Design {
    pub fn from_table(t: &Table) -> Self {
        Self {
            table_name: t.name.clone(),
            engine: t.engine.clone().unwrap_or_default(),
            charset: t.charset.clone().unwrap_or_default(),
            comment: t.comment.clone().unwrap_or_default(),
            cols: t.columns.iter().map(ColSpec::from_column).collect(),
        }
    }
}

/// One column changed in a way that needs MODIFY (same name) or CHANGE
/// (renamed).
fn column_changed(old: &Column, new: &ColSpec) -> bool {
    let old_default: Option<Option<String>> = old.default.clone().map(Some);
    old.name != new.name
        || old.column_type != new.col_type
        || old.nullable != new.nullable
        || old.is_auto_increment() != new.auto_inc
        || old_default != new.default
        || old.comment != new.comment
}

/// The ALTER clauses for the columns, in order. Adds carry `AFTER` the
/// previous column so the physical order follows the designer.
fn column_clauses(original: &Table, cols: &[ColSpec]) -> Result<Vec<String>> {
    let mut clauses = Vec::new();
    let mut prev_name: Option<String> = None;
    let mut kept: Vec<String> = Vec::new(); // names still present

    for (i, spec) in cols.iter().enumerate() {
        let name = spec.name.trim();
        match &spec.orig_name {
            None => {
                let after = match prev_name.as_ref() {
                    Some(p) => format!(" AFTER {}", sqlgen::quote_ident(p)),
                    None => " FIRST".to_string(),
                };
                clauses.push(format!("ADD COLUMN {}{}", spec.definition()?, after));
            }
            Some(orig) => {
                if !original.columns.iter().any(|c| &c.name == orig) {
                    return Err(MffError::Invalid(format!(
                        "column {orig} is not part of table {}",
                        original.name
                    )));
                }
                let old_col = original
                    .columns
                    .iter()
                    .find(|c| &c.name == orig)
                    .unwrap();
                if orig != name {
                    clauses.push(format!(
                        "CHANGE COLUMN {} {}",
                        sqlgen::quote_ident(orig),
                        spec.definition()?
                    ));
                } else if column_changed(old_col, spec) {
                    clauses.push(format!("MODIFY COLUMN {}", spec.definition()?));
                }
                kept.push(orig.clone());
            }
        }
        prev_name = Some(name.to_string());
        let _ = i;
    }

    // Columns present on the server but removed in the designer.
    for c in &original.columns {
        if !cols.iter().any(|s| s.orig_name.as_deref() == Some(&c.name)) {
            clauses.push(format!("DROP COLUMN {}", sqlgen::quote_ident(&c.name)));
        }
    }
    Ok(clauses)
}

/// True when the design would drop at least one column (data loss).
pub fn drops_columns(original: &Table, cols: &[ColSpec]) -> bool {
    original
        .columns
        .iter()
        .any(|c| !cols.iter().any(|s| s.orig_name.as_deref() == Some(&c.name)))
}

/// Build the statement list. Empty list = nothing to do.
pub fn build_alter(original: &Table, design: &Design) -> Result<Vec<String>> {
    let table = original.name.as_str();
    let new_name = design.table_name.trim();
    if new_name.is_empty() {
        return Err(MffError::Invalid("table name must not be empty".into()));
    }
    if new_name.contains('`') {
        return Err(MffError::Invalid(
            "table name must not contain a backtick".into(),
        ));
    }

    let mut out = Vec::new();
    let clauses = column_clauses(original, &design.cols)?;

    // Table options.
    let mut opts: Vec<String> = Vec::new();
    let engine = design.engine.trim();
    if !engine.is_empty() && original.engine.as_deref() != Some(engine) {
        opts.push(format!("ENGINE = {}", engine));
    }
    let charset = design.charset.trim();
    if !charset.is_empty() && original.charset.as_deref() != Some(charset) {
        // CONVERT also rewrites the data of character columns.
        opts.push(format!("CONVERT TO CHARACTER SET {charset}"));
    }
    if original.comment.as_deref().unwrap_or("") != design.comment.trim() {
        opts.push(format!("COMMENT = {}", sqlgen::quote_string(design.comment.trim())));
    }

    if !clauses.is_empty() || !opts.is_empty() {
        let mut body: Vec<String> = clauses;
        body.extend(opts);
        out.push(format!(
            "ALTER TABLE {} {}",
            sqlgen::quote_ident(table),
            body.join(",\n  ")
        ));
    }

    // Rename is its own statement (MySQL cannot mix it with other ALTERs).
    if new_name != table {
        out.push(format!(
            "RENAME TABLE {} TO {}",
            sqlgen::quote_ident(table),
            sqlgen::quote_ident(new_name)
        ));
    }
    Ok(out)
}

/// True when the statement list only changes metadata (no column drops).
pub fn is_destructive(original: &Table, design: &Design) -> bool {
    drops_columns(original, &design.cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Column;

    fn col(name: &str, ctype: &str) -> Column {
        Column {
            name: name.into(),
            column_type: ctype.into(),
            nullable: true,
            ..Default::default()
        }
    }

    fn table() -> Table {
        Table {
            name: "items".into(),
            engine: Some("InnoDB".into()),
            charset: Some("utf8mb4".into()),
            comment: None,
            columns: vec![col("id", "int"), col("title", "varchar(100)")],
            ..Default::default()
        }
    }

    fn spec(orig: Option<&str>, name: &str, ctype: &str) -> ColSpec {
        ColSpec {
            orig_name: orig.map(str::to_string),
            name: name.into(),
            col_type: ctype.into(),
            nullable: true,
            ..Default::default()
        }
    }

    #[test]
    fn no_changes_yields_empty() {
        let t = table();
        let d = Design::from_table(&t);
        assert!(build_alter(&t, &d).unwrap().is_empty());
    }

    #[test]
    fn modify_and_add_in_order() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.cols[1].col_type = "varchar(200)".into();
        d.cols.push(ColSpec {
            orig_name: None,
            name: "price".into(),
            col_type: "decimal(10,2)".into(),
            nullable: true,
            ..Default::default()
        });
        let sql = build_alter(&t, &d).unwrap();
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("MODIFY COLUMN `title` varchar(200)"), "{sql:?}");
        assert!(sql[0].contains("ADD COLUMN `price` decimal(10,2) NULL AFTER `title`"), "{sql:?}");
    }

    #[test]
    fn rename_uses_change_clause() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.cols[0].name = "item_id".into();
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("CHANGE COLUMN `id` `item_id` int NULL"), "{sql:?}");
    }

    #[test]
    fn drop_column_reported_and_listed() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.cols.remove(1); // drop `title`
        assert!(drops_columns(&t, &d.cols));
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("DROP COLUMN `title`"), "{sql:?}");
    }

    #[test]
    fn rename_table_is_separate_statement() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.table_name = "products".into();
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql.is_empty() || sql.len() == 1); // nothing else changed
        assert_eq!(
            sql.last().map(String::as_str),
            Some("RENAME TABLE `items` TO `products`")
        );
    }

    #[test]
    fn engine_and_charset_change() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.engine = "Aria".into();
        d.charset = "tis620".into();
        d.comment = "stock".into();
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("ENGINE = Aria"), "{sql:?}");
        assert!(sql[0].contains("CONVERT TO CHARACTER SET tis620"), "{sql:?}");
        assert!(sql[0].contains("COMMENT = 'stock'"), "{sql:?}");
    }

    #[test]
    fn validation_errors() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.cols[0].name = String::new();
        assert!(build_alter(&t, &d).is_err());
        d.cols[0].name = "bad`name".into();
        assert!(build_alter(&t, &d).is_err());
        d.cols[0].name = "ok".into();
        d.cols[0].col_type = String::new();
        assert!(build_alter(&t, &d).is_err());
    }

    #[test]
    fn defaults_render_typed() {
        let t = table();
        let mut d = Design::from_table(&t);
        d.cols[1].default = Some(Some("42".into()));
        d.cols[1].col_type = "int".into();
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("DEFAULT 42"), "{sql:?}");
        d.cols[1].default = Some(Some("abc".into()));
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("DEFAULT 'abc'"), "{sql:?}");
    }
}
