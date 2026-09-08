---
description: "Task list for V1 → V2 Unattended Remote Upgrade Path"
---

# Tasks: V1 → V2 Unattended Remote Upgrade Path

**Input**: `specs/001-v2-remote-upgrade/` — plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md
**Constitution**: v1.3.0 — gate PASS. This *is* the Principle VI feature.

**Tests**: INCLUDED. `plan.md` (Testing) + `quickstart.md` specify Pester for `update.ps1`,
pytest for `fleet-status.py`, and a manual failure-injection matrix for the installer `[Code]`.

**Scope reminder**: only **Luminosa** has a fielded v1 — Solira is greenfield (research D3).
Luminosa v2 keeps every v1 identifier (research D4). The migration runs inside the v2
installer `[Code]`, invoked by the *fixed* fielded `update.ps1`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: different file, no incomplete dependency.
- All paths repo-root-relative.

---

## Phase 1: Setup

- [ ] T001 Create `installer/v2/` with `common.iss` (declare empty `[Code]` procs: `Snapshot`, `SeedConfig`, `EnsureService`, `HealthCheck`, `Rollback`, `ReportOutcome`, `SelfHealOnBoot`, `RenameService`, `Relocate`, `EnsureAutoUpdateTask`), `luminosa.iss` (`#include "common.iss"`, AppId `{{EC5738FF-E229-4BB2-9438-ACD2BD11AAC8}`, `DefaultDirName={autopf}\Luminosa Log Uploader`), `solira.iss` (new AppId, `{autopf}\Solira Log Uploader`) — all three compile with `ISCC.exe`.
- [ ] T002 [P] Create `updater/update.ps1` skeleton: discover install root (`Split-Path $PSScriptRoot`), read `<root>\VERSION`, probe for `loguploaderservice.exe` / `pquploader-<product>[-beta].exe`; log a run header to `%ProgramData%\PicoQuant\LuminosaLogUploader\update\update.log`; exit 0. (Replaces the fielded script at the same path.)
- [ ] T003 [P] Create `tools/fleet-status.py` skeleton: argparse (`--json`, `--beta-gate`), read `EXPECTED_ADMIN_API_KEY` from env/`.env`, print an empty table; exit 0.
- [ ] T004 [P] Create `docs/manual-upgrade-runbook.md` skeleton and `docs/manual-intervention.json` (`[]`).
- [ ] T005 [P] Create `tests/updater/` Pester project with `fixtures/releases-latest.json`, `fixtures/releases-list.json` (a stable + two prereleases), `fixtures/*.sha256`.
- [ ] T006 [P] Create `installer/v1-bridge.iss` as a **stub with a header comment only** — documents the not-built reboot-cadence fallback (research D10) so it is discoverable.

---

## Phase 2: Foundational (Blocking Prerequisites)

**⚠️ No user-story work starts until this phase is done.** These land in the **v2 Rust crate**
(`specs/002-v2-config-backup-telemetry`) and in `installer/v2/common.iss`.

- [ ] T007 Add `src/upgrade.rs` to the v2 crate: `version_json()` → prints `{version,product,channel}`; `is_newer(remote: &str) -> bool` (semver precedence incl. prerelease per semver.org); `upgrade_report(args)` → POST `measurement_type:"upgrade_attempt"` per `specs/001-v2-remote-upgrade/contracts/upgrade-telemetry.schema.json` via `src/api.rs`.
- [ ] T008 [P] Wire subcommands into the v2 crate `src/main.rs`: `version --json`, `is-newer <remote>` (exit 0 iff remote strictly newer), `upgrade-report <outcome> [--cause C --from V --to V --health-ms N --config-note S]`.
- [ ] T009 `installer/v2/common.iss` `[Code]` helpers: `WriteAttemptLog(json)` → `%ProgramData%\PicoQuant\LuminosaLogUploader\upgrade\attempts.log`; `AcquireMutex/ReleaseMutex('Global\PQLuminosaUpgrade')`; `RunExeCapture(params): record` (exit code + stdout, for `once` / `version --json`).
- [ ] T010 `installer/v2/common.iss` `Snapshot()` per `contracts/rollback-bundle.md`: copy `loguploaderservice.exe`, `VERSION`, `updater\update.ps1`, `settings.py` (if present) into `…\rollback\<from>\`, write `manifest.json` (`outcome:"pending"`); return false (abort, old version untouched) if any copy fails.
- [ ] T011 `installer/v2/common.iss` `SeedConfig()` per `data-model.md`: write `config.toml` with v2 defaults if absent (idempotent — FR-014a); if `settings.py` has `service_interval_seconds`, map to `cycle_interval_secs`; collect untranslated v1 keys into a list for `ReportOutcome`; never modify `settings.py` (FR-014c).
- [ ] T012 `installer/v2/common.iss` `EnsureService()`: ensure the service (`LumiLogUploadService` for Luminosa, name from a `#define`) is registered and points at `{app}\loguploaderservice.exe`; start it; then `EnsureAutoUpdateTask()`.
- [ ] T013 `installer/v2/common.iss` `HealthCheck()` per research D6: within 10 min, service `RUNNING` **and** `{app}\loguploaderservice.exe once` exit 0 with `"heartbeat":{"ok":true}` in stdout JSON; 2 retries at 60 s on transient failure; return pass/fail.
- [ ] T014 `installer/v2/common.iss` `Rollback()` per `contracts/rollback-bundle.md`: stop the broken service, restore the 4 bundle files to their locations, re-point/register the service at the restored exe, start, verify `RUNNING`; set `manifest.outcome` (`rolled_back` / `rollback_failed`).
- [ ] T015 `installer/v2/common.iss` `ReportOutcome(outcome, cause, extras)`: shell `{app}\loguploaderservice.exe upgrade-report …` (best-effort; log, do not fail the install if the POST fails) + `WriteAttemptLog`.
- [ ] T016 `installer/v2/common.iss` `SelfHealOnBoot()`: if `<root>\VERSION` is a v2 version, the v2 service will not reach `RUNNING`, and a `rollback\<from>\` with `outcome:"pending"` exists → run `Rollback()`. Invoked from the top of `update.ps1` and as an install-time `RunOnce`.

**Checkpoint**: `ISCC` builds `luminosa.iss`; the v2 exe answers `version --json` / `is-newer` / `upgrade-report`.

---

## Phase 3: User Story 1 — A fielded v1 machine moves itself to v2 (Priority: P1) 🎯 MVP

**Goal**: publish a stable v2 → every reachable Luminosa v1 machine self-installs v2, keeps uploading, reports success — no login.

**Independent Test**: quickstart §2 — real v1 VM, `schtasks /Run` the AutoUpdate task, assert `VERSION`=v2, service RUNNING, one `agent_status` + one `upgrade_attempt(ok)` at the backend, no operator session.

### Tests for User Story 1

- [ ] T017 [P] [US1] `tests/updater/Select-Release.Tests.ps1` (Pester): `stable` → `/releases/latest`; `beta` → highest semver incl. prerelease from `fixtures/releases-list.json`.
- [ ] T018 [P] [US1] `tests/updater/Select-Asset.Tests.ps1`: product-correct asset regex `^<Product> Log Uploader( Beta)? Setup\.exe$`; `.sha256` pairing; SHA-256 verify pass and mismatch → `integrity_failed`, installer not run.
- [ ] T019 [P] [US1] `tests/updater/Decide-Act.Tests.ps1`: `is-newer` path; v1-machine fallback (`[Version]` then string compare); current ≥ remote → no-op.

### Implementation for User Story 1

- [ ] T020 [US1] `updater/update.ps1` — full behaviour per `contracts/updater-cli.md` steps 1–10: `SelfHealOnBoot` first; resolve product+channel (`version --json`, fallback `luminosa`/`stable`); query GitHub per channel; pick product asset + `.sha256`; decide via `is-newer` (fallback); download to the update dir; **verify SHA-256** (record `integrity_failed` + exit 0 on mismatch); `& <exe> stop`; run `Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-`; on non-zero exit → step 9 (T031); else `& <exe> start`; exit 0. Host-unreachable / restricted network → keep current, log, exit 0.
- [ ] T021 [US1] `installer/v2/common.iss` `CurStepChanged`: `ssInstall` → `AcquireMutex` + `Snapshot` (abort clean if false); `ssPostInstall` → `SeedConfig` → `EnsureService` → `HealthCheck`; on fail → `Rollback`; always `ReportOutcome` + `ReleaseMutex`. Installer exits 0 unless `Rollback` returned `rollback_failed`.
- [ ] T022 [US1] `installer/v2/luminosa.iss` — final: `[Files]` new `loguploaderservice.exe` (the Rust binary), `updater\update.ps1`, `VERSION`; `#define ServiceName "LumiLogUploadService"`, `#define MigrateIdentity "no"`; `OutputBaseFilename` from `/DPQ_CHANNEL` (`Luminosa Log Uploader[ Beta] Setup`); keep `CreateAutoUpdateTask` in `[Code]`.
- [ ] T023 [US1] Atomic `VERSION` write: `common.iss` writes `<app>\VERSION` via temp-file + rename as the last install step; confirm the v2 `update.ps1` overwrites the fielded one at `<app>\updater\update.ps1` (FR-011 basis, Constitution II).
- [ ] T024 [US1] Idempotency & convergence (US1 #2/#3/#4): re-run makes no change (`is-newer` no; `config.toml` kept; service already correct); a machine on any v1 point release converges (the check is version-compare, not from-version-specific).

**Checkpoint**: MVP — a stable v2 migrates a real v1 VM unattended.

---

## Phase 4: User Story 2 — A failed or interrupted upgrade leaves a working service (Priority: P1)

**Goal**: every failure mode (bad download, power loss, v2 won't start, v2 starts but can't upload, disk full, overlap) ends with a **running, working** uploader and a retry later.

**Independent Test**: quickstart §4 matrix — restore `v1-clean`, inject each failure, assert a working uploader + the right `upgrade_attempt` outcome.

### Tests for User Story 2

- [ ] T025 [P] [US2] `tests/updater/Integrity-Fail.Tests.ps1`: sha mismatch → `integrity_failed` recorded, `Setup.exe` never launched, exit 0, retry next run.
- [ ] T026 [P] [US2] `tests/installer/ExitCode.md` + a harness script: assert the installer exit-code contract — `0` on healthy, `0` on `rolled_back`, non-zero only on `rollback_failed` (per `contracts/installer-cli.md`).

### Implementation for User Story 2

- [ ] T027 [US2] `common.iss` — `HealthCheck` fail path: `Rollback` → start v1 → verify `RUNNING` → `manifest.outcome="rolled_back"` → `ReportOutcome("health_check_failed", cause)` (FR-010, US2 #4).
- [ ] T028 [US2] `common.iss` `SelfHealOnBoot` — power-loss-mid-swap recovery from a `pending` bundle; covered by quickstart §4 row 3 (US2 #2).
- [ ] T029 [US2] `common.iss` `Snapshot` — disk-full / write failure → return false → installer aborts, old version untouched, `ReportOutcome("install_failed","snapshot: no space")` (edge: disk full).
- [ ] T030 [US2] `common.iss` mutex — a second concurrent `Setup.exe` waits then no-ops (`is-newer` now says current); install never corrupted (FR-012).
- [ ] T031 [US2] `updater/update.ps1` step 9 — installer non-zero exit: `& <on-disk exe> start`, Event Log **Error**, `update.log` entry, exit 1 (last-resort signal; the fielded v1 script would just `throw`).
- [ ] T032 [US2] Retry-not-pinned: after a rollback, the next `update.ps1` run re-attempts while `is-newer` still says act; a backoff marker (`upgrade\backoff.txt`, capped) may delay but never stops retrying (FR-010b, US2 #5).

**Checkpoint**: every quickstart §4 row leaves a working uploader.

---

## Phase 5: User Story 3 — The maintainer can see which machines have upgraded (Priority: P2)

**Goal**: a per-machine rollout view (v1 / v2 / migrating / rolled_back / manual_required / stuck) from telemetry alone, plus a beta-gate check.

**Independent Test**: quickstart — feed synthetic records, produce an accurate per-machine table.

### Tests for User Story 3

- [ ] T033 [P] [US3] `tests/tools/test_fleet_status.py` (pytest): synthetic `agent_status` + `upgrade_attempt` inputs → correct `state` classification for each case in `data-model.md`.

### Implementation for User Story 3

- [ ] T034 [US3] `tools/fleet-status.py` — query `GET /api/v2/admin/products/{luminosa,solira}/telemetry?measurement_type=agent_status` and `=upgrade_attempt`, paginate (`limit`/`offset`), build per-machine records keyed by `machine_id` (+ `instrument_serial`).
- [ ] T035 [US3] `tools/fleet-status.py` — `state` rules per `data-model.md` (`rolled_back` = last `upgrade_attempt` failed + current is v1; `stuck` = v1 + stable exists + > N days + no `upgrade_attempt`; `migrating` = non-terminal phase newer than any `agent_status`); table + `--json` output; columns: machine_id, serial, current_version, channel, last_seen, last_upgrade, state. Also print a **rollout summary**: migrated %, days since publication, vs the **95% / 14-day** bound (FR-004); flag if `stuck` count exceeds a `--stuck-threshold` (→ bridge-release signal) (FR-019, FR-020, FR-022).
- [ ] T036 [US3] `tools/fleet-status.py` — merge `docs/manual-intervention.json`: listed machines render `manual_required`, excluded from `stuck` (FR-023a).
- [ ] T037 [US3] `tools/fleet-status.py --beta-gate` — for each `channel=beta` machine: days observed + Sev-1 count using the **FR-005d definitions** — `outcome=rollback_failed`; crash-loop (≥3 service starts in 1 h **or** `cycle.ok=false` on ≥3 consecutive heartbeats); stopped service (no heartbeat for ≥3× cycle interval while reachable). Print PASS/FAIL vs **≥7 days / ≥3 beta instruments / 0 Sev-1** (FR-005d, FR-005e, SC-001a).
- [ ] T038 [US3] Confirm `common.iss` `ReportOutcome` always emits a terminal `outcome` so "tried and failed" ≠ "never tried" in the view (FR-020, US3 #2).

**Checkpoint**: `fleet-status.py` classifies a mixed fleet correctly; `--beta-gate` gives a clear PASS/FAIL.

---

## Phase 6: User Story 4 — The upgrade mechanism can upgrade itself (Priority: P2)

**Goal**: a v2 release that relocates / renames / re-plumbs is applied by the *old* mechanism and leaves the machine on the *new* mechanism, with all future upgrades working.

**Independent Test**: quickstart §5 (beta→beta hop through v2's own `update.ps1`) + a dedicated relocate test `.iss`.

### Tests for User Story 4

- [ ] T039 [P] [US4] Quickstart §5 harness: publish `v2.0.0-beta.2`, `schtasks /Run`, assert the **v2** `update.ps1` (uses `is-newer`, semver-with-prerelease) upgrades beta→beta and health-checks (FR-018, SC-007).

### Implementation for User Story 4

- [ ] T040 [US4] `common.iss` `RenameService(old,new)` + `Relocate(oldDir,newDir)`: stop+delete old service, register new (LocalSystem, auto-delayed) at the new exe path, move `[Files]`, rewrite the AutoUpdate task `/TR` to the new `update.ps1` path, delete the old task; assert no old service/task/dir remains (FR-017, US4 #1/#3).
- [ ] T041 [US4] Gate `RenameService`/`Relocate` behind `#define MigrateIdentity` — **`"no"` for `luminosa.iss`** (research D4). Add `tests/installer/relocate-test.iss` (`MigrateIdentity="yes"`, throwaway AppId) that exercises the path on a VM.
- [ ] T042 [US4] `update.ps1` — verify post-migration continuity: same-path `update.ps1` replacement is picked up next run; the recreated task fires; product/channel/compare all sourced from the v2 exe (FR-018, US4 #2).
- [ ] T043 [US4] `common.iss` `EnsureAutoUpdateTask` — recreate with **`/SC ONSTART /DELAY 0000:30`** *and* **`/SC DAILY /ST 03:00 /RU SYSTEM /RL HIGHEST`**; idempotent (`/Create /F`) (research D10, SC-007).

**Checkpoint**: relocate-test `.iss` migrates identity cleanly on a VM; Luminosa keeps its identity; v2→v2.x is prompt.

---

## Phase 7: User Story 5 — Machines without a working v1 updater (Priority: P3)

**Goal**: never auto-touched, always counted, movable by a documented runbook.

**Independent Test**: quickstart §6 — VM with the AutoUpdate task deleted; confirm no change, `manual_required` in the view, runbook clears it.

- [ ] T044 [P] [US5] `docs/manual-upgrade-runbook.md` — full procedure: how a machine lands here, RDP/on-site steps, `Setup.exe /VERYSILENT`, verify service + `AutoUpdate` task + `loguploaderservice.exe version`, then remove the entry from `docs/manual-intervention.json` (FR-023b).
- [ ] T045 [US5] `docs/manual-intervention.json` — document the entry shape (`machine_id` | `instrument_serial`, `reason`, `added_utc`, `cleared_utc?`) inline in the runbook; entries are hand-edited (no tool needed).
- [ ] T046 [US5] `tools/fleet-status.py` — a `stuck` machine (v1, no `upgrade_attempt` after the window) is the **discovery signal**; document in the runbook that the operator investigates and, if it lacks a working updater, adds it to `manual-intervention.json` (FR-023, FR-023a).
- [ ] T047 [P] [US5] Confirm no code path attempts an alternate unattended channel for these machines (FR-023) — assert in the updater tests that an absent/failed task means the updater simply never runs (there is no fallback trigger).

**Checkpoint**: quickstart §6 passes.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T048 [P] `.github/workflows/release.yml` — **this task owns the file** (spec 002 T045 defers to it). Composition rules (research D12): `v*-beta.N` tag → `prerelease: true` + `-beta` matrix only; `v2.0.0` → **Luminosa assets only**; `v2.0.z` / `v2.y.*` → full `product × channel` matrix. Build ISCC per `(product, channel)` with `/DPQ_CHANNEL`; `cargo build --release` per `(product, channel)` with the env from spec 002 T036; publish installer + `.sha256` + service exe named per `contracts/installer-cli.md`; add the "no secret in `updater/`, `installer/`, `src/`" grep gate (spec 002 SC-006 + Constitution II).
- [ ] T049 [P] `README.MD` — v2 auto-update section: channels, how a v1 machine migrates, `tools/fleet-status.py` usage, `--beta-gate`, updater log locations, link to the manual runbook (Constitution Dev Workflow).
- [ ] T050 [P] Amend `specs/002-v2-config-backup-telemetry/contracts/cli.md` — Luminosa deployed identifiers are `loguploaderservice.exe` / `LumiLogUploadService` (kept for migration surface, research D4); Solira uses `pquploader-solira[-beta]` / `PQUploaderSolira`.
- [ ] T051 `Invoke-Pester tests/updater/` all green; `pytest tests/tools/` all green.
- [ ] T052 Run `quickstart.md` steps 1–6 on a real current-v1 Windows VM, including every failure-injection row; record outcomes (this is the Constitution VI "tested on a real Windows install" gate).
- [ ] T053 Dry-run the beta gate: with ≥3 beta VMs running ≥7 days, `fleet-status.py --beta-gate` returns PASS before any stable `v2.0.0` tag is pushed (FR-005d, SC-001a).

---

## Dependencies & Execution Order

- **Setup (P1)** → **Foundational (P2)** → **US1..US5 (P3–P7)** → **Polish (P8)**.
- Foundational blocks everything: the installer `[Code]` procs (T009–T016) and the v2-exe
  subcommands (T007–T008) are used by every story.

### Story dependencies

- **US1 (P1)** — Foundational only. The MVP.
- **US2 (P1)** — builds on US1's `common.iss` `CurStepChanged` wiring (T021); the rollback
  procs exist from Foundational (T014).
- **US3 (P2)** — independent of US1/US2 code (only needs `upgrade_attempt` records to exist,
  which US1/US2 produce); `fleet-status.py` can be built in parallel.
- **US4 (P2)** — needs US1's installer + updater; the relocate procs (T040) reuse Foundational.
- **US5 (P3)** — mostly docs + one updater-test assertion; needs `fleet-status.py` (US3) for
  the `manual_required` merge.

### Suggested order (one developer)

Setup → Foundational → **US1 (MVP)** → US2 → US4 → US3 → US5 → Polish.

### Parallel opportunities

- Setup: T002–T006 in parallel.
- Foundational: T007/T008 (v2 crate) parallel with T009–T016 (installer) — different repos-of-concern.
- Per story, all `[P]` test tasks run in parallel.
- **US3 (`fleet-status.py`) can be built alongside US1/US2** — different files entirely.

## Implementation Strategy

**MVP** = Setup + Foundational + **US1** (T001–T024): a stable v2 release migrates a real v1
VM unattended. Validate with quickstart §2, then §3 (idempotency).

**Incremental**: +US2 (rollback safety — do **before** any beta goes to real instruments) →
+US4 (v2→v2.x + relocate capability) → +US3 (rollout visibility + beta gate) → +US5
(manual path) → Polish → **T052/T053 gate** → first `v2.0.0-beta.1`.

## Notes

- The fielded v1 `update.ps1` is immutable for the first hop — every task that touches
  delivery must stay inside its contract (`contracts/updater-cli.md` "Fixed constraints").
- Installer `[Code]` is validated by the manual quickstart matrix (Inno Pascal has no unit
  harness); `update.ps1` and `fleet-status.py` have real automated tests.
- T007/T008 extend the **spec 002** Rust crate — coordinate with that feature's implementation.
- T048 and spec 002 T045 edit the **same** `release.yml` — land one reconciled workflow.
