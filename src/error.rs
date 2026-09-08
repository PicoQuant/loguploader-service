//! Failure taxonomy shared by every submission path (FR-006, FR-012, FR-014, FR-026, FR-027).
//!
//! The service loop never aborts on any of these — a category is recorded, surfaced in the
//! heartbeat where possible, and the cycle continues (Constitution I).

use std::fmt;

/// How a heartbeat or backup submission (or a local file read feeding one) failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCategory {
    /// No route to the backend: DNS / connect refused / timeout before any HTTP status.
    NoNetwork,
    /// Backend returned 401 — the compiled fleet token was rejected (FR-026).
    Auth,
    /// Backend returned 400 / 404 / 422 — the request is malformed or names an unknown
    /// product/file_key. Surface loudly; do NOT blindly retry (FR-014, FR-027).
    RejectedBadRequest,
    /// Backend returned 413 — the body is over `CONFIG_BACKUP_MAX_BYTES` (FR-014).
    TooLarge,
    /// Backend returned 5xx or an otherwise unexpected status; transient (FR-027).
    BackendError,
    /// A watched file was open with a sharing mode we could not read, or changed mid-read.
    FileLocked,
    /// A watched file was not present this cycle.
    FileAbsent,
    /// An unexpected local error (serialization, path resolution, etc.).
    Internal,
}

impl FailureCategory {
    /// Wire / log token — matches `contracts/heartbeat-payload.schema.json`
    /// `last_failure_category` enum and `data-model.md`.
    pub fn as_str(self) -> &'static str {
        match self {
            FailureCategory::NoNetwork => "no_network",
            FailureCategory::Auth => "authentication",
            FailureCategory::RejectedBadRequest => "rejected_bad_request",
            FailureCategory::TooLarge => "too_large",
            FailureCategory::BackendError => "backend_error",
            FailureCategory::FileLocked => "file_locked",
            FailureCategory::FileAbsent => "file_absent",
            FailureCategory::Internal => "internal",
        }
    }

    /// `blocked_backups[].reason` token, for the subset of categories that block a backup
    /// (`locked` | `absent` | `too_large` | `rejected`). `None` for categories that mean
    /// "retry next cycle" rather than "blocked" (FR-007).
    pub fn blocked_reason(self) -> Option<&'static str> {
        match self {
            FailureCategory::FileLocked => Some("locked"),
            FailureCategory::FileAbsent => Some("absent"),
            FailureCategory::TooLarge => Some("too_large"),
            FailureCategory::RejectedBadRequest => Some("rejected"),
            _ => None,
        }
    }

    /// True if the same submission is worth retrying on the next cycle unchanged.
    ///
    /// `Auth` is retryable-next-cycle (a fix arrives as a new build carrying a valid token,
    /// FR-026) but is NOT retried *within* a cycle — see `api::should_retry_in_cycle`.
    pub fn retryable_next_cycle(self) -> bool {
        matches!(
            self,
            FailureCategory::NoNetwork
                | FailureCategory::Auth
                | FailureCategory::BackendError
                | FailureCategory::FileLocked
        )
    }
}

impl fmt::Display for FailureCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A categorized error with a short human-readable context string (goes to `cycles.log`
/// and the Event Log — never contains the token).
#[derive(Debug, Clone)]
pub struct AgentError {
    pub category: FailureCategory,
    pub context: String,
}

impl AgentError {
    pub fn new(category: FailureCategory, context: impl Into<String>) -> Self {
        Self {
            category,
            context: context.into(),
        }
    }

    pub fn internal(context: impl Into<String>) -> Self {
        Self::new(FailureCategory::Internal, context)
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.category, self.context)
    }
}

impl std::error::Error for AgentError {}

pub type AgentResult<T> = Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_set_matches_data_model() {
        // data-model.md: NoNetwork, Auth, BackendError, FileLocked
        for c in [
            FailureCategory::NoNetwork,
            FailureCategory::Auth,
            FailureCategory::BackendError,
            FailureCategory::FileLocked,
        ] {
            assert!(c.retryable_next_cycle(), "{c} should retry next cycle");
        }
        for c in [
            FailureCategory::RejectedBadRequest,
            FailureCategory::TooLarge,
            FailureCategory::FileAbsent,
        ] {
            assert!(!c.retryable_next_cycle(), "{c} should NOT be blindly retried");
        }
    }

    #[test]
    fn blocked_reasons_are_the_four_wire_values() {
        assert_eq!(FailureCategory::FileLocked.blocked_reason(), Some("locked"));
        assert_eq!(FailureCategory::FileAbsent.blocked_reason(), Some("absent"));
        assert_eq!(FailureCategory::TooLarge.blocked_reason(), Some("too_large"));
        assert_eq!(
            FailureCategory::RejectedBadRequest.blocked_reason(),
            Some("rejected")
        );
        assert_eq!(FailureCategory::NoNetwork.blocked_reason(), None);
    }
}
