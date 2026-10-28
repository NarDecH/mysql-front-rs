//! # mff-core
//!
//! Core library for **mysql-front-rs** — a Rust rewrite of the MySQL-Front 6
//! GUI client (originally written in Delphi/Pascal).
//!
//! The crate contains the non-UI half of the application:
//!
//! * [`account`] — saved connections ("accounts") and the connection registry
//! * [`model`]   — the schema domain model (databases, tables, columns, keys, …)
//! * [`session`] — a live session against a MySQL server built on the
//!   [`mysql`](https://crates.io/crates/mysql) crate
//! * [`sqlgen`]  — identifier/value quoting, `CREATE TABLE` DDL and batched
//!   `INSERT` generation
//! * [`sqlsplit`] — SQL script splitting (strings, comments, `DELIMITER`)
//! * [`export`]  — result grids to CSV / JSON / SQL / HTML / XML
//! * [`import`]  — CSV / JSON / SQL files into a table
//!
//! Both the desktop GUI (`mff-gui`) and the command line tool (`mff-cli`)
//! are thin layers on top of this library.

pub mod account;
pub mod error;
pub mod export;
pub mod import;
pub mod model;
pub mod session;
pub mod sqlgen;
pub mod sqlsplit;
pub mod transfer;

pub use account::{Connection, Registry, SslMode};
pub use error::{MffError, Result};
pub use export::ExportFormat;
pub use transfer::{TransferItem, TransferOptions, TransferSummary};
pub use model::{Column, DatabaseObjects, ForeignKey, IndexKey, KeyKind, Table, TableBrief};
pub use session::{Cell, ColMeta, ExecOutcome, Grid, ServerInfo, Session};

/// Crate version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
