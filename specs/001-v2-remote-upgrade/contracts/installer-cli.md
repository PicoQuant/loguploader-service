# Contract — v2 installer (`installer/v2/*.iss`, Inno Setup 6)

Built per `(product, channel)`. `ISCC.exe /DPQ_CHANNEL=beta` sets
`OutputBaseFilename = "<Product> Log Uploader Beta Setup"` (else `"… Setup"`).
Luminosa keeps `AppId {EC5738FF-E229-4BB2-9438-ACD2BD11AAC8}`, dir
`C:\Program Files\Luminosa Log Uploader`, exe `loguploaderservice.exe`, service
`LumiLogUploadService` (research D4).

## Invocation

Only ever run silently by the updater:

```
Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-
```

Interactive run is allowed (support/manual runbook) but the `[Run]` service lines stay
`skipifsilent`; the migration `[Code]` runs in **both** modes.

## `[Code]` sequence (`common.iss`, driven from `CurStepChanged`)

| Step | Function | Must guarantee |
|---|---|---|
| `ssInstall` (before files) | `Snapshot` | rollback bundle (`data-model.md`) complete on disk before any file is replaced (FR-007, FR-010a). Abort with exit 0 + `install_failed` telemetry if the bundle can't be written (disk full) — old version untouched. |
| `ssPostInstall` | `SeedConfig` | `config.toml` written (idempotent, FR-014a); v1 `settings.py` untouched (FR-014c) |
| `ssPostInstall` | `EnsureService` | service registered/pointed at `loguploaderservice.exe` and started; `CreateAutoUpdateTask` (idempotent; adds `/SC DAILY` alongside `/SC ONSTART` for v2) |
| `ssPostInstall` | `HealthCheck` | within 10 min: service `RUNNING` **and** `loguploaderservice.exe once` → exit 0, `heartbeat.ok == true` (research D6); 2 retries at 60 s for transient network |
| on `HealthCheck` fail | `Rollback` | restore the 4 bundle files, re-register/point the service at the old exe, start it, confirm `RUNNING`; `manifest.outcome = "rolled_back"` (FR-010, FR-010b, FR-014c) |
| always (terminal) | `ReportOutcome` | shell `loguploaderservice.exe upgrade-report <outcome> <cause>` → one `measurement_type: upgrade_attempt` POST (research D7). Best-effort; failure to report is logged, not fatal. |
| deferred | rollback-bundle cleanup | on success, mark bundle for +30-day deletion by the v2 agent (research D11) |

## Exit codes

| Code | Meaning | Updater reaction |
|---|---|---|
| `0` | Reached a working state — **either** v2 healthy **or** rolled back to a working previous version | proceed normally (step 10) |
| `≠ 0` | Could **not** reach any working state (e.g. rollback itself failed — `rollback_failed`, a true Sev-1) | updater step 9: restart whatever exe is on disk, Event Log error, exit 1 |

The installer MUST NOT exit non-zero merely because the *upgrade* failed — a clean rollback
to a working previous version is a **success exit (0)** with a `rolled_back` telemetry record.

## Idempotency & interruption (FR-008, FR-012, FR-013)

- A second `Setup.exe` for a version already installed makes no changes (`VERSION` already ≥,
  `config.toml` already present, service already correct).
- `Snapshot` into an existing `rollback\<from>\` overwrites cleanly.
- **Self-heal on boot**: `common.iss` also installs a tiny one-shot check (a `RunOnce` /
  a guard in `update.ps1`) — if `VERSION` says v2 but the v2 service is absent/won't start
  and a `rollback\` bundle exists, restore it. Covers power loss between "old removed" and
  "new running".
- Overlap: a machine-wide mutex (`Global\PQLuminosaUpgrade`) — a second instance waits or
  no-ops.

## Solira (`solira.iss`)

New `AppId`, dir `C:\Program Files\Solira Log Uploader`, exe `pquploader-solira[-beta].exe`,
service `PQUploaderSolira`. No `Snapshot`/`Rollback` migration path on **first** install
(there is no previous version); the same `common.iss` health-check + rollback applies to
Solira v2→v2.x upgrades.
