//! `config.cfg`, the same INI file the rest of the house uses, plus
//! environment overrides for the values a container has to inject.
//!
//! There is no `.env` here on purpose. A dotfile that is read by whatever
//! process happens to source it, and that every tutorial tells you to commit
//! "just the example" of, is not where a database password belongs. The file
//! sits next to the binary, is written on first run with safe defaults, and
//! every key can be overridden by `AOP_SYNC_*` for the deployments that keep
//! their secrets in the orchestrator instead.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, anyhow};
use configparser::ini::Ini;

/// Written on first run so a self-hoster edits a file that already has the
/// right shape rather than guessing key names from an error message.
pub const DEFAULT_CONFIG: &str = r#"[server]
bind_addr = 127.0.0.1
bind_port = 8090

[database]
# Whole connection string, because that is what a container hands you.
url = postgres://aop:aop@localhost:5432/aop_sync

[idp]
# The Alterion identity provider. Point this at your own instance and every
# other endpoint follows from its discovery document.
issuer = https://auth.coraldune.cloud
# Only needed if your IdP requires client authentication on introspection.
client_id =
client_secret =
# How long an introspection answer is trusted. Short, because it is the
# window in which a revoked token still works.
token_cache_secs = 60

[cors]
# Comma separated. The desktop app sends no Origin, so this is for browsers.
allowed_origins = http://localhost:1420

[sync]
# How far the log may run past the newest stored snapshot before the server
# starts asking clients for a fresh one.
snapshot_every = 500

[logging]
level = info
"#;

/// Everything the server needs to start, already resolved.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    /// Base URL of the identity provider, with no trailing slash.
    pub issuer: String,
    pub idp_client_id: Option<String>,
    pub idp_client_secret: Option<String>,
    pub token_cache_ttl: Duration,
    pub allowed_origins: Vec<String>,
    pub snapshot_every: i64,
    pub log_level: String,
}

/// Where the file lives: beside the binary, matching the IdP, so a service
/// unit that sets no working directory still finds it.
pub fn config_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("config.cfg")
}

impl Config {
    pub fn create_if_missing(path: &Path) -> Result<()> {
        if !path.exists() {
            std::fs::write(path, DEFAULT_CONFIG)
                .map_err(|e| anyhow!("create {}: {e}", path.display()))?;
            log::info!("wrote a default {}", path.display());
        }
        Ok(())
    }

    /// Read the file if it is there, then let the environment win.
    ///
    /// A missing file is not an error: a container that passes everything in
    /// as environment variables should not have to ship one.
    pub fn load(path: &Path) -> Result<Self> {
        let mut ini = Ini::new();
        if path.exists() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow!("read {}: {e}", path.display()))?;
            ini.read(raw).map_err(|e| anyhow!("parse {}: {e}", path.display()))?;
        }
        Ok(Self::from_ini(&ini))
    }

    /// The resolution itself, kept apart from the filesystem so it can be
    /// exercised without one.
    pub fn from_ini(ini: &Ini) -> Self {
        let read = |section: &str, key: &str| -> Option<String> {
            let env_key = format!("AOP_SYNC_{}_{}", section.to_uppercase(), key.to_uppercase());
            env::var(&env_key)
                .ok()
                .or_else(|| ini.get(section, key))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };

        let bind_addr = read("server", "bind_addr").unwrap_or_else(|| "127.0.0.1".into());
        let bind_port = read("server", "bind_port")
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8090);

        Self {
            bind_address: format!("{bind_addr}:{bind_port}"),
            database_url: read("database", "url")
                .unwrap_or_else(|| "postgres://aop:aop@localhost:5432/aop_sync".into()),
            // Trailing slashes are stripped once, here, so nothing downstream
            // ever builds a URL with a double slash in it.
            issuer: read("idp", "issuer")
                .unwrap_or_else(|| "https://auth.coraldune.cloud".into())
                .trim_end_matches('/')
                .to_string(),
            idp_client_id: read("idp", "client_id"),
            idp_client_secret: read("idp", "client_secret"),
            token_cache_ttl: Duration::from_secs(
                read("idp", "token_cache_secs")
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60),
            ),
            allowed_origins: read("cors", "allowed_origins")
                .map(|v| {
                    v.split(',')
                        .map(|o| o.trim().to_string())
                        .filter(|o| !o.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            snapshot_every: read("sync", "snapshot_every")
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(500),
            log_level: read("logging", "level").unwrap_or_else(|| "info".into()),
        }
    }
}
