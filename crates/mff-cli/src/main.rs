//! `mff-cli` — command-line companion to the mysql-front-rs GUI.
//!
//! Subcommands:
//! * `connections`  — manage the saved connection registry
//! * `info`         — show server information
//! * `exec`         — run one SQL statement and print the result
//! * `script`       — run a .sql script file
//! * `export`       — dump a table or query to csv/json/sql/html/xml
//! * `import`       — load a csv/json/sql file into a table

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use mff_core::export::{export_to_string, ExportFormat, ExportOptions};
use mff_core::import::{import_csv, import_json, import_sql, CsvImportOptions};
use mff_core::session::{Cell, ExecOutcome, Grid, Session};
use mff_core::{Connection, Registry};

#[derive(Parser)]
#[command(
    name = "mff-cli",
    version = mff_core::VERSION,
    about = "MySQL-Front in Rust — command line client",
    after_help = "Connections are stored in the shared mysql-front-rs registry \
                  (the same file the GUI uses)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage saved connections.
    Connections {
        #[command(subcommand)]
        action: ConnAction,
    },
    /// Show server information.
    Info {
        /// Connection id or name from the registry.
        #[arg(long)]
        connection: String,
    },
    /// Run one SQL statement and print its result.
    Exec {
        #[arg(long)]
        connection: String,
        /// SQL text to execute (exactly one statement).
        #[arg(long)]
        sql: String,
        /// Output format for result sets.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
        /// Write output to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print rows even for huge results.
        #[arg(long)]
        no_limit: bool,
    },
    /// Run a .sql script file.
    Script {
        #[arg(long)]
        connection: String,
        /// Path to the .sql file.
        #[arg(long)]
        file: PathBuf,
        /// Continue after a failing statement.
        #[arg(long)]
        continue_on_error: bool,
    },
    /// Export a table or a query to a file.
    Export {
        #[arg(long)]
        connection: String,
        /// Database containing the table (or the default of the connection).
        #[arg(long)]
        database: Option<String>,
        /// Table to export.
        #[arg(long, group = "source")]
        table: Option<String>,
        /// Query to export (alternative to --table).
        #[arg(long, group = "source")]
        sql: Option<String>,
        /// Export format.
        #[arg(long, value_enum, default_value = "csv")]
        format: FormatArg,
        /// Output file (extension appended when missing).
        #[arg(long)]
        out: PathBuf,
        /// WHERE clause used with --table.
        #[arg(long, alias = "where")]
        where_clause: Option<String>,
        /// Row limit used with --table (0 = unlimited).
        #[arg(long, default_value_t = 0)]
        limit: u64,
        /// CSV delimiter.
        #[arg(long, default_value = ",")]
        delimiter: char,
        /// Batch size for SQL INSERT statements.
        #[arg(long, default_value_t = 1000)]
        batch_size: usize,
    },
    /// Import a file into a table.
    Import {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        database: Option<String>,
        /// Target table (csv/json only).
        #[arg(long)]
        table: Option<String>,
        /// Input file (type detected from the extension).
        #[arg(long)]
        file: PathBuf,
        /// CSV delimiter.
        #[arg(long, default_value = ",")]
        delimiter: char,
        /// The first CSV row holds column names.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        header: bool,
        /// Continue after a failing statement (sql import).
        #[arg(long)]
        continue_on_error: bool,
    },
}

#[derive(Subcommand)]
enum ConnAction {
    /// List saved connections.
    List,
    /// Add a connection.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "localhost")]
        host: String,
        #[arg(long, default_value_t = 3306)]
        port: u16,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long, default_value = "")]
        password: String,
        #[arg(long)]
        database: Option<String>,
        #[arg(long, default_value = "utf8mb4")]
        charset: String,
        #[arg(long, value_enum, default_value = "preferred")]
        ssl: SslArg,
    },
    /// Remove a connection by name.
    Remove { name: String },
}

#[derive(ValueEnum, Clone, Copy)]
enum OutputFormat {
    Table,
    Csv,
    Json,
}

#[derive(ValueEnum, Clone, Copy)]
enum FormatArg {
    Csv,
    Json,
    Sql,
    Html,
    Xml,
}

#[derive(ValueEnum, Clone, Copy)]
enum SslArg {
    Disabled,
    Preferred,
    Required,
    VerifyIdentity,
}

impl From<SslArg> for mff_core::SslMode {
    fn from(v: SslArg) -> Self {
        match v {
            SslArg::Disabled => mff_core::SslMode::Disabled,
            SslArg::Preferred => mff_core::SslMode::Preferred,
            SslArg::Required => mff_core::SslMode::Required,
            SslArg::VerifyIdentity => mff_core::SslMode::VerifyIdentity,
        }
    }
}

impl From<FormatArg> for ExportFormat {
    fn from(v: FormatArg) -> Self {
        match v {
            FormatArg::Csv => ExportFormat::Csv,
            FormatArg::Json => ExportFormat::Json,
            FormatArg::Sql => ExportFormat::Sql,
            FormatArg::Html => ExportFormat::Html,
            FormatArg::Xml => ExportFormat::Xml,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Connections { action } => connections(action),
        Command::Info { connection } => {
            let session = open(&connection)?;
            let info = session.info();
            println!("server version : {}", info.version);
            println!("host           : {}:{}", info.host, info.port);
            println!("user           : {}", info.user);
            let dbs = session.user_databases()?;
            println!("databases ({})  : {}", dbs.len(), dbs.join(", "));
            Ok(())
        }
        Command::Exec {
            connection,
            sql,
            format,
            out,
            no_limit,
        } => {
            let session = open(&connection)?;
            let started = Instant::now();
            match session.run_statement(&sql)? {
                ExecOutcome::Grid(grid) => {
                    let text = match format {
                        OutputFormat::Table => render_table(&grid, if no_limit { usize::MAX } else { 100 }),
                        OutputFormat::Csv => export_to_string(
                            &grid,
                            None,
                            &ExportOptions::default(),
                        )?,
                        OutputFormat::Json => export_to_string(
                            &grid,
                            None,
                            &ExportOptions { format: ExportFormat::Json, ..Default::default() },
                        )?,
                    };
                    write_out(out.as_ref(), &text)?;
                    eprintln!("{} row(s) in {:?}", grid.row_count(), started.elapsed());
                }
                ExecOutcome::Affected { affected, last_insert_id, info } => {
                    println!("OK, {affected} row(s) affected, last_insert_id={last_insert_id}");
                    if let Some(info) = info {
                        println!("info: {info}");
                    }
                    eprintln!("done in {:?}", started.elapsed());
                }
            }
            Ok(())
        }
        Command::Script {
            connection,
            file,
            continue_on_error,
        } => {
            let session = open(&connection)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let statements = mff_core::sqlsplit::split_statements(&text);
            let started = Instant::now();
            let results = session.run_script(&statements, continue_on_error);
            let mut failed = 0;
            for (i, r) in results.iter().enumerate() {
                match r {
                    Ok(o) => println!("[{}/{}] {}", i + 1, results.len(), o.summary()),
                    Err(e) => {
                        failed += 1;
                        eprintln!("[{}/{}] FAILED: {e}", i + 1, results.len());
                    }
                }
            }
            eprintln!(
                "{} statement(s) in {:?} ({failed} failed)",
                results.len(),
                started.elapsed()
            );
            if failed > 0 && !continue_on_error {
                bail!("{failed} statement(s) failed");
            }
            Ok(())
        }
        Command::Export {
            connection,
            database,
            table,
            sql,
            format,
            out,
            where_clause,
            limit,
            delimiter,
            batch_size,
        } => {
            let session = open(&connection)?;
            let db = match &database {
                Some(d) => d.clone(),
                None => bail!("--database is required"),
            };
            let started = Instant::now();

            let (grid, table_meta) = match (&table, &sql) {
                (Some(t), None) => {
                    let meta = session.table_schema(&db, t).ok();
                    let grid = session.table_rows(
                        &db,
                        t,
                        where_clause.as_deref(),
                        None,
                        0,
                        if limit > 0 { limit } else { u64::MAX / 4 },
                    )?;
                    (grid, meta)
                }
                (None, Some(q)) => (session.query(q)?, None),
                _ => bail!("exactly one of --table or --sql is required"),
            };

            let opts = ExportOptions {
                format: format.into(),
                delimiter,
                batch_size,
                ..Default::default()
            };
            let text = export_to_string(&grid, table_meta.as_ref(), &opts)?;
            let mut path = out;
            if path.extension().is_none() {
                path.set_extension(opts.format.extension());
            }
            std::fs::write(&path, text)
                .with_context(|| format!("cannot write {}", path.display()))?;
            eprintln!(
                "exported {} row(s) to {} in {:?}",
                grid.row_count(),
                path.display(),
                started.elapsed()
            );
            Ok(())
        }
        Command::Import {
            connection,
            database,
            table,
            file,
            delimiter,
            header,
            continue_on_error,
        } => {
            let session = open(&connection)?;
            let db = database.context("--database is required")?;
            let started = Instant::now();
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            match ext.as_str() {
                "csv" | "txt" | "tsv" => {
                    let table = table.context("--table is required for csv imports")?;
                    let delim = if ext == "tsv" { '\t' } else { delimiter };
                    let opts = CsvImportOptions {
                        delimiter: delim,
                        has_header: header,
                        ..Default::default()
                    };
                    let n = import_csv(&session, &db, &table, &file, &opts, None)?;
                    eprintln!("imported {n} row(s) in {:?}", started.elapsed());
                }
                "json" => {
                    let table = table.context("--table is required for json imports")?;
                    let n = import_json(&session, &db, &table, &file, None)?;
                    eprintln!("imported {n} row(s) in {:?}", started.elapsed());
                }
                "sql" => {
                    // Select the target database before running the script
                    // (script files usually contain unqualified names).
                    session.run_statement(&format!(
                        "USE {}",
                        mff_core::sqlgen::quote_ident(&db)
                    ))?;
                    let n = import_sql(&session, &file, continue_on_error)?;
                    eprintln!("executed {n} statement(s) in {:?}", started.elapsed());
                }
                other => bail!("unsupported import type: .{other} (use csv/json/sql)"),
            }
            Ok(())
        }
    }
}

fn connections(action: ConnAction) -> Result<()> {
    let mut reg = Registry::load_default();
    match action {
        ConnAction::List => {
            if reg.connections.is_empty() {
                println!("(no saved connections — use `connections add`)");
                return Ok(());
            }
            println!("{:<22} {:<28} {:<16} {}", "NAME", "HOST:PORT", "USER", "DB");
            for c in reg.sorted() {
                println!(
                    "{:<22} {:<28} {:<16} {}",
                    c.name,
                    c.host_port(),
                    c.user,
                    c.database.as_deref().unwrap_or("-")
                );
            }
        }
        ConnAction::Add {
            name,
            host,
            port,
            user,
            password,
            database,
            charset,
            ssl,
        } => {
            let mut c = Connection::new(name);
            c.host = host;
            c.port = port;
            c.user = user;
            c.password = password;
            c.database = database;
            c.charset = charset;
            c.ssl_mode = ssl.into();
            reg.upsert(c)?;
            reg.save_default()?;
            println!("saved.");
        }
        ConnAction::Remove { name } => {
            let c = reg
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("connection '{name}' not found"))?;
            let id = c.id.clone();
            if reg.remove(&id) {
                reg.save_default()?;
                println!("removed '{name}'.");
            }
        }
    }
    Ok(())
}

fn open(name: &str) -> Result<Session> {
    let reg = Registry::load_default();
    let conn = reg
        .get(name)
        .cloned()
        .with_context(|| format!("connection '{name}' not found"))?;
    let started = Instant::now();
    let session = Session::connect(&conn).context("connect failed")?;
    eprintln!("connected to {} in {:?}", conn.host_port(), started.elapsed());
    Ok(session)
}

fn write_out(out: Option<&PathBuf>, text: &str) -> Result<()> {
    match out {
        Some(p) => {
            std::fs::write(p, text)?;
            eprintln!("written to {}", p.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// Minimal aligned text rendering of a grid (like a MySQL client).
fn render_table(grid: &Grid, max_rows: usize) -> String {
    const PADDING: usize = 2;
    let ncols = grid.columns.len();
    if ncols == 0 {
        return "(no result set)\n".to_string();
    }
    let shown = grid.rows.len().min(max_rows);
    let mut cells: Vec<Vec<String>> = Vec::with_capacity(shown + 1);
    cells.push(grid.columns.iter().map(|c| c.name.clone()).collect());
    for row in &grid.rows[..shown] {
        cells.push(row.iter().map(|c| cell_display(c)).collect());
    }
    let mut widths = vec![0usize; ncols];
    for row in &cells {
        for (i, cell) in row.iter().take(ncols).enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    let sep = |out: &mut String| {
        out.push('+');
        for w in &widths {
            out.push_str(&"-".repeat(w + PADDING));
            out.push('+');
        }
        out.push('\n');
    };
    sep(&mut out);
    for (ri, row) in cells.iter().enumerate() {
        out.push('|');
        for (i, cell) in row.iter().take(ncols).enumerate() {
            let pad = widths[i] - cell.chars().count();
            out.push(' ');
            out.push_str(cell);
            out.push_str(&" ".repeat(pad + PADDING - 1));
            out.push('|');
        }
        out.push('\n');
        if ri == 0 {
            sep(&mut out);
        }
    }
    if grid.rows.len() > shown {
        out.push_str(&format!(
            "… {} more row(s) not shown (use --no-limit)\n",
            grid.rows.len() - shown
        ));
    }
    out
}

fn cell_display(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".to_string(),
        Cell::Text(s) => s.replace('\n', "\\n"),
        Cell::Bytes(_) => c.display("NULL"),
    }
}
