//! Device usage history for `device_sort = "recent"`. Kept in a separate TOML
//! file from `config.toml` so hand-edits to config don't clobber timestamps,
//! and vice versa.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    /// device_id -> unix seconds when last set as default.
    #[serde(default)]
    pub last_used: HashMap<String, u64>,
}

impl History {
    pub fn touch(&mut self, device_id: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_used.insert(device_id.to_string(), now);
    }

    pub fn last_used_at(&self, device_id: &str) -> Option<u64> {
        self.last_used.get(device_id).copied()
    }
}

pub fn load(path: &Path) -> std::io::Result<History> {
    match fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("history parse: {e}"),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(History::default()),
        Err(e) => Err(e),
    }
}

pub fn save(path: &Path, history: &History) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(history).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("history write: {e}"),
        )
    })?;
    fs::write(path, text)
}

pub fn default_history_path() -> PathBuf {
    crate::config::data_dir().join("history.toml")
}
