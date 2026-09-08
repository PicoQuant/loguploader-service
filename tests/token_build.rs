//! T034 (US4) — the agent sends its single compiled token value verbatim, and a build with
//! no token still runs (it just logs `Auth` failures). The "empty token + PROFILE=release
//! => build error" rule is enforced in `build.rs` and cannot be exercised from here without
//! spawning a second `cargo build`; it is covered by the CI matrix
//! (`.github/workflows/windows-build.yml`) instead. (FR-024, FR-025)

use pquploader::config::{self, Config};

#[test]
fn compiled_token_is_a_single_value_no_commas() {
    // build.rs takes the FIRST comma-separated entry — the binary must carry exactly one.
    assert!(
        !config::FLEET_TOKEN.contains(','),
        "the compiled token must be a single value, not a list"
    );
}

#[test]
fn has_fleet_token_reflects_compile_time_presence() {
    let cfg = Config::with_base_url("http://example.invalid");
    assert_eq!(cfg.has_fleet_token(), !config::FLEET_TOKEN.is_empty());
}

#[test]
fn agent_sends_the_verbatim_compiled_token_as_x_telemetry_token() {
    let mut server = mockito::Server::new();
    let bucket = pquploader::product::Product::current().bucket();
    let expected = if config::FLEET_TOKEN.is_empty() {
        mockito::Matcher::Any
    } else {
        mockito::Matcher::Exact(config::FLEET_TOKEN.to_string())
    };
    let m = server
        .mock(
            "POST",
            format!("/api/v2/products/{bucket}/telemetry").as_str(),
        )
        .match_header("x-telemetry-token", expected)
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .create();

    let cfg = Config::with_base_url(server.url());
    let state = pquploader::state::LocalBackupState::default();
    let id = pquploader::identity::Identity {
        machine_id: "m".into(),
        serial: pquploader::identity::Serial::Known("SN-1".into()),
        os: pquploader::identity::OsInfo {
            version: "10".into(),
            build: "0".into(),
            arch: "x86_64".into(),
        },
    };
    let ctx = pquploader::cycle::CycleContext {
        started: pquploader::cycle::now_utc(),
        duration_ms: 1,
        blocked_backups: vec![],
        last_failure_category: None,
        cycle_ok: true,
    };
    let env = pquploader::telemetry::build_envelope(&cfg, &id, &state, &ctx);
    pquploader::api::Api::new(&cfg)
        .post_heartbeat(&env)
        .expect("200");
    m.assert();
}
