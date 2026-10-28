//! Background job plumbing: every long-running operation (connect, load
//! data, run a script, …) runs on its own thread and reports back through
//! one channel that the UI drains each frame.

use std::sync::mpsc::{channel, Receiver, Sender};

use mff_core::model::{DatabaseObjects, Table};
use mff_core::session::{Grid, Session};

/// One completed section of a query execution.
#[derive(Debug, Clone)]
pub enum QuerySection {
    Grid(Grid),
    Affected {
        affected: u64,
        last_insert_id: u64,
        info: Option<String>,
    },
    Error(String),
}

/// Messages sent from worker threads to the UI thread.
pub enum UiMsg {
    Connected {
        conn_id: String,
        result: std::result::Result<Session, String>,
    },
    DatabasesLoaded {
        conn_id: String,
        result: std::result::Result<Vec<String>, String>,
    },
    ObjectsLoaded {
        conn_id: String,
        db: String,
        result: std::result::Result<DatabaseObjects, String>,
    },
    DataLoaded {
        conn_id: String,
        table: String,
        result: std::result::Result<(Grid, u64, Vec<String>), String>,
    },
    SchemaLoaded {
        conn_id: String,
        table: String,
        result: std::result::Result<Table, String>,
    },
    QueryDone {
        conn_id: String,
        tab: usize,
        sections: Vec<QuerySection>,
        elapsed_ms: u128,
    },
    VariablesLoaded {
        conn_id: String,
        result: std::result::Result<Vec<(String, String)>, String>,
    },
    Renamed {
        conn_id: String,
        is_view: bool,
        old: String,
        new: String,
        result: std::result::Result<(), String>,
    },
    Created {
        conn_id: String,
        name: String,
        result: std::result::Result<(), String>,
    },
    DbRenamed {
        conn_id: String,
        new_db: String,
        result: std::result::Result<(), String>,
    },
    TransferDone {
        conn_id: String,
        text: String,
        ok: bool,
        target_same_server: bool,
        target_db: String,
    },
    /// An UPDATE/DELETE issued from the data grid finished.
    DataEdited {
        conn_id: String,
        result: std::result::Result<(), String>,
    },
    /// An INSERT from the "new row" dialog finished.
    RowInserted {
        conn_id: String,
        result: std::result::Result<(), String>,
    },
    /// A DROP TABLE/VIEW/… from the object browser finished.
    Dropped {
        conn_id: String,
        obj: String,
        result: std::result::Result<(), String>,
    },
    /// The data-view export (background) finished.
    ExportDone {
        conn_id: String,
        result: std::result::Result<usize, String>,
    },
    ProcessListLoaded {
        conn_id: String,
        result: std::result::Result<Grid, String>,
    },
    Killed {
        conn_id: String,
        id: u64,
        result: std::result::Result<(), String>,
    },
    UserListLoaded {
        conn_id: String,
        result: std::result::Result<Grid, String>,
    },
    /// CREATE USER / DROP USER finished (message describes the action).
    UserDone {
        conn_id: String,
        result: std::result::Result<String, String>,
    },
    /// The SQL completion index for one database finished loading.
    CompletionLoaded {
        conn_id: String,
        db: String,
        result: std::result::Result<mff_core::suggest::CompletionIndex, String>,
    },
    Status(String),
}

/// The UI end of the job channel.
pub struct Jobs {
    pub tx: Sender<UiMsg>,
    rx: Receiver<UiMsg>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Jobs {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }

    /// Get a sender to hand to worker threads.
    pub fn sender(&self) -> Sender<UiMsg> {
        self.tx.clone()
    }

    /// Drain all pending messages.
    pub fn drain(&mut self) -> Vec<UiMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            out.push(msg);
        }
        out
    }
}

#[allow(dead_code)]
pub type JobResult<T> = std::result::Result<T, String>;
