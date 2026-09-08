//! `version` / `is-newer` / `upgrade-report` subcommands.
//!
//! `is-newer` + `upgrade-report` are consumed by the v1→v2 unattended upgrade
//! (`specs/001-v2-remote-upgrade`): the installer calls `is-newer` to decide whether to
//! apply a release and `upgrade-report` to POST a terminal `upgrade_attempt` outcome
//! (`specs/001-v2-remote-upgrade/contracts/upgrade-telemetry.schema.json`). Built here so
//! the v2 binary the updater invokes already has them.

use std::cmp::Ordering;
use std::process::ExitCode;

use serde_json::json;

use crate::api::Api;
use crate::config::{self, Config};
use crate::identity;

// ---- version --------------------------------------------------------------

pub fn print_version(as_json: bool) {
    let cfg = Config::load();
    if as_json {
        let v = json!({
            "product": config::PRODUCT,
            "channel": config::CHANNEL,
            "version": config::VERSION,
            "api_base_url": cfg.api_base_url,
            "fleet_token": if cfg.has_fleet_token() { "present" } else { "absent" },
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!("product:       {}", config::PRODUCT);
        println!("channel:       {}", config::CHANNEL);
        println!("version:       {}", config::VERSION);
        println!("api_base_url:  {}", cfg.api_base_url);
        println!(
            "fleet_token:   {}",
            if cfg.has_fleet_token() {
                "present"
            } else {
                "absent"
            }
        );
    }
    // SC-006: never print the token value itself.
}

// ---- is-newer -----------------------------------------------------------

/// Exit 0 iff `remote` is a strictly newer semver than the compiled version.
pub fn is_newer_cli(remote: &str) -> ExitCode {
    match compare_semver(remote, config::VERSION) {
        Some(Ordering::Greater) => ExitCode::SUCCESS,
        Some(_) => ExitCode::from(1),
        None => {
            eprintln!(
                "is-newer: could not parse a version ('{remote}' vs '{}')",
                config::VERSION
            );
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SemVer {
    core: [u64; 3],
    pre: Vec<Prerelease>,
}

#[derive(Debug, PartialEq, Eq)]
enum Prerelease {
    Num(u64),
    Text(String),
}

fn parse_semver(s: &str) -> Option<SemVer> {
    let s = s.trim().trim_start_matches('v');
    let (core_str, pre_str) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    // ignore build metadata
    let core_str = core_str.split('+').next().unwrap_or(core_str);
    let mut core = [0u64; 3];
    for (i, part) in core_str.split('.').enumerate() {
        if i >= 3 {
            return None;
        }
        core[i] = part.parse().ok()?;
    }
    let pre = match pre_str {
        None => Vec::new(),
        Some(p) => p
            .split('+')
            .next()
            .unwrap_or(p)
            .split('.')
            .map(|id| match id.parse::<u64>() {
                Ok(n) => Prerelease::Num(n),
                Err(_) => Prerelease::Text(id.to_string()),
            })
            .collect(),
    };
    Some(SemVer { core, pre })
}

/// SemVer 2.0.0 precedence, including prerelease rules (a prerelease < its release).
fn compare_semver(a: &str, b: &str) -> Option<Ordering> {
    let (va, vb) = (parse_semver(a)?, parse_semver(b)?);
    Some(cmp_semver(&va, &vb))
}

fn cmp_semver(a: &SemVer, b: &SemVer) -> Ordering {
    match a.core.cmp(&b.core) {
        Ordering::Equal => {}
        other => return other,
    }
    match (a.pre.is_empty(), b.pre.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater, // 1.0.0 > 1.0.0-beta.1
        (false, true) => Ordering::Less,
        (false, false) => cmp_prerelease(&a.pre, &b.pre),
    }
}

fn cmp_prerelease(a: &[Prerelease], b: &[Prerelease]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let o = match (x, y) {
            (Prerelease::Num(m), Prerelease::Num(n)) => m.cmp(n),
            (Prerelease::Num(_), Prerelease::Text(_)) => Ordering::Less,
            (Prerelease::Text(_), Prerelease::Num(_)) => Ordering::Greater,
            (Prerelease::Text(m), Prerelease::Text(n)) => m.cmp(n),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

// ---- upgrade-report ---------------------------------------------------

/// `upgrade-report <outcome> [--cause C --from V --to V --health-ms N --config-note S]...`
pub fn upgrade_report_cli(args: &[String]) -> ExitCode {
    let Some(outcome) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("upgrade-report: missing <outcome>");
        return ExitCode::from(2);
    };
    const OUTCOMES: [&str; 6] = [
        "ok",
        "integrity_failed",
        "install_failed",
        "health_check_failed",
        "rolled_back",
        "rollback_failed",
    ];
    if !OUTCOMES.contains(&outcome.as_str()) {
        eprintln!("upgrade-report: outcome must be one of {OUTCOMES:?}");
        return ExitCode::from(2);
    }

    let mut cause: Option<String> = None;
    let mut from_version: Option<String> = None;
    let mut to_version: Option<String> = None;
    let mut health_ms: Option<i64> = None;
    let mut config_notes: Vec<String> = Vec::new();

    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--cause" => cause = it.next().cloned(),
            "--from" => from_version = it.next().cloned(),
            "--to" => to_version = it.next().cloned(),
            "--health-ms" => health_ms = it.next().and_then(|v| v.parse().ok()),
            "--config-note" => {
                if let Some(v) = it.next() {
                    config_notes.push(v.clone());
                }
            }
            other => {
                eprintln!("upgrade-report: unknown flag {other}");
                return ExitCode::from(2);
            }
        }
    }

    let cfg = Config::load();
    let machine_id = identity::machine_id();
    let payload = json!({
        "machine_id": machine_id,
        "from_version": from_version.unwrap_or_default(),
        "to_version": to_version.unwrap_or_else(|| config::VERSION.to_string()),
        "outcome": outcome,
        "cause": cause,
        "channel": config::CHANNEL,
        "attempt_utc": crate::cycle::now_rfc3339(),
        "agent_version": config::VERSION,
        "health_ms": health_ms,
        "config_notes": if config_notes.is_empty() { serde_json::Value::Null } else { json!(config_notes) },
    });
    let envelope = json!({
        "measurement_type": "upgrade_attempt",
        "measured_at": crate::cycle::now_rfc3339(),
        "instrument_serial": identity::instrument_serial(&cfg.product.serial_file()).wire_value(),
        "payload": payload,
    });

    crate::logging::init(true);
    match Api::new(&cfg).post_heartbeat(&envelope) {
        Ok(_) => {
            println!("upgrade-report: {outcome} recorded");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!(
                "upgrade-report: submit failed ({e}); the upgrade outcome itself is unaffected"
            );
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_precedence() {
        assert_eq!(compare_semver("2.0.0", "1.9.9"), Some(Ordering::Greater));
        assert_eq!(compare_semver("2.0.1", "2.0.1"), Some(Ordering::Equal));
        assert_eq!(compare_semver("2.0.0", "2.1.0"), Some(Ordering::Less));
    }

    #[test]
    fn prerelease_is_lower_than_release() {
        assert_eq!(
            compare_semver("2.0.0-beta.1", "2.0.0"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_semver("2.0.0", "2.0.0-beta.1"),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn prerelease_ordering() {
        assert_eq!(
            compare_semver("2.0.0-beta.2", "2.0.0-beta.10"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_semver("2.0.0-alpha", "2.0.0-beta"),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn v_prefix_and_two_component_versions() {
        assert_eq!(compare_semver("v2.1", "2.0.0"), Some(Ordering::Greater));
        assert_eq!(compare_semver("0.11", "0.11.0"), Some(Ordering::Equal));
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(compare_semver("not-a-version", "1.0.0"), None);
    }
}
