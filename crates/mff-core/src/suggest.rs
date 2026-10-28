//! SQL completion suggestions: pure, unit-tested logic.
//!
//! The GUI asks [`context`] for the word before the caret and
//! [`candidates`] for the ranked suggestion list; both work on character
//! indices (matching egui's `CCursor` semantics).

use std::collections::{BTreeMap, HashSet};

/// Keyword list shared with the formatter (same vocabulary).
pub use crate::sqlfmt::KEYWORDS;

/// What the server knows about one database: every table/view and its
/// columns. Built from one `information_schema.COLUMNS` query.
#[derive(Debug, Clone, Default)]
pub struct CompletionIndex {
    /// Table and view names in original case.
    pub tables: Vec<String>,
    /// table/view name lowercased → column names in original case.
    pub columns: BTreeMap<String, Vec<String>>,
}

impl CompletionIndex {
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut tables: Vec<String> = Vec::new();
        let mut columns: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (table, col) in pairs {
            let key = table.to_lowercase();
            let entry = columns.entry(key).or_default();
            if entry.is_empty() {
                tables.push(table);
            }
            if !entry.iter().any(|c| c.eq_ignore_ascii_case(&col)) {
                entry.push(col);
            }
        }
        tables.sort_by_key(|t| t.to_lowercase());
        Self { tables, columns }
    }
}

/// Kind of a suggestion (drives ordering and colouring in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestKind {
    Keyword,
    Table,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub kind: SuggestKind,
}

/// The identifier word before the caret (possibly empty), as a character
/// range into the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordHit {
    /// Char index of the first character of the word.
    pub start: usize,
    /// Char index one past the last character of the word (== caret).
    pub end: usize,
    pub text: String,
}

/// What completion applies at the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestCtx {
    pub hit: WordHit,
    /// True when the word follows a dot (`products.` / `p.`) — then only
    /// columns of the resolved table are offered.
    pub dotted: bool,
    /// Table that the dotted identifier resolved to (lowercase key).
    pub dotted_table: Option<String>,
    /// Uppercased token before the word (`FROM`, `JOIN`, …) or `None`.
    pub prev: Option<String>,
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Tokens of the text, character-based: identifiers (kept verbatim) and
/// single non-space characters.
fn tokens(text: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_ident_char(c) {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            out.push((start, chars[start..i].iter().collect()));
        } else if c.is_whitespace() {
            i += 1;
        } else {
            out.push((i, c.to_string()));
            i += 1;
        }
    }
    out
}

/// The identifier word ending exactly at `caret` (if any).
pub fn word_before_caret(text: &str, caret: usize) -> WordHit {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let mut start = caret;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    WordHit {
        start,
        end: caret,
        text: chars[start..caret].iter().collect(),
    }
}

/// Identifier immediately before the word (alias/dotted resolution).
fn ident_before(text: &str, before: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut end = before.min(chars.len());
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    if start == end {
        None
    } else {
        Some(chars[start..end].iter().collect())
    }
}

/// Uppercased token just before the word (identifier or punctuation).
fn prev_token(text: &str, word_start: usize) -> Option<String> {
    let toks = tokens(&text.chars().take(word_start).collect::<String>());
    toks.last().map(|(_, t)| t.to_ascii_uppercase())
}

/// alias → table pairs from FROM/JOIN clauses (`FROM products p`,
/// `JOIN orders AS o`).
pub fn alias_map(text: &str) -> Vec<(String, String)> {
    const NOT_ALIAS: &[&str] = &[
        "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "UNION", "LEFT", "RIGHT", "INNER",
        "OUTER", "CROSS", "JOIN", "ON", "USING", "SET", "VALUES", "SELECT", "AS", "FOR", "INTO",
        "STRAIGHT_JOIN", "USE", "IGNORE", "PARTITION",
    ];
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let up = toks[i].1.to_ascii_uppercase();
        if up == "FROM" || up == "JOIN" {
            // table
            if let Some((_, table)) = toks.get(i + 1) {
                if is_ident_char(table.chars().next().unwrap_or(' ')) {
                    let table = table.clone();
                    let mut j = i + 2;
                    // optional AS
                    if toks
                        .get(j)
                        .map(|(_, t)| t.eq_ignore_ascii_case("AS"))
                        .unwrap_or(false)
                    {
                        j += 1;
                    }
                    if let Some((_, alias)) = toks.get(j) {
                        let up_alias = alias.to_ascii_uppercase();
                        if is_ident_char(alias.chars().next().unwrap_or(' '))
                            && !NOT_ALIAS.contains(&up_alias.as_str())
                        {
                            out.push((alias.to_lowercase(), table));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Full completion context at the caret. `index` may be empty (offline) —
/// only keywords are then offered.
pub fn context(index: &CompletionIndex, text: &str, caret: usize) -> SuggestCtx {
    let hit = word_before_caret(text, caret);
    let chars: Vec<char> = text.chars().collect();
    let dotted = hit.start > 0 && chars.get(hit.start - 1) == Some(&'.');
    let prev = prev_token(text, hit.start);

    let dotted_table = if dotted {
        ident_before(text, hit.start.saturating_sub(1)).and_then(|ident| {
            let key = ident.to_lowercase();
            if index.columns.contains_key(&key) {
                Some(key)
            } else {
                alias_map(text)
                    .into_iter()
                    .find(|(a, _)| *a == key)
                    .map(|(_, t)| t.to_lowercase())
            }
        })
    } else {
        None
    };

    SuggestCtx {
        hit,
        dotted,
        dotted_table,
        prev,
    }
}

/// Ranked suggestions for the context, at most `max` entries.
pub fn candidates(ctx: &SuggestCtx, index: &CompletionIndex, max: usize) -> Vec<Suggestion> {
    let word = ctx.hit.text.to_lowercase();
    let mut out: Vec<Suggestion> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let push = |out: &mut Vec<Suggestion>,
                seen: &mut HashSet<String>,
                text: String,
                kind: SuggestKind| {
        if seen.insert(text.to_lowercase()) {
            out.push(Suggestion { text, kind });
        }
    };

    if ctx.dotted {
        // Columns of the resolved table only.
        let cols = ctx
            .dotted_table
            .as_ref()
            .and_then(|t| index.columns.get(t));
        if let Some(cols) = cols {
            for c in cols {
                if word.is_empty() || c.to_lowercase().starts_with(&word) {
                    push(&mut out, &mut seen, c.clone(), SuggestKind::Column);
                }
            }
        }
        return out.into_iter().take(max).collect();
    }

    let table_ctx = matches!(
        ctx.prev.as_deref(),
        Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "DATABASE" | "SHOW" | "DESCRIBE")
    );

    if table_ctx {
        for t in &index.tables {
            if word.is_empty() || t.to_lowercase().starts_with(&word) {
                push(&mut out, &mut seen, t.clone(), SuggestKind::Table);
            }
        }
        return out.into_iter().take(max).collect();
    }

    // Mixed context: columns of all tables, then tables, then keywords.
    let mut columns: Vec<String> = Vec::new();
    for cols in index.columns.values() {
        for c in cols {
            if word.is_empty() || c.to_lowercase().starts_with(&word) {
                columns.push(c.clone());
            }
        }
    }
    columns.sort_by_key(|c| c.to_lowercase());
    columns.truncate(max);
    for c in columns {
        push(&mut out, &mut seen, c, SuggestKind::Column);
    }
    if out.len() < max {
        for t in &index.tables {
            if word.is_empty() || t.to_lowercase().starts_with(&word) {
                push(&mut out, &mut seen, t.clone(), SuggestKind::Table);
                if out.len() >= max {
                    break;
                }
            }
        }
    }
    if out.len() < max && word.len() >= 2 {
        for k in KEYWORDS {
            if k.to_lowercase().starts_with(&word) {
                push(&mut out, &mut seen, (*k).to_string(), SuggestKind::Keyword);
                if out.len() >= max {
                    break;
                }
            }
        }
    }
    out.into_iter().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> CompletionIndex {
        CompletionIndex::from_pairs(
            [
                ("products", "id"),
                ("products", "name"),
                ("products", "price"),
                ("products", "note"),
                ("orders", "id"),
                ("orders", "product_id"),
                ("orders", "qty"),
                ("v_active", "name"),
            ]
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string())),
        )
    }

    #[test]
    fn index_builds_tables_and_columns() {
        let ix = index();
        assert_eq!(
            ix.tables,
            vec!["orders", "products", "v_active"]
        );
        assert_eq!(
            ix.columns["products"],
            vec!["id", "name", "price", "note"]
        );
        // duplicate columns deduplicate
        let dup = CompletionIndex::from_pairs([("t", "id"), ("t", "ID"), ("t", "x")]
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string())));
        assert_eq!(dup.columns["t"].len(), 2);
    }

    #[test]
    fn word_extraction() {
        let text = "SELECT na FROM x";
        let caret = "SELECT na".chars().count();
        let hit = word_before_caret(text, caret);
        assert_eq!(hit.text, "na");
        assert_eq!(hit.start, caret - 2);

        // caret at the very end grabs the LAST word
        let hit = word_before_caret(text, text.chars().count());
        assert_eq!(hit.text, "x");

        // caret right after a space → empty word
        let hit = word_before_caret("SELECT ", 7);
        assert_eq!(hit.text, "");
        assert_eq!(hit.start, 7);
    }

    #[test]
    fn from_context_suggests_tables() {
        let ix = index();
        let text = "SELECT * FROM pro";
        let caret = text.chars().count();
        let ctx = context(&ix, text, caret);
        assert_eq!(ctx.prev.as_deref(), Some("FROM"));
        let cands = candidates(&ctx, &ix, 12);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].text, "products");
        assert_eq!(cands[0].kind, SuggestKind::Table);
    }

    #[test]
    fn mixed_context_columns_first_then_tables_then_keywords() {
        let ix = index();
        let text = "SELECT id FROM products WHERE x = '' ORDER BY na";
        let caret = text.chars().count();
        let ctx = context(&ix, text, caret);
        let cands = candidates(&ctx, &ix, 12);
        assert_eq!(cands[0].text, "name"); // column of products (and v_active, deduped)
        assert_eq!(cands[0].kind, SuggestKind::Column);

        // no column matches → tables match the prefix
        let text = "SELECT * FROM products WHERE ord";
        let caret = text.chars().count();
        let ctx = context(&ix, text, caret);
        let cands = candidates(&ctx, &ix, 12);
        assert!(cands
            .iter()
            .any(|s| s.text == "orders" && s.kind == SuggestKind::Table));
        assert!(!cands.iter().any(|s| s.kind == SuggestKind::Column));
    }

    #[test]
    fn keywords_only_from_two_chars() {
        let ix = CompletionIndex::default();
        let ctx = context(&ix, "SEL", 3);
        let cands = candidates(&ctx, &ix, 12);
        assert!(cands.iter().any(|s| s.text == "SELECT"));
        // one character: no keyword noise
        let ctx = context(&ix, "S", 1);
        assert!(candidates(&ctx, &ix, 12).is_empty());
    }

    #[test]
    fn dot_resolves_table_directly() {
        let ix = index();
        let text = "SELECT products. FROM products";
        let caret = "SELECT products.".chars().count();
        let ctx = context(&ix, text, caret);
        assert!(ctx.dotted);
        assert_eq!(ctx.dotted_table.as_deref(), Some("products"));
        let cands = candidates(&ctx, &ix, 12);
        let names: Vec<_> = cands.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "price", "note"]);
    }

    #[test]
    fn dot_resolves_alias() {
        let ix = index();
        let text = "SELECT p. FROM products AS p JOIN orders o ON o.id = p.id";
        let caret = "SELECT p.".chars().count();
        let ctx = context(&ix, text, caret);
        assert_eq!(ctx.dotted_table.as_deref(), Some("products"));
        let cands = candidates(&ctx, &ix, 12);
        assert!(cands.iter().any(|s| s.text == "price"));

        // "o." resolves to orders
        let head = "SELECT p. FROM products AS p JOIN orders o ON o.";
        let ctx = context(&ix, text, head.chars().count());
        assert_eq!(ctx.dotted_table.as_deref(), Some("orders"));
    }

    #[test]
    fn alias_map_parses() {
        let am = alias_map("SELECT * FROM products p JOIN orders AS o ON p.id = o.product_id");
        assert!(am.contains(&("p".into(), "products".into())));
        assert!(am.contains(&("o".into(), "orders".into())));
        // keywords never become aliases
        let am = alias_map("SELECT * FROM products WHERE id = 1");
        assert!(am.is_empty());
        let am = alias_map("SELECT * FROM products");
        assert!(am.is_empty());
    }

    #[test]
    fn cap_limits_results() {
        let ix = index();
        let text = "SELECT * FROM "; // tables context, empty word
        let ctx = context(&ix, text, text.chars().count());
        let cands = candidates(&ctx, &ix, 2);
        assert_eq!(cands.len(), 2);
    }

    #[test]
    fn unicode_caret_is_char_based() {
        // Thai text before the SQL must not shift the word detection.
        let text = "สวัสดี SELECT na";
        let caret = text.chars().count();
        let hit = word_before_caret(text, caret);
        assert_eq!(hit.text, "na");
    }
}
