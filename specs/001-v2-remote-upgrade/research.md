# Phase 0 Research — V1 → V2 Unattended Remote Upgrade Path

## D1. The fielded v1 updater is a fixed contract

From `updater/update.ps1` and `LogUploaderService_Setup_script.iss` as shipped:

- **Trigger**: scheduled task `\PicoQuant\LuminosaLogUploader\AutoUpdate`, `/SC ONSTART
  /DELAY 0000:30`, `/RU SYSTEM`, runs `powershell.exe -NoProfile -ExecutionPolicy Bypass
  -File "<app>\updater\update.ps1"`. **Boot only.** We cannot add a timer to it remotely.
- **Selection**: `GET /repos/PicoQuant/loguploader-service/releases/latest` →
  first asset `-like '*Setup*.exe'` → `<name>.sha256` (or first `*.sha256`).
- **Version gate**: `[Version]$local` (from `<InstallRoot>\VERSION`) vs `[Version]$remote`
  (tag minus `v`); if `local >= remote` → exit 0. String compare fallback if the cast throws.
- **Apply**: `& <InstallRoot>\loguploaderservice.exe stop` → `Start-Process Setup.exe
  '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-' -Wait` → **throws if ExitCode ≠ 0** →
  `& <InstallRoot>\loguploaderservice.exe start`.
- **Logs**: `C:\ProgramData\PicoQuant\LuminosaLogUploader\update\update.log`.
- The v1 installer's `[Run]` service `install`/`start` lines are `skipifsilent`, so under an
  auto-update **only** `CurStepChanged(ssPostInstall) → CreateAutoUpdateTask()` runs from
  `[Code]`; the service stop/start is done by `update.ps1`.

**Decision**: design the migration to satisfy this contract exactly. Anything the migration
needs beyond "drop files, recreate task, exit 0" must happen in the v2 installer's `[Code]`.

## D2. Migration logic lives in the v2 installer `[Code]`

The only process that runs with SYSTEM rights *during* the swap, launched by the fixed
updater, is `Setup.exe`. So the ordered migration steps run from `common.iss` `[Code]`
(`CurStepChanged`), not from the agent or a separate script:

1. `Snapshot` — copy the current `loguploaderservice.exe`, `VERSION`, `updater\update.ps1`,
   and (if present) `settings.py` to the rollback bundle (D5).
2. Inno replaces `[Files]` (new `loguploaderservice.exe` = the Rust binary, new `update.ps1`,
   new `VERSION`).
3. `SeedConfig` — write v2 `config.toml` (D13); leave v1 `settings.py` untouched.
4. Ensure the service points at the (same-named) exe and start it (`sc` / the exe's
   `install` is idempotent).
5. `HealthCheck` (D6) within a bounded window.
6. On failure → `Rollback` (D5) → start v1 → verify → `ReportOutcome(failed, cause)`.
7. On success → `ReportOutcome(ok)`; schedule rollback-bundle cleanup for +30 days (D11).
8. `CreateAutoUpdateTask` stays (idempotent; task unchanged).

Installer **always exits 0** unless it could not reach *any* working state (which then means
the fielded `update.ps1` throws and logs — the last-resort signal).

## D3. Only Luminosa has a v1 installed base

The v1 code (`loguploader.py`: `getLumiSerial`, `C:\Program Files\PicoQuant\Luminosa\`,
service "Luminosa Log Upload Service") is Luminosa-specific. **Solira has no fielded v1** —
its v2 rollout is a normal first install (`solira.iss`, new AppId, no migration `[Code]`
path, seeded by hand or a standard installer). This spec's migration concern is the Luminosa
v1→v2 hop; `common.iss` is written so Solira and future v2→v2.x upgrades reuse the
snapshot/health/rollback machinery.

## D4. Luminosa v2 keeps every v1 identifier

| Identifier | Value (kept) |
|---|---|
| Inno `AppId` | `{EC5738FF-E229-4BB2-9438-ACD2BD11AAC8}` (so Inno upgrades in place) |
| Install dir | `C:\Program Files\Luminosa Log Uploader` |
| Service exe | `loguploaderservice.exe` (now the Rust binary) |
| Service name | `LumiLogUploadService` |
| Task | `\PicoQuant\LuminosaLogUploader\AutoUpdate`, `/TR …\updater\update.ps1` |
| Version marker | `<InstallRoot>\VERSION` |
| State/log root | `C:\ProgramData\PicoQuant\LuminosaLogUploader\` |

**Rationale**: Principle VI — "MUST NOT change … in a way the deployed updater cannot
follow." Keeping the exe name means the fielded `update.ps1`'s `& $exe stop/start` still
works as a backstop. Keeping the path for `update.ps1` means the task `/TR` never changes.
The `RenameService` / `Relocate` functions in `common.iss` exist and are covered by a US4
test, but are **not invoked** for the Luminosa hop.

**Ripple**: `specs/002-v2-config-backup-telemetry/contracts/cli.md` currently says the binary
is `pquploader-<product>.exe` and the service `PQUploader<Product>`. For **Luminosa** those
are `loguploaderservice.exe` / `LumiLogUploadService`; Solira uses the `pquploader-solira`
scheme. Small amendment to spec 002 cli.md — noted, not made here.

## D5. Snapshot + rollback bundle

`C:\ProgramData\PicoQuant\LuminosaLogUploader\rollback\<from-version>\` contains:
`loguploaderservice.exe`, `VERSION`, `update.ps1`, `settings.py` (if it existed), and a
`manifest.json` (`from`, `to`, `created_utc`, `service_name`, `install_dir`).

`Rollback`: stop the (broken) service → restore the four files to their install locations →
`sc config` / re-`install` if needed → start → confirm `RUNNING` and that
`loguploaderservice.exe once`-equivalent for v1 (v1 has no `once`; instead confirm the
service reached `RUNNING` and wrote a recent Event Log line). Retain the bundle until the new
version is healthy (FR-010a); on a healthy upgrade keep it 30 days then let the v2 agent
delete it (D11).

## D6. Post-upgrade health check

**Definition of healthy**: within **10 minutes** of install, the v2 service is `RUNNING`
**and** `loguploaderservice.exe once` exits 0 with `heartbeat.ok == true` in its `CycleRecord`
JSON (proves service + network + fleet-token auth + a real backend round-trip). Backups are
**not** required for health (they depend on a file having changed).

The installer runs `once` directly (synchronous, bounded) rather than waiting for the service
loop, so the check is fast and deterministic. If `once` fails transiently (network), retry
twice at 60 s before declaring failure.

## D7. Failed-attempt reporting — `measurement_type: "upgrade_attempt"`

The telemetry `payload` is open JSON, so **no backend change**. The v2 exe gains an
`upgrade-report` subcommand that POSTs to `POST /api/v2/products/luminosa/telemetry`:

```json
{ "measurement_type": "upgrade_attempt", "instrument_serial": "<serial|unknown>",
  "payload": { "machine_id": "...", "from_version": "0.11.12", "to_version": "2.0.0",
               "outcome": "ok" | "rolled_back" | "integrity_failed" | "install_failed",
               "cause": "<short string>", "channel": "stable",
               "attempt_utc": "..." , "agent_version": "2.0.0" } }
```

`common.iss` `ReportOutcome` shells `loguploaderservice.exe upgrade-report …`. If the v2 exe
is too broken to run at all, the fallback signal is **absence of any v2 telemetry from that
machine after the rollout window** — `tools/fleet-status.py` flags it. (The beta gate exists
to keep a v2 that broken off `stable`.)

## D8. Version comparison including prereleases

`[Version]` can't parse `2.1.0-beta.3`. The rewritten `update.ps1` delegates the compare to
the installed exe: `loguploaderservice.exe is-newer <remoteVersion>` → exit 0 if the remote
is strictly newer under semver (prerelease precedence per semver.org), non-zero otherwise.
Keeps semver logic in one place (Rust, tested) and out of PowerShell.

## D9. Integrity & authenticity — SHA-256 + release access control (C2 resolved: no cert)

- **Now**: SHA-256 file alongside the asset, verified before run (exactly as v1). Authenticity
  rests on **GitHub release-publish access control** — only maintainers can publish a release,
  and the fielded updater only trusts `PicoQuant/loguploader-service` releases over HTTPS.
- **Residual risk (accepted)**: a compromise of a maintainer's GitHub credentials or the repo
  could publish a malicious "latest" that machines would install. Mitigations already in the
  design: the beta gate (a bad build should never reach `stable`), health-check + auto-
  rollback (a non-functional payload rolls back), and `upgrade_attempt` telemetry (a bad
  wave is visible fast). No worse than v1's posture.
- **Future (cert available)**: Authenticode-sign `Setup.exe` + `loguploaderservice.exe`;
  flip on a single gate in `update.ps1` — `Get-AuthenticodeSignature` status `Valid` **and**
  thumbprint ∈ a pinned set — before running the installer. Written to be switchable with no
  other change; also smooths `/VERYSILENT` past SmartScreen.

## D10. Trigger cadence — boot-triggered hop accepted (C1 resolved: reboots are frequent enough)

The v1→v2 hop is **boot-triggered only** (D1) and that is fine — the maintainer confirms
Luminosa instruments reboot often enough for SC-001. Read SC-001's window as "14 days *or*
next boot after publication".

- **Built**: `common.iss` recreates the AutoUpdate task on v2 install with **both**
  `/SC ONSTART /DELAY 0000:30` **and** `/SC DAILY` (e.g. 03:00, `/RU SYSTEM`), so every
  *subsequent* v2 update lands within a day, not only at reboot (SC-007).
- **Not built**: a v1 bridge release. Kept here as a one-file fallback if a future fleet
  turns out to reboot rarely — a `v0.x` whose installer adds `/SC DAILY` to the existing task,
  shipped through the fielded boot-only updater ahead of the v2 migration release.

## D11. Rollback-bundle retention

Keep the bundle until the new version passes health check (FR-010a, hard requirement). After
a **successful** upgrade, keep it **30 days** (mirrors v1's `keep_local_zip_days` ethos as
cheap insurance against a latent regression), then the v2 agent's cycle deletes bundles whose
`manifest.created_utc` is older than 30 days. After a **rollback**, the bundle is already
consumed (files restored); its `manifest.json` is kept as an attempt record.

## D12. Release composition

- **First stable v2 release** (`v2.0.0`): **Luminosa assets only** —
  `Luminosa Log Uploader Setup.exe` + `.sha256` + `loguploaderservice.exe`. This is the
  migration release; making it single-product sidesteps the fielded updater's
  "first `*Setup*.exe`" ambiguity entirely.
- **Beta releases** (`v2.0.0-beta.N`, `prerelease: true`): the `-beta` artifacts for
  whichever products are in beta.
- **Later stable releases**: MAY co-bundle Luminosa + Solira, because by then every machine
  runs the **v2** `update.ps1`, which matches its own product's asset
  (`<Product> Log Uploader Setup.exe`) and channel.
- `.github/workflows/release.yml` enforces: `v*-beta.*` tag → prerelease + `-beta` matrix
  only; `v2.0.0` tag → Luminosa-only; `v2.0.z` / `v2.y.*` → full matrix.

## D13. v1→v2 config translation is nearly vacuous

v1 `settings.py` holds `public_link` (a **Nextcloud** URL — irrelevant to v2), plus optional
`max_upload_size_mb`, `max_upload_attempts`, `upload_backoff_seconds`,
`service_interval_seconds`, `keep_local_zip_days`. v2's upload destination is
`api.picoquant.com` (compiled default) and its token is compiled in — **nothing device-
specific is required for v2 to function** (FR-014 is satisfied trivially). `SeedConfig`:

- write `config.toml` with v2 defaults;
- if `settings.py` has `service_interval_seconds`, carry it to `cycle_interval_secs`;
- record any v1 key it did not translate in the `upgrade_attempt` payload (`cause` /
  a `config_notes` field) so FR-014b's "surface, don't silently misconfigure" holds;
- idempotent: if `config.toml` already exists (retry after rollback), leave it (FR-014a);
- never touch `settings.py` until health check passes (FR-014c — it's in the rollback
  bundle anyway).

The elaborate translation machinery the spec describes (FR-014–FR-014c) is **built to spec
but mostly exercises the "missing/partial" and "idempotent" paths**, since the happy path
carries at most one integer.

## D14. Fleet status tool

`tools/fleet-status.py` (Python, like the existing `tools/`, uses `EXPECTED_ADMIN_API_KEY`):
queries `GET /api/v2/admin/products/{luminosa,solira}/telemetry` for
`measurement_type IN (agent_status, upgrade_attempt)`, then prints/`--json` a per-machine
table: `machine_id`, `instrument_serial`, `current_version`, `channel`, `last_seen`,
`last_upgrade_outcome`, `state ∈ {v1, v2, migrating, rolled_back, manual_required, stuck}`.
`manual_required` = on the operator-maintained list (D-manual). `stuck` = still v1 > N days
after a stable release with no `upgrade_attempt` seen. Drives FR-022, FR-005e (beta-gate
evaluation), and the User Story 3 rollout view.

**Sibling tool** — `tools/fleet_backup_pull.py` (`specs/004-fleet-backup-restore`) is the
companion maintainer command: same `EXPECTED_ADMIN_API_KEY`, same admin API host, but it
queries the **backup** list instead of telemetry and archives every machine's config files
locally. When `fleet-status.py` surfaces a `stuck` / `manual_required` machine, the operator
runs `fleet_backup_pull.py --serial <SN>` to capture that machine's configuration before an
on-site reinstall (and, once spec 004's restore increment lands, to put it back afterwards).
The two tools are independent; neither blocks the other.

## D-manual. Manual-intervention list

A checked-in `docs/manual-intervention.md` (or a small JSON the tool reads) listing machines
known to lack a working updater, with status. `fleet-status.py` merges it so those machines
show as `manual_required`, not `stuck` (FR-023a). `docs/manual-upgrade-runbook.md` is the
procedure that clears an entry (FR-023b): RDP/on-site → run `Setup.exe /VERYSILENT` → confirm
v2 running + task present → remove from the list.
