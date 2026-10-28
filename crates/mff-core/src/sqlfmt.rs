//! A light SQL formatter: uppercases keywords and puts major clauses
//! (FROM / WHERE / ORDER BY / …) and top-level list items on their own
//! indented lines. String literals, quoted identifiers and comments are
//! preserved verbatim.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Keywords recognised for uppercasing (also used by suggestions).
pub const KEYWORDS: &[&str] = &[
    "SELECT", "DISTINCT", "DISTINCTROW", "FROM", "WHERE", "AND", "OR", "XOR", "NOT", "NULL", "IS",
    "IN", "LIKE", "RLIKE", "REGEXP", "BETWEEN", "EXISTS", "AS", "JOIN", "LEFT", "RIGHT", "INNER",
    "OUTER", "CROSS", "FULL", "ON", "USING", "GROUP", "BY", "HAVING", "ORDER", "LIMIT", "OFFSET",
    "UNION", "ALL", "ANY", "SOME", "INSERT", "INTO", "VALUES", "VALUE", "UPDATE", "SET", "DELETE",
    "CREATE", "ALTER", "DROP", "TABLE", "VIEW", "INDEX", "DATABASE", "SCHEMA", "PRIMARY", "KEY",
    "FOREIGN", "REFERENCES", "CONSTRAINT", "UNIQUE", "DEFAULT", "AUTO_INCREMENT", "ENGINE",
    "CHARSET", "CHARACTER", "COLLATE", "COMMENT", "IF", "ELSE", "THEN", "END", "CASE", "WHEN",
    "ASC", "DESC", "USE", "SHOW", "DESCRIBE", "EXPLAIN", "BEGIN", "COMMIT", "ROLLBACK", "START",
    "TRANSACTION", "TRUNCATE", "REPLACE", "DUPLICATE", "INTERVAL", "CAST", "TRUE", "FALSE",
    "PROCEDURE", "FUNCTION", "TRIGGER", "EVENT", "RETURNS", "RETURN", "DECLARE", "CALL",
];

fn keywords() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| KEYWORDS.iter().copied().collect())
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// An identifier or keyword: uppercased form + original spelling.
    Word { up: String, orig: String },
    Comma,
    LParen,
    RParen,
    Semi,
    /// String literal / quoted identifier — verbatim including the quotes.
    Str(String),
    /// `-- …` or `/* … */` — verbatim.
    Comment(String),
    /// A run of whitespace outside strings.
    Space,
    Other(char),
}

fn lex(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '\'' | '"' | '`' => {
                let quote = c;
                let mut s = String::new();
                s.push(quote);
                chars.next();
                while let Some(&n) = chars.peek() {
                    s.push(n);
                    chars.next();
                    if n == quote {
                        // A doubled quote is an escaped quote — keep going.
                        if chars.peek() == Some(&quote) {
                            s.push(quote);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // Backslash escapes inside double/single quoted strings.
                    if n == '\\' && quote != '`' {
                        if let Some(&e) = chars.peek() {
                            s.push(e);
                            chars.next();
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            '-' if chars.clone().nth(1) == Some('-') => {
                let mut s = String::from("--");
                chars.next();
                chars.next();
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Tok::Comment(s));
            }
            '#' => {
                let mut s = String::from("#");
                chars.next();
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Tok::Comment(s));
            }
            '/' if chars.clone().nth(1) == Some('*') => {
                let mut s = String::from("/*");
                chars.next();
                chars.next();
                while let Some(&n) = chars.peek() {
                    s.push(n);
                    chars.next();
                    if n == '*' && chars.peek() == Some(&'/') {
                        s.push('/');
                        chars.next();
                        break;
                    }
                }
                out.push(Tok::Comment(s));
            }
            c if c.is_whitespace() => {
                while chars.peek().map(|n| n.is_whitespace()).unwrap_or(false) {
                    chars.next();
                }
                out.push(Tok::Space);
            }
            ',' => {
                chars.next();
                out.push(Tok::Comma);
            }
            '(' => {
                chars.next();
                out.push(Tok::LParen);
            }
            ')' => {
                chars.next();
                out.push(Tok::RParen);
            }
            ';' => {
                chars.next();
                out.push(Tok::Semi);
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                let mut w = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' || n == '$' {
                        w.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(Tok::Word {
                    up: w.to_ascii_uppercase(),
                    orig: w,
                });
            }
            other => {
                chars.next();
                out.push(Tok::Other(other));
            }
        }
    }
    out
}

/// True when this word must not continue the previous line (e.g. `JOIN`
/// after `LEFT`, `ALL` after `UNION`).
fn no_break_after_prev(prev: &str, up: &str) -> bool {
    matches!(
        (prev, up),
        ("LEFT", "OUTER")
            | ("RIGHT", "OUTER")
            | ("FULL", "OUTER")
            | ("LEFT", "JOIN")
            | ("RIGHT", "JOIN")
            | ("INNER", "JOIN")
            | ("CROSS", "JOIN")
            | ("FULL", "JOIN")
            | ("OUTER", "JOIN")
            | ("UNION", "ALL")
    )
}

/// True when a line break goes before this word.
fn is_clause_start(up: &str) -> bool {
    matches!(
        up,
        "SELECT"
            | "FROM"
            | "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "VALUES"
            | "SET"
            | "JOIN"
            | "LEFT"
            | "RIGHT"
            | "INNER"
            | "CROSS"
            | "FULL"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "TRUNCATE"
            | "REPLACE"
            | "USE"
            | "SHOW"
    )
}

fn indent(level: usize) -> String {
    "    ".repeat(level)
}

/// Format one SQL script (one or more statements).
pub fn format(src: &str) -> String {
    let toks = lex(src);
    let kw = keywords();
    let mut out = String::with_capacity(src.len() + 64);
    let mut depth = 0usize; // paren depth
    let mut line_open = false; // something already on the current line
    let mut prev_word = String::new();
    let mut after_semi = true; // statement just started → no break for leading clause words

    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Word { up, orig } => {
                let text = if kw.contains(up.as_str()) {
                    up.clone()
                } else {
                    orig.clone()
                };
                // A clause word starts a new line unless we are right at the
                // start of a statement (or continuing a compound like LEFT JOIN).
                let starts_clause = is_clause_start(up) && !after_semi;
                let cont = no_break_after_prev(&prev_word, up);
                if starts_clause && !cont && line_open {
                    out.push('\n');
                    out.push_str(&indent(depth));
                } else if line_open && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(&text);
                line_open = true;
                prev_word = up.clone();
                after_semi = false;
            }
            Tok::Comma => {
                out.push(',');
                if depth == 0 {
                    out.push('\n');
                    out.push_str(&indent(depth + 1));
                } else {
                    out.push(' ');
                }
            }
            Tok::LParen => {
                if line_open && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push('(');
                depth += 1;
                line_open = true;
            }
            Tok::RParen => {
                depth = depth.saturating_sub(1);
                out.push(')');
            }
            Tok::Semi => {
                out.push(';');
                out.push('\n');
                line_open = false;
                prev_word.clear();
                after_semi = true;
            }
            Tok::Str(s) => {
                if line_open && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(s);
                line_open = true;
            }
            Tok::Comment(s) => {
                if s.starts_with("--") || s.starts_with('#') {
                    // Line comments end the line.
                    if line_open {
                        out.push(' ');
                    }
                    out.push_str(s.trim_end());
                    out.push('\n');
                    out.push_str(&indent(depth));
                    line_open = false;
                } else {
                    if line_open && !out.ends_with(' ') && !out.ends_with('\n') {
                        out.push(' ');
                    }
                    out.push_str(s);
                }
            }
            Tok::Space => {
                if line_open
                    && !out.ends_with(' ')
                    && !out.ends_with('\n')
                    && !out.ends_with('(')
                {
                    out.push(' ');
                }
            }
            Tok::Other(c) => {
                // Operators get a space before them (`a = 1`, `id > 10`)
                // unless they follow another operator (`>=`, `:=`, `-1`).
                if matches!(c, '=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%')
                    && out
                        .chars()
                        .last()
                        .map(|p| p.is_alphanumeric() || p == '_' || p == '$' || p == '\'' || p == '`' || p == ')')
                        .unwrap_or(false)
                {
                    out.push(' ');
                }
                out.push(*c);
                line_open = true;
            }
        }
        i += 1;
    }

    // Tidy: trim trailing spaces per line, collapse >1 blank lines, trim ends.
    let mut lines: Vec<String> = out
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    while lines.len() >= 2 && lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    let mut cleaned = String::new();
    let mut blanks = 0;
    for l in &lines {
        if l.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        cleaned.push_str(l);
        cleaned.push('\n');
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercases_and_breaks_clauses() {
        let sql = "select id,name from users where id>10 order by id desc limit 20";
        let out = format(sql);
        assert!(out.contains("SELECT id,"));
        assert!(out.contains("\nFROM users"));
        assert!(out.contains("\nWHERE"));
        assert!(out.contains("\nORDER BY id DESC"));
        assert!(out.contains("\nLIMIT 20"));
    }

    #[test]
    fn preserves_strings_and_case() {
        let sql = "select * from t where name = 'select from where'";
        let out = format(sql);
        assert!(out.contains("'select from where'"), "{out}");
    }

    #[test]
    fn preserves_backtick_identifiers() {
        let sql = "select `order`, `group` from t";
        let out = format(sql);
        assert!(out.contains("`order`"), "{out}");
        assert!(out.contains("`group`"), "{out}");
    }

    #[test]
    fn keeps_comments() {
        let sql = "select 1 -- pick one\nfrom t /* inline */ where x = 1";
        let out = format(sql);
        assert!(out.contains("-- pick one"), "{out}");
        assert!(out.contains("/* inline */"), "{out}");
    }

    #[test]
    fn joins_stay_together() {
        let sql = "select * from a left join b on a.id = b.id inner join c on b.x = c.x";
        let out = format(sql);
        assert!(out.contains("LEFT JOIN b"), "{out}");
        assert!(out.contains("INNER JOIN c"), "{out}");
    }

    #[test]
    fn multi_statement() {
        let out = format("update t set a=1 where id=2; delete from t where id=3;");
        assert!(out.contains("UPDATE t"), "{out}");
        assert!(out.contains("SET a = 1"), "{out}");
        assert!(out.contains(";\n"), "{out}");
        assert!(out.contains("DELETE"), "{out}");
        assert!(out.contains("FROM t"), "{out}");
    }

    #[test]
    fn escaped_quote_in_string() {
        let out = format("select * from t where s = 'it\\'s ok' and x = 1");
        assert!(out.contains("'it\\'s ok'"), "{out}");
    }
}
