//! Saved connections ("accounts") and the connection registry.
//!
//! The original MySQL-Front 6 stored its connections in an XML document in
//! the user's application-data folder. This rewrite keeps the same idea but
//! uses a human-friendly JSON file:
//!
//! * Windows: `%APPDATA%\mysql-front\connections.json`
//! * Linux/macOS: `~/.config/mysql-front/connections.json`

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{MffError, Result};

/// TLS/SSL mode used when connecting to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SslMode {
    /// Do not use TLS at all.
    Disabled,
    /// Use TLS if the server supports it (the MySQL default behaviour).
    #[default]
    Preferred,
    /// Require TLS but do not verify the server certificate.
    Required,
    /// Require TLS and verify the server certificate against the system
    /// trust store.
    #[serde(rename = "verify_identity")]
    VerifyIdentity,
}

impl fmt::Display for SslMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SslMode::Disabled => "disabled",
            SslMode::Preferred => "preferred",
            SslMode::Required => "required",
            SslMode::VerifyIdentity => "verify_identity",
        };
        f.write_str(s)
    }
}

impl SslMode {
    /// Parse a mode from its textual form (case-insensitive).
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "none" | "off" => Ok(SslMode::Disabled),
            "preferred" | "" => Ok(SslMode::Preferred),
            "required" => Ok(SslMode::Required),
            "verify_identity" | "verify-ca" | "verify-full" => Ok(SslMode::VerifyIdentity),
            other => Err(MffError::Invalid(format!("unknown SSL mode: {other}"))),
        }
    }
}

/// One saved connection — the equivalent of a MySQL-Front "account".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Stable unique id (UUID v4).
    pub id: String,
    /// Display name, unique inside the registry.
    pub name: String,
    /// Host name or IP address.
    pub host: String,
    /// TCP port (default 3306).
    pub port: u16,
    /// Login user name.
    pub user: String,
    /// Login password.
    ///
    /// > **Note:** like the original MySQL-Front, the password is stored
    /// > unencrypted in the registry file. Keep the file private.
    #[serde(default)]
    pub password: String,
    /// Optional default database selected after connecting.
    #[serde(default)]
    pub database: Option<String>,
    /// Character set requested after connecting (default `utf8mb4`).
    #[serde(default = "default_charset")]
    pub charset: String,
    /// TLS mode.
    #[serde(default)]
    pub ssl_mode: SslMode,
    /// TCP connect timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Free-form user comments.
    #[serde(default)]
    pub comments: String,
}

fn default_charset() -> String {
    "utf8mb4".to_string()
}

fn default_timeout() -> u64 {
    10
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "New connection".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: String::new(),
            database: None,
            charset: default_charset(),
            ssl_mode: SslMode::default(),
            timeout_secs: default_timeout(),
            comments: String::new(),
        }
    }
}

impl Connection {
    /// Create a connection with a fresh id.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Create a fresh UUID for the connection id.
    pub fn new_id() -> String {
        new_id()
    }

    /// Build the `host:port` display string.
    pub fn host_port(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn new_id() -> String {
    // Small dependency-free UUID-v4-style id: timestamp + process entropy
    // mixed through an LCG. Uniqueness is all we need here.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut acc = nanos ^ pid.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut hex = String::with_capacity(32);
    for _ in 0..32 {
        acc = acc
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let nib = ((acc >> 60) & 0xF) as usize;
        hex.push("0123456789abcdef"[nib..].chars().next().unwrap());
    }
    let chars: Vec<char> = hex.chars().collect();
    let mut out = String::with_capacity(36);
    for (i, c) in chars.iter().enumerate() {
        if matches!(i, 8 | 12 | 16 | 20) {
            out.push('-');
        }
        let mut ch = *c;
        if i == 12 {
            ch = '4'; // version nibble
        }
        if i == 16 {
            ch = "89ab"[(nanos as usize) % 4..].chars().next().unwrap(); // variant nibble
        }
        out.push(ch);
    }
    out
}

/// The set of all saved connections, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub connections: Vec<Connection>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Default file location, as described in the module docs.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mysql-front").join("connections.json"))
    }

    /// Load a registry from `path`.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let reg: Registry = serde_json::from_str(&text)
            .map_err(|e| MffError::Registry(format!("{}: {e}", path.display())))?;
        Ok(reg)
    }

    /// Write the registry back to `path`, creating parent directories.
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Load from the default location; missing or broken files yield an
    /// empty registry instead of an error.
    pub fn load_default() -> Self {
        match Self::default_path() {
            Some(p) => Self::load(&p).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Save to the default location when one exists.
    pub fn save_default(&self) -> Result<()> {
        match Self::default_path() {
            Some(p) => self.save(&p),
            None => Err(MffError::Other("no default config directory".into())),
        }
    }

    /// Look a connection up by id or (case-insensitive) name.
    pub fn get(&self, id_or_name: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.id == id_or_name)
            .or_else(|| {
                self.connections
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(id_or_name))
            })
    }

    /// True when another connection already uses this name.
    pub fn name_taken(&self, name: &str, except_id: Option<&str>) -> bool {
        self.connections.iter().any(|c| {
            c.name.eq_ignore_ascii_case(name) && except_id.map(|id| c.id != id).unwrap_or(true)
        })
    }

    /// Insert or replace a connection (matched by id). Validates the name.
    pub fn upsert(&mut self, conn: Connection) -> Result<()> {
        if conn.name.trim().is_empty() {
            return Err(MffError::Invalid("connection name must not be empty".into()));
        }
        if self.name_taken(&conn.name, Some(&conn.id)) {
            return Err(MffError::DuplicateName(conn.name));
        }
        match self.connections.iter_mut().find(|c| c.id == conn.id) {
            Some(existing) => *existing = conn,
            None => self.connections.push(conn),
        }
        Ok(())
    }

    /// Remove a connection by id; returns true when something was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.connections.len();
        self.connections.retain(|c| c.id != id);
        self.connections.len() != before
    }

    /// Connections sorted by name.
    pub fn sorted(&self) -> Vec<&Connection> {
        let mut v: Vec<&Connection> = self.connections.iter().collect();
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_roundtrip() {
        let mut reg = Registry::new();
        let mut c = Connection::new("Local");
        c.host = "127.0.0.1".into();
        c.port = 3307;
        reg.upsert(c).unwrap();

        let json = serde_json::to_string(&reg).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.connections.len(), 1);
        assert_eq!(back.connections[0].port, 3307);
        assert_eq!(back.connections[0].ssl_mode, SslMode::Preferred);
    }

    #[test]
    fn duplicate_names_rejected() {
        let mut reg = Registry::new();
        let mut a = Connection::new("Prod");
        a.id = "a".into();
        reg.upsert(a).unwrap();
        assert!(matches!(
            reg.upsert(Connection::new("prod")),
            Err(MffError::DuplicateName(_))
        ));
        // Renaming the same entry to the same name is fine.
        let mut a = reg.connections[0].clone();
        a.name = "PROD".into();
        reg.upsert(a).unwrap();
    }

    #[test]
    fn get_by_name_is_case_insensitive() {
        let mut reg = Registry::new();
        reg.upsert(Connection::new("MyDB")).unwrap();
        assert!(reg.get("mydb").is_some());
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn ssl_mode_parse() {
        assert_eq!(SslMode::parse("REQUIRED").unwrap(), SslMode::Required);
        assert_eq!(SslMode::parse("disabled").unwrap(), SslMode::Disabled);
        assert!(SslMode::parse("bogus").is_err());
    }

    #[test]
    fn ids_are_unique_enough() {
        let a = Connection::new("a").id;
        let b = Connection::new("b").id;
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }
}
