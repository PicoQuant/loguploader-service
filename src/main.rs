//! pquploader — PicoQuant v2 device telemetry + configuration-backup agent.
//!
//! Thin shell: all logic lives in the `pquploader` library (`src/lib.rs`) so the
//! integration tests in `tests/` can exercise it. See
//! `specs/002-v2-config-backup-telemetry/` for the full contract.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    pquploader::cli::run(args)
}
