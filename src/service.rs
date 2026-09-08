//! Windows service integration (FR-028). SCM control handler + `install` / `uninstall`.
//!
//! Service name / display name / account are part of the migration surface
//! (`contracts/cli.md`); changes go through `specs/001-v2-remote-upgrade` (Constitution VI).

#![cfg(windows)]

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::logging;
use crate::product::Product;
use crate::run_loop;

const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

fn service_name() -> String {
    format!("PQUploader{}", Product::current().dir_name())
}

fn display_name() -> String {
    format!("PicoQuant {} Log Uploader", Product::current().dir_name())
}

fn description() -> String {
    "Submits device telemetry heartbeats and daily configuration-file backups to \
     api.picoquant.com. Device -> backend only."
        .to_string()
}

// ---- SCM entry point -------------------------------------------------------

define_windows_service!(ffi_service_main, service_main);

pub fn run_as_service() -> ExitCode {
    match service_dispatcher::start(service_name(), ffi_service_main) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "This program is a Windows service and must be started by the Service Control \
                 Manager.\nUse `pquploader install` then `sc start {}`, or `pquploader debug` \
                 to run it in the foreground.\n(dispatcher error: {e})",
                service_name()
            );
            ExitCode::from(1)
        }
    }
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run() {
        // Nowhere to return this to; the Event Log is the channel.
        logging::init(false);
        log::error!("service terminated with an error: {e}");
    }
}

fn run() -> windows_service::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));

    let handler_stop = stop.clone();
    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                handler_stop.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(service_name(), event_handler)?;

    let running = ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(running.clone())?;

    // Blocks until `stop` is set by the control handler.
    run_loop::serve(&stop);

    status_handle.set_service_status(ServiceStatus {
        current_state: ServiceState::StopPending,
        controls_accepted: ServiceControlAccept::empty(),
        wait_hint: Duration::from_secs(5),
        ..running.clone()
    })?;
    status_handle.set_service_status(ServiceStatus {
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        wait_hint: Duration::default(),
        ..running
    })?;
    Ok(())
}

// ---- install / uninstall -------------------------------------------------

pub fn install() -> ExitCode {
    match do_install() {
        Ok(()) => {
            println!("installed service {} ({})", service_name(), display_name());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("install failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn do_install() -> Result<(), String> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    // Idempotent: if it already exists, just refresh config + description.
    if let Ok(existing) = manager.open_service(
        service_name(),
        ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG,
    ) {
        let _ = existing.set_description(description());
        let _ = existing.set_delayed_auto_start(true);
        register_event_source_best_effort();
        return Ok(());
    }

    let info = ServiceInfo {
        name: OsString::from(service_name()),
        display_name: OsString::from(display_name()),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("run")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    let service = manager
        .create_service(&info, ServiceAccess::CHANGE_CONFIG)
        .map_err(|e| e.to_string())?;
    service
        .set_description(description())
        .map_err(|e| e.to_string())?;
    // auto-delayed start
    service
        .set_delayed_auto_start(true)
        .map_err(|e| e.to_string())?;

    register_event_source_best_effort();
    Ok(())
}

fn register_event_source_best_effort() {
    if let Err(e) = logging::register_event_source() {
        eprintln!("warning: could not register Event Log source: {e}");
    }
}

pub fn uninstall(purge: bool) -> ExitCode {
    match do_uninstall(purge) {
        Ok(()) => {
            println!(
                "removed service {}{}",
                service_name(),
                if purge { " and purged v2agent dir" } else { "" }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("uninstall failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn do_uninstall(purge: bool) -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| e.to_string())?;

    match manager.open_service(
        service_name(),
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    ) {
        Ok(service) => {
            if let Ok(status) = service.query_status() {
                if status.current_state != ServiceState::Stopped {
                    let _ = service.stop();
                    // give the SCM a moment; not fatal if it lingers
                    for _ in 0..25 {
                        std::thread::sleep(Duration::from_millis(200));
                        if let Ok(s) = service.query_status() {
                            if s.current_state == ServiceState::Stopped {
                                break;
                            }
                        }
                    }
                }
            }
            service.delete().map_err(|e| e.to_string())?;
        }
        Err(_) => {
            // already gone — idempotent
        }
    }

    let _ = logging::deregister_event_source();

    if purge {
        let dir = Product::current().agent_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
