//! T039 (US5) — `state.json` survives a round trip; a truncated / garbage file or a higher
//! `schema_version` yields empty first-run state with a log and no panic (FR-034).

use std::fs;
use std::path::PathBuf;

use pquploader::state::{LocalBackupState, SCHEMA_VERSION};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pqu_state_it_{}_{tag}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d.join("state.json")
}

#[test]
fn save_then_load_round_trips() {
    let p = scratch("round");
    let mut s = LocalBackupState::default();
    s.record_success(
        "pqdevice_db",
        &"d".repeat(64),
        "2026-09-08",
        "2026-09-08T09:00:00Z",
    );
    s.record_success(
        "settings/Device.xml",
        &"e".repeat(64),
        "2026-09-08",
        "2026-09-08T09:00:01Z",
    );
    s.last_heartbeat_utc = Some("2026-09-08T09:00:02Z".to_string());
    s.save_to(&p).unwrap();

    let back = LocalBackupState::load_from(&p);
    assert_eq!(back.schema_version, SCHEMA_VERSION);
    assert_eq!(back.files.len(), 2);
    assert_eq!(back.file("pqdevice_db").unwrap().last_backup_sha256, "d".repeat(64));
    assert_eq!(back.last_heartbeat_utc.as_deref(), Some("2026-09-08T09:00:02Z"));
    assert_eq!(back.last_backup_days().get("settings/Device.xml").map(String::as_str), Some("2026-09-08"));

    fs::remove_file(&p).ok();
}

#[test]
fn atomic_write_leaves_no_tmp_behind() {
    let p = scratch("atomic");
    LocalBackupState::default().save_to(&p).unwrap();
    assert!(p.exists());
    assert!(!p.with_extension("json.tmp").exists());
    fs::remove_file(&p).ok();
}

#[test]
fn garbage_file_loads_as_empty_state() {
    let p = scratch("garbage");
    fs::write(&p, b"\x00\x01not json at all]]]").unwrap();
    let s = LocalBackupState::load_from(&p);
    assert!(s.files.is_empty());
    assert_eq!(s.schema_version, SCHEMA_VERSION);
    fs::remove_file(&p).ok();
}

#[test]
fn truncated_json_loads_as_empty_state() {
    let p = scratch("trunc");
    fs::write(&p, br#"{"schema_version":1,"files":{"pqdevice_db":{"last_backup_sha256":"#).unwrap();
    let s = LocalBackupState::load_from(&p);
    assert!(s.files.is_empty());
    fs::remove_file(&p).ok();
}

#[test]
fn higher_schema_version_is_treated_as_empty() {
    let p = scratch("future");
    fs::write(
        &p,
        br#"{"schema_version":2,"files":{"k":{"last_backup_sha256":"aa","last_backup_utc_day":"2026-01-01","last_success_utc":"2026-01-01T00:00:00Z"}}}"#,
    )
    .unwrap();
    let s = LocalBackupState::load_from(&p);
    assert!(s.files.is_empty(), "a newer schema must not be half-parsed");
    fs::remove_file(&p).ok();
}
