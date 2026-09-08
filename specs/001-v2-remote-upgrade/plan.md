# Implementation Plan: V1 → V2 Unattended Remote Upgrade Path

**Branch**: `v2-specs` (feature dir `specs/001-v2-remote-upgrade`) | **Date**: 2026-09-08 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/001-v2-remote-upgrade/spec.md`

## Summary

Move every fielded **Luminosa** v1 machine onto v2 with nobody touching the machine, and never
leave a machine worse off. The v1 auto-updater on those machines is a **fixed contract** we
cannot change retroactively: a boot-triggered SYSTEM scheduled task runs
`{app}\updater\update.ps1`, which polls the GitHub *latest* release, grabs the first
`*Setup*.exe` + its `.sha256`, verifies the hash, stops the service, runs the installer
`/VERYSILENT`, restarts. The migration therefore rides in **as an ordinary v2 `Setup.exe`**
that the fielded updater installs — and the migration logic (snapshot → install → config seed
→ health check → automatic rollback on failure → report outcome) lives **inside that
installer's `[Code]` section**, plus a drop-in replacement `update.ps1` that all *subsequent*
v2 updates use (channel-aware, semver-correct, same rollback pattern).

Scope note: **v1 only ever ran on Luminosa** (the v1 code is Luminosa-specific). Solira has no
installed base — Solira v2 is a greenfield install, not a migration. This spec is about the
Luminosa v1→v2 hop and the v2→v2.x mechanism that follows it.

Deliverables live in this repo: `updater/update.ps1` (rewritten), `installer/v2/*.iss` (the
migration/upgrade installer with rollback), `docs/manual-upgrade-runbook.md`,
`tools/fleet-status.py`, and the `.github/workflows/release.yml` rules that make the **first
stable v2 release a Luminosa-only migration release**.

## Technical Context

**Languages**: PowerShell 5.1 (the updater — fixed by what's fielded), Inno Setup 6 Pascal
(the installer `[Code]` — matches v1), Python 3.10 (`tools/fleet-status.py` — matches existing
`tools/`). No new runtime on customer machines; PowerShell and `schtasks` are OS-present.

**Primary Dependencies**: GitHub Releases API (`/releases/latest`, `/releases`), `schtasks.exe`,
Windows SCM (`sc.exe` / the v2 exe's `install`/`uninstall`), the v2 agent binary from
`specs/002-v2-config-backup-telemetry` (its `once`, `version`, and a new `upgrade-report`
subcommand), and the PicoQuant telemetry endpoint (`POST /api/v2/products/luminosa/telemetry`,
open JSON payload — **no backend change**, `measurement_type: "upgrade_attempt"`).

**Storage**: files only. Rollback bundle + attempt records under
`C:\ProgramData\PicoQuant\LuminosaLogUploader\` (the dir the fielded updater already uses).
`VERSION` in the install dir stays the on-disk version marker.

**Testing**: a Windows VM with a real current-v1 install (service + scheduled task) → publish
a `-beta` v2 → let the task fire → assert migrated + uploading + rolled-back-on-injected-
failure. `Pester` for `update.ps1` unit tests; manual matrix in `quickstart.md` for the
installer `[Code]` paths.

**Target Platform**: Windows 10/11 x64, unattended, SYSTEM.

**Project Type**: single project — scripts + installer + one helper tool in this repo.

**Fixed constraints (the deployed v1 updater — cannot change for the v1→v2 hop)**:
- trigger: `\PicoQuant\LuminosaLogUploader\AutoUpdate`, `/SC ONSTART /DELAY 0000:30`, SYSTEM
- picks: first asset matching `*Setup*.exe`, then `<name>.sha256` (or first `*.sha256`)
- integrity: SHA-256 file compare only
- version compare: `[Version]` cast (numeric), string fallback
- install: `Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-`, must exit `0`
- it does `& <InstallRoot>\loguploaderservice.exe stop` before and `… start` after
- reads `<InstallRoot>\VERSION` to decide "am I current?"

**Constraints (chosen)**: Luminosa v2 keeps the v1 **AppId** `{EC5738FF-E229-4BB2-9438-ACD2BD11AAC8}`,
install dir `C:\Program Files\Luminosa Log Uploader`, exe name `loguploaderservice.exe`,
service `LumiLogUploadService`, and task path — so the migration surface is ~zero (see
research D4). The installer must exit `0` even when it self-rolled-back.

**Scale/Scope**: Luminosa fleet order 10²–10³. The migration installer + updater target
< ~1200 lines total.

**Clarifications resolved (maintainer, 2026-09-08)**:
- **C1 — reboot cadence: OK.** Luminosa instruments reboot often enough that the
  **boot-triggered** v1→v2 hop meets SC-001 without a v1 bridge release. SC-001's window is
  read as "14 days *or* next boot after publication". v2's `update.ps1` still adds a `/SC
  DAILY` trigger so every *subsequent* v2 update is prompt (SC-007). `installer/v1-bridge.iss`
  is **not** built; the approach stays documented in research D10 as a fallback only.
- **C2 — no code-signing certificate** (possibly later). Integrity/authenticity for the
  v1→v2 hop = **SHA-256 file compare + GitHub release-publish access control**, exactly as
  v1. `update.ps1` and the release workflow are written so an Authenticode check can be
  switched on later (one pinned-thumbprint gate) without other changes. Residual risk
  documented in research D9.

## Constitution Check

*GATE: Must pass before Phase 0. Re-check after Phase 1.* Constitution **v1.3.0**. Result: **PASS.**

This feature *is* the Principle VI feature. Bullet-by-bullet:

| Principle VI clause | How this plan satisfies it |
|---|---|
| No major version published until an unattended path from every deployed version is **tested on a real Windows install** | `quickstart.md` provisions a real current-v1 VM and drives the full hop; the **beta gate** (FR-005d: ≥7 days / ≥3 beta instruments / 0 Sev-1) precedes any stable cut. |
| Migration surface MUST NOT change in a way the deployed updater can't follow; if it must, the release is installable by the OLD updater and rewrites the updater + task itself | The Luminosa v2 installer **is** a `*Setup*.exe` + `.sha256` the fielded `update.ps1` installs. It keeps AppId / dir / exe name / service name / task (research D4) and **overwrites `update.ps1` in place** (same path → task `/TR` unchanged). The service-rename / relocate capability is built + tested (US4) but not triggered for Luminosa. |
| Idempotent; rolls forward safely if interrupted; a partial upgrade leaves a working service | Installer is transactional: snapshot first, health-check, auto-rollback; a boot-time self-heal check restores a working version if power was lost mid-swap. `update.ps1` no-ops when `VERSION ≥ remote`. |
| Losing remote-upgrade of a fielded machine = Sev-1 | Health-check-and-rollback (FR-010) + the beta gate + `measurement_type: upgrade_attempt` telemetry so a failed hop is visible fleet-wide. |

Other principles:
- **I (never crash the loop)** — same spirit: the updater and installer must never exit
  leaving the machine worse; every branch ends at a running, working service; all errors are
  caught and reported, never `throw`n out to the fielded updater (which would just log).
- **II (single source of truth / no secrets)** — `VERSION` stays authoritative and is
  updated atomically with the binary; the updater/installer never handle the fleet token (it
  is compiled into the v2 exe by spec 002); no secret in `update.ps1` or the `.iss`.
- **IV (observable)** — every attempt → Windows Event Log + `update.log` +
  `installer_task.log` + one `upgrade_attempt` telemetry record.
- **V (self-contained, minimal deps)** — PowerShell + Inno + one Python helper; nothing new
  installed on customer machines.
- **Build section (v1.3.0 staged rollout)** — FR-005a–FR-005e; the plan builds the
  channel-aware `update.ps1`, the per-channel release rules, and `tools/fleet-status.py` to
  evaluate the beta gate.

**No violations. No Complexity Tracking entries.**

### Post-Design Constitution Re-check (after Phase 1)

Design introduces no violations. Key confirmations:
- **VI** — `contracts/updater-cli.md` + `contracts/installer-cli.md` keep the fielded
  updater's contract exactly (asset glob, `.sha256`, `/VERYSILENT`, exit 0, `& $exe
  stop/start`), overwrite `update.ps1` in place, and never change AppId/dir/exe/service/task
  for Luminosa. The installer **always exits 0 when it reached a working state** (v2 healthy
  *or* rolled back) — a non-zero exit only for `rollback_failed`.
- **I** — every branch in both contracts terminates at a `RUNNING` working service; the
  installer catches all errors and reports, never lets the fielded updater `throw`.
- **II** — `VERSION` written atomically with the binary; no token/secret in `update.ps1` or
  the `.iss` (the token is compiled into the v2 exe by spec 002).
- **IV** — `update.log` + `installer_task.log` + Event Log + one `upgrade_attempt` telemetry
  per terminal outcome (`contracts/upgrade-telemetry.schema.json`).
- **V** — PowerShell + Inno + one Python helper; nothing new on customer machines.
- Staged rollout — `release.yml` rules (research D12), `is-newer` semver compare (D8),
  `tools/fleet-status.py` evaluates the beta gate (D14).

## Project Structure

### Documentation (this feature)

```text
specs/001-v2-remote-upgrade/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── updater-cli.md            # update.ps1 behaviour, inputs, exit codes, logs
│   ├── installer-cli.md          # Setup.exe silent switches + [Code] contract + exit codes
│   ├── upgrade-telemetry.schema.json   # measurement_type: upgrade_attempt payload
│   └── rollback-bundle.md        # on-device layout of the retained previous version
└── tasks.md                      # /speckit-tasks output
```

### Source Code (repository root)

```text
updater/
└── update.ps1                    # REWRITTEN: asks the installed exe for {version,product,channel};
                                  #   stable -> /releases/latest, beta -> newest incl. prerelease;
                                  #   product-correct asset; SHA-256 (+ signature if C2); semver-correct
                                  #   compare incl. -beta.N; drives Setup.exe; reports outcome

installer/
└── v2/
    ├── luminosa.iss              # AppId {EC5738FF-...}, dir C:\Program Files\Luminosa Log Uploader,
    │                             #   exe loguploaderservice.exe, service LumiLogUploadService
    ├── solira.iss                # NEW AppId, greenfield (no migration [Code])
    ├── common.iss                # shared [Code]: Snapshot, InstallV2, SeedConfig, HealthCheck,
    │                             #   Rollback, ReportOutcome, SelfHealOnBoot, (optional) RenameService
    └── (channel handled by ISCC /DPQ_CHANNEL=beta -> OutputBaseFilename "... Beta Setup")

tools/
├── fleet-status.py              # queries api.picoquant.com admin telemetry; pivots machines by
│                                #   version / channel / last upgrade_attempt outcome (FR-022, FR-005e)
└── fleet_backup_pull.py         # sibling maintainer tool — specs/004-fleet-backup-restore.
                                 #   Same admin key + host, queries the BACKUP list; archives each
                                 #   machine's config. Use it to capture a stuck/manual machine's
                                 #   config before an on-site reinstall.

docs/
└── manual-upgrade-runbook.md     # FR-023b: move a no-working-updater machine to v2 by hand

.github/workflows/
└── release.yml                   # v* tag -> stable release; v*-beta.N -> prerelease;
                                  #   FIRST stable v2 release = Luminosa-only (migration release);
                                  #   later releases may co-bundle products (v2 updater is product-aware)
```

**Structure Decision**: everything ships in `loguploader-service` (same repo as v1 and v2).
The updater and installer are the two moving parts; `common.iss` holds the reusable
snapshot/health/rollback code so Solira and future v2→v2.x upgrades share it. `VERSION`
remains the single on-disk version marker (Principle II).

## Complexity Tracking

No constitution violations — table omitted. Two decisions worth recording:

| Decision | Why | Alternative rejected |
|---|---|---|
| Migration logic inside the installer `[Code]`, not a separate agent step | It is the only code that runs with SYSTEM rights *during* the swap, invoked by the fixed fielded updater. | A pre-flight agent step can't perform the swap; a post-install PowerShell needs its own trigger the fielded updater won't add. |
| Luminosa v2 keeps every v1 identifier (AppId, dir, exe, service, task) | Principle VI: "MUST NOT change … in a way the deployed updater cannot follow." Zero surface = lowest risk for the one hop we get. | Renaming to `PQUploaderLuminosa` / `pquploader-luminosa.exe` for cross-product uniformity — real but cosmetic value, real migration risk. Capability still built + tested for Solira/future (US4). |
