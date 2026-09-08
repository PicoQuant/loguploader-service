//! T017 (US1) — a failing heartbeat is categorised, `run_once` still returns a
//! `CycleRecord`, the loop would continue, and only the latest heartbeat state is kept
//! (no backlog) — FR-006, SC-009.

use pquploader::config::Config;
use pquploader::cycle::{self};
use pquploader::error::FailureCategory;
use pquploader::identity::{Identity, OsInfo, Serial};
use pquploader::product::Product;
use pquploader::state::LocalBackupState;

fn identity() -> Identity {
    Identity {
        machine_id: "00000000-0000-0000-0000-000000000000".to_string(),
        serial: Serial::Unknown,
        os: OsInfo {
            version: "10.0.19045".to_string(),
            build: "19045".to_string(),
            arch: "x86_64".to_string(),
        },
    }
}

#[test]
fn failing_heartbeat_still_produces_a_cycle_record() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", Product::current().bucket());
    let _m = server
        .mock("POST", path.as_str())
        .with_status(500)
        .with_body("{}")
        .expect_at_least(1)
        .create();

    let cfg = Config::with_base_url(server.url());
    let mut state = LocalBackupState::default();

    let record = cycle::run_once(&cfg, &identity(), &mut state);

    assert!(!record.heartbeat.ok);
    assert_eq!(
        record.heartbeat.category.as_deref(),
        Some(FailureCategory::BackendError.as_str())
    );
    // the cycle itself is still "ok" — a backend outage is expected, not an internal error
    assert!(record.cycle_ok());
    // no heartbeat timestamp recorded on failure (latest-state-only, no backlog)
    assert!(state.last_heartbeat_utc.is_none());
    // a summary is still persisted for support
    assert!(state.last_cycle.is_some());
}

#[test]
fn recovered_heartbeat_updates_last_seen_with_no_backlog() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", Product::current().bucket());
    let _m = server
        .mock("POST", path.as_str())
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect_at_least(1)
        .create();

    let cfg = Config::with_base_url(server.url());
    let mut state = LocalBackupState::default();
    state.last_heartbeat_utc = Some("2026-09-07T00:00:00Z".to_string());

    let record = cycle::run_once(&cfg, &identity(), &mut state);
    assert!(record.heartbeat.ok);
    // exactly one last-seen value — the new one — kept
    let ts = state.last_heartbeat_utc.clone().unwrap();
    assert_ne!(ts, "2026-09-07T00:00:00Z");
}
