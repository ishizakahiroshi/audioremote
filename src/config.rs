//! Persisted server settings. Lives in `%APPDATA%\audioremote\config.toml`
//! (or `$XDG_CONFIG_HOME/audioremote/config.toml` as a courtesy on non-Windows,
//! though the server itself only runs on Windows).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// CIDR allowlist (e.g. `"203.0.113.0/24"`, `"198.51.100.5/32"`). Empty = allow
    /// any source address (only the bearer token gates non-loopback clients).
    /// When non-empty, a **non-loopback** peer whose IP is outside every listed
    /// network is refused (403) before token checking. Loopback is always
    /// allowed so the host itself never locks out.
    pub allowed_networks: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // LAN-first: the whole point is to control the host from another
            // machine on the LAN (Win11 guest / phone / VM). Non-loopback
            // clients still need a bearer token; loopback is bypassed. Set to
            // "127.0.0.1" via `audioremote setup` to lock it down.
            bind: "0.0.0.0".to_string(),
            port: 17650,
            allowed_networks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Legacy single token from earlier v0.1 dev builds. Migrated into `tokens`
    /// on load and then dropped from the serialized form. Do not read this at
    /// runtime — use `tokens` / `token_matches`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Named bearer tokens. A request authenticates if its bearer matches any
    /// entry with `revoked = false`. Managed via `audioremote token add|revoke`.
    #[serde(default)]
    pub tokens: Vec<TokenEntry>,
    pub require_token: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            token: None,
            tokens: Vec::new(),
            require_token: true,
        }
    }
}

/// One named bearer token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEntry {
    /// Human label shown by `audioremote token list`. Not required to be unique;
    /// revoke-by-name acts on every non-revoked match.
    pub name: String,
    pub token: String,
    /// Unix seconds when issued (0 if hand-added / unknown).
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub revoked: bool,
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

    /// True if `presented` matches any non-revoked token. Compares against every
    /// candidate in constant time (no early return on first match / mismatch)
    /// so timing does not leak which token — or how much of one — was correct.
    pub fn token_matches(&self, presented: &str) -> bool {
        let mut ok = false;
        for t in &self.auth.tokens {
            if t.revoked {
                continue;
            }
            ok |= ct_eq(presented.as_bytes(), t.token.as_bytes());
        }
        ok
    }

    /// The token to embed in shareable LAN URLs: the first still-valid entry.
    pub fn share_token(&self) -> Option<&str> {
        self.auth
            .tokens
            .iter()
            .find(|t| !t.revoked)
            .map(|t| t.token.as_str())
    }
}

/// Constant-time byte-slice equality. Unequal lengths return false without a
/// byte-by-byte early exit; equal lengths accumulate all differences before
/// comparing. Tokens are fixed-length (`ar_live_` + 48 hex), so the length
/// branch is not a meaningful oracle in practice.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Load config from disk. Migrates a legacy single `token` into `tokens`,
/// ensures at least one active token exists, and persists **only when something
/// changed** (migration or token generation) so a valid hand-edited config
/// keeps its comments/formatting and a read-only config still boots. Returns
/// `(config, was_token_generated)`.
pub fn load_or_init(path: &Path) -> std::io::Result<(Config, bool)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let existed = path.exists();
    let mut config: Config = match fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("config parse: {e}"))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e),
    };

    let mut dirty = false;

    // Migrate a legacy single `token` into `tokens`, then forget the field
    // (it is `skip_serializing_if = None`, so taking it drops it from the file).
    if let Some(legacy) = config.auth.token.take() {
        dirty = true; // dropping the legacy field changes the serialized form
        let legacy = legacy.trim().to_string();
        if !legacy.is_empty() && !config.auth.tokens.iter().any(|t| t.token == legacy) {
            config.auth.tokens.push(TokenEntry {
                name: "default".to_string(),
                token: legacy,
                created_at: now_unix(),
                revoked: false,
            });
        }
    }

    // Guarantee at least one usable token (first run, or all revoked).
    let has_active = config.auth.tokens.iter().any(|t| !t.revoked);
    let generated = if !has_active {
        config.auth.tokens.push(TokenEntry {
            name: "default".to_string(),
            token: generate_token(),
            created_at: now_unix(),
            revoked: false,
        });
        dirty = true;
        true
    } else {
        false
    };

    if !existed || dirty {
        save(path, &config)?;
    }
    Ok((config, generated))
}

pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    let text = toml::to_string_pretty(config).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("config write: {e}"))
    })?;
    fs::write(path, text)
}

/// Append a new named token and return the generated token string.
pub fn add_named_token(config: &mut Config, name: &str) -> String {
    let token = generate_token();
    config.auth.tokens.push(TokenEntry {
        name: name.to_string(),
        token: token.clone(),
        created_at: now_unix(),
        revoked: false,
    });
    token
}

/// Revoke every non-revoked token matching `name_or_token` (by label or exact
/// token value). Returns how many were revoked.
pub fn revoke_token(config: &mut Config, name_or_token: &str) -> usize {
    let mut n = 0;
    for t in &mut config.auth.tokens {
        if !t.revoked && (t.name == name_or_token || t.token == name_or_token) {
            t.revoked = true;
            n += 1;
        }
    }
    n
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

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
