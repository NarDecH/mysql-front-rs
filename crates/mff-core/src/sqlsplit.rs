//! SQL script splitting — the useful subset of the original 29k-line
//! `SQLParser.pas` that is actually needed for executing scripts:
//! correct handling of quoted strings, comments and `DELIMITER`.

/// Split a SQL script into individual statements.
///
/// * string literals (`'…'`, `"…"`) and identifiers (`` `…` ``) with doubled
///   quote escapes are respected
/// * `-- …`, `# …` line comments and `/* … */` block comments are skipped
///   (including inside executable `/*!…*/` comments, whose content is kept)
/// * `DELIMITER x` directives switch the statement terminator (e.g. for
///   stored routine dumps) and are not emitted as statements
pub fn split_statements(input: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut delimiter: String = ";".to_string();
    let mut current = String::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("DELIMITER ").or(trimmed.strip_prefix("delimiter ")) {
            let new_delim = rest.trim().to_string();
            if !new_delim.is_empty() {
                // Flush whatever is pending before switching delimiters.
                if !current.trim().is_empty() {
                    statements.push(std::mem::take(&mut current));
                }
                delimiter = new_delim;
                continue;
            }
        }
        current.push_str(line);
        current.push('\n');

        // Look for a statement terminator outside of quotes/comments.
        while let Some(pos) = find_terminator(&current, &delimiter) {
            let stmt = current[..pos].to_string();
            let rest = current[pos + delimiter.len()..].to_string();
            current = rest;
            if !is_effectively_empty(&stmt) {
                statements.push(stmt.trim().to_string());
            }
        }
    }
    if !is_effectively_empty(&current) {
        statements.push(current.trim().to_string());
    }
    statements
}

/// Find the end position of a statement: the first occurrence of
/// `terminator` that is *not* inside a string, identifier quote or comment.
fn find_terminator(text: &str, terminator: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with('\'') || rest.starts_with('"') || rest.starts_with('`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && quote != b'`' {
                    i += 2; // backslash escape
                    continue;
                }
                if bytes[i] == quote {
                    if i + 1 < bytes.len() && bytes[i + 1] == quote {
                        i += 2; // doubled quote escape
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Line comments.
        if rest.starts_with('#') {
            i = text[i..].find('\n').map(|p| i + p).unwrap_or(bytes.len());
            continue;
        }
        if rest.starts_with("--")
            && rest
                .as_bytes()
                .get(2)
                .map(|b| *b == b' ' || *b == b'\t' || *b == b'\n' || *b == b'\r')
                .unwrap_or(true)
        {
            i = text[i..].find('\n').map(|p| i + p).unwrap_or(bytes.len());
            continue;
        }
        // Block comment (including executable /*! ... */).
        if rest.starts_with("/*") {
            match text[i + 2..].find("*/") {
                Some(end) => {
                    i += 2 + end + 2;
                    continue;
                }
                None => return None, // unterminated comment: keep waiting
            }
        }
        if rest.starts_with(terminator) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// True when a statement contains nothing but whitespace and comments.
pub fn is_effectively_empty(stmt: &str) -> bool {
    let mut rest = stmt;
    loop {
        let t = rest.trim();
        if t.is_empty() {
            return true;
        }
        if t.starts_with('#') {
            rest = t.split_once('\n').map(|(_, r)| r).unwrap_or("");
            continue;
        }
        if t.starts_with("--") {
            rest = t.split_once('\n').map(|(_, r)| r).unwrap_or("");
            continue;
        }
        if t.starts_with("/*") {
            // Executable comments (/*! ... */) are real statements.
            if t.starts_with("/*!") {
                return false;
            }
            match t.find("*/") {
                Some(end) => {
                    rest = &t[end + 2..];
                    continue;
                }
                None => return true,
            }
        }
        return false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_statements() {
        let sql = "SELECT 1; SELECT 2;\nUPDATE t SET a=1;";
        let out = split_statements(sql);
        assert_eq!(out, vec!["SELECT 1", "SELECT 2", "UPDATE t SET a=1"]);
    }

    #[test]
    fn semicolons_inside_strings_do_not_split() {
        let sql = "INSERT INTO t VALUES ('a;b', \"c;d\", `e;f`); SELECT 2;";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("'a;b'"));
        assert!(out[0].contains("\"c;d\""));
        assert!(out[0].contains("`e;f`"));
    }

    #[test]
    fn escapes_are_respected() {
        let sql = r#"SELECT 'it''s; fine'; SELECT 'back\'; "#;
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("it''s; fine"));
        assert!(out[1].contains("back\\"));
    }

    #[test]
    fn comments_are_skipped() {
        let sql = "-- leading comment ; with semicolon\nSELECT 1; # trailing; comment\n/* block ; comment */ SELECT 2;";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn pure_comment_statements_are_dropped() {
        // The semicolon inside the comment does not split; a statement that
        // consists only of a comment is dropped.
        let sql = "-- just a comment\n# another\nSELECT 1;";
        let out = split_statements(sql);
        assert_eq!(out, vec!["-- just a comment\n# another\nSELECT 1"]);
        let out = split_statements("-- only a comment;");
        assert!(out.is_empty());
    }

    #[test]
    fn delimiter_directive_works() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("CREATE PROCEDURE"));
        assert!(out[0].contains("SELECT 1; SELECT 2; END"));
        assert_eq!(out[1], "SELECT 3");
    }

    #[test]
    fn executable_comments_stay_intact() {
        let sql = "/*!40000 ALTER TABLE `t` DISABLE KEYS */; SELECT 1;";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("/*!40000"));
    }

    #[test]
    fn unterminated_string_waits_for_more() {
        // A single statement without terminator still gets flushed at EOF.
        let out = split_statements("SELECT 'unterminated");
        assert_eq!(out.len(), 1);
    }
}
