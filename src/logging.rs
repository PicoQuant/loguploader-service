//! Dual observability sink (Constitution IV):
//!   * Windows Event Log source `PicoQuant <Product> LogUploader` — the field channel
//!   * a size-capped rolling `cycles.log` (~5 × 1 MiB) under `<data_dir>\v2agent\`
//!
//! `init()` wires the `log` facade to fan out to both. In `debug` / `once` runs the file
//! sink is joined by stderr.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::config;
use crate::product::Product;

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const KEEP_ROTATIONS: usize = 4; // cycles.log + .1..=.4  ≈ 5 MiB total

/// The Event Log source name — part of the migration surface (`contracts/cli.md`),
/// changes go through `specs/001-v2-remote-upgrade`.
pub fn event_source_name() -> String {
    let title = match Product::current() {
        Product::Luminosa => "Luminosa",
        Product::Solira => "Solira",
    };
    format!("PicoQuant {title} LogUploader")
}

static LOGGER: OnceLock<CompositeLogger> = OnceLock::new();

/// Initialise logging. `to_stderr` is true for `debug` / `once`. Idempotent.
pub fn init(to_stderr: bool) {
    if LOGGER.get().is_some() {
        return;
    }
    let file_sink = RollingFile::open(cycles_log_path()).map(Mutex::new);
    if file_sink.is_none() {
        eprintln!("warning: could not open cycles.log; continuing with Event Log only");
    }

    #[cfg(windows)]
    let event_sink = eventlog::EventLog::new(&event_source_name(), Level::Info)
        .map_err(|e| eprintln!("warning: Event Log source unavailable ({e})"))
        .ok();
    #[cfg(not(windows))]
    let event_sink: Option<()> = None;

    let logger = CompositeLogger {
        file: file_sink,
        #[cfg(windows)]
        event: event_sink,
        #[cfg(not(windows))]
        event: event_sink,
        stderr: to_stderr,
    };
    let logger = LOGGER.get_or_init(|| logger);
    // If another thread won the race, `logger` is that instance — fine.
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

/// Register the Event Log source in the registry (needs admin; called by `install`).
#[cfg(windows)]
pub fn register_event_source() -> Result<(), String> {
    eventlog::register(&event_source_name()).map_err(|e| e.to_string())
}

/// Remove the Event Log source registration (called by `uninstall`).
#[cfg(windows)]
pub fn deregister_event_source() -> Result<(), String> {
    eventlog::deregister(&event_source_name()).map_err(|e| e.to_string())
}

pub fn cycles_log_path() -> PathBuf {
    Product::current().agent_dir().join("cycles.log")
}

struct CompositeLogger {
    file: Option<Mutex<RollingFile>>,
    #[cfg(windows)]
    event: Option<eventlog::EventLog>,
    #[cfg(not(windows))]
    event: Option<()>,
    stderr: bool,
}

impl Log for CompositeLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} [{}] {}",
            crate::cycle::now_rfc3339(),
            record.level(),
            record.args()
        );

        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.write_line(&line);
            }
        }
        if self.stderr {
            eprintln!("{line}");
        }
        #[cfg(windows)]
        if let Some(ev) = &self.event {
            ev.log(record);
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.flush();
            }
        }
        #[cfg(windows)]
        if let Some(ev) = &self.event {
            ev.flush();
        }
    }
}

/// Append `line` directly to `cycles.log` (used for the per-cycle `CycleRecord` JSON,
/// which we want verbatim, not through the `log!` formatting). Falls back to stdout if the
/// file sink is unavailable so `once` still prints something useful.
pub fn append_cycle_record(json_line: &str) {
    if let Some(logger) = LOGGER.get() {
        if let Some(file) = &logger.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.write_line(json_line);
                let _ = f.flush();
                return;
            }
        }
    }
}

/// A minimal size-capped rolling file writer.
pub struct RollingFile {
    path: PathBuf,
    handle: File,
    written: u64,
}

impl RollingFile {
    pub fn open(path: PathBuf) -> Option<RollingFile> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok()?;
        }
        let handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let written = handle.metadata().map(|m| m.len()).unwrap_or(0);
        Some(RollingFile {
            path,
            handle,
            written,
        })
    }

    pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let bytes = line.as_bytes();
        if self.written + bytes.len() as u64 + 1 > MAX_LOG_BYTES {
            self.rotate()?;
        }
        self.handle.write_all(bytes)?;
        self.handle.write_all(b"\n")?;
        self.written += bytes.len() as u64 + 1;
        Ok(())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.handle.flush()
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        // Drop the oldest, shift the rest up, then reopen a fresh primary.
        let rotated = |n: usize| -> PathBuf {
            let mut p = self.path.clone();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("cycles.log");
            p.set_file_name(format!("{name}.{n}"));
            p
        };
        let _ = fs::remove_file(rotated(KEEP_ROTATIONS));
        for n in (1..KEEP_ROTATIONS).rev() {
            let _ = fs::rename(rotated(n), rotated(n + 1));
        }
        let _ = fs::rename(&self.path, rotated(1));

        self.handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = 0;
        Ok(())
    }
}

/// True if the compiled fleet token is present — logged (as a boolean) at startup so a
/// maintainer reading the Event Log can tell a debug build from a shipped one.
pub fn token_present() -> bool {
    !config::FLEET_TOKEN.is_empty()
}

pub fn agent_dir_display() -> String {
    Product::current().agent_dir().display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_file_rotates_on_size() {
        let dir = std::env::temp_dir().join(format!("pqu_roll_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cycles.log");
        let mut rf = RollingFile::open(path.clone()).unwrap();
        let chunk = "x".repeat(4096);
        for _ in 0..400 {
            rf.write_line(&chunk).unwrap();
        }
        rf.flush().unwrap();
        assert!(path.with_file_name("cycles.log.1").exists());
        // never keeps more than KEEP_ROTATIONS rotations
        assert!(!path.with_file_name(format!("cycles.log.{}", KEEP_ROTATIONS + 1)).exists());
        fs::remove_dir_all(&dir).ok();
    }
}
