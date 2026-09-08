//! Outbound HTTP to `api.picoquant.com` — the client side of `contracts/backend-api.md`.
//!
//! Blocking `ureq` + rustls (`webpki-roots` trust anchors, so TLS behaviour does not depend
//! on the machine's cert-store patch state — research D3). Only ever sends
//! `X-TELEMETRY-TOKEN`, never the admin key (FR-020).

use std::time::Duration;

use serde_json::Value;

use crate::config::{self, Config};
use crate::error::{AgentError, FailureCategory};
use crate::multipart::MultipartBody;

const CONNECT_TIMEOUT_SECS: u64 = 10;
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_SECS: [u64; 2] = [2, 4];

pub struct Api {
    agent: ureq::Agent,
    cfg: Config,
}

/// Parsed `200` body from either endpoint (fields we care about).
#[derive(Debug, Default)]
pub struct OkResponse {
    pub id: Option<String>,
    pub deduplicated: bool,
}

impl Api {
    pub fn new(cfg: &Config) -> Api {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout_read(Duration::from_secs(cfg.http_timeout_secs))
            .timeout_write(Duration::from_secs(cfg.http_timeout_secs))
            .user_agent(concat!("pquploader/", env!("PQ_VERSION")))
            .build();
        Api {
            agent,
            cfg: cfg.clone(),
        }
    }

    /// `POST /api/v2/products/{bucket}/telemetry` (JSON). `envelope` is the full request body
    /// (`measurement_type`, `measured_at`, `instrument_serial`, `payload`).
    pub fn post_heartbeat(&self, envelope: &Value) -> Result<OkResponse, AgentError> {
        let url = self.cfg.endpoint("telemetry");
        let body = serde_json::to_vec(envelope)
            .map_err(|e| AgentError::internal(format!("serialize heartbeat: {e}")))?;
        self.send_with_retry(&url, "application/json", &body, "heartbeat")
    }

    /// `POST /api/v2/products/{bucket}/backup` (multipart/form-data).
    pub fn post_backup(&self, body: MultipartBody) -> Result<OkResponse, AgentError> {
        let url = self.cfg.endpoint("backup");
        let (bytes, content_type) = body.finish();
        self.send_with_retry(&url, &content_type, &bytes, "backup")
    }

    fn send_with_retry(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        what: &str,
    ) -> Result<OkResponse, AgentError> {
        let mut last: Option<AgentError> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            match self.send_once(url, content_type, body) {
                Ok(ok) => return Ok(ok),
                Err(err) => {
                    let retry = should_retry_in_cycle(err.category) && attempt < MAX_ATTEMPTS;
                    log::warn!(
                        "{what} attempt {attempt}/{MAX_ATTEMPTS} failed: {err}{}",
                        if retry { " (will retry)" } else { "" }
                    );
                    last = Some(err);
                    if retry {
                        std::thread::sleep(Duration::from_secs(BACKOFF_SECS[(attempt - 1) as usize]));
                        continue;
                    }
                    break;
                }
            }
        }
        Err(last.unwrap_or_else(|| AgentError::internal("no attempts made")))
    }

    fn send_once(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<OkResponse, AgentError> {
        let req = self
            .agent
            .post(url)
            .set("X-TELEMETRY-TOKEN", config::FLEET_TOKEN)
            .set("Content-Type", content_type);

        match req.send_bytes(body) {
            Ok(resp) => parse_ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let detail = resp
                    .into_string()
                    .unwrap_or_default()
                    .chars()
                    .take(300)
                    .collect::<String>();
                Err(AgentError::new(
                    category_for_status(code),
                    format!("HTTP {code}: {detail}"),
                ))
            }
            Err(ureq::Error::Transport(t)) => Err(AgentError::new(
                FailureCategory::NoNetwork,
                format!("transport error: {t}"),
            )),
        }
    }
}

fn parse_ok(resp: ureq::Response) -> Result<OkResponse, AgentError> {
    let text = resp
        .into_string()
        .map_err(|e| AgentError::new(FailureCategory::BackendError, format!("read body: {e}")))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(OkResponse {
        id: v.get("id").and_then(Value::as_str).map(str::to_string),
        deduplicated: v
            .get("deduplicated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `contracts/backend-api.md` status → client category.
pub fn category_for_status(code: u16) -> FailureCategory {
    match code {
        401 => FailureCategory::Auth,
        413 => FailureCategory::TooLarge,
        400 | 404 | 422 => FailureCategory::RejectedBadRequest,
        500..=599 => FailureCategory::BackendError,
        _ => FailureCategory::BackendError,
    }
}

/// Only `NoNetwork` and `BackendError` are retried *within* a cycle (research D3).
/// A 4xx is never retried in-cycle; `Auth` waits for the next cycle / a new build.
pub fn should_retry_in_cycle(category: FailureCategory) -> bool {
    matches!(
        category,
        FailureCategory::NoNetwork | FailureCategory::BackendError
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_matches_contract() {
        assert_eq!(category_for_status(401), FailureCategory::Auth);
        assert_eq!(category_for_status(413), FailureCategory::TooLarge);
        assert_eq!(category_for_status(422), FailureCategory::RejectedBadRequest);
        assert_eq!(category_for_status(404), FailureCategory::RejectedBadRequest);
        assert_eq!(category_for_status(400), FailureCategory::RejectedBadRequest);
        assert_eq!(category_for_status(503), FailureCategory::BackendError);
    }

    #[test]
    fn only_network_and_backend_retry_in_cycle() {
        assert!(should_retry_in_cycle(FailureCategory::NoNetwork));
        assert!(should_retry_in_cycle(FailureCategory::BackendError));
        assert!(!should_retry_in_cycle(FailureCategory::Auth));
        assert!(!should_retry_in_cycle(FailureCategory::RejectedBadRequest));
        assert!(!should_retry_in_cycle(FailureCategory::TooLarge));
    }
}
