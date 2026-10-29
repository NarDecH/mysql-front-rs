#!/usr/bin/env python3
"""Core batch 2: schemadiff module + edit-def transforms + designer tests
for index/FK clauses + lib.rs registration."""
import io

# ── lib.rs: register schemadiff ─────────────────────────────────────────
p = "crates/mff-core/src/lib.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("pub mod edit;", "pub mod edit;\npub mod schemadiff;")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ── schemadiff.rs ───────────────────────────────────────────────────────
schemadiff = '''//! Schema diff: turn the *source* table's structure into ALTER statements
//! that bring the *target* table to the same shape. Reuses the table
//! designer's diff machinery.

use crate::designer::{build_alter, Design};
use crate::error::Result;
use crate::model::Table;
use crate::session::Session;

pub struct SchemaDiff {
    pub source: Table,
    pub target: Table,
    /// ALTER statements (possibly empty when structures match).
    pub alter: Vec<String>,
}

/// Load both schemas and compute the ALTERs for the target.
pub fn diff_tables(
    src: &Session,
    src_db: &str,
    src_table: &str,
    tgt: &Session,
    tgt_db: &str,
    tgt_table: &str,
) -> Result<SchemaDiff> {
    let source = src.table_schema(src_db, src_table)?;
    let target = tgt.table_schema(tgt_db, tgt_table)?;
    let mut design = Design::from_table(&source);
    // The target keeps its own name; only its structure is brought over.
    design.table_name = target.name.clone();
    design.engine = source.engine.clone().unwrap_or_default();
    design.charset = source.charset.clone().unwrap_or_default();
    design.comment = source.comment.clone().unwrap_or_default();
    let alter = build_alter(&target, &design)?;
    Ok(SchemaDiff {
        source,
        target,
        alter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::designer::Design;

    #[test]
    fn identical_structures_yield_no_alter() {
        // Both sides loaded by the same fake path is covered through
        // build_alter directly; here we exercise Design round trip.
        let t = crate::designer::tests::table();
        let d = Design::from_table(&t);
        let alter = build_alter(&t, &d).unwrap();
        assert!(alter.is_empty());
    }
}
'''
io.open("crates/mff-core/src/schemadiff.rs", "w", encoding="utf-8", newline="\n").write(schemadiff)

# ── edit-def transforms in sqlgen.rs ────────────────────────────────────
p = "crates/mff-core/src/sqlgen.rs"
s = io.open(p, encoding="utf-8").read()
old = "pub fn qualify_view_definition(sql: &str, old_db: &str, new_db: &str) -> String {"
new = '''/// Rewrite a stored `CREATE …` definition so it can be re-run as a
/// replacement: views become `CREATE OR REPLACE VIEW`, routines become
/// `CREATE OR REPLACE PROCEDURE/FUNCTION` (MariaDB syntax), triggers and
/// events are prefixed with `DROP … IF EXISTS;`.
pub fn make_replaceable(kind: &str, definition: &str) -> String {
    let def = definition.trim();
    let body = def.strip_prefix("CREATE").unwrap_or(def).trim_start();
    match kind.to_ascii_uppercase().as_str() {
        "VIEW" => format!("CREATE OR REPLACE {body}"),
        "PROCEDURE" | "FUNCTION" => format!("CREATE OR REPLACE {body}"),
        "TRIGGER" => format!(
            "DROP TRIGGER IF EXISTS {};\\n{}",
            extract_name(kind, def)
                .map(|n| quote_ident(&n))
                .unwrap_or_else(|| "??".into()),
            def
        ),
        "EVENT" => format!(
            "DROP EVENT IF EXISTS {};\\n{}",
            extract_name(kind, def)
                .map(|n| quote_ident(&n))
                .unwrap_or_else(|| "??".into()),
            def
        ),
        _ => def.to_string(),
    }
}

/// Pull the object name out of a CREATE statement (best effort, skips
/// ALGORITHM/DEFINER clauses).
fn extract_name(kind: &str, def: &str) -> Option<String> {
    let tokens: Vec<&str> = def.split_whitespace().collect();
    let kw = kind.to_ascii_uppercase();
    let mut after_kind = false;
    for tok in &tokens {
        let up = tok.trim_matches(|c| c == '`').to_ascii_uppercase();
        if !after_kind {
            if up == kw || up == "ALGORITHM" || up == "DEFINER" || up == "=" || up.starts_with("DEFINER=") || up.starts_with("ALGORITHM=") {
                if up == kw {
                    after_kind = true;
                }
                continue;
            }
            continue;
        }
        // First token after the kind keyword that isn't IF NOT EXISTS.
        if up == "IF" || up == "NOT" || up == "EXISTS" {
            continue;
        }
        let mut name = tok.trim_matches(|c| c == '`' || c == '\\'').to_string();
        // DEFINER=`u`@`h` might be glued to the name.
        if let Some(pos) = name.find('`') {
            if pos == 0 {
                continue;
            }
        }
        if name.ends_with(',') {
            name.pop();
        }
        return Some(name);
    }
    None
}

pub fn qualify_view_definition(sql: &str, old_db: &str, new_db: &str) -> String {'''
assert old in s, "sqlgen anchor"
s = s.replace(old, new)

# tests for make_replaceable in sqlgen tests module
old = """#[cfg(test)]
mod tests {
    use super::*;"""
new = """#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_definition_becomes_or_replace() {
        let out = make_replaceable(
            "VIEW",
            "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` VIEW `v` AS SELECT 1",
        );
        assert!(out.starts_with("CREATE OR REPLACE ALGORITHM"), "{out}");
    }

    #[test]
    fn procedure_becomes_or_replace() {
        let out = make_replaceable(
            "PROCEDURE",
            "CREATE DEFINER=`root`@`localhost` PROCEDURE `p`() BEGIN SELECT 1; END",
        );
        assert!(out.starts_with("CREATE OR REPLACE DEFINER"), "{out}");
    }

    #[test]
    fn trigger_gets_drop_prefix() {
        let out = make_replaceable(
            "TRIGGER",
            "CREATE TRIGGER `t` BEFORE INSERT ON `x` FOR EACH ROW SET @a = 1",
        );
        assert!(out.starts_with("DROP TRIGGER IF EXISTS `t`;\\nCREATE TRIGGER"), "{out}");
    }

    #[test]
    fn plain_create_passthrough_for_unknown_kind() {
        assert_eq!(make_replaceable("TABLE", "CREATE TABLE t (a int)"), "CREATE TABLE t (a int)");
    }""" 
assert old in s, "sqlgen tests anchor"
s = s.replace(old, new, 1)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("core batch 2 ok")
