//! Subcommand parsing + dispatch. `main.rs` is a one-line shell over `run`.

use std::process::ExitCode;

use crate::{cycle, run_loop, upgrade};

pub const USAGE: &str = "\
pquploader — PicoQuant v2 telemetry + config-backup agent

USAGE:
    pquploader <COMMAND>

COMMANDS:
    run                     Service entry point (invoked by the SCM). Runs the cycle loop.
    debug                   Run the cycle loop in the foreground; Ctrl-C to stop.
    once                    Run exactly one cycle, print the CycleRecord as JSON, exit.
    install                 Register the Windows service and Event Log source.
    uninstall [--purge]     Remove the service (and, with --purge, the local state dir).
    version [--json]        Print product / channel / version / backend / token presence.
    is-newer <remote>       Exit 0 iff <remote> is a strictly newer semver than this build.
    upgrade-report <outcome> [--cause C --from V --to V --health-ms N --config-note S]...
                            POST an upgrade_attempt telemetry record (v1->v2 installer).
";

pub fn run(args: Vec<String>) -> ExitCode {
    let cmd = args.first().map(String::as_str).unwrap_or("run");
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match cmd {
        "run" => dispatch_run(),
        "debug" => run_loop::run_foreground(),
        "once" => cycle::run_once_cli(),
        "install" => dispatch_install(),
        "uninstall" => dispatch_uninstall(&rest),
        "version" | "--version" | "-V" => {
            upgrade::print_version(rest.iter().any(|a| a == "--json"));
            ExitCode::SUCCESS
        }
        "is-newer" => match rest.first() {
            Some(remote) => upgrade::is_newer_cli(remote),
            None => {
                eprintln!("is-newer: missing <remote> version argument");
                ExitCode::from(2)
            }
        },
        "upgrade-report" => upgrade::upgrade_report_cli(&rest),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn dispatch_run() -> ExitCode {
    #[cfg(windows)]
    {
        crate::service::run_as_service()
    }
    #[cfg(not(windows))]
    {
        eprintln!("`run` is only supported on Windows");
        ExitCode::from(1)
    }
}

fn dispatch_install() -> ExitCode {
    #[cfg(windows)]
    {
        crate::service::install()
    }
    #[cfg(not(windows))]
    {
        eprintln!("`install` is only supported on Windows");
        ExitCode::from(1)
    }
}

fn dispatch_uninstall(args: &[String]) -> ExitCode {
    let purge = args.iter().any(|a| a == "--purge");
    #[cfg(windows)]
    {
        crate::service::uninstall(purge)
    }
    #[cfg(not(windows))]
    {
        let _ = purge;
        eprintln!("`uninstall` is only supported on Windows");
        ExitCode::from(1)
    }
}
