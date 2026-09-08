//! T016 / T029 (US1, US3) — the exact wire contract for both endpoints, driven against a
//! local mock server. No real network. Covers `contracts/backend-api.md` §1–§2 and the
//! retry policy.

use pquploader::api::{category_for_status, Api};
use pquploader::config::{Channel, Config};
use pquploader::cycle::{now_utc, BlockedBackup, CycleContext};
use pquploader::error::FailureCategory;
use pquploader::identity::{Identity, OsInfo, Serial};
use pquploader::product::Product;
use pquploader::state::LocalBackupState;
use pquploader::telemetry;

fn identity(serial: Serial) -> Identity {
    Identity {
        machine_id: "0f4a1111-2222-3333-4444-555566667777".to_string(),
        serial,
        os: OsInfo {
            version: "10.0.19045".to_string(),
            build: "19045".to_string(),
            arch: "x86_64".to_string(),
        },
    }
}

fn ctx() -> CycleContext {
    CycleContext {
        started: now_utc(),
        duration_ms: 5,
        blocked_backups: vec![BlockedBackup {
            file_key: "pqdevice_db".to_string(),
            reason: "locked",
        }],
        last_failure_category: Some(FailureCategory::FileLocked),
        cycle_ok: true,
    }
}

fn bucket() -> String {
    Product::current().bucket().to_string()
}

#[test]
fn heartbeat_request_matches_contract() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", bucket());

    let m = server
        .mock("POST", path.as_str())
        .match_header("x-telemetry-token", mockito::Matcher::Any)
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"measurement_type":"agent_status"}"#.to_string(),
        ))
        .with_status(200)
        .with_body(r#"{"ok":true,"id":"abc","received_at":"2026-09-08T08:17:11Z"}"#)
        .create();

    let cfg = Config::with_base_url(server.url());
    let state = LocalBackupState::default();
    let envelope = telemetry::build_envelope(
        &cfg,
        &identity(Serial::Known("SN-1".into())),
        &state,
        &ctx(),
    );

    // shape assertions on the envelope we actually send
    assert_eq!(envelope["measurement_type"], "agent_status");
    assert_eq!(envelope["instrument_serial"], "SN-1");
    assert!(envelope["payload"]
        .as_object()
        .map(|o| !o.is_empty())
        .unwrap_or(false));
    assert!(!envelope["payload"]["instrument_serial"].is_string()); // serial is on the envelope, not payload
    assert_eq!(envelope["payload"]["channel"], Channel::current().as_str());

    let api = Api::new(&cfg);
    let resp = api.post_heartbeat(&envelope).expect("200 heartbeat");
    assert!(!resp.deduplicated);
    m.assert();
}

#[test]
fn heartbeat_missing_serial_is_still_sent_as_unknown() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", bucket());
    let m = server
        .mock("POST", path.as_str())
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"instrument_serial":"unknown"}"#.to_string(),
        ))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .create();

    let cfg = Config::with_base_url(server.url());
    let state = LocalBackupState::default();
    let envelope = telemetry::build_envelope(&cfg, &identity(Serial::Unknown), &state, &ctx());
    assert_eq!(envelope["payload"]["serial_source"], "unknown");
    Api::new(&cfg)
        .post_heartbeat(&envelope)
        .expect("unknown serial accepted");
    m.assert();
}

#[test]
fn status_codes_map_to_expected_categories() {
    let cases = [
        (200u16, None),
        (401, Some(FailureCategory::Auth)),
        (404, Some(FailureCategory::RejectedBadRequest)),
        (422, Some(FailureCategory::RejectedBadRequest)),
        (413, Some(FailureCategory::TooLarge)),
        (500, Some(FailureCategory::BackendError)),
    ];
    for (code, expected) in cases {
        let mut server = mockito::Server::new();
        let path = format!("/api/v2/products/{}/telemetry", bucket());
        let _m = server
            .mock("POST", path.as_str())
            .with_status(code as usize)
            .with_body("{}")
            .expect_at_least(1)
            .create();

        let cfg = Config::with_base_url(server.url());
        let state = LocalBackupState::default();
        let env = telemetry::build_envelope(
            &cfg,
            &identity(Serial::Known("SN-1".into())),
            &state,
            &ctx(),
        );
        let got = Api::new(&cfg).post_heartbeat(&env);
        match expected {
            None => assert!(got.is_ok(), "code {code} should be ok"),
            Some(cat) => {
                let err = got.expect_err(&format!("code {code} should fail"));
                assert_eq!(err.category, cat, "code {code}");
                assert_eq!(category_for_status(code), cat);
            }
        }
    }
}

#[test]
fn backend_error_is_retried_three_times_then_left_for_next_cycle() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", bucket());
    // 3 attempts total (1 + 2 retries), 2s + 4s backoff between them
    let m = server
        .mock("POST", path.as_str())
        .with_status(503)
        .with_body("{}")
        .expect(3)
        .create();

    let cfg = Config::with_base_url(server.url());
    let state = LocalBackupState::default();
    let env = telemetry::build_envelope(
        &cfg,
        &identity(Serial::Known("SN-1".into())),
        &state,
        &ctx(),
    );
    let err = Api::new(&cfg).post_heartbeat(&env).unwrap_err();
    assert_eq!(err.category, FailureCategory::BackendError);
    m.assert(); // exactly 3 attempts
}

#[test]
fn auth_failure_is_not_retried_within_a_cycle() {
    let mut server = mockito::Server::new();
    let path = format!("/api/v2/products/{}/telemetry", bucket());
    let m = server
        .mock("POST", path.as_str())
        .with_status(401)
        .with_body("{}")
        .expect(1) // no in-cycle retry for 4xx
        .create();

    let cfg = Config::with_base_url(server.url());
    let state = LocalBackupState::default();
    let env = telemetry::build_envelope(
        &cfg,
        &identity(Serial::Known("SN-1".into())),
        &state,
        &ctx(),
    );
    let err = Api::new(&cfg).post_heartbeat(&env).unwrap_err();
    assert_eq!(err.category, FailureCategory::Auth);
    m.assert();
}

#[test]
fn connection_refused_is_no_network() {
    // nothing is listening on this port
    let cfg = Config::with_base_url("http://127.0.0.1:9");
    let state = LocalBackupState::default();
    let env = telemetry::build_envelope(
        &cfg,
        &identity(Serial::Known("SN-1".into())),
        &state,
        &ctx(),
    );
    let err = Api::new(&cfg).post_heartbeat(&env).unwrap_err();
    assert_eq!(err.category, FailureCategory::NoNetwork);
}

#[test]
fn both_endpoints_always_carry_full_attribution() {
    // US3 / SC-005 / SC-008b: product bucket + channel + machine_id + agent_version +
    // instrument_serial + a client UTC timestamp on every submission.
    let cfg = Config::with_base_url("http://example.invalid");
    let state = LocalBackupState::default();
    let id = identity(Serial::Known("SN-42".into()));
    let c = ctx();

    let hb = telemetry::build_envelope(&cfg, &id, &state, &c);
    assert_eq!(hb["payload"]["product"], Product::current().bucket());
    assert_eq!(hb["payload"]["channel"], Channel::current().as_str());
    assert_eq!(hb["payload"]["machine_id"], id.machine_id);
    assert_eq!(hb["payload"]["agent_version"], pquploader::config::VERSION);
    assert_eq!(hb["instrument_serial"], "SN-42");
    assert!(
        hb["measured_at"].as_str().unwrap().ends_with('Z')
            || hb["measured_at"].as_str().unwrap().contains('+')
    );

    let (body, ct) = pquploader::backup::build_submission(
        &id,
        "pqdevice_conf",
        std::path::Path::new(r"C:\Program Files\PicoQuant\Luminosa\PQDevice.conf"),
        &"a".repeat(64),
        b"payload",
        Some("2026-09-08T07:55:00Z"),
        "2026-09-08T08:00:00Z",
    )
    .finish();
    let text = String::from_utf8_lossy(&body);
    assert!(ct.starts_with("multipart/form-data; boundary="));
    for needle in [
        "SN-42",
        id.machine_id.as_str(),
        pquploader::config::VERSION,
        "content_sha256",
    ] {
        assert!(text.contains(needle), "backup body missing {needle}");
    }
}
