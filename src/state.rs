//! Persisted local backup state — `contracts/local-state.schema.json` (FR-034).
//!
//! Atomic write (`state.json.tmp` → fsync → rename) under `<data_dir>\v2agent\`, OUTSIDE
//! the install dir so a v2 self-update preserves it. An unreadable file or a `schema_version`
//! we don't recognise is treated as empty first-run state — never a panic (FR-016, D9).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::product::Product;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBackupState {
    /// sha256 of the file contents at the last successful backup (FR-010).
    pub last_backup_sha256: String,
    /// UTC day (`YYYY-MM-DD`) of the last successful backup — the once-per-day gate (FR-011).
    pub last_backup_utc_day: String,
    pub last_success_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleCounts {
    #[serde(default)]
    pub sent: u32,
    #[serde(default)]
    pub unchanged: u32,
    #[serde(default)]
    pub blocked: u32,
    #[serde(default)]
    pub retry_later: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleSummary {
    pub started_utc: String,
    pub ok: bool,
    pub heartbeat_ok: bool,
    pub counts: CycleCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalBackupState {
    pub schema_version: u32,
    #[serde(default)]
    pub files: BTreeMap<String, FileBackupState>,
    #[serde(default)]
    pub last_heartbeat_utc: Option<String>,
    #[serde(default)]
    pub last_cycle: Option<CycleSummary>,
}

impl Default for LocalBackupState {
    fn default() -> Self {
        LocalBackupState {
            schema_version: SCHEMA_VERSION,
            files: BTreeMap::new(),
            last_heartbeat_utc: None,
            last_cycle: None,
        }
    }
}

impl LocalBackupState {
    pub fn path() -> PathBuf {
        Product::current().agent_dir().join("state.json")
    }

    /// Load persisted state, or an empty state if the file is missing / unreadable / a
    /// schema we don't understand. Logs the reason; never fails.
    pub fn load() -> LocalBackupState {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &std::path::Path) -> LocalBackupState {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return LocalBackupState::default();
            }
            Err(e) => {
                log::warn!("state.json unreadable ({e}); starting from empty state");
                return LocalBackupState::default();
            }
        };
        match serde_json::from_str::<LocalBackupState>(&text) {
            Ok(s) if s.schema_version == SCHEMA_VERSION => s,
            Ok(s) => {
                log::warn!(
                    "state.json schema_version {} != {SCHEMA_VERSION}; treating as empty",
                    s.schema_version
                );
                LocalBackupState::default()
            }
            Err(e) => {
                log::warn!("state.json is corrupt ({e}); starting from empty state");
                LocalBackupState::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    /// Atomic write: `<path>.tmp` in the same dir → `sync_all` → `rename` over `<path>`.
    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn file(&self, key: &str) -> Option<&FileBackupState> {
        self.files.get(key)
    }

    /// Record a confirmed successful backup for `key` (FR-012 — only on a 200).
    pub fn record_success(&mut self, key: &str, sha256: &str, utc_day: &str, utc_ts: &str) {
        self.files.insert(
            key.to_string(),
            FileBackupState {
                last_backup_sha256: sha256.to_string(),
                last_backup_utc_day: utc_day.to_string(),
                last_success_utc: utc_ts.to_string(),
            },
        );
    }

    /// `file_key -> YYYY-MM-DD` freshness hint for the heartbeat payload.
    pub fn last_backup_days(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|(k, v)| (k.clone(), v.last_backup_utc_day.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pqu_state_{}_{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("state.json")
    }

    #[test]
    fn round_trips() {
        let p = tmp("round");
        let mut s = LocalBackupState::default();
        s.record_success(
            "pqdevice_conf",
            &"a".repeat(64),
            "2026-09-08",
            "2026-09-08T08:00:00Z",
        );
        s.last_heartbeat_utc = Some("2026-09-08T08:00:01Z".to_string());
        s.save_to(&p).unwrap();

        let back = LocalBackupState::load_from(&p);
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert_eq!(
            back.file("pqdevice_conf").unwrap().last_backup_utc_day,
            "2026-09-08"
        );
        fs::remove_file(&p).ok();
    }

    #[test]
    fn missing_file_is_empty_state() {
        let p = tmp("missing");
        fs::remove_file(&p).ok();
        let s = LocalBackupState::load_from(&p);
        assert!(s.files.is_empty());
        assert_eq!(s.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn garbage_is_empty_state() {
        let p = tmp("garbage");
        fs::write(&p, b"{ this is not json").unwrap();
        let s = LocalBackupState::load_from(&p);
        assert!(s.files.is_empty());
        fs::remove_file(&p).ok();
    }

    #[test]
    fn higher_schema_version_is_empty_state() {
        let p = tmp("future");
        fs::write(&p, br#"{"schema_version": 999, "files": {"x": {"last_backup_sha256":"z","last_backup_utc_day":"2026-01-01","last_success_utc":"2026-01-01T00:00:00Z"}}}"#).unwrap();
        let s = LocalBackupState::load_from(&p);
        assert!(s.files.is_empty());
        fs::remove_file(&p).ok();
    }
}
