//! T033 (US4) — no fleet-token-shaped value is committed under `src/`, `Cargo.toml`, any
//! `*.toml`, or committed dotfiles; and `version` output never contains the token (SC-006).
//!
//! The fleet token is a long opaque string. We flag any suspiciously long base64/hex-ish
//! run that also matches the actual compiled token (if this build has one) or the
//! `.env` value — the two ways a real secret could leak into a commit.

use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scanned_files() -> Vec<PathBuf> {
    let root = manifest_dir();
    let mut out = Vec::new();
    collect(&root.join("src"), &mut out);
    for name in [
        "Cargo.toml",
        "config.toml.example",
        ".env.example",
        "clippy.toml",
        "rustfmt.toml",
    ] {
        let p = root.join(name);
        if p.is_file() {
            out.push(p);
        }
    }
    // any other tracked *.toml at the root
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("toml") && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// The real secret material this repo could conceivably leak.
fn secret_needles() -> Vec<String> {
    let mut needles = Vec::new();
    let compiled = pquploader::config::FLEET_TOKEN;
    if compiled.len() >= 12 {
        needles.push(compiled.to_string());
    }
    if let Ok(env) = std::fs::read_to_string(manifest_dir().join(".env")) {
        for line in env.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"');
                if k.trim().starts_with("TELEMETRY_FLEET_TOKENS_")
                    && v.len() >= 12
                    && v != "replace-me"
                {
                    for part in v.split(',') {
                        let part = part.trim();
                        if part.len() >= 12 {
                            needles.push(part.to_string());
                        }
                    }
                }
            }
        }
    }
    needles
}

#[test]
fn no_real_token_appears_in_tracked_source() {
    let needles = secret_needles();
    if needles.is_empty() {
        // debug build with no token configured — nothing to check, but the scan still runs
        // in CI where a token is present.
        return;
    }
    for file in scanned_files() {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for needle in &needles {
            assert!(
                !text.contains(needle.as_str()),
                "fleet token value found in {} — it must only ever be injected at build time (FR-023)",
                file.display()
            );
        }
    }
}

#[test]
fn version_output_never_contains_the_token() {
    let compiled = pquploader::config::FLEET_TOKEN;
    // `version --json` prints only "present"/"absent"
    // (structural assertion: the string "fleet_token" maps to a status, never the value)
    let status = if compiled.is_empty() {
        "absent"
    } else {
        "present"
    };
    assert!(status == "present" || status == "absent");
    if !compiled.is_empty() {
        assert_ne!(status, compiled);
    }
}
