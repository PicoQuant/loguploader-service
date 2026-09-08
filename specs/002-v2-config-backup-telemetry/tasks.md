---
description: "Task list for V2 — Config Backup & Device Telemetry"
---

# Tasks: V2 — Config Backup & Device Telemetry

**Input**: Design documents from `specs/002-v2-config-backup-telemetry/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/
**Constitution**: v1.3.0 — gate passes (Principle V allows a compiled binary; Build section
adds the staged-rollout / beta-channel mandate).

**Tests**: INCLUDED. `plan.md` (Testing) and `research.md` (D13) specify a concrete
unit + integration test approach with named files, and every user story has an
"Independent Test". Test tasks are listed per story; they do not require strict
red-first ordering but should be written alongside or before the code they cover.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1–US5 from spec.md; Setup / Foundational / Polish carry no story label
- All paths are repo-root-relative. Single Rust binary crate (see `plan.md` → Project Structure).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Rust crate skeleton that compiles.

- [ ] T001 Create `Cargo.toml` at repo root: package `pquploader`, `edition = "2021"`, `rust-version` pinned; `[[bin]]` `pquploader`; deps `windows-service`, `ureq` (features `tls` via `rustls`), `webpki-roots`, `serde` + `serde_json`, `sha2`, `time` (features `formatting`, `macros`), `winreg`, `eventlog`, `log`, `toml`; `[dev-dependencies]` `mockito`; `[profile.release]` `lto = true`, `strip = true`, `opt-level = "z"` — **do NOT set `panic = "abort"`** (the loop relies on `catch_unwind`).
- [ ] T002 Create `src/main.rs` with argument parsing for subcommands `run` | `debug` | `once` | `install` | `uninstall` | `version` (dispatch stubs only; compiles and prints usage).
- [ ] T003 [P] Create `build.rs`: read env `PQ_PRODUCT`, `PQ_CHANNEL` (default `stable`), `TELEMETRY_FLEET_TOKENS_<PRODUCT>` (also load a gitignored `.env` if present), read repo `VERSION`; emit `cargo:rustc-env=PQ_PRODUCT`, `PQ_CHANNEL`, `PQ_FLEET_TOKEN` (first comma-separated entry), `PQ_VERSION`; emit the Windows version resource from `VERSION`. Placeholder-tolerant in debug; hard `panic!` on missing/invalid `PQ_PRODUCT`/`PQ_CHANNEL` or empty token when `PROFILE=release`. Never echo the token to build output. (FR-002b, FR-002e, FR-024)
- [ ] T004 [P] Add `rustfmt.toml`, `clippy.toml`, and `.cargo/config.toml` (default target `x86_64-pc-windows-msvc`).
- [ ] T005 [P] Add `config.toml.example` (keys: `cycle_interval_secs = 1800`, `api_base_url = "https://api.picoquant.com"`, `backup_max_bytes = 52428800`, `http_timeout_secs = 60`) and update `.env.example` with both `TELEMETRY_FLEET_TOKENS_LUMINOSA` / `_SOLIRA` and a one-line note that the build reads the matching one.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: shared modules every user story builds on.

**⚠️ No user-story work starts until this phase is done.**

- [ ] T006 Create `src/error.rs`: `FailureCategory { NoNetwork, Auth, RejectedBadRequest, TooLarge, BackendError, FileLocked, FileAbsent, Internal }` + `AgentError`; `FailureCategory::retryable_next_cycle()` per `data-model.md`.
- [ ] T007 Create `src/config.rs`: compile-time constants via `env!` (`PQ_PRODUCT`, `PQ_CHANNEL`, `PQ_FLEET_TOKEN`, `PQ_VERSION`); `Channel { Stable, Beta }` parsed from `PQ_CHANNEL`; `Config` struct loaded from `config.toml` next to the exe with hard-coded defaults; single resolution path `compiled default → config.toml → default` (Constitution II). Channel is compile-time only — never read from `config.toml` (FR-002e).
- [ ] T008 [P] Create `src/product.rs`: `Product { Luminosa, Solira }` from `PQ_PRODUCT`; `bucket()`, `install_dir()`, `data_dir()`; static `WatchedFileSpec` table per product (`pqdevice_db`, `pqdevice_conf` = `Fixed` under InstallDir; `settings/` = `Glob *.xml` under DataDir; `usersettings/` = `Glob UserSettings\*.xml` under DataDir) — **logs explicitly excluded** (FR-001, FR-008).
- [ ] T009 [P] Create `src/identity.rs`: `machine_id()` from `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` via `winreg` (`KEY_WOW64_64KEY`, zero-GUID fallback); `instrument_serial()` from `<data_dir>\Logs\LastOpenSerial.txt` (last whitespace token) returning `Serial::Known(s)` / `Serial::Unknown` (FR-009a); `os_info()` → `{ version, build, arch }`.
- [ ] T010 [P] Create `src/logging.rs`: register/write Windows Event Log source `PicoQuant <Product> LogUploader` via `eventlog`; size-capped rolling `cycles.log` writer (≈5×1 MB) at `<data_dir>\v2agent\`; init the `log` facade to fan out to both (Constitution IV).
- [ ] T011 Create `src/state.rs`: `LocalBackupState` + `FileBackupState` per `contracts/local-state.schema.json`; `load()` from `<data_dir>\v2agent\state.json` (unreadable / `schema_version` mismatch → empty + log, never panic); `save()` atomic (`state.json.tmp` → fsync → rename).
- [ ] T012 [P] Create `src/multipart.rs`: `MultipartBody::new()` with random boundary; `.text(name, val)`, `.file(name, filename, bytes)`; `.finish() -> (Vec<u8>, content_type_string)` (RFC 7578, minimal).
- [ ] T013 Create `src/api.rs`: build a `ureq::Agent` (rustls + `webpki-roots`, connect 10 s / read `http_timeout_secs`); `post_heartbeat(&Config, serde_json::Value)` and `post_backup(&Config, MultipartBody)`; URL = `{api_base_url}/api/v2/products/{bucket}/{telemetry|backup}`; header `X-TELEMETRY-TOKEN` only (never admin); map status → `FailureCategory` per `contracts/backend-api.md`; retry ≤3 with 2 s→4 s backoff for `NoNetwork`/`BackendError` only (FR-019, FR-020, FR-021, FR-035).
- [ ] T014 Create `src/cycle.rs`: `CycleRecord` / `CycleSummary` types (`data-model.md`); `run_once(&Config, &Identity, &mut LocalBackupState) -> CycleRecord` **skeleton** — resolves identity, opens logging, runs a (currently empty) backup pass then heartbeat pass, each wrapped so one failing sub-step is logged and does not abort the cycle (Constitution I, FR-029); writes a `CycleRecord` to `cycles.log` + an Event Log summary; persists state.
- [ ] T015 Create `src/run_loop.rs`: `serve(stop: &AtomicBool)` — loop that calls `cycle::run_once` every `cycle_interval_secs`, sleeping in ≤1 s steps for responsive stop; process-local single-flight guard (FR-031); `std::panic::catch_unwind` around each cycle body as a backstop (Constitution I).

**Checkpoint**: crate compiles, `once` runs an empty cycle and prints a `CycleRecord`.

---

## Phase 3: User Story 1 — The fleet is visible from the backend (Priority: P1) 🎯 MVP

**Goal**: every machine POSTs an `agent_status` heartbeat each cycle; a maintainer can see version + last-seen per instrument.

**Independent Test**: run `once` on a machine with network → backend telemetry list (filtered by serial) shows a current record with version + OS; power off → last-seen identifies it as not reporting.

### Tests for User Story 1

- [ ] T016 [P] [US1] `tests/api_contract.rs` (mockito): assert the exact heartbeat request — path, `X-TELEMETRY-TOKEN` header, `measurement_type = "agent_status"`, non-empty `payload`, `instrument_serial` present — and drive `200 / 401 / 404 / 422 / 500 / connection-refused` → expected `FailureCategory` + retry count.
- [ ] T017 [P] [US1] `tests/failure_categories.rs`: a failing heartbeat records its category, `run_once` still returns a `CycleRecord`, the loop continues, and no heartbeat backlog is kept (latest state only) (FR-006, SC-009).

### Implementation for User Story 1

- [ ] T018 [US1] Create `src/telemetry.rs`: build the `agent_status` payload per `contracts/heartbeat-payload.schema.json` (`machine_id`, `agent_version`, `product`, `channel`, `serial_source`, `os{}`, `cycle{}`, `blocked_backups[]`, `last_backup_days`); `send_heartbeat(&Config, &Identity, &CycleContext, &Api) -> Result<(), FailureCategory>`. `channel` comes from `PQ_CHANNEL` (FR-002f, FR-004).
- [ ] T019 [US1] Wire the heartbeat pass into `src/cycle.rs::run_once` (runs after the backup pass); record `heartbeat { ok, category }` in `CycleRecord`; update `state.last_heartbeat_utc` on success (FR-003, FR-004, FR-005).
- [ ] T020 [US1] Wire `once` and `debug` in `src/main.rs`: `once` = one `cycle::run_once` then print the `CycleRecord` as JSON and exit 0; `debug` = `run_loop::serve` in the foreground with Ctrl-C → stop.

**Checkpoint**: `once` produces a real heartbeat in the backend; MVP demoable.

---

## Phase 4: User Story 2 — Changed configuration files are backed up daily (Priority: P1)

**Goal**: each watched file is uploaded only on content change, at most once per UTC day, retried on failure, never blocking the others.

**Independent Test**: modify one watched file → `once` → exactly one backup reaches the backend; run again same day → nothing sent; next day unchanged → nothing; next day changed → one.

### Tests for User Story 2

- [ ] T021 [P] [US2] `tests/change_detection.rs`: sha256 baseline vs current — unchanged → skip, changed → send, no state (first run) → send (FR-010, FR-016).
- [ ] T022 [P] [US2] `tests/daily_limit_utc.rs`: once-per-UTC-day gate incl. a cycle straddling midnight; a failed send does **not** advance `last_backup_utc_day`; a `deduplicated: true` `200` **does** advance it (FR-011, FR-012, SC-003, SC-004).
- [ ] T023 [P] [US2] `tests/multipart_format.rs`: the backup body carries `content`, `instrument_serial`, `machine_id`, `file_key`, `source_path`, `content_sha256` (+ optional `file_mtime`, `agent_version`, `client_timestamp`) exactly as `contracts/backend-api.md`.

### Implementation for User Story 2

- [ ] T024 [US2] Create `src/watchset.rs`: expand `WatchedFileSpec` (`Fixed` + `Glob`) → `Vec<ResolvedWatchedFile>`; open each with share-read only; on success return `(bytes, sha256, mtime)`; classify `Locked` (sharing violation / size changed mid-read), `Absent`, `TooLarge(size)` vs `config.backup_max_bytes` (FR-009, FR-013, FR-014, FR-015).
- [ ] T025 [US2] Create `src/backup.rs`: for each `ResolvedWatchedFile`, decide `unchanged | skipped_today | send | blocked | retry_later` against `LocalBackupState`; build the `BackupSubmission` multipart; call `api::post_backup`; on `200` (incl. `deduplicated`) set `{ last_backup_sha256, last_backup_utc_day, last_success_utc }` (FR-010, FR-011, FR-012, FR-016, FR-018).
- [ ] T026 [US2] Wire the backup pass into `src/cycle.rs::run_once` **before** the heartbeat; collect blocked/oversize/absent/rejected outcomes and pass them to `telemetry` as `blocked_backups` (FR-007, SC-012); persist `LocalBackupState` after the pass.
- [ ] T027 [US2] Failure routing in `src/backup.rs`: `NoNetwork`/`BackendError`/`FileLocked` → `retry_later` (no state change, retried next cycle); `RejectedBadRequest`/`TooLarge`/`FileAbsent` → `blocked` (surfaced in heartbeat, not blindly retried) (FR-012, FR-014, FR-027).

**Checkpoint**: US1 + US2 both work from `once`; exactly-once-per-day verified.

---

## Phase 5: User Story 3 — Backups and telemetry are attributable & queryable per instrument (Priority: P2)

**Goal**: every submission carries product bucket + serial (or `unknown`) + machine id + version + UTC timestamp; Luminosa vs Solira land in separate buckets.

**Independent Test**: submit from a Luminosa and a Solira build → each record carries the right bucket and full attribution; a serial-less machine still stores, flagged unknown.

### Tests for User Story 3

- [ ] T028 [P] [US3] `tests/build_identity.rs`: `PQ_PRODUCT` → `bucket`, `install_dir`, `data_dir`, watched-file set; `PQ_CHANNEL` → `Channel`; a `build.rs` unit check that an unknown/empty `PQ_PRODUCT` or invalid `PQ_CHANNEL` fails a release build.
- [ ] T029 [P] [US3] Extend `tests/api_contract.rs`: assert both endpoints always carry `product` bucket + `channel`, `machine_id`, `agent_version`, `instrument_serial` (value or `"unknown"`), and a client UTC timestamp; cover the `serial_source = "unknown"` path (SC-005, SC-008b, US3 S3).

### Implementation for User Story 3

- [ ] T030 [US3] In `src/telemetry.rs` and `src/backup.rs`, populate all attribution fields from `product.rs` + `identity.rs`; set `serial_source` (`file` | `unknown`); never drop a submission for a missing serial (FR-019, FR-022).
- [ ] T031 [US3] In `src/api.rs`, take the `{bucket}` URL segment from the compile-time `PQ_PRODUCT` only — there is no runtime product switch (FR-002d, SC-008a).
- [ ] T032 [US3] In `src/identity.rs`, finalize the `"unknown"` serial marker and surface `serial_source: "unknown"` + a `blocked`-style note in the heartbeat so a maintainer sees it (US3 S3).

**Checkpoint**: records are filterable by instrument and product on the backend.

---

## Phase 6: User Story 4 — The fleet token is protected and rotatable (Priority: P1)

**Goal**: no token in source or git history; token injected at build from a CI secret; rotation works with no submission gap.

**Independent Test**: grep the tree/history → no token; build with two backend-valid values → both submit; retire one → only the retired-token build fails.

### Tests for User Story 4

- [ ] T033 [P] [US4] Add `tests/no_secret_in_source.rs` (or a CI step) that fails if a value matching the fleet-token shape appears anywhere under `src/`, `Cargo.toml`, `*.toml`, or committed dotfiles; assert `version` output never contains the token (SC-006).
- [ ] T034 [P] [US4] `tests/token_build.rs` (or `build.rs` unit): empty token + `PROFILE=release` → build error; debug build → allowed, agent logs `Auth` failures; the agent sends its single compiled value verbatim (FR-024, FR-025).

### Implementation for User Story 4

- [ ] T035 [US4] Finalize `build.rs`: `PQ_FLEET_TOKEN` = first comma-separated entry of `TELEMETRY_FLEET_TOKENS_<PRODUCT>`; hard error on empty in release; same token both channels; ensure the value is not echoed to build stdout/stderr or any emitted file (FR-023, FR-024, FR-024a).
- [ ] T036 [US4] Rewrite `.github/workflows/windows-build.yml`: matrix `product: [luminosa, solira] × channel: [stable, beta]`; step passes `PQ_PRODUCT`, `PQ_CHANNEL`, and `secrets.TELEMETRY_FLEET_TOKENS_<PRODUCT>` (uppercased) as env to `cargo build --release`; upload one artifact per `(product, channel)`; add a "no secret in source" grep gate (FR-002b, FR-002e, FR-024).
- [ ] T037 [US4] `src/main.rs` `version` subcommand: print product, channel, `PQ_VERSION`, `api_base_url`, and `fleet_token: present|absent` — never the value.
- [ ] T038 [US4] Add a token-rotation runbook: `README.MD` section + cross-ref from `quickstart.md` (ship new build → backend accepts old + new → retire old value in `TELEMETRY_FLEET_TOKENS_<PRODUCT>`) (FR-025, SC-007).

**Checkpoint**: CI produces per-product binaries with no secret in the tree.

---

## Phase 7: User Story 5 — Operating the service on a single machine (Priority: P3)

**Goal**: runs unattended as a Windows service, survives reboots, never stops on a recoverable error, keeps retrievable local records.

**Independent Test**: `install` → `sc start` → wait a cycle → Event Log + `cycles.log` + `state.json` updated → reboot → resumes with no logon → `uninstall`.

### Tests for User Story 5

- [ ] T039 [P] [US5] `tests/state_persistence.rs`: `save()` then `load()` round-trips; a truncated/garbage `state.json` → `load()` returns empty state, logs, no panic; a higher `schema_version` → treated as empty (FR-034).
- [ ] T040 [P] [US5] Document the manual service test in `quickstart.md` §5 (install / start / reboot / uninstall) — not automated.

### Implementation for User Story 5

- [ ] T041 [US5] Create `src/service.rs`: `windows-service` control handler — `START_PENDING → RUNNING`, `SERVICE_CONTROL_STOP` → set the stop flag and report `STOP_PENDING` then `STOPPED`; if started from a console, print the "must be started by the SCM / use `install`" guidance (v1 parity) (FR-028).
- [ ] T042 [US5] `src/main.rs` `install` / `uninstall`: register service `PQUploader<Product>` (display `PicoQuant <Product> Log Uploader`, start = auto-delayed, account = LocalSystem) + the Event Log source; idempotent; `uninstall --purge` also removes `<data_dir>\v2agent\` (FR-028).
- [ ] T043 [US5] In `src/run_loop.rs`, guarantee: stop flag checked every ≤1 s; `catch_unwind` per cycle re-enters the loop on the next interval; exactly one Event Log summary per cycle (Constitution I + IV, FR-029, FR-030).
- [ ] T044 [US5] Create `installer/v2/pquploader-luminosa.iss` and `installer/v2/pquploader-solira.iss` (Inno Setup): install the exe + `config.toml.example`, run `install`, register the `\PicoQuant\LuminosaLogUploader\AutoUpdate` scheduled task + ship `updater/update.ps1`, support `/VERYSILENT`, keep v1-updater-compatible asset names — coordinate the migration surface with `specs/001-v2-remote-upgrade` (Constitution VI).

**Checkpoint**: installable, reboot-resilient, self-healing service.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T045 [P] Rewrite `.github/workflows/release.yml`: same `product × channel` matrix; a `vX.Y.Z-beta.N` tag builds only the `beta` artifacts and publishes a GitHub Release with `prerelease: true`; a `vX.Y.Z` tag builds the `stable` artifacts and publishes a normal release. Per-`(product,channel)` installer + `.sha256` + service exe with asset names the v1 updater consumes; `generate_release_notes: true` (Constitution Build section, VI; FR-005a–FR-005c in `specs/001`).
- [ ] T046 [P] Reconcile versioning: keep `VERSION` as the single source of truth consumed by `build.rs`; retire/port `tools/gen_build_versions.py`; update `tools/release.sh` for the Rust build (Constitution II).
- [ ] T047 [P] `README.MD` v2 section: per-product build, `config.toml`, service install/uninstall, Event Log + `cycles.log` + `state.json` locations, token rotation.
- [ ] T048 `cargo fmt --check` + `cargo clippy -- -D warnings`; fix findings.
- [ ] T049 Run `quickstart.md` end-to-end against `https://api.picoquant.com` with a test token from `.env`; verify heartbeat + backups + byte-exact round trip via admin queries (SC-001, SC-002, SC-005).
- [ ] T050 `cargo test` full pass; explicitly confirm exactly-once-per-UTC-day (SC-003/SC-004) and that a locked/oversized/rejected file never blocks the others (SC-012).

---

## Dependencies & Execution Order

### Phase order

- **Setup (P1)** → **Foundational (P2)** → **User Stories (P3–P7)** → **Polish (P8)**.
- Foundational blocks all stories. `src/cycle.rs` (T014) + `src/run_loop.rs` (T015) skeletons live in Foundational so US1 and US5 can both build on them.

### User-story dependencies

- **US1 (P1)** — needs Foundational only. Fills the heartbeat pass in `cycle.rs`.
- **US2 (P1)** — needs Foundational; touches `cycle.rs` (heartbeat pass from US1 not required, but T026 feeds `blocked_backups` into the heartbeat, so land US1's `telemetry.rs` interface first or stub it).
- **US3 (P2)** — refines fields produced by US1 + US2 → do after both (small).
- **US4 (P1)** — mostly `build.rs` + CI + `main.rs version`; independent of US1–US3, can run in parallel after Setup/Foundational.
- **US5 (P3)** — needs `run_loop.rs`/`cycle.rs` (Foundational) and US1 (a real cycle to run); do after US1.

### Suggested order for one developer

Setup → Foundational → **US1 (MVP)** → US2 → US4 → US3 → US5 → Polish.

### Parallel opportunities

- Setup: T003, T004, T005 in parallel.
- Foundational: T008, T009, T010, T012 in parallel (distinct files); T011, T013, T014, T015 after their deps.
- Per story, all `tests/*.rs` tasks marked [P] run in parallel.
- US4 can proceed alongside US1–US3 (different files: `build.rs`, `.github/`, `main.rs`).

## Parallel Example: Foundational

```
# after T006/T007:
Task: T008  src/product.rs
Task: T009  src/identity.rs
Task: T010  src/logging.rs
Task: T012  src/multipart.rs
```

## Implementation Strategy

### MVP (US1 only)

1. Phase 1 Setup → 2. Phase 2 Foundational → 3. Phase 3 US1 → 4. **Validate**: `once`
   produces a heartbeat visible in the backend admin list → 5. demo.

### Incremental delivery

MVP (US1) → +US2 (config backups) → +US4 (CI per-product, no-secret gate) → +US3
(attribution polish) → +US5 (service + installer) → Polish. Each increment is
independently testable and does not break the previous.

## Notes

- `[P]` = different file, no incomplete dependency.
- Tests live under `tests/` (integration, `mockito`) — no real network in `cargo test`.
- Runtime outcome SCs (SC-001, SC-002, SC-008, SC-010) are validated operationally via
  `quickstart.md` (T049), not by a code task.
- The Solira watched-file paths are an assumption pending confirmation (spec "Open Items");
  `product.rs` (T008) isolates them so only the Solira build is affected.
- `solira` must be enabled in the backend `ALLOWED_PRODUCTS` before the Solira build ships
  (spec 003).
- **Release channels**: this feature only *carries* the `stable`/`beta` constant and *reports*
  it in the heartbeat. The channel-aware updater (which release a machine pulls) and the
  beta→stable promotion gate (≥7 days / ≥3 beta instruments / 0 Sev-1) are in
  `specs/001-v2-remote-upgrade` (FR-005a–FR-005e) and constitution v1.3.0.
- Commit after each task or logical group.
