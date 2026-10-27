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
        result: std::result::Result<(Grid, u64), String>,
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
