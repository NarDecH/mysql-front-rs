//! Export result grids to CSV / JSON / SQL / HTML / XML.
//!
//! Covers the file-based export formats of the original `TTExportSQL`,
//! `TTExportText` (CSV), `TTExportHTML` and `TTExportXML` tool classes;
//! ODBC/Access/Excel targets and PDF printing are on the roadmap.

use std::io::{BufWriter, Write};

use serde_json::json;

use crate::error::Result;
use crate::model::Table;
use crate::session::{Cell, Grid};
use crate::sqlgen;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    #[default]
    Csv,
    Json,
    Sql,
    Html,
    Xml,
}

impl ExportFormat {
    /// Parse from a name (`csv`, `json`, `sql`, `html`, `xml`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" | "text" | "txt" => Some(ExportFormat::Csv),
            "json" => Some(ExportFormat::Json),
            "sql" => Some(ExportFormat::Sql),
            "html" | "htm" => Some(ExportFormat::Html),
            "xml" => Some(ExportFormat::Xml),
            _ => None,
        }
    }

    /// Canonical file extension.
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Sql => "sql",
            ExportFormat::Html => "html",
            ExportFormat::Xml => "xml",
        }
    }

    /// All format names, e.g. for UI pickers.
    pub fn all() -> &'static [&'static str] {
        &["csv", "json", "sql", "html", "xml"]
    }
}

/// Options controlling a single export run.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Text used for NULL cells (default `NULL`).
    pub null_text: String,
    /// CSV delimiter.
    pub delimiter: char,
    /// Emit a header row with column names (CSV/XML-attributes).
    pub include_header: bool,
    /// For SQL exports: batch this many rows per INSERT.
    pub batch_size: usize,
    /// For SQL exports: include the CREATE TABLE statement first.
    pub include_create: bool,
    /// For SQL exports: emit an explicit column list.
    pub include_column_list: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Csv,
            null_text: "NULL".to_string(),
            delimiter: ',',
            include_header: true,
            batch_size: 1000,
            include_create: true,
            include_column_list: true,
        }
    }
}

/// Export a grid to a byte vector (writes through a temp buffer).
pub fn export_to_string(grid: &Grid, table: Option<&Table>, opts: &ExportOptions) -> Result<String> {
    let mut buf = BufWriter::new(Vec::new());
    export(grid, table, opts, &mut buf)?;
    let bytes = buf.into_inner().map_err(std::io::IntoInnerError::into_error)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Export a grid to any writer.
pub fn export<W: Write>(
    grid: &Grid,
    table: Option<&Table>,
    opts: &ExportOptions,
    out: &mut W,
) -> Result<()> {
    match opts.format {
        ExportFormat::Csv => export_csv(grid, opts, out),
        ExportFormat::Json => export_json(grid, out),
        ExportFormat::Sql => export_sql(grid, table, opts, out),
        ExportFormat::Html => export_html(grid, opts, out),
        ExportFormat::Xml => export_xml(grid, opts, out),
    }
}

/// CSV export: header row (optional) + data rows; quoting handled by the
/// `csv` crate (RFC 4180 style, matching the original `CSVUtils.pas`).
pub fn export_csv<W: Write>(grid: &Grid, opts: &ExportOptions, out: &mut W) -> Result<()> {
    let mut w = csv::WriterBuilder::new()
        .delimiter(opts.delimiter as u8)
        .has_headers(opts.include_header)
        .from_writer(out);

    if opts.include_header {
        let header: Vec<String> = grid.columns.iter().map(|c| c.name.clone()).collect();
        w.write_record(&header)?;
    }
    for row in &grid.rows {
        let record: Vec<String> = row.iter().map(|c| c.display(&opts.null_text)).collect();
        w.write_record(&record)?;
    }
    w.flush()?;
    Ok(())
}

/// JSON export: `{"columns": […], "rows": [[…], …]}` with numbers kept as
/// numbers, NULL as `null` and everything else as strings.
pub fn export_json<W: Write>(grid: &Grid, out: &mut W) -> Result<()> {
    let columns: Vec<&str> = grid.columns.iter().map(|c| c.name.as_str()).collect();
    let mut rows = Vec::with_capacity(grid.rows.len());
    for row in &grid.rows {
        let mut vals = Vec::with_capacity(row.len());
        for (cell, col) in row.iter().zip(grid.columns.iter()) {
            vals.push(match cell {
                Cell::Null => serde_json::Value::Null,
                Cell::Bytes(b) => json!(String::from_utf8_lossy(b).into_owned()),
                Cell::Text(s) => match crate::sqlgen::type_class(&col.type_str) {
                    crate::sqlgen::TypeClass::Numeric => {
                        if let Ok(i) = s.parse::<i128>() {
                            json!(i)
                        } else if let Ok(u) = s.parse::<u128>() {
                            json!(u)
                        } else if let Ok(f) = s.parse::<f64>() {
                            json!(f)
                        } else {
                            json!(s)
                        }
                    }
                    _ => json!(s),
                },
            });
        }
        rows.push(vals);
    }
    let doc = json!({ "columns": columns, "rows": rows });
    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

/// SQL export: optional `CREATE TABLE` + batched multi-row `INSERT`s.
pub fn export_sql<W: Write>(
    grid: &Grid,
    table: Option<&Table>,
    opts: &ExportOptions,
    out: &mut W,
) -> Result<()> {
    let table_name = table
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "exported_data".to_string());

    if opts.include_create {
        if let Some(t) = table {
            let ddl = t
                .create_statement
                .clone()
                .unwrap_or_else(|| sqlgen::create_table_sql(t));
            writeln!(out, "{ddl}\n")?;
        }
    }

    let (stmts, _count) = sqlgen::insert_statements(
        &table_name,
        grid,
        opts.batch_size,
        opts.include_column_list,
    );
    for s in &stmts {
        writeln!(out, "{s}")?;
    }
    Ok(())
}

/// HTML export: a standalone, self-styled table document.
pub fn export_html<W: Write>(grid: &Grid, opts: &ExportOptions, out: &mut W) -> Result<()> {
    write!(
        out,
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>mysql-front-rs export</title>
<style>
  body {{ font-family: "Segoe UI", sans-serif; margin: 1.5em; }}
  table {{ border-collapse: collapse; }}
  th, td {{ border: 1px solid #bbb; padding: 4px 10px; text-align: left; }}
  th {{ background: #2d5b8e; color: #fff; }}
  tr:nth-child(even) {{ background: #f3f6fa; }}
  td.null {{ color: #999; font-style: italic; }}
</style>
</head>
<body>
<table>
"#
    )?;
    if opts.include_header {
        out.write_all(b"<thead><tr>")?;
        for c in &grid.columns {
            write!(out, "<th>{}</th>", escape_html(&c.name))?;
        }
        out.write_all(b"</tr></thead>\n")?;
    }
    out.write_all(b"<tbody>\n")?;
    for row in &grid.rows {
        out.write_all(b"<tr>")?;
        for cell in row {
            match cell {
                Cell::Null => out.write_all(b"<td class=\"null\">NULL</td>")?,
                Cell::Bytes(b) => {
                    write!(out, "<td>{}</td>", escape_html(&String::from_utf8_lossy(b)))?
                }
                Cell::Text(s) => write!(out, "<td>{}</td>", escape_html(s))?,
            }
        }
        out.write_all(b"</tr>\n")?;
    }
    out.write_all(b"</tbody>\n</table>\n</body>\n</html>\n")?;
    Ok(())
}

/// XML export with a simple, stable schema:
/// `<result><row><column name="…">value</column>…</row>…</result>`.
pub fn export_xml<W: Write>(grid: &Grid, _opts: &ExportOptions, out: &mut W) -> Result<()> {
    out.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<result>\n")?;
    for row in &grid.rows {
        out.write_all(b"  <row>\n")?;
        for (cell, col) in row.iter().zip(grid.columns.iter()) {
            let name = escape_xml_attr(&col.name);
            match cell {
                Cell::Null => write!(out, "    <column name=\"{name}\" null=\"true\"/>")?,
                Cell::Bytes(b) => {
                    write!(
                        out,
                        "    <column name=\"{name}\">{}</column>",
                        escape_xml_text(&String::from_utf8_lossy(b))
                    )?
                }
                Cell::Text(s) => write!(
                    out,
                    "    <column name=\"{name}\">{}</column>",
                    escape_xml_text(s)
                )?,
            }
            out.write_all(b"\n")?;
        }
        out.write_all(b"  </row>\n")?;
    }
    out.write_all(b"</result>\n")?;
    Ok(())
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_text(s: &str) -> String {
    escape_html(s)
}

fn escape_xml_attr(s: &str) -> String {
    escape_xml_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ColMeta;

    fn sample_grid() -> Grid {
        Grid {
            columns: vec![
                ColMeta::new("id", "LONG"),
                ColMeta::new("name", "VARCHAR"),
                ColMeta::new("score", "DOUBLE"),
            ],
            rows: vec![
                vec![
                    Cell::Text("1".into()),
                    Cell::Text("Ann".into()),
                    Cell::Text("9.5".into()),
                ],
                vec![
                    Cell::Text("2".into()),
                    Cell::Null,
                    Cell::Text("3.25".into()),
                ],
            ],
        }
    }

    #[test]
    fn csv_export_roundtrip_shape() {
        let opts = ExportOptions::default();
        let s = export_to_string(&sample_grid(), None, &opts).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "id,name,score");
        assert_eq!(lines[1], "1,Ann,9.5");
        assert_eq!(lines[2], "2,NULL,3.25");
    }

    #[test]
    fn csv_custom_delimiter() {
        let opts = ExportOptions {
            delimiter: ';',
            ..Default::default()
        };
        let s = export_to_string(&sample_grid(), None, &opts).unwrap();
        assert!(s.contains("id;name;score"));
    }

    #[test]
    fn json_keeps_numbers() {
        let s = export_to_string(&sample_grid(), None, &ExportOptions { format: ExportFormat::Json, ..Default::default() }).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(doc["columns"][0], "id");
        assert_eq!(doc["rows"][0][0], 1);
        assert_eq!(doc["rows"][1][1], serde_json::Value::Null);
        assert_eq!(doc["rows"][0][2], 9.5);
    }

    #[test]
    fn sql_export_includes_inserts() {
        let opts = ExportOptions {
            format: ExportFormat::Sql,
            include_create: false,
            ..Default::default()
        };
        let s = export_to_string(&sample_grid(), None, &opts).unwrap();
        assert!(s.contains("INSERT INTO `exported_data` (`id`, `name`, `score`) VALUES"));
        assert!(s.contains("(1, 'Ann', 9.5)"));
        assert!(s.contains("(2, NULL, 3.25)"));
    }

    #[test]
    fn html_escapes_markup() {
        let g = Grid {
            columns: vec![ColMeta::new("x", "VARCHAR")],
            rows: vec![vec![Cell::Text("<b>&</b>".into())]],
        };
        let s = export_to_string(&g, None, &ExportOptions { format: ExportFormat::Html, ..Default::default() }).unwrap();
        assert!(s.contains("&lt;b&gt;&amp;&lt;/b&gt;"));
        assert!(!s.contains("<b>"));
    }

    #[test]
    fn xml_shape() {
        let s = export_to_string(&sample_grid(), None, &ExportOptions { format: ExportFormat::Xml, ..Default::default() }).unwrap();
        assert!(s.contains("<column name=\"name\">Ann</column>"));
        assert!(s.contains("<column name=\"name\" null=\"true\"/>"));
    }

    #[test]
    fn format_parse() {
        assert_eq!(ExportFormat::parse("CSV"), Some(ExportFormat::Csv));
        assert_eq!(ExportFormat::parse(" nope "), None);
    }
}
