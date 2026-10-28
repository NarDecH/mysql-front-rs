#!/usr/bin/env python3
"""Append needs_confirm_no_where + tests to sqlsplit.rs."""
import io

p = "crates/mff-core/src/sqlsplit.rs"
s = io.open(p, encoding="utf-8").read()
assert "needs_confirm_no_where" not in s

s += '''
/// True when the statement is an UPDATE or DELETE **without a WHERE
/// clause** — the kind of statement that rewrites or removes every row of
/// the table and is worth a confirmation dialog.
///
/// Strings, quoted identifiers, `#`, `--` and `/* */` comments are skipped
/// so a literal `'where'` or a commented-out condition does not count.
pub fn needs_confirm_no_where(stmt: &str) -> bool {
    #[derive(PartialEq)]
    enum St {
        Code,
        SQuote,
        DQuote,
        Backtick,
        LineComment,
        BlockComment,
    }
    let mut st = St::Code;
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut flush = |words: &mut Vec<String>, cur: &mut String| {
        if !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
    };
    let chars: Vec<char> = stmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match st {
            St::Code => match c {
                '\\'' => {
                    flush(&mut words, &mut cur);
                    st = St::SQuote;
                }
                '"' => {
                    flush(&mut words, &mut cur);
                    st = St::DQuote;
                }
                '`' => {
                    flush(&mut words, &mut cur);
                    st = St::Backtick;
                }
                '-' if next == Some('-') => {
                    flush(&mut words, &mut cur);
                    st = St::LineComment;
                    i += 1;
                }
                '#' => {
                    flush(&mut words, &mut cur);
                    st = St::LineComment;
                }
                '/' if next == Some('*') => {
                    flush(&mut words, &mut cur);
                    st = St::BlockComment;
                    i += 1;
                }
                c if c.is_alphanumeric() || c == '_' || c == '$' => cur.push(c),
                _ => flush(&mut words, &mut cur),
            },
            St::SQuote => {
                if c == '\\\\' {
                    i += 1;
                } else if c == '\\'' {
                    // Doubled '' is an escaped quote and stays in the string.
                    if next == Some('\\'') {
                        i += 1;
                    } else {
                        st = St::Code;
                    }
                }
            }
            St::DQuote => {
                if c == '\\\\' {
                    i += 1;
                } else if c == '"' {
                    st = St::Code;
                }
            }
            St::Backtick => {
                if c == '`' {
                    if next == Some('`') {
                        i += 1;
                    } else {
                        st = St::Code;
                    }
                }
            }
            St::LineComment => {
                if c == '\\n' {
                    st = St::Code;
                }
            }
            St::BlockComment => {
                if c == '*' && next == Some('/') {
                    st = St::Code;
                    i += 1;
                }
            }
        }
        i += 1;
    }
    flush(&mut words, &mut cur);
    let upper: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
    let Some(first) = upper.first() else {
        return false;
    };
    if first != "UPDATE" && first != "DELETE" {
        return false;
    }
    // `DELETE FROM t WHERE …` / `UPDATE t SET … WHERE …`: a WHERE anywhere
    // in the statement guards it (subqueries with their own WHERE also
    // count — good enough for the safety net).
    !upper.iter().any(|w| w == "WHERE")
}

#[cfg(test)]
mod no_where_tests {
    use super::needs_confirm_no_where as risky;

    #[test]
    fn flags_bare_updates_and_deletes() {
        assert!(risky("UPDATE t SET x = 1"));
        assert!(risky("DELETE FROM t"));
        assert!(risky("  update `t` set x = 1 ;"));
        assert!(risky("DELETE\\nFROM t\\n;"));
    }

    #[test]
    fn allows_where_clauses() {
        assert!(!risky("UPDATE t SET x = 1 WHERE id = 2"));
        assert!(!risky(
            "DELETE FROM t WHERE id IN (SELECT id FROM u WHERE ok = 1)"
        ));
    }

    #[test]
    fn ignores_strings_and_comments() {
        assert!(risky("UPDATE t SET x = 'where'"));
        assert!(risky("UPDATE t SET x = \\"WHERE\\""));
        assert!(risky("UPDATE t SET x = 1 -- WHERE id = 2\\n"));
        assert!(risky("UPDATE t SET x = 1 /* WHERE */"));
        assert!(risky("# a WHERE comment\\nUPDATE t SET x = 1"));
        // quoted identifier named where
        assert!(risky("UPDATE `where` SET x = 1"));
    }

    #[test]
    fn other_statements_are_untouched() {
        assert!(!risky("SELECT * FROM t"));
        assert!(!risky("INSERT INTO t VALUES (1)"));
        assert!(!risky("TRUNCATE t"));
        assert!(!risky(""));
    }
}
'''

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("appended")
