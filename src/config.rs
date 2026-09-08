//! Compile-time build identity + the single runtime-config resolution path (Constitution II).
//!
//! Resolution order for every tunable: **compiled default → `config.toml` next to the exe
//! → hard-coded default**. Product, channel and the fleet token are compile-time only and
//! are never read from `config.toml` (FR-002e).

use std::path::PathBuf;

use serde::Deserialize;

use crate::product::Product;

// ---- compile-time constants (from build.rs) ----------------------------------

pub const PRODUCT: &str = env!("PQ_PRODUCT");
pub const CHANNEL: &str = env!("PQ_CHANNEL");
pub const VERSION: &str = env!("PQ_VERSION");
/// The one fleet token this build carries. Empty only in a debug build with no secret.
pub const FLEET_TOKEN: &str = env!("PQ_FLEET_TOKEN");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn current() -> Channel {
        match CHANNEL {
            "beta" => Channel::Beta,
            "stable" => Channel::Stable,
            other => panic!("build emitted an invalid PQ_CHANNEL: {other}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }
}

// ---- runtime config ---------------------------------------------------------

const DEFAULT_CYCLE_INTERVAL_SECS: u64 = 1800;
const DEFAULT_API_BASE_URL: &str = "https://api.picoquant.com";
const DEFAULT_BACKUP_MAX_BYTES: u64 = 52_428_800; // 50 MiB
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct Config {
    pub product: Product,
    pub channel: Channel,
    pub cycle_interval_secs: u64,
    pub api_base_url: String,
    pub backup_max_bytes: u64,
    pub http_timeout_secs: u64,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    cycle_interval_secs: Option<u64>,
    api_base_url: Option<String>,
    backup_max_bytes: Option<u64>,
    http_timeout_secs: Option<u64>,
}

impl Config {
    /// Load config, reading `config.toml` next to the running executable if present.
    /// An unreadable / malformed file logs a warning and falls back to defaults — never panics.
    pub fn load() -> Config {
        let path = config_toml_path();
        let file = match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => match toml::from_str::<FileConfig>(&text) {
                Ok(fc) => fc,
                Err(e) => {
                    log::warn!("config.toml is invalid ({e}); using defaults");
                    FileConfig::default()
                }
            },
            None => FileConfig::default(),
        };
        Config::from_file(file)
    }

    fn from_file(file: FileConfig) -> Config {
        Config {
            product: Product::current(),
            channel: Channel::current(),
            cycle_interval_secs: file
                .cycle_interval_secs
                .filter(|v| *v > 0)
                .unwrap_or(DEFAULT_CYCLE_INTERVAL_SECS),
            api_base_url: file
                .api_base_url
                .map(|s| s.trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_string()),
            backup_max_bytes: file
                .backup_max_bytes
                .filter(|v| *v > 0)
                .unwrap_or(DEFAULT_BACKUP_MAX_BYTES),
            http_timeout_secs: file
                .http_timeout_secs
                .filter(|v| *v > 0)
                .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS),
        }
    }

    /// `{api_base_url}/api/v2/products/{bucket}/{leaf}` (FR-020).
    pub fn endpoint(&self, leaf: &str) -> String {
        format!(
            "{}/api/v2/products/{}/{}",
            self.api_base_url,
            self.product.bucket(),
            leaf
        )
    }

    pub fn has_fleet_token(&self) -> bool {
        !FLEET_TOKEN.is_empty()
    }

    /// Defaults with the backend URL overridden — used by the integration tests to point
    /// the agent at a local mock server.
    pub fn with_base_url(api_base_url: impl Into<String>) -> Config {
        let mut c = Config::from_file(FileConfig::default());
        c.api_base_url = api_base_url.into().trim_end_matches('/').to_string();
        c.http_timeout_secs = 3;
        c
    }
}

fn config_toml_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_file_absent() {
        let c = Config::from_file(FileConfig::default());
        assert_eq!(c.cycle_interval_secs, 1800);
        assert_eq!(c.api_base_url, "https://api.picoquant.com");
        assert_eq!(c.backup_max_bytes, 52_428_800);
        assert_eq!(c.http_timeout_secs, 60);
    }

    #[test]
    fn file_overrides_win_and_trailing_slash_trimmed() {
        let fc = FileConfig {
            cycle_interval_secs: Some(60),
            api_base_url: Some("https://staging.example.com/".to_string()),
            backup_max_bytes: Some(10),
            http_timeout_secs: Some(5),
        };
        let c = Config::from_file(fc);
        assert_eq!(c.cycle_interval_secs, 60);
        assert_eq!(c.api_base_url, "https://staging.example.com");
        assert_eq!(c.backup_max_bytes, 10);
        assert_eq!(c.http_timeout_secs, 5);
    }

    #[test]
    fn zero_or_empty_overrides_are_ignored() {
        let fc = FileConfig {
            cycle_interval_secs: Some(0),
            api_base_url: Some(String::new()),
            backup_max_bytes: Some(0),
            http_timeout_secs: Some(0),
        };
        let c = Config::from_file(fc);
        assert_eq!(c.cycle_interval_secs, 1800);
        assert_eq!(c.api_base_url, "https://api.picoquant.com");
        assert_eq!(c.backup_max_bytes, 52_428_800);
    }

    #[test]
    fn endpoint_shape() {
        let c = Config::from_file(FileConfig::default());
        let ep = c.endpoint("telemetry");
        assert!(ep.starts_with("https://api.picoquant.com/api/v2/products/"));
        assert!(ep.ends_with("/telemetry"));
    }
}
