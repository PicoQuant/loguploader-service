//! Build script: injects the compile-time constants the agent needs and stamps the
//! Windows version resource. Mirrors v1's `PUBLIC_LINK` pattern (Constitution II).
//!
//! Inputs (env, set by CI or a local gitignored `.env`):
//!   PQ_PRODUCT                        -> `luminosa` | `solira`            (required)
//!   PQ_CHANNEL                        -> `stable` | `beta` (default `stable`)
//!   TELEMETRY_FLEET_TOKENS_<PRODUCT>  -> comma-separated token list; first entry is used
//!   (repo file) VERSION              -> agent version string
//!
//! Emits (consumed with `env!` in `src/config.rs`):
//!   PQ_PRODUCT, PQ_CHANNEL, PQ_FLEET_TOKEN, PQ_VERSION
//!
//! Rules (FR-002b, FR-002e, FR-024):
//!   * a *release* build hard-`panic!`s on a missing/unknown product, an invalid channel,
//!     or an empty token.
//!   * a debug build tolerates placeholders (empty token allowed — the agent just logs
//!     `Auth` failures) so `cargo test` / `cargo check` work with no secrets present.
//!   * the token value is NEVER echoed to build stdout/stderr or written to any emitted file.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let profile = env::var("PROFILE").unwrap_or_default();
    let is_release = profile == "release";

    load_dotenv();

    // Re-run triggers.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=VERSION");
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-env-changed=PQ_PRODUCT");
    println!("cargo:rerun-if-env-changed=PQ_CHANNEL");
    println!("cargo:rerun-if-env-changed=TELEMETRY_FLEET_TOKENS_LUMINOSA");
    println!("cargo:rerun-if-env-changed=TELEMETRY_FLEET_TOKENS_SOLIRA");

    // ---- product -----------------------------------------------------------
    let product_raw = env::var("PQ_PRODUCT").unwrap_or_default();
    let product = product_raw.trim().to_ascii_lowercase();
    let product = match product.as_str() {
        "luminosa" | "solira" => product,
        "" if !is_release => "luminosa".to_string(),
        "" => panic!("PQ_PRODUCT is required for a release build (luminosa|solira)"),
        other if !is_release => {
            println!("cargo:warning=unknown PQ_PRODUCT '{other}', defaulting to luminosa for this debug build");
            "luminosa".to_string()
        }
        other => panic!("PQ_PRODUCT '{other}' is not a known product (luminosa|solira)"),
    };

    // ---- channel ----------------------------------------------------------
    let channel_raw = env::var("PQ_CHANNEL").unwrap_or_default();
    let channel = channel_raw.trim().to_ascii_lowercase();
    let channel = match channel.as_str() {
        "" => "stable".to_string(),
        "stable" | "beta" => channel,
        other if !is_release => {
            println!("cargo:warning=invalid PQ_CHANNEL '{other}', defaulting to stable for this debug build");
            "stable".to_string()
        }
        other => panic!("PQ_CHANNEL '{other}' is invalid (stable|beta)"),
    };

    // ---- fleet token ----------------------------------------------------
    // First comma-separated entry of TELEMETRY_FLEET_TOKENS_<PRODUCT> (FR-024).
    let token_var = format!("TELEMETRY_FLEET_TOKENS_{}", product.to_ascii_uppercase());
    let token_list = env::var(&token_var).unwrap_or_default();
    let token = token_list
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();

    if token.is_empty() {
        if is_release {
            panic!("{token_var} is empty — a release build must carry a fleet token (FR-024)");
        } else {
            println!(
                "cargo:warning={token_var} not set — debug build will run with no token and log Auth failures"
            );
        }
    }
    // Never print the token itself.

    // ---- version --------------------------------------------------------
    let version = read_version();

    // ---- emit ----------------------------------------------------------
    println!("cargo:rustc-env=PQ_PRODUCT={product}");
    println!("cargo:rustc-env=PQ_CHANNEL={channel}");
    println!("cargo:rustc-env=PQ_FLEET_TOKEN={token}");
    println!("cargo:rustc-env=PQ_VERSION={version}");

    emit_version_resource(&product, &channel, &version);
}

/// Minimal `.env` loader (KEY=VALUE, `#` comments, optional surrounding quotes).
/// Does not override a variable already present in the environment (CI wins).
fn load_dotenv() {
    let path = Path::new(".env");
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if k.is_empty() {
            continue;
        }
        if env::var_os(k).is_none() {
            // SAFETY: build scripts are single-threaded at this point.
            env::set_var(k, v);
        }
    }
}

fn read_version() -> String {
    let raw = fs::read_to_string("VERSION")
        .unwrap_or_else(|e| panic!("cannot read repo VERSION file: {e}"));
    let v = raw.trim().to_string();
    if v.is_empty() {
        panic!("VERSION file is empty");
    }
    v
}

fn emit_version_resource(product: &str, channel: &str, version: &str) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let product_title = match product {
        "solira" => "Solira",
        _ => "Luminosa",
    };
    let channel_suffix = if channel == "beta" { " Beta" } else { "" };
    let display = format!("PicoQuant {product_title}{channel_suffix} Log Uploader");

    // `filevers` needs 4 numeric parts; VERSION may be `X.Y` or `X.Y.Z` (+ prerelease tag).
    let core = version.split('-').next().unwrap_or(version);
    let mut parts: Vec<u16> = core
        .split('.')
        .filter_map(|p| p.parse::<u16>().ok())
        .collect();
    while parts.len() < 4 {
        parts.push(0);
    }

    let mut res = winresource::WindowsResource::new();
    res.set("CompanyName", "PicoQuant")
        .set("FileDescription", &display)
        .set("ProductName", &display)
        .set("FileVersion", version)
        .set("ProductVersion", version)
        .set("InternalName", "pquploader")
        .set("OriginalFilename", "pquploader.exe")
        .set_version_info(winresource::VersionInfo::FILEVERSION, pack(&parts))
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, pack(&parts));
    if let Err(e) = res.compile() {
        // Not fatal for a dev build without the resource toolchain.
        println!("cargo:warning=could not compile Windows version resource: {e}");
    }
}

fn pack(parts: &[u16]) -> u64 {
    ((parts[0] as u64) << 48)
        | ((parts[1] as u64) << 32)
        | ((parts[2] as u64) << 16)
        | (parts[3] as u64)
}
