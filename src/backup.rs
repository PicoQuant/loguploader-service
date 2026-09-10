//! The backup pass: for each watched file decide what to do against `LocalBackupState`,
//! send when needed, and update state only on a confirmed `200` (FR-010, FR-011, FR-012,
//! FR-016, FR-018). A locked / oversized / rejected file never blocks the others (SC-012).

use crate::api::Api;
use crate::config::Config;
use crate::cycle::{self, BlockedBackup, CycleContext, FileAction, FileOutcome};
use crate::error::FailureCategory;
use crate::identity::Identity;
use crate::multipart::MultipartBody;
use crate::state::{FileBackupState, LocalBackupState};
use crate::watchset::{self, ReadResult};

/// The change-detection + once-per-UTC-day decision for one file (`data-model.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// sha == last successful backup — nothing to do (FR-010).
    Unchanged,
    /// changed, but already backed up today (UTC) and the file is daily-gated — wait for
    /// tomorrow (FR-011). Never returned for files with `daily_limit == false` (FR-011a).
    SkippedToday,
    /// no state (first run, FR-016) or changed since a day before today — send.
    Send,
}

/// Pure decision function — no I/O, unit-tested by `tests/change_detection.rs` +
/// `tests/daily_limit_utc.rs`.
///
/// `daily_limit == false` (FR-011a): a changed file is always `Send`, however many times it
/// already went out today.
pub fn decide_action(
    prev: Option<&FileBackupState>,
    current_sha256: &str,
    today_utc: &str,
    daily_limit: bool,
) -> Decision {
    match prev {
        Some(fs) if fs.last_backup_sha256 == current_sha256 => Decision::Unchanged,
        Some(fs) if daily_limit && fs.last_backup_utc_day == today_utc => Decision::SkippedToday,
        _ => Decision::Send,
    }
}

/// Runs the whole pass and returns one `FileOutcome` per discovered file.
/// Mutates `state` (success records) and `ctx` (blocked list, last failure, cycle_ok).
pub fn run_pass(
    cfg: &Config,
    identity: &Identity,
    state: &mut LocalBackupState,
    ctx: &mut CycleContext,
) -> Vec<FileOutcome> {
    let api = Api::new(cfg);
    let today = cycle::utc_day(ctx.started);
    let now_ts = cycle::to_rfc3339(ctx.started);

    let resolved = watchset::resolve_and_read(cfg);
    let mut outcomes = Vec::with_capacity(resolved.len());

    for file in resolved {
        let outcome = process_file(
            &api,
            identity,
            state,
            ctx,
            &file.file_key,
            &file.abs_path,
            file.read_result,
            file.daily_limit,
            &today,
            &now_ts,
        );
        outcomes.push(outcome);
    }
    outcomes
}

#[allow(clippy::too_many_arguments)]
fn process_file(
    api: &Api,
    identity: &Identity,
    state: &mut LocalBackupState,
    ctx: &mut CycleContext,
    file_key: &str,
    abs_path: &std::path::Path,
    read_result: ReadResult,
    daily_limit: bool,
    today: &str,
    now_ts: &str,
) -> FileOutcome {
    // ---- read-side classification -------------------------------------------
    let (bytes, sha256, mtime) = match read_result {
        ReadResult::Ok {
            bytes,
            sha256,
            mtime,
        } => (bytes, sha256, mtime),
        ReadResult::Locked => {
            log::warn!(
                "{file_key}: locked/torn at {}, retry next cycle",
                abs_path.display()
            );
            ctx.note_blocked(file_key, "locked");
            ctx.note_failure(FailureCategory::FileLocked);
            return outcome(
                file_key,
                FileAction::RetryLater,
                Some(FailureCategory::FileLocked),
            );
        }
        ReadResult::Absent => {
            log::info!("{file_key}: absent this cycle ({})", abs_path.display());
            ctx.note_blocked(file_key, "absent");
            return outcome(
                file_key,
                FileAction::Blocked,
                Some(FailureCategory::FileAbsent),
            );
        }
        ReadResult::TooLarge(size) => {
            log::warn!(
                "{file_key}: {size} bytes at {} exceeds backup_max_bytes; skipped",
                abs_path.display()
            );
            ctx.note_blocked(file_key, "too_large");
            return outcome(
                file_key,
                FileAction::Blocked,
                Some(FailureCategory::TooLarge),
            );
        }
    };

    // ---- change detection + daily gate (data-model.md) ---------------------
    match decide_action(state.file(file_key), &sha256, today, daily_limit) {
        Decision::Unchanged => return outcome(file_key, FileAction::Unchanged, None), // FR-010
        Decision::SkippedToday => {
            return outcome(file_key, FileAction::SkippedToday, None); // FR-011
        }
        Decision::Send => {} // no state (first run, FR-016) or changed on an earlier day
    }

    // ---- build + send -----------------------------------------------------
    let mtime_str = mtime.map(cycle::to_rfc3339);
    let body = build_submission(
        identity,
        file_key,
        abs_path,
        &sha256,
        &bytes,
        mtime_str.as_deref(),
        now_ts,
    );

    match api.post_backup(body) {
        Ok(resp) => {
            let action = if resp.deduplicated {
                FileAction::Deduplicated
            } else {
                FileAction::Sent
            };
            // deduplicated: true is still success — advance the daily gate (FR-012).
            state.record_success(file_key, &sha256, today, now_ts);
            log::info!(
                "{file_key}: {} ({} bytes)",
                action_word(action),
                bytes.len()
            );
            outcome(file_key, action, None)
        }
        Err(err) => {
            ctx.note_failure(err.category);
            route_failure(ctx, file_key, err.category)
        }
    }
}

/// FR-012 / FR-014 / FR-027 failure routing.
fn route_failure(ctx: &mut CycleContext, file_key: &str, category: FailureCategory) -> FileOutcome {
    match category {
        // retried next cycle, no state change, not "blocked" in the heartbeat sense
        FailureCategory::NoNetwork | FailureCategory::BackendError => {
            log::warn!("{file_key}: {category}; retry next cycle");
            outcome(file_key, FileAction::RetryLater, Some(category))
        }
        FailureCategory::FileLocked => {
            ctx.note_blocked(file_key, "locked");
            outcome(file_key, FileAction::RetryLater, Some(category))
        }
        // 401 — keep retrying (a valid token arrives as a new build), surfaced via heartbeat
        FailureCategory::Auth => {
            log::warn!("{file_key}: token rejected (401); retry next cycle");
            outcome(file_key, FileAction::RetryLater, Some(category))
        }
        // not blindly retried — surfaced and left until the file changes / a new build ships
        FailureCategory::RejectedBadRequest => {
            log::error!("{file_key}: backend rejected the submission as bad request");
            ctx.note_blocked(file_key, "rejected");
            outcome(file_key, FileAction::Blocked, Some(category))
        }
        FailureCategory::TooLarge => {
            ctx.note_blocked(file_key, "too_large");
            outcome(file_key, FileAction::Blocked, Some(category))
        }
        FailureCategory::FileAbsent => {
            ctx.note_blocked(file_key, "absent");
            outcome(file_key, FileAction::Blocked, Some(category))
        }
        FailureCategory::Internal => {
            ctx.cycle_ok = false;
            outcome(file_key, FileAction::RetryLater, Some(category))
        }
    }
}

/// `contracts/backend-api.md` §2 — the multipart parts, in a stable order.
/// `pub` so `tests/multipart_format.rs` can assert the wire shape.
pub fn build_submission(
    identity: &Identity,
    file_key: &str,
    source_path: &std::path::Path,
    sha256: &str,
    bytes: &[u8],
    file_mtime: Option<&str>,
    client_timestamp: &str,
) -> MultipartBody {
    let filename = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("content.bin");

    let mut body = MultipartBody::new()
        .file("content", filename, bytes)
        .text("instrument_serial", identity.serial.wire_value())
        .text("machine_id", &identity.machine_id)
        .text("file_key", file_key)
        .text("source_path", &source_path.display().to_string())
        .text("content_sha256", sha256)
        .text("agent_version", crate::config::VERSION)
        .text("client_timestamp", client_timestamp);
    if let Some(m) = file_mtime {
        body = body.text("file_mtime", m);
    }
    body
}

fn outcome(file_key: &str, action: FileAction, category: Option<FailureCategory>) -> FileOutcome {
    FileOutcome {
        file_key: file_key.to_string(),
        action,
        category: category.map(|c| c.as_str().to_string()),
    }
}

fn action_word(a: FileAction) -> &'static str {
    match a {
        FileAction::Sent => "sent",
        FileAction::Deduplicated => "deduplicated",
        FileAction::Unchanged => "unchanged",
        FileAction::SkippedToday => "skipped_today",
        FileAction::Blocked => "blocked",
        FileAction::RetryLater => "retry_later",
    }
}

impl CycleContext {
    pub fn note_blocked(&mut self, file_key: &str, reason: &'static str) {
        if !self
            .blocked_backups
            .iter()
            .any(|b| b.file_key == file_key && b.reason == reason)
        {
            self.blocked_backups.push(BlockedBackup {
                file_key: file_key.to_string(),
                reason,
            });
        }
    }

    pub fn note_failure(&mut self, category: FailureCategory) {
        self.last_failure_category = Some(category);
        if category == FailureCategory::Internal {
            self.cycle_ok = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, OsInfo, Serial};

    fn fake_identity() -> Identity {
        Identity {
            machine_id: "00000000-0000-0000-0000-000000000000".to_string(),
            serial: Serial::Known("SN-1".to_string()),
            os: OsInfo {
                version: "10.0.19045".to_string(),
                build: "19045".to_string(),
                arch: "x86_64".to_string(),
            },
            instrument_sw: Default::default(),
        }
    }

    #[test]
    fn multipart_carries_the_contract_fields() {
        let id = fake_identity();
        let body = build_submission(
            &id,
            "pqdevice_conf",
            std::path::Path::new(r"C:\Program Files\PicoQuant\Luminosa\PQDevice.conf"),
            &"a".repeat(64),
            b"payload",
            Some("2026-09-08T07:55:00Z"),
            "2026-09-08T08:00:00Z",
        );
        let (bytes, ct) = body.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert!(ct.starts_with("multipart/form-data; boundary="));
        for needle in [
            "name=\"content\"; filename=\"PQDevice.conf\"",
            "name=\"instrument_serial\"",
            "name=\"machine_id\"",
            "name=\"file_key\"",
            "name=\"source_path\"",
            "name=\"content_sha256\"",
            "name=\"file_mtime\"",
            "name=\"agent_version\"",
            "name=\"client_timestamp\"",
        ] {
            assert!(text.contains(needle), "missing part: {needle}");
        }
    }

    #[test]
    fn note_blocked_dedupes() {
        let mut ctx = CycleContext {
            started: crate::cycle::now_utc(),
            duration_ms: 0,
            blocked_backups: vec![],
            last_failure_category: None,
            cycle_ok: true,
        };
        ctx.note_blocked("k", "locked");
        ctx.note_blocked("k", "locked");
        assert_eq!(ctx.blocked_backups.len(), 1);
    }
}
