//! The heartbeat pass: build the `agent_status` envelope per
//! `contracts/heartbeat-payload.schema.json` and POST it (FR-003, FR-004, FR-005).
//!
//! Runs after the backup pass so `blocked_backups` and `last_failure_category` reflect this
//! cycle (FR-002f, FR-007). A failure here never crashes the loop (FR-006).

use serde_json::{json, Value};

use crate::api::Api;
use crate::config::Config;
use crate::cycle::{CycleContext, FileOutcome, HeartbeatOutcome};
use crate::identity::Identity;
use crate::state::LocalBackupState;

pub fn run_pass(
    cfg: &Config,
    identity: &Identity,
    state: &LocalBackupState,
    ctx: &CycleContext,
    _file_outcomes: &[FileOutcome],
) -> HeartbeatOutcome {
    let api = Api::new(cfg);
    let envelope = build_envelope(cfg, identity, state, ctx);

    match api.post_heartbeat(&envelope) {
        Ok(_) => {
            log::info!("heartbeat ok");
            HeartbeatOutcome {
                ok: true,
                category: None,
            }
        }
        Err(err) => {
            log::warn!("heartbeat failed: {err}");
            HeartbeatOutcome {
                ok: false,
                category: Some(err.category.as_str().to_string()),
            }
        }
    }
}

/// Full request body for `POST /api/v2/products/{bucket}/telemetry`.
pub fn build_envelope(
    cfg: &Config,
    identity: &Identity,
    state: &LocalBackupState,
    ctx: &CycleContext,
) -> Value {
    json!({
        "measurement_type": "agent_status",
        "measured_at": crate::cycle::to_rfc3339(ctx.started),
        "instrument_serial": identity.serial.wire_value(),
        "payload": build_payload(cfg, identity, state, ctx),
    })
}

/// The `payload` object — matches `heartbeat-payload.schema.json` exactly.
fn build_payload(
    cfg: &Config,
    identity: &Identity,
    state: &LocalBackupState,
    ctx: &CycleContext,
) -> Value {
    let blocked: Vec<Value> = ctx
        .blocked_backups
        .iter()
        .map(|b| json!({ "file_key": b.file_key, "reason": b.reason }))
        .collect();

    let last_backup_days: serde_json::Map<String, Value> = state
        .last_backup_days()
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    json!({
        "machine_id": identity.machine_id,
        "agent_version": crate::config::VERSION,
        "product": cfg.product.bucket(),
        "channel": cfg.channel.as_str(),
        "serial_source": identity.serial.source(),
        "os": {
            "version": identity.os.version,
            "build": identity.os.build,
            "arch": identity.os.arch,
        },
        "cycle": {
            "started_utc": crate::cycle::to_rfc3339(ctx.started),
            "duration_ms": ctx.duration_ms,
            "ok": ctx.cycle_ok,
        },
        "last_failure_category": ctx.last_failure_category.map(|c| c.as_str()),
        "blocked_backups": blocked,
        "last_backup_days": last_backup_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, Config};
    use crate::cycle::{BlockedBackup, CycleContext};
    use crate::error::FailureCategory;
    use crate::identity::{Identity, OsInfo, Serial};
    use crate::product::Product;

    fn cfg() -> Config {
        Config {
            product: Product::current(),
            channel: Channel::current(),
            cycle_interval_secs: 1800,
            api_base_url: "https://api.picoquant.com".to_string(),
            backup_max_bytes: 52_428_800,
            http_timeout_secs: 60,
        }
    }

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
            started: crate::cycle::now_utc(),
            duration_ms: 812,
            blocked_backups: vec![BlockedBackup {
                file_key: "pqdevice_db".to_string(),
                reason: "locked",
            }],
            last_failure_category: Some(FailureCategory::FileLocked),
            cycle_ok: true,
        }
    }

    #[test]
    fn envelope_has_required_shape() {
        let state = LocalBackupState::default();
        let env = build_envelope(&cfg(), &identity(Serial::Known("SN-1".into())), &state, &ctx());
        assert_eq!(env["measurement_type"], "agent_status");
        assert_eq!(env["instrument_serial"], "SN-1");
        assert!(env["measured_at"].as_str().unwrap().contains('T'));
        let p = &env["payload"];
        for key in [
            "machine_id",
            "agent_version",
            "product",
            "channel",
            "serial_source",
            "os",
            "cycle",
        ] {
            assert!(!p[key].is_null(), "payload.{key} missing");
        }
        assert_eq!(p["serial_source"], "file");
        assert_eq!(p["cycle"]["duration_ms"], 812);
        assert_eq!(p["last_failure_category"], "file_locked");
        assert_eq!(p["blocked_backups"][0]["reason"], "locked");
    }

    #[test]
    fn unknown_serial_path() {
        let state = LocalBackupState::default();
        let env = build_envelope(&cfg(), &identity(Serial::Unknown), &state, &ctx());
        assert_eq!(env["instrument_serial"], "unknown");
        assert_eq!(env["payload"]["serial_source"], "unknown");
    }

    #[test]
    fn no_failure_serializes_null() {
        let state = LocalBackupState::default();
        let mut c = ctx();
        c.last_failure_category = None;
        c.blocked_backups.clear();
        let env = build_envelope(&cfg(), &identity(Serial::Known("SN-1".into())), &state, &c);
        assert!(env["payload"]["last_failure_category"].is_null());
        assert_eq!(env["payload"]["blocked_backups"].as_array().unwrap().len(), 0);
    }
}
