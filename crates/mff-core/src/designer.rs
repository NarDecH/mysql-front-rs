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

/// One index as edited in the designer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdxSpec {
    /// Original name when this index exists on the server.
    pub orig_name: Option<String>,
    pub name: String,
    /// `INDEX`, `UNIQUE` or `FULLTEXT` (PRIMARY stays untouched).
    pub kind: String,
    /// Comma-separated column names.
    pub columns: String,
}

/// One foreign key as edited in the designer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FkSpec {
    pub orig_name: Option<String>,
    pub name: String,
    pub columns: String,
    pub ref_table: String,
    pub ref_columns: String,
    pub on_delete: String,
    pub on_update: String,
}

/// The whole design as edited by the user.
#[derive(Debug, Clone, Default)]
pub struct Design {
    pub table_name: String,
    pub engine: String,
    pub charset: String,
    pub comment: String,
    pub cols: Vec<ColSpec>,
    pub indexes: Vec<IdxSpec>,
    pub foreign_keys: Vec<FkSpec>,
}

/// Split "a, b, c" into trimmed, validated column names.
fn parse_col_list(spec: &str, available: &[Column], what: &str, name: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let name_col = part.trim();
        if name_col.is_empty() {
            continue;
        }
        if name_col.contains('`') {
            return Err(MffError::Invalid(format!(
                "{what} {name}: column `{name_col}` must not contain a backtick"
            )));
        }
        if !available.iter().any(|c| c.name == name_col) {
            return Err(MffError::Invalid(format!(
                "{what} {name}: column \"{name_col}\" is not part of the table"
            )));
        }
        out.push(name_col.to_string());
    }
    if out.is_empty() {
        return Err(MffError::Invalid(format!(
            "{what} {name}: pick at least one column"
        )));
    }
    Ok(out)
}

impl Design {
    pub fn from_table(t: &Table) -> Self {
        let indexes = t
            .keys
            .iter()
            .filter(|k| k.kind != crate::model::KeyKind::Primary)
            .map(|k| IdxSpec {
                orig_name: Some(k.name.clone()),
                name: k.name.clone(),
                kind: match k.kind {
                    crate::model::KeyKind::Unique => "UNIQUE".into(),
                    crate::model::KeyKind::Fulltext => "FULLTEXT".into(),
                    crate::model::KeyKind::Spatial => "SPATIAL".into(),
                    _ => "INDEX".into(),
                },
                columns: k
                    .columns
                    .iter()
                    .map(|(c, len)| match len {
                        Some(n) => format!("{c}({n})"),
                        None => c.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            })
            .collect();
        let foreign_keys = t
            .foreign_keys
            .iter()
            .map(|f| FkSpec {
                orig_name: Some(f.name.clone()),
                name: f.name.clone(),
                columns: f.columns.join(", "),
                ref_table: f.ref_table.clone(),
                ref_columns: f.ref_columns.join(", "),
                on_delete: f.on_delete.clone(),
                on_update: f.on_update.clone(),
            })
            .collect();
        Self {
            table_name: t.name.clone(),
            engine: t.engine.clone().unwrap_or_default(),
            charset: t.charset.clone().unwrap_or_default(),
            comment: t.comment.clone().unwrap_or_default(),
            cols: t.columns.iter().map(ColSpec::from_column).collect(),
            indexes,
            foreign_keys,
        }
    }
}

/// Parse an index column list, allowing optional sub-part lengths:
/// `a, b(191)` → `a`, `b(191)` (validated against the table's columns).
fn parse_idx_col_list(spec: &str, available: &[Column], name: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let entry = part.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.contains('`') {
            return Err(MffError::Invalid(format!(
                "index {name}: column `{entry}` must not contain a backtick"
            )));
        }
        // strip an optional "(len)" sub-part for the column-name check
        let bare = match entry.find('(') {
            Some(pos) if entry.ends_with(')') => &entry[..pos],
            _ => entry,
        };
        let bare = bare.trim();
        if !available.iter().any(|c| c.name == bare) {
            return Err(MffError::Invalid(format!(
                "index {name}: column \"{bare}\" is not part of the table"
            )));
        }
        out.push(entry.to_string());
    }
    if out.is_empty() {
        return Err(MffError::Invalid(format!(
            "index {name}: pick at least one column"
        )));
    }
    Ok(out)
}

fn index_clauses(original: &Table, indexes: &[IdxSpec]) -> Result<Vec<String>> {
    let mut clauses = Vec::new();
    for idx in indexes {
        let name = idx.name.trim();
        if name.is_empty() || name.contains('`') {
            return Err(MffError::Invalid(format!(
                "index name \"{name}\" is empty or contains a backtick"
            )));
        }
        let cols = parse_idx_col_list(&idx.columns, &original.columns, name)?;
        let col_sql = cols
            .iter()
            .map(|entry| match entry.find('(') {
                Some(pos) if entry.ends_with(')') => {
                    let bare = entry[..pos].trim();
                    let len = &entry[pos..];
                    format!("{}{}", sqlgen::quote_ident(bare), len)
                }
                _ => sqlgen::quote_ident(entry),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let kind = idx.kind.trim().to_ascii_uppercase();
        let kind_sql = match kind.as_str() {
            "INDEX" | "" => "INDEX",
            "UNIQUE" => "UNIQUE INDEX",
            "FULLTEXT" => "FULLTEXT INDEX",
            "SPATIAL" => "SPATIAL INDEX",
            other => {
                return Err(MffError::Invalid(format!(
                    "index {name}: unknown kind {other}"
                )))
            }
        };
        match &idx.orig_name {
            None => clauses.push(format!(
                "ADD {kind_sql} {} ({col_sql})",
                sqlgen::quote_ident(name)
            )),
            Some(orig) => {
                if orig != name {
                    return Err(MffError::Invalid(format!(
                        "index {orig}: renaming indexes is not supported — drop and re-add"
                    )));
                }
                // Unchanged index → no clause at all.
                if let Some(key) = original.keys.iter().find(|k| k.name == *orig) {
                    let key_kind = match key.kind {
                        crate::model::KeyKind::Unique => "UNIQUE INDEX",
                        crate::model::KeyKind::Fulltext => "FULLTEXT INDEX",
                        crate::model::KeyKind::Spatial => "SPATIAL INDEX",
                        _ => "INDEX",
                    };
                    let key_cols = key
                        .columns
                        .iter()
                        .map(|(c, len)| match len {
                            Some(n) => format!("{c}({n})"),
                            None => c.clone(),
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                        .replace(' ', "");
                    if key_kind == kind_sql && key_cols == idx.columns.replace(' ', "") {
                        continue;
                    }
                }
                clauses.push(format!("DROP INDEX {}", sqlgen::quote_ident(name)));
                clauses.push(format!(
                    "ADD {kind_sql} {} ({col_sql})",
                    sqlgen::quote_ident(name)
                ));
            }
        }
    }
    // Indexes that were on the server but are gone from the designer.
    for key in &original.keys {
        if key.kind == crate::model::KeyKind::Primary {
            continue;
        }
        if !indexes
            .iter()
            .any(|i| i.orig_name.as_deref() == Some(&key.name))
        {
            clauses.push(format!("DROP INDEX {}", sqlgen::quote_ident(&key.name)));
        }
    }
    Ok(clauses)
}

fn fk_clauses(original: &Table, fks: &[FkSpec]) -> Result<Vec<String>> {
    let mut clauses = Vec::new();
    for fk in fks {
        let name = fk.name.trim();
        if name.is_empty() || name.contains('`') {
            return Err(MffError::Invalid(format!(
                "foreign key name \"{name}\" is empty or contains a backtick"
            )));
        }
        let ref_table = fk.ref_table.trim();
        if ref_table.is_empty() || ref_table.contains('`') {
            return Err(MffError::Invalid(format!(
                "foreign key {name}: referenced table is empty or invalid"
            )));
        }
        let cols = parse_col_list(&fk.columns, &original.columns, "foreign key", name)?;
        let col_sql = cols
            .iter()
            .map(|c| sqlgen::quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        // Referenced columns belong to the other table — only check syntax.
        let ref_cols: Vec<String> = fk
            .ref_columns
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .collect();
        if ref_cols.is_empty() || ref_cols.iter().any(|c| c.contains('`')) {
            return Err(MffError::Invalid(format!(
                "foreign key {name}: referenced columns are empty or invalid"
            )));
        }
        let ref_col_sql = ref_cols
            .iter()
            .map(|c| sqlgen::quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let action = |v: &str| -> Result<String> {
            let up = v.trim().to_ascii_uppercase();
            match up.as_str() {
                "" | "RESTRICT" | "CASCADE" | "SET NULL" | "NO ACTION" => {
                    Ok(if up.is_empty() {
                        "RESTRICT".into()
                    } else {
                        up
                    })
                }
                other => Err(MffError::Invalid(format!(
                    "foreign key {name}: unknown action {other}"
                ))),
            }
        };
        let on_delete = action(&fk.on_delete)?;
        let on_update = action(&fk.on_update)?;
        match &fk.orig_name {
            None => clauses.push(format!(
                "ADD CONSTRAINT {} FOREIGN KEY ({col_sql}) REFERENCES {} ({ref_col_sql}) \
                 ON DELETE {on_delete} ON UPDATE {on_update}",
                sqlgen::quote_ident(name),
                sqlgen::quote_ident(ref_table),
            )),
            Some(orig) => {
                // Unchanged foreign key → no clause at all.
                if let Some(ofk) = original.foreign_keys.iter().find(|f| f.name == *orig) {
                    let cols_eq = ofk
                        .columns
                        .iter()
                        .map(|c| c.as_str())
                        .eq(cols.iter().map(|c| c.as_str()));
                    let ref_cols_eq = ofk
                        .ref_columns
                        .iter()
                        .map(|c| c.as_str())
                        .eq(ref_cols.iter().map(|c| c.as_str()));
                    let ref_table_eq = ofk.ref_table == ref_table;
                    let actions_eq = ofk.on_delete == on_delete && ofk.on_update == on_update;
                    if cols_eq && ref_cols_eq && ref_table_eq && actions_eq {
                        continue;
                    }
                }
                clauses.push(format!("DROP FOREIGN KEY {}", sqlgen::quote_ident(name)));
                clauses.push(format!(
                    "ADD CONSTRAINT {} FOREIGN KEY ({col_sql}) REFERENCES {} ({ref_col_sql}) \
                     ON DELETE {on_delete} ON UPDATE {on_update}",
                    sqlgen::quote_ident(name),
                    sqlgen::quote_ident(ref_table),
                ));
            }
        }
    }
    for fk in &original.foreign_keys {
        if !fks.iter().any(|f| f.orig_name.as_deref() == Some(&fk.name)) {
            clauses.push(format!("DROP FOREIGN KEY {}", sqlgen::quote_ident(&fk.name)));
        }
    }
    Ok(clauses)
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
    let mut clauses = column_clauses(original, &design.cols)?;
    clauses.extend(index_clauses(original, &design.indexes)?);
    clauses.extend(fk_clauses(original, &design.foreign_keys)?);

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
pub mod tests {
    pub use super::*;
    use crate::model::Column;

    pub fn table() -> Table {
        test_table()
    }

    fn col(name: &str, ctype: &str) -> Column {
        Column {
            name: name.into(),
            column_type: ctype.into(),
            nullable: true,
            ..Default::default()
        }
    }

    fn test_table() -> Table {
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
    fn unchanged_index_and_fk_emit_no_clauses() {
        use crate::model::{ForeignKey, IndexKey, KeyKind};
        let mut t = test_table();
        t.keys = vec![IndexKey {
            name: "idx_title".into(),
            kind: KeyKind::Index,
            columns: vec![("title".into(), None)],
        }];
        t.foreign_keys = vec![ForeignKey {
            name: "fk_owner".into(),
            columns: vec!["title".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: "CASCADE".into(),
            on_update: "RESTRICT".into(),
        }];
        let design = Design::from_table(&t);
        let alter = build_alter(&t, &design).unwrap();
        assert!(
            alter.is_empty(),
            "unchanged index/FK must not emit clauses: {alter:?}"
        );
    }

    #[test]
    fn no_changes_yields_empty() {
        let t = test_table();
        let d = Design::from_table(&t);
        assert!(build_alter(&t, &d).unwrap().is_empty());
    }

    #[test]
    fn modify_and_add_in_order() {
        let t = test_table();
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
        let t = test_table();
        let mut d = Design::from_table(&t);
        d.cols[0].name = "item_id".into();
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("CHANGE COLUMN `id` `item_id` int NULL"), "{sql:?}");
    }

    #[test]
    fn drop_column_reported_and_listed() {
        let t = test_table();
        let mut d = Design::from_table(&t);
        d.cols.remove(1); // drop `title`
        assert!(drops_columns(&t, &d.cols));
        let sql = build_alter(&t, &d).unwrap();
        assert!(sql[0].contains("DROP COLUMN `title`"), "{sql:?}");
    }

    #[test]
    fn rename_table_is_separate_statement() {
        let t = test_table();
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
        let t = test_table();
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
        let t = test_table();
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
        let t = test_table();
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
