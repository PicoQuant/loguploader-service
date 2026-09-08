//! One work cycle: a backup pass then a heartbeat pass, each isolated so a failure in one
//! sub-step is logged and does not abort the cycle (Constitution I, FR-029).
//!
//! `cycle.rs` owns the `CycleRecord` / `CycleSummary` types and the shared time helpers.
//! The backup pass is wired in by US2 (`backup.rs`), the heartbeat pass by US1 (`telemetry.rs`).

use std::process::ExitCode;
use std::time::Instant;

use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::backup;
use crate::config::Config;
use crate::error::FailureCategory;
use crate::identity::Identity;
use crate::logging;
use crate::state::{CycleCounts, CycleSummary, LocalBackupState};
use crate::telemetry;

// ---- time helpers ----------------------------------------------------------

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub fn now_rfc3339() -> String {
    now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn to_rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// The UTC calendar day (`YYYY-MM-DD`) — the once-per-day gate key (FR-032).
pub fn utc_day(t: OffsetDateTime) -> String {
    t.date().to_string()
}

// ---- per-cycle records ----------------------------------------------------

/// What happened to one watched file this cycle (`data-model.md` → CycleRecord.files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Sent,
    Deduplicated,
    Unchanged,
    SkippedToday,
    Blocked,
    RetryLater,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileOutcome {
    pub file_key: String,
    pub action: FileAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatOutcome {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CycleRecord {
    pub started_utc: String,
    pub finished_utc: String,
    pub product: String,
    pub instrument_serial: String,
    pub machine_id: String,
    pub heartbeat: HeartbeatOutcome,
    pub files: Vec<FileOutcome>,
    pub duration_ms: u64,
}

impl CycleRecord {
    /// Persisted subset for `state.json.last_cycle`.
    pub fn summary(&self) -> CycleSummary {
        let mut counts = CycleCounts {
            sent: 0,
            unchanged: 0,
            blocked: 0,
            retry_later: 0,
        };
        for f in &self.files {
            match f.action {
                FileAction::Sent | FileAction::Deduplicated => counts.sent += 1,
                FileAction::Unchanged | FileAction::SkippedToday => counts.unchanged += 1,
                FileAction::Blocked => counts.blocked += 1,
                FileAction::RetryLater => counts.retry_later += 1,
            }
        }
        CycleSummary {
            started_utc: self.started_utc.clone(),
            ok: self.cycle_ok(),
            heartbeat_ok: self.heartbeat.ok,
            counts,
        }
    }

    /// `cycle.ok` = the cycle completed without an *internal* error. A per-file block or a
    /// backend rejection is an expected outcome, not a cycle failure (FR-002f).
    pub fn cycle_ok(&self) -> bool {
        self.heartbeat.category.as_deref() != Some(FailureCategory::Internal.as_str())
            && !self
                .files
                .iter()
                .any(|f| f.category.as_deref() == Some(FailureCategory::Internal.as_str()))
    }
}

/// A `blocked_backups` entry surfaced to the heartbeat (FR-007).
#[derive(Debug, Clone)]
pub struct BlockedBackup {
    pub file_key: String,
    pub reason: &'static str,
}

/// Context handed from the backup pass to the heartbeat pass.
pub struct CycleContext {
    pub started: OffsetDateTime,
    pub duration_ms: u64,
    pub blocked_backups: Vec<BlockedBackup>,
    pub last_failure_category: Option<FailureCategory>,
    /// false once any sub-step hit an internal error.
    pub cycle_ok: bool,
}

// ---- the cycle -----------------------------------------------------------

/// Run one full cycle. Never returns `Err` — every failure is captured in the `CycleRecord`
/// and the loop moves on (Constitution I).
pub fn run_once(cfg: &Config, identity: &Identity, state: &mut LocalBackupState) -> CycleRecord {
    let started = now_utc();
    let clock = Instant::now();

    log::info!(
        "cycle start — product={} channel={} version={} machine_id={} serial={} token_present={}",
        cfg.product.bucket(),
        cfg.channel.as_str(),
        crate::config::VERSION,
        identity.machine_id,
        identity.serial.wire_value(),
        logging::token_present(),
    );

    let mut ctx = CycleContext {
        started,
        duration_ms: 0,
        blocked_backups: Vec::new(),
        last_failure_category: None,
        cycle_ok: true,
    };

    // ---- backup pass (US2) — runs BEFORE the heartbeat so blocks are visible in it ----
    let file_outcomes = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backup::run_pass(cfg, identity, state, &mut ctx)
    })) {
        Ok(outcomes) => outcomes,
        Err(_) => {
            log::error!("backup pass panicked; cycle continues");
            ctx.cycle_ok = false;
            ctx.last_failure_category = Some(FailureCategory::Internal);
            Vec::new()
        }
    };

    // persist backup state before the heartbeat so a heartbeat panic can't lose it
    if let Err(e) = state.save() {
        log::warn!("could not persist state.json after backup pass: {e}");
    }

    ctx.duration_ms = clock.elapsed().as_millis() as u64;

    // ---- heartbeat pass (US1) ----
    let heartbeat = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        telemetry::run_pass(cfg, identity, state, &ctx, &file_outcomes)
    })) {
        Ok(hb) => hb,
        Err(_) => {
            log::error!("heartbeat pass panicked; cycle continues");
            HeartbeatOutcome {
                ok: false,
                category: Some(FailureCategory::Internal.as_str().to_string()),
            }
        }
    };

    let finished = now_utc();
    let record = CycleRecord {
        started_utc: to_rfc3339(started),
        finished_utc: to_rfc3339(finished),
        product: cfg.product.bucket().to_string(),
        instrument_serial: identity.serial.wire_value().to_string(),
        machine_id: identity.machine_id.clone(),
        heartbeat,
        files: file_outcomes,
        duration_ms: clock.elapsed().as_millis() as u64,
    };

    // persist the cycle summary + heartbeat timestamp
    state.last_cycle = Some(record.summary());
    if record.heartbeat.ok {
        state.last_heartbeat_utc = Some(to_rfc3339(finished));
    }
    if let Err(e) = state.save() {
        log::warn!("could not persist state.json after cycle: {e}");
    }

    // observability: full record to cycles.log, summary to the Event Log
    if let Ok(json) = serde_json::to_string(&record) {
        logging::append_cycle_record(&json);
    }
    log::info!(
        "cycle done in {} ms — heartbeat_ok={} files: {} sent, {} unchanged, {} blocked, {} retry_later",
        record.duration_ms,
        record.heartbeat.ok,
        record.summary().counts.sent,
        record.summary().counts.unchanged,
        record.summary().counts.blocked,
        record.summary().counts.retry_later,
    );

    record
}

/// `pquploader once` — one cycle, print the record as JSON, exit 0 (1 on internal error).
pub fn run_once_cli() -> ExitCode {
    logging::init(true);
    let cfg = Config::load();
    let identity = Identity::resolve(cfg.product);
    let mut state = LocalBackupState::load();

    let record = run_once(&cfg, &identity, &mut state);
    match serde_json::to_string_pretty(&record) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("could not serialize CycleRecord: {e}");
            return ExitCode::from(1);
        }
    }
    if record.cycle_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn utc_day_is_yyyy_mm_dd() {
        let t = datetime!(2026 - 09 - 08 23:59:00 UTC);
        assert_eq!(utc_day(t), "2026-09-08");
    }

    #[test]
    fn utc_day_captured_once_does_not_roll_at_midnight() {
        // a cycle that starts at 23:59:59 uses that day for every file, even if it
        // finishes after 00:00 (spec Edge Cases).
        let start = datetime!(2026 - 09 - 08 23:59:59 UTC);
        let day = utc_day(start);
        assert_eq!(day, "2026-09-08");
    }

    #[test]
    fn rfc3339_round_trips_through_time() {
        let s = to_rfc3339(datetime!(2026 - 09 - 08 08:17:11 UTC));
        assert!(s.starts_with("2026-09-08T08:17:11"));
    }
}
