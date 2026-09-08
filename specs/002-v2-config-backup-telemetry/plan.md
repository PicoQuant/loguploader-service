# Implementation Plan: V2 — Config Backup & Device Telemetry

**Branch**: `v2-specs` (feature dir `specs/002-v2-config-backup-telemetry`) | **Date**: 2026-09-08 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-v2-config-backup-telemetry/spec.md`

## Summary

Rebuild the log-uploader as a small, unattended **Windows service** that no longer touches
instrument logs. It does two things on a fixed cycle: (1) POST a **telemetry heartbeat**
(machine id, instrument serial, v2 version, OS) to `api.picoquant.com`, and (2) back up a
per-product set of **device configuration files** to that backend's backup endpoint — only
when a file's content changed since its last successful backup, at most once per UTC day.

Transport is the PicoQuant Telemetry API, authenticated with a **per-product, non-expiring
fleet token** compiled into the build from a CI secret (one build per product: `luminosa`,
`solira`). The agent is device→backend only, never crashes its loop, isolates every cycle,
and keeps local records a remote maintainer can retrieve. It is delivered to the installed
base by the separately-specified v1→v2 unattended upgrade (`specs/001-v2-remote-upgrade`).

Technical approach: a single self-contained **Rust** binary (`x86_64-pc-windows-msvc`), no
runtime on the target, minimal crates, `windows-service` for SCM integration, blocking HTTP
(`ureq` + rustls), local state in a JSON file under `C:\ProgramData`.

## Technical Context

**Language/Version**: Rust (stable, edition 2021, MSRV pinned in `Cargo.toml`, currently 1.74+).

**Primary Dependencies** (minimal, each justified in `research.md`):
`windows-service` (SCM), `ureq` + `rustls` + `webpki-roots` (blocking HTTPS, no async
runtime, self-contained cert roots), `serde` + `serde_json` (payloads, state), `sha2`
(content hashing), `time` (UTC dates), `winreg` (MachineGuid), `eventlog` + `log` (Windows
Event Log), `toml` (runtime config). Multipart bodies hand-rolled (~30 lines) to avoid a
dependency.

**Storage**: local files only. `C:\ProgramData\PicoQuant\<Product>\v2agent\state.json`
(per-file last-successful-backup fingerprint + UTC day, last heartbeat) and a rolling
`cycles.log`. No database. No cloud storage on the client — the backend owns durable storage.

**Testing**: `cargo test` — unit tests for change-detection, the once-per-UTC-day gate,
failure categorisation, config/product resolution; integration tests for the HTTP contract
against a local mock server (`mockito`). Plus `quickstart.md`: a manual end-to-end run against
the real backend using a test token from `.env` (the flow already verified in this repo's
scratchpad `test_submission.sh`).

**Target Platform**: Windows 10 / 11 x64. Runs as a Windows service under **LocalSystem**
(needs to read `C:\Program Files\PicoQuant\<Product>\`, read `HKLM\...\MachineGuid`, write
`C:\ProgramData\PicoQuant\<Product>\`).

**Project Type**: single project — one Rust binary crate at the repository root. v1's Python
files remain in the tree until `specs/001-v2-remote-upgrade` completes the fleet migration.

**Performance Goals**: not performance-sensitive. One cycle every 30 min (default,
configurable). A cycle does ≤ ~10 small file reads + hashes and 1–5 HTTPS POSTs; must finish
well under the cycle interval and use < 30 MB RSS.

**Constraints**: self-contained single `.exe`, no external runtime or DLLs beyond the OS;
never `panic!` out of the service loop; all once-per-day logic in UTC; no inbound network;
no secret in source or git history; local state survives reboot and v2 self-update.

**Scale/Scope**: small fleet (order 10²–10³ instruments across both products). Codebase target
< ~2500 LOC. 2 products now, extensible to more via a table, no structural change.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Constitution v1.1.0. Result: **CONDITIONAL — 1 blocking item requires a constitution
amendment before `/speckit-tasks`.**

| Principle | Assessment |
|---|---|
| I. Never Crash the Service Loop | **PASS (by design).** `cycle::run_once` returns `Result`; the service loop logs and continues on `Err`, with a `catch_unwind` backstop around the cycle body. All network/file/registry calls return typed errors; bounded retry + backoff in `api`. (FR-029, FR-006) |
| II. Single Source of Truth for Version & Config | **PASS.** `VERSION` stays authoritative; `build.rs` reads it and stamps the binary + Windows version resource + installer. Fleet token injected only at build from the `TELEMETRY_FLEET_TOKENS_<PRODUCT>` CI secret, never committed (FR-023/024). One runtime config resolution path: compiled default → `config.toml` next to the exe → documented default. |
| III. Non-Destructive Local File Handling | **PASS (strengthened).** v2 never deletes or writes instrument files — it only reads watched files, skipping locked/absent ones (FR-013). No zip staging, no retention cleanup. The principle's letter (zip retention, delete-after-upload) no longer applies; its intent (never lose instrument data) is fully met. |
| IV. Observable by Default | **PASS with note.** Every cycle logs to the Windows Event Log: product, instrument serial, machine id, per-file outcome, failure category (FR-030). **Note:** v1's literal `Uploaded:` success-string contract does not carry to v2; machine-detectable outcome markers move into structured `cycles.log` lines and the backend heartbeat (FR-005, FR-007). Fold into the amendment. |
| V. Minimal Dependencies, Self-Contained Delivery | **BLOCKING VIOLATION (letter).** Principle V mandates *"a single self-contained PyInstaller EXE"* and Python. v2 is a Rust binary. This meets the principle's **intent better** (no interpreter, smaller, faster start, harder to tamper) but contradicts its wording and the Build section's PyInstaller workflow. **Requires a constitution amendment** (see Complexity Tracking + Next Actions). Dependency count stays small; each crate is justified in `research.md`. |
| VI. Remote Upgradeability Is Non-Negotiable | **PASS (delegated + constrained).** Delivery is `specs/001-v2-remote-upgrade`. This plan's obligations: (a) v2 release artifacts keep names/behaviour the v1 updater consumes (installer `*Setup*.exe` + `.sha256`, `/VERYSILENT`); (b) `state.json` lives outside the install dir so a self-update preserves it (FR-034); (c) per-product builds so a machine is never upgraded to the wrong product (FR-002c). |

**Build, Release & Distribution section** — also needs amending alongside Principle V:
- workflow tooling (PyInstaller → `cargo build`), still per the same `.github/workflows` files
- `client_version.json` "idempotent per day per machine" → superseded by the per-interval
  heartbeat (FR-003); the daily-once idempotency now applies to **config backups** (FR-011)
- installer still registers the AutoUpdate scheduled task and ships `updater/update.ps1`
  (unchanged, owned by spec 001)

**Gate outcome**: proceed to Phase 0/1 design (language-agnostic parts are unaffected).
`/speckit-tasks` and `/speckit-implement` are **blocked** until `/speckit-constitution`
amends Principle V + the Build section (MINOR bump — guidance updated, no principle removed).

### Post-Design Constitution Re-check (after Phase 1)

No new violations introduced by the design. Confirmations:
- **I** — `run_loop.rs` (catch_unwind + log-and-continue) and `error.rs` categories keep the
  loop alive on every failure path in `data-model.md`.
- **II** — `state.json` schema and `config.toml` keep one resolution path; `build.rs` is the
  only place the token/version enter, from CI secrets.
- **III** — the design reads watched files only; `ResolvedWatchedFile` has no delete path.
- **IV** — `CycleRecord` → Event Log + `cycles.log`; `blocked_backups` surfaces conditions in
  the heartbeat.
- **VI** — `state.json` under `%ProgramData%` (not the install dir); CLI `install/uninstall`
  contract matches v1's verbs for the updater; per-product artifacts.
- **V** — still the one blocking item; unchanged by design. Amendment required before
  `/speckit-tasks`.

## Project Structure

### Documentation (this feature)

```text
specs/002-v2-config-backup-telemetry/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── cli.md
│   ├── backend-api.md
│   ├── heartbeat-payload.schema.json
│   └── local-state.schema.json
└── tasks.md             # /speckit-tasks output (NOT created here; blocked on amendment)
```

### Source Code (repository root)

```text
Cargo.toml                     # crate manifest, pinned MSRV, minimal deps
build.rs                       # reads VERSION + PQ_PRODUCT + TELEMETRY_FLEET_TOKENS_<PRODUCT>
                               #   env -> cargo:rustc-env for compile-time constants;
                               #   emits Windows version resource
src/
├── main.rs                    # arg parsing: run | debug | install | uninstall | once
├── service.rs                 # windows-service handler: start/stop, status, dispatch to loop
├── run_loop.rs                # sleep/wake loop, single-cycle mutex, catch_unwind backstop
├── cycle.rs                   # one cycle: heartbeat pass + backup pass, error isolation
├── config.rs                  # compile-time (product, token, version) + config.toml (intervals, overrides)
├── product.rs                 # Product enum + per-product paths & watched-file globs
├── identity.rs                # MachineGuid (registry), instrument serial (LastOpenSerial.txt)
├── watchset.rs                # expand watched-file globs -> concrete files; read w/ share-safe open
├── backup.rs                  # change detection, once-per-UTC-day gate, build+send backup
├── telemetry.rs               # build heartbeat payload (incl. blocked-backup conditions), send
├── api.rs                     # HTTP: telemetry POST (json) + backup POST (multipart), retry/backoff, error categories
├── multipart.rs               # tiny multipart/form-data writer
├── state.rs                   # LocalBackupState load/save (atomic write) at %ProgramData%\PicoQuant\<Product>\v2agent\state.json
├── logging.rs                 # Event Log sink + rolling cycles.log
└── error.rs                   # FailureCategory { NoNetwork, Auth, RejectedBadRequest, TooLarge, BackendError, FileLocked, FileAbsent, Internal }
tests/
├── change_detection.rs
├── daily_limit_utc.rs
├── failure_categories.rs
├── product_resolution.rs
├── multipart_format.rs
└── api_contract.rs            # against mockito

installer/
└── v2/                        # per-product Inno Setup scripts (owned jointly with spec 001)

.github/workflows/
├── windows-build.yml          # rewritten: matrix over [luminosa, solira], cargo build, per-product token secret
└── release.yml                # rewritten: same matrix; publishes installer + .sha256 + exe with v1-compatible asset names
```

**Structure Decision**: Single Rust binary crate at the repository root (not a workspace —
scope doesn't warrant it). v1 Python files stay put until `specs/001-v2-remote-upgrade`
retires them. Per-product differences (bucket, token, file paths) are **compile-time**
constants from `build.rs` plus a small per-product table in `product.rs`; there is exactly one
`.exe` per product, so nothing selects product at runtime.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Principle V: Rust binary instead of "PyInstaller EXE" + Python | The user has chosen a compiled single-binary service. A native binary removes the bundled interpreter, cuts artifact size and cold-start time, shrinks the CVE surface on rarely-patched customer machines, and is harder to tamper with — all of which serve Principle V's stated *intent*. | Staying on Python + PyInstaller keeps the letter but keeps every downside the principle exists to limit (a frozen interpreter that breaks on OS changes, a large `_MEI` unpack, `win32*` runtime pulls). The mismatch is with the principle's wording, not its purpose — so the fix is a MINOR constitution amendment, not a worse implementation. |
| Build section: `cargo` build + matrix workflow instead of the PyInstaller/Inno step | Follows directly from the language change; same `.github/workflows` files, same release artifacts and asset names. | n/a — mechanical consequence of the above. |
| Build section: per-interval heartbeat replaces `client_version.json` "idempotent per day" | Fleet "last-seen"/liveness (FR-003, FR-005, SC-001) needs a heartbeat more often than daily; "alive" must be distinguishable from "stopped". Daily-once idempotency moves to config backups (FR-011), which is where it still matters. | A once-a-day heartbeat cannot tell a healthy machine from one that died 20 hours ago — defeats the primary purpose of v2. |
