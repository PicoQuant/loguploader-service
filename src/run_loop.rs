//! The sleep/wake service loop (Constitution I, FR-029, FR-030, FR-031).
//!
//! * one `cycle::run_once` every `cycle_interval_secs`
//! * sleeps in ≤1 s steps so `SERVICE_CONTROL_STOP` is honoured promptly
//! * a process-local single-flight guard: cycles never overlap
//! * `catch_unwind` around every cycle body as the last-resort backstop — a panic is
//!   logged and the loop re-enters on the next interval, it never propagates out

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::cycle;
use crate::identity::Identity;
use crate::logging;
use crate::state::LocalBackupState;

/// Runs cycles until `stop` becomes true. Shared by the SCM path and `debug`.
pub fn serve(stop: &AtomicBool) {
    logging::init(is_console());
    let cfg = Config::load();
    let identity = Identity::resolve(cfg.product);

    log::info!(
        "service loop starting — interval {}s, agent_dir {}",
        cfg.cycle_interval_secs,
        logging::agent_dir_display()
    );

    // FR-031: only one cycle acts at a time. The loop is single-threaded so this is a
    // belt-and-braces guard against a future re-entrancy bug.
    let running = AtomicBool::new(false);

    // Run one cycle immediately, then on the interval.
    let mut next_run = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_run {
            run_guarded_cycle(&cfg, &identity, &running);
            next_run = Instant::now() + Duration::from_secs(cfg.cycle_interval_secs);
        }
        // responsive stop: nap in ≤1 s slices
        sleep_interruptible(Duration::from_secs(1), stop);
    }

    log::info!("service loop stopped");
    log::logger().flush();
}

fn run_guarded_cycle(cfg: &Config, identity: &Identity, running: &AtomicBool) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::warn!("previous cycle still running; skipping this tick (FR-031)");
        return;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut state = LocalBackupState::load();
        let _ = cycle::run_once(cfg, identity, &mut state);
    }));
    if result.is_err() {
        log::error!("cycle panicked at the top level; loop continues to the next interval");
    }

    running.store(false, Ordering::SeqCst);
}

fn sleep_interruptible(total: Duration, stop: &AtomicBool) {
    let step = Duration::from_millis(200);
    let mut slept = Duration::ZERO;
    while slept < total {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(step);
        slept += step;
    }
}

/// `pquploader debug` — run the same loop in the foreground; Ctrl-C stops it.
pub fn run_foreground() -> ExitCode {
    let stop = Arc::new(AtomicBool::new(false));
    install_ctrl_c(stop.clone());
    eprintln!("pquploader debug — running the cycle loop in the foreground; Ctrl-C to stop.");
    serve(&stop);
    ExitCode::SUCCESS
}

fn is_console() -> bool {
    // `debug` / `once` set stderr logging; the SCM path does not. We detect "started from a
    // console" cheaply: the SCM entry point never sets this env var, `run_foreground` does.
    std::env::var_os("PQ_FOREGROUND").is_some()
}

// ---- Ctrl-C (no extra crate) -----------------------------------------------

static CTRL_C_STOP: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

#[cfg(windows)]
fn install_ctrl_c(stop: Arc<AtomicBool>) {
    std::env::set_var("PQ_FOREGROUND", "1");
    let _ = CTRL_C_STOP.set(stop);

    // minimal kernel32 binding (win32 BOOL = i32, DWORD = u32)
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    unsafe extern "system" fn handler(_ctrl_type: u32) -> i32 {
        if let Some(s) = CTRL_C_STOP.get() {
            s.store(true, Ordering::Relaxed);
        }
        1 // handled
    }

    unsafe {
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(not(windows))]
fn install_ctrl_c(stop: Arc<AtomicBool>) {
    std::env::set_var("PQ_FOREGROUND", "1");
    let _ = CTRL_C_STOP.set(stop);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruptible_sleep_returns_early_when_stopped() {
        let stop = AtomicBool::new(true);
        let start = Instant::now();
        sleep_interruptible(Duration::from_secs(30), &stop);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn single_flight_guard_blocks_reentry() {
        let running = AtomicBool::new(false);
        assert!(running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok());
        // second attempt fails while "running"
        assert!(running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err());
    }
}
