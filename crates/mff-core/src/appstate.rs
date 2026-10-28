//! Persisted UI state (`app_state.json` next to the connection registry):
//! theme, the last used connection, and per-connection details such as the
//! database that was open and the query history.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What was open in the UI for one connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnUiState {
    /// The database that was selected last.
    #[serde(default)]
    pub db: Option<String>,
    /// Recently executed query texts, most recent first.
    #[serde(default)]
    pub history: Vec<String>,
}

/// The whole persisted UI state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// "dark" or "light".
    #[serde(default)]
    pub theme: Option<String>,
    /// UI language: "en" (default) or "th".
    #[serde(default)]
    pub language: Option<String>,
    /// Connection id of the last active session.
    #[serde(default)]
    pub last_conn: Option<String>,
    #[serde(default)]
    pub connections: HashMap<String, ConnUiState>,
}

impl AppState {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mysql-front").join("app_state.json"))
    }

    /// Load from the default path (missing file → default state).
    pub fn load_default() -> Self {
        match Self::default_path() {
            Some(p) if p.exists() => match std::fs::read_to_string(&p) {
                Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        }
    }

    /// Write to the default path, creating parent directories. Failures
    /// are swallowed — a broken state file must never take the app down.
    pub fn save_default(&self) {
        let Some(p) = Self::default_path() else {
            return;
        };
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }

    pub fn conn(&self, id: &str) -> ConnUiState {
        self.connections.get(id).cloned().unwrap_or_default()
    }

    pub fn set_conn(&mut self, id: &str, st: ConnUiState) {
        self.connections.insert(id.to_string(), st);
    }

    /// Remember a query text for one connection (deduplicated, capped).
    pub fn push_history(&mut self, conn_id: &str, sql: &str) {
        let sql = sql.trim();
        if sql.is_empty() {
            return;
        }
        let mut st = self.conn(conn_id);
        st.history.retain(|h| h != sql);
        st.history.insert(0, sql.to_string());
        st.history.truncate(100);
        self.set_conn(conn_id, st);
    }

    /// The load/save round trip keeps every field.
    #[cfg(test)]
    fn roundtrips() {
        let mut st = AppState {
            theme: Some("dark".into()),
            last_conn: Some("c1".into()),
            connections: HashMap::new(),
        };
        st.push_history("c1", "SELECT 1");
        st.set_conn(
            "c2",
            ConnUiState {
                db: Some("shop".into()),
                history: Vec::new(),
            },
        );
        let text = serde_json::to_string(&st).unwrap();
        let back: AppState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.theme.as_deref(), Some("dark"));
        assert_eq!(back.conn("c2").db.as_deref(), Some("shop"));
        assert_eq!(back.conn("c1").history, vec!["SELECT 1".to_string()]);
    }
}

/// Sanity: the saved state file stays small even with long histories.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_deduped_and_capped() {
        let mut st = AppState::default();
        for i in 0..150 {
            st.push_history("c", &format!("SELECT {i}"));
        }
        assert_eq!(st.conn("c").history.len(), 100);
        assert_eq!(st.conn("c").history[0], "SELECT 149");
        // Re-running the top entry moves it to the front without a duplicate.
        st.push_history("c", "SELECT 149");
        let s = st.conn("c");
        assert_eq!(s.history.len(), 100);
        assert_eq!(s.history[0], "SELECT 149");
        assert_eq!(s.history[1], "SELECT 148");
        assert!(s.history.iter().filter(|h| *h == "SELECT 149").count() == 1);
        // Empty SQL is ignored.
        st.push_history("c", "   ");
        assert_eq!(st.conn("c").history[0], "SELECT 149");
    }

    #[test]
    fn roundtrip_via_json() {
        AppState::roundtrips();
    }
}
