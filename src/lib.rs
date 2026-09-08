//! pquploader library surface — the agent's modules, exposed so the integration tests in
//! `tests/` can exercise them. The `pquploader` binary is a thin shell over `cli::run`.
//!
//! See `specs/002-v2-config-backup-telemetry/` for the full contract.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

pub mod api;
pub mod backup;
pub mod cli;
pub mod config;
pub mod cycle;
pub mod error;
pub mod identity;
pub mod logging;
pub mod multipart;
pub mod product;
pub mod run_loop;
pub mod state;
pub mod telemetry;
pub mod upgrade;
pub mod watchset;

#[cfg(windows)]
pub mod service;
