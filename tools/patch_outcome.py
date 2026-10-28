#!/usr/bin/env python3
"""Fix TransferOutcome model: move error into TransferSummary."""
import io

p = r"crates\mff-core\src\transfer.rs"
src = io.open(p, encoding="utf-8").read()

old = """#[derive(Debug, Clone, Default)]
pub struct TransferSummary {
    pub tables: usize,
    pub views: usize,
    pub rows: u64,
}"""
assert old in src, "summary struct"
src = src.replace(old, """#[derive(Debug, Clone, Default)]
pub struct TransferSummary {
    pub tables: usize,
    pub views: usize,
    pub rows: u64,
    /// Set when the transfer stopped early.
    pub error: Option<String>,
}""", 1)

old = r'''impl TransferOutcome {
    fn fail(mut summary: TransferSummary, error: String) -> Self {
        summary.error = Some(error);
        TransferOutcome { summary: summary.clone(), error: None }
    }

    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }

    pub fn describe(&self) -> String {
        let s = &self.summary;
        let mut text = format!(
            "{} table(s), {} view(s), {} row(s) transferred",
            s.tables, s.views, s.rows
        );
        if let Some(e) = &self.error {
            text.push_str(&format!(" \u{2014} failed: {e}"));
        }
        text
    }
}'''
assert old in src, "outcome impl"
src = src.replace(old, r'''impl TransferOutcome {
    pub fn is_ok(&self) -> bool {
        self.summary.error.is_none()
    }

    pub fn describe(&self) -> String {
        let s = &self.summary;
        let mut text = format!(
            "{} table(s), {} view(s), {} row(s) transferred",
            s.tables, s.views, s.rows
        );
        if let Some(e) = &s.error {
            text.push_str(&format!(" \u{2014} failed: {e}"));
        }
        text
    }
}''', 1)

old = """    let mut summary = TransferSummary::default();
    let fail = |mut s: TransferSummary, e: String| {
        s.error = Some(e);
        TransferOutcome { summary: s, error: None }
    };"""
assert old in src, "fail closure"
src = src.replace(old, """    let mut summary = TransferSummary::default();
    let fail = |mut s: TransferSummary, e: String| {
        s.error = Some(e);
        TransferOutcome { summary: s, error: None }
    };
    let _ = &summary;""", 1)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("outcome model fixed")
