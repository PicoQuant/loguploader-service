//! T022 (US2) — the once-per-UTC-day gate: a straddling-midnight cycle, a failed send
//! NOT advancing `last_backup_utc_day`, and a `deduplicated: true` 200 that DOES advance it
//! (FR-011, FR-012, SC-003, SC-004).

mod common;

use pquploader::backup::{decide_action, Decision};
use pquploader::config::Config;
use pquploader::cycle::{self, utc_day};
use pquploader::identity::{Identity, OsInfo, Serial};
use pquploader::product::Product;
use pquploader::state::{FileBackupState, LocalBackupState};
use pquploader::watchset::sha256_hex;
use time::macros::datetime;

fn prev(sha: &str, day: &str) -> FileBackupState {
    FileBackupState {
        last_backup_sha256: sha.to_string(),
        last_backup_utc_day: day.to_string(),
        last_success_utc: format!("{day}T12:00:00Z"),
    }
}

#[test]
fn changed_file_already_backed_up_today_is_gated() {
    let old = sha256_hex(b"v1");
    let new = sha256_hex(b"v2");
    let state = prev(&old, "2026-09-08");
    assert_eq!(
        decide_action(Some(&state), &new, "2026-09-08"),
        Decision::SkippedToday
    );
}

#[test]
fn cycle_straddling_midnight_uses_the_start_day_for_every_file() {
    // cycle starts 23:59:59 on the 8th; even if it runs past midnight the gate key is "the 8th"
    let start = datetime!(2026 - 09 - 08 23:59:59 UTC);
    let day = utc_day(start);
    assert_eq!(day, "2026-09-08");

    let new = sha256_hex(b"changed just before midnight");
    let state = prev(&sha256_hex(b"older"), "2026-09-07");
    // day-before-today -> send
    assert_eq!(decide_action(Some(&state), &new, &day), Decision::Send);
    // and once recorded for "the 8th", a second change the same wall-clock-day is gated
    let mut s = LocalBackupState::default();
    s.record_success("k", &new, &day, "2026-09-08T23:59:59Z");
    let newer = sha256_hex(b"changed again after midnight but same cycle-day");
    assert_eq!(
        decide_action(s.file("k"), &newer, &day),
        Decision::SkippedToday
    );
}

#[test]
fn failed_send_does_not_advance_the_day_but_dedup_200_does() {
    let _guard = common::env_lock();
    let roots = common::FakeRoots::create("daily");
    roots.write_watched("PQDevice.conf", b"device config bytes");

    let identity = Identity {
        machine_id: "00000000-0000-0000-0000-000000000000".to_string(),
        serial: Serial::Known("SN-DAILY".to_string()),
        os: OsInfo {
            version: "10.0.19045".into(),
            build: "19045".into(),
            arch: "x86_64".into(),
        },
    };
    let bucket = Product::current().bucket().to_string();

    // ---- cycle 1: backend rejects the backup with 500 -> day NOT advanced ----
    {
        let mut server = mockito::Server::new();
        let _backup = server
            .mock("POST", format!("/api/v2/products/{bucket}/backup").as_str())
            .with_status(500)
            .with_body("{}")
            .expect_at_least(1)
            .create();
        let _hb = server
            .mock(
                "POST",
                format!("/api/v2/products/{bucket}/telemetry").as_str(),
            )
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .expect_at_least(1)
            .create();

        let cfg = Config::with_base_url(server.url());
        let mut state = LocalBackupState::default();
        let record = cycle::run_once(&cfg, &identity, &mut state);

        let conf = record
            .files
            .iter()
            .find(|f| f.file_key == "pqdevice_conf")
            .expect("pqdevice_conf outcome");
        assert_eq!(conf.action, cycle::FileAction::RetryLater);
        assert!(
            state.file("pqdevice_conf").is_none(),
            "day must not advance on failure"
        );
    }

    // ---- cycle 2: backend dedupes with 200 {deduplicated:true} -> day advanced ----
    {
        let mut server = mockito::Server::new();
        let _backup = server
            .mock("POST", format!("/api/v2/products/{bucket}/backup").as_str())
            .with_status(200)
            .with_body(r#"{"ok":true,"id":"x","deduplicated":true,"size_bytes":19}"#)
            .expect_at_least(1)
            .create();
        let _hb = server
            .mock(
                "POST",
                format!("/api/v2/products/{bucket}/telemetry").as_str(),
            )
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .expect_at_least(1)
            .create();

        let cfg = Config::with_base_url(server.url());
        let mut state = LocalBackupState::default();
        let record = cycle::run_once(&cfg, &identity, &mut state);

        let conf = record
            .files
            .iter()
            .find(|f| f.file_key == "pqdevice_conf")
            .unwrap();
        assert_eq!(conf.action, cycle::FileAction::Deduplicated);
        let fs = state
            .file("pqdevice_conf")
            .expect("day advanced on dedup 200");
        assert_eq!(fs.last_backup_utc_day, cycle::utc_day(cycle::now_utc()));
    }
}
