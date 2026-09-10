//! T021 (US2) — content-hash change detection: unchanged → skip, changed → send,
//! no prior state (first run) → send (FR-010, FR-016).

use pquploader::backup::{decide_action, Decision};
use pquploader::state::FileBackupState;
use pquploader::watchset::sha256_hex;

fn prev(sha: &str, day: &str) -> FileBackupState {
    FileBackupState {
        last_backup_sha256: sha.to_string(),
        last_backup_utc_day: day.to_string(),
        last_success_utc: format!("{day}T00:00:00Z"),
    }
}

#[test]
fn first_run_with_no_state_sends() {
    let sha = sha256_hex(b"<config/>");
    assert_eq!(
        decide_action(None, &sha, "2026-09-08", true),
        Decision::Send
    );
}

#[test]
fn unchanged_content_is_skipped_even_on_a_new_day() {
    let sha = sha256_hex(b"<config/>");
    let state = prev(&sha, "2026-09-01");
    assert_eq!(
        decide_action(Some(&state), &sha, "2026-09-08", true),
        Decision::Unchanged
    );
}

#[test]
fn unchanged_content_is_skipped_regardless_of_the_daily_limit() {
    let sha = sha256_hex(b"<config/>");
    let state = prev(&sha, "2026-09-08");
    assert_eq!(
        decide_action(Some(&state), &sha, "2026-09-08", false),
        Decision::Unchanged
    );
}

#[test]
fn changed_content_on_a_later_day_sends() {
    let old = sha256_hex(b"<config v=1/>");
    let new = sha256_hex(b"<config v=2/>");
    let state = prev(&old, "2026-09-07");
    assert_eq!(
        decide_action(Some(&state), &new, "2026-09-08", true),
        Decision::Send
    );
}

#[test]
fn hash_is_content_based_not_mtime_based() {
    // identical bytes -> identical digest regardless of when they were written
    assert_eq!(sha256_hex(b"same bytes"), sha256_hex(b"same bytes"));
    assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
}
