//! Persisted server settings. Lives in `%APPDATA%\audioremote\config.toml`
//! (or `$XDG_CONFIG_HOME/audioremote/config.toml` as a courtesy on non-Windows,
//! though the server itself only runs on Windows).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub audio: AudioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
    /// Reserved for a future allowlist check. Currently informational only.
    pub allowed_networks: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // LAN-first: the whole point is to control the host from another
            // machine on the LAN (Win11 guest / phone / VM). Token auth still
            // protects non-loopback clients. Set to "127.0.0.1" via
            // `audioremote setup` if you want to lock it down.
            bind: "0.0.0.0".to_string(),
            port: 17650,
            allowed_networks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// If empty at load time, a fresh token is generated and the config is
    /// rewritten. Displayed once on first-run console output.
    pub token: String,
    pub require_token: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            require_token: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// v0.1 always switches the three roles together; kept for future opt-out.
    pub unify_roles: bool,
    pub device_sort: SortPolicy,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            unify_roles: true,
            device_sort: SortPolicy::State,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortPolicy {
    State,
    Name,
    Recent,
}

impl Config {
    /// True when the bind address is anything other than a loopback literal.
    /// Used to power the guest UI's "LAN exposed" badge.
    pub fn lan_exposed(&self) -> bool {
        let b = self.server.bind.trim();
        !(b == "127.0.0.1" || b == "::1" || b.eq_ignore_ascii_case("localhost"))
    }
}

/// Load config from disk. If missing, write out defaults. If the token is
/// empty (either newly created or blanked out by hand), generate one and
/// persist. Returns `(config, was_token_generated)`.
pub fn load_or_init(path: &Path) -> std::io::Result<(Config, bool)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut config: Config = match fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("config parse: {e}"))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e),
    };

    let generated = if config.auth.token.is_empty() {
        config.auth.token = generate_token();
        true
    } else {
        false
    };

    save(path, &config)?;
    Ok((config, generated))
}

pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    let text = toml::to_string_pretty(config).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("config write: {e}"))
    })?;
    fs::write(path, text)
}

/// `%APPDATA%\audioremote\config.toml` on Windows.
pub fn default_config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub fn data_dir() -> PathBuf {
    // Prefer APPDATA (Windows). Fall back to home for other OSes so dev on
    // WSL / macOS at least does not panic.
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("audioremote");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".audioremote");
    }
    PathBuf::from(".audioremote")
}

/// 24 random bytes hex-encoded, prefixed with `ar_live_`.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).expect("OS RNG unavailable");
    let mut hex = String::with_capacity(48);
    for b in bytes {
        hex.push(nibble(b >> 4));
        hex.push(nibble(b & 0x0f));
    }
    format!("ar_live_{hex}")
}

fn nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => '0',
    }
}
