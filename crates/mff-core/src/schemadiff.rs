//! Schema diff: turn the *source* table's structure into ALTER statements
//! that bring the *target* table to the same shape. Reuses the table
//! designer's diff machinery.

use crate::designer::{build_alter, Design};
use crate::error::Result;
use crate::model::Table;
use crate::session::Session;

#[derive(Clone)]
pub struct SchemaDiff {
    pub source_db: String,
    pub source_table: String,
    pub target_db: String,
    pub target_table: String,
    /// Connection whose session receives the ALTERs (GUI bookkeeping).
    pub target_conn: String,
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
        source_db: src_db.to_string(),
        source_table: src_table.to_string(),
        target_db: tgt_db.to_string(),
        target_table: tgt_table.to_string(),
        target_conn: String::new(),
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
