#!/usr/bin/env python3
"""Designer: index + foreign-key editing (core), schema diff, and
edit-definition transforms."""
import io

# ── designer.rs: extend Design with indexes/FKs + build_alter clauses ───
p = "crates/mff-core/src/designer.rs"
s = io.open(p, encoding="utf-8").read()

old = """/// The whole design as edited by the user.
#[derive(Debug, Clone, Default)]
pub struct Design {
    pub table_name: String,
    pub engine: String,
    pub charset: String,
    pub comment: String,
    pub cols: Vec<ColSpec>,
}"""
new = """/// One index as edited in the designer.
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
                "{what} {name}: column \\"{name_col}\\" is not part of the table"
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
}"""
assert old in s, "Design struct"
s = s.replace(old, new)

old = """impl Design {
    pub fn from_table(t: &Table) -> Self {
        Self {
            table_name: t.name.clone(),
            engine: t.engine.clone().unwrap_or_default(),
            charset: t.charset.clone().unwrap_or_default(),
            comment: t.comment.clone().unwrap_or_default(),
            cols: t.columns.iter().map(ColSpec::from_column).collect(),
        }
    }
}"""
new = """impl Design {
    pub fn from_table(t: &Table) -> Self {
        Self {
            table_name: t.name.clone(),
            engine: t.engine.clone().unwrap_or_default(),
            charset: t.charset.clone().unwrap_or_default(),
            comment: t.comment.clone().unwrap_or_default(),
            cols: t.columns.iter().map(ColSpec::from_column).collect(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }
}

fn index_clauses(original: &Table, indexes: &[IdxSpec]) -> Result<Vec<String>> {
    let mut clauses = Vec::new();
    for idx in indexes {
        let name = idx.name.trim();
        if name.is_empty() || name.contains('`') {
            return Err(MffError::Invalid(format!(
                "index name \\"{name}\\" is empty or contains a backtick"
            )));
        }
        let cols = parse_col_list(&idx.columns, &original.columns, "index", name)?;
        let col_sql = cols
            .iter()
            .map(|c| sqlgen::quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let kind = idx.kind.trim().to_ascii_uppercase();
        let kind_sql = match kind.as_str() {
            "INDEX" | "" => "INDEX",
            "UNIQUE" => "UNIQUE INDEX",
            "FULLTEXT" => "FULLTEXT INDEX",
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
                "foreign key name \\"{name}\\" is empty or contains a backtick"
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
        let ref_cols = parse_col_list(&fk.ref_columns, &original.columns, "foreign key", name)?;
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
                "ADD CONSTRAINT {} FOREIGN KEY ({col_sql}) REFERENCES {} ({ref_col_sql}) \\
                 ON DELETE {on_delete} ON UPDATE {on_update}",
                sqlgen::quote_ident(name),
                sqlgen::quote_ident(ref_table),
            )),
            Some(_) => {
                clauses.push(format!("DROP FOREIGN KEY {}", sqlgen::quote_ident(name)));
                clauses.push(format!(
                    "ADD CONSTRAINT {} FOREIGN KEY ({col_sql}) REFERENCES {} ({ref_col_sql}) \\
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
}"""
assert old in s, "Design impl"
s = s.replace(old, new)

# build_alter: include index + FK clauses
old = """    let mut out = Vec::new();
    let clauses = column_clauses(original, &design.cols)?;

    // Table options.
    let mut opts: Vec<String> = Vec::new();"""
new = """    let mut out = Vec::new();
    let mut clauses = column_clauses(original, &design.cols)?;
    clauses.extend(index_clauses(original, &design.indexes)?);
    clauses.extend(fk_clauses(original, &design.foreign_keys)?);

    // Table options.
    let mut opts: Vec<String> = Vec::new();"""
assert old in s, "build_alter"
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("designer.rs ok")
