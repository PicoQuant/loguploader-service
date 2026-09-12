# Implementation Plan: V2 — Config Backup & Device Telemetry

**Branch**: `v2-specs` (feature dir `specs/002-v2-config-backup-telemetry`) | **Date**: 2026-09-08 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-v2-config-backup-telemetry/spec.md`

## Summary

Rebuild the log-uploader as a small, unattended **Windows service** that no longer touches
instrument logs. It does two things on a fixed cycle: (1) POST a **telemetry heartbeat**
(machine id, instrument serial, v2 version, OS) to `api.picoquant.com`, and (2) back up a
per-product set of **device configuration files** to that backend's backup endpoint — only
when a file's content changed since its last successful backup; settings `*.xml` files at
most once per UTC day, `PQDevice.db` / `PQDevice.conf` on every change (FR-011a).

Transport is the PicoQuant Telemetry API, authenticated with a **per-product, non-expiring
fleet token** compiled into the build from a CI secret (one build per product: `luminosa`,
`solira`). The agent is device→backend only, never crashes its loop, isolates every cycle,
and keeps local records a remote maintainer can retrieve. It is delivered to the installed
base by the separately-specified v1→v2 unattended upgrade (`specs/001-v2-remote-upgrade`).

Technical approach: a single self-contained **Rust** binary (`x86_64-pc-windows-msvc`), no
runtime on the target, minimal crates, `windows-service` for SCM integration, blocking HTTP
(`ureq` + rustls), local state in a JSON file under `C:\ProgramData`.

Every document the agent submits or persists is also documented at the **meaning** level, not
just structurally: a hand-authored semantic data dictionary, on the pattern of the sibling
`pm100` app's `docs/data-dictionary/`, sits alongside the existing structural JSON Schemas
and is checked for full field coverage in CI (FR-036–FR-040, D15). It was consolidated
2026-09-12 to a **repo-wide** location (`docs/data-dictionary/`, not this spec's own
`contracts/`) so spec 001's own output document could join it and reuse concepts instead of
redefining them.

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
CI builds a **product × channel matrix** — `luminosa`, `luminosa`-beta, `solira`,
`solira`-beta — each with its bucket, token, and channel (`stable`/`beta`) compiled in
(FR-002b, FR-002e). Channel-aware self-update is owned by spec 001.

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

Constitution v1.3.0 (Principle V amended for a compiled binary; Build section adds the
staged-rollout mandate). Result: **PASS.**

| Principle | Assessment |
|---|---|
| I. Never Crash the Service Loop | **PASS (by design).** `cycle::run_once` returns `Result`; the service loop logs and continues on `Err`, with a `catch_unwind` backstop around the cycle body. All network/file/registry calls return typed errors; bounded retry + backoff in `api`. (FR-029, FR-006) |
| II. Single Source of Truth for Version & Config | **PASS.** `VERSION` stays authoritative; `build.rs` reads it and stamps the binary + Windows version resource + installer. Fleet token injected only at build from the `TELEMETRY_FLEET_TOKENS_<PRODUCT>` CI secret, never committed (FR-023/024). One runtime config resolution path: compiled default → `config.toml` next to the exe → documented default. |
| III. Non-Destructive Local File Handling | **PASS (strengthened).** v2 never deletes or writes instrument files — it only reads watched files, skipping locked/absent ones (FR-013). No zip staging, no retention cleanup. The principle's letter (zip retention, delete-after-upload) no longer applies; its intent (never lose instrument data) is fully met. |
| IV. Observable by Default | **PASS.** Every cycle logs to the Windows Event Log: product, instrument serial, machine id, per-file outcome, failure category (FR-030). v1's literal `Uploaded:` string contract is superseded (constitution v1.2.0) by structured `cycles.log` lines + the backend heartbeat (FR-005, FR-007). |
| V. Minimal Dependencies, Self-Contained Delivery | **PASS** (constitution v1.2.0 generalized this to "single self-contained executable, compiled build preferred"). v2 is a Rust binary; dependency count stays small; each crate justified in `research.md`. |
| VI. Remote Upgradeability Is Non-Negotiable | **PASS (delegated + constrained).** Delivery is `specs/001-v2-remote-upgrade`. This plan's obligations: (a) v2 release artifacts keep names/behaviour the v1 updater consumes (installer `*Setup*.exe` + `.sha256`, `/VERYSILENT`); (b) `state.json` lives outside the install dir so a self-update preserves it (FR-034); (c) per-product **and per-channel** builds so a machine is never upgraded to the wrong product or across channels (FR-002c, FR-002e); (d) the heartbeat carries the channel so spec 001's beta→stable promotion gate is measurable (FR-002f, constitution v1.3.0 Build section). |

**Build, Release & Distribution section** (constitution v1.2.0 + v1.3.0) — satisfied by:
- workflow tooling `cargo build` per the same `.github/workflows` files
- per-interval heartbeat supersedes `client_version.json`; daily-once now governs config
  backups (FR-011)
- installer registers the AutoUpdate scheduled task and ships `updater/update.ps1` (spec 001)
- **staged rollout**: `stable`/`beta` compiled in, product × channel matrix, beta = GitHub
  prerelease; the beta→stable promotion gate (≥7 days / ≥3 beta instruments / 0 Sev-1) is
  owned by `specs/001-v2-remote-upgrade`, and this plan feeds it by putting the channel in
  every heartbeat (FR-002e, FR-002f)

**Gate outcome**: **PASS** — proceed. `/speckit-tasks` is unblocked (constitution amended to
v1.3.0).

**Note (2026-09-12, /speckit-analyze I1)**: the constitution was further amended to **v1.4.0**
after this gate ran, adding **Principle VII (Semantic Output Schema)**. That principle is not
re-evaluated in the table above since it postdates it; its assessment against this plan is in
the Post-Design Constitution Re-check below, added alongside Phase 10 (T052–T058).

### Post-Design Constitution Re-check (after Phase 1)

No violations. Confirmations:
- **I** — `run_loop.rs` (catch_unwind + log-and-continue) and `error.rs` categories keep the
  loop alive on every failure path in `data-model.md`.
- **II** — `state.json` schema and `config.toml` keep one resolution path; `build.rs` is the
  only place the token/version/channel enter, from CI.
- **III** — the design reads watched files only; `ResolvedWatchedFile` has no delete path.
- **IV** — `CycleRecord` → Event Log + `cycles.log`; `blocked_backups` surfaces conditions in
  the heartbeat; the FR-036–FR-040 semantic dictionary extends "observable" from *that a
  submission happened* to *what each field in it means* — a maintainer reading a raw
  heartbeat or `state.json` off a support ticket doesn't have to re-derive meaning from
  source (D15).
- **V** — Rust binary; small justified dependency set (`research.md`).
  `tools/check_data_dictionary.py` (D15) is Python, matching the existing `tools/` scripts
  (e.g. `fleet_backup_pull.py`) — it is a CI-only doc-coverage check, never linked into the
  agent binary, so it does not add a runtime dependency.
- **VI** — `state.json` under `%ProgramData%` (not the install dir); CLI `install/uninstall`
  matches v1's verbs; per-product **and per-channel** artifacts; channel in every heartbeat
  for the promotion gate.
- **VII** (added to the constitution 2026-09-12, after this plan's initial gate — **PASS**) —
  `docs/data-dictionary/{semantic-model.json,field-mappings.json,README.md}` (repo-wide, not
  spec-002-local — consolidated the same day, per the principle's own intent of one dictionary
  rather than one per feature) is exactly the semantic dictionary the principle requires,
  covering all three documents v2 submits or persists; coverage enforced in CI by
  `tools/check_data_dictionary.py` (SC-013, Phase 10).

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
└── tasks.md             # /speckit-tasks output

docs/data-dictionary/    # repo-wide (FR-036-040, consolidated 2026-09-12 — not spec-002-local;
│                        #   also covers specs/001-v2-remote-upgrade's upgrade_attempt document)
├── README.md
├── semantic-model.json
├── field-mappings.json
└── schema/
    ├── semantic-model.schema.json    # format of ../semantic-model.json
    └── field-mappings.schema.json    # format of ../field-mappings.json
```

### Source Code (repository root)

```text
Cargo.toml                     # crate manifest, pinned MSRV, minimal deps
build.rs                       # reads VERSION + PQ_PRODUCT + PQ_CHANNEL +
                               #   TELEMETRY_FLEET_TOKENS_<PRODUCT> env -> cargo:rustc-env
                               #   compile-time constants; emits Windows version resource
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
├── build_identity.rs         # product + channel resolution
├── multipart_format.rs
├── state_persistence.rs
└── api_contract.rs            # against mockito

tools/
└── check_data_dictionary.py   # CI check: every schema leaf pointer resolves via field-mappings.json
                               #   to a semantic-model.json id (SC-013, D15) — not a Rust dep

installer/
└── v2/                        # per-product Inno Setup scripts (owned jointly with spec 001)

.github/workflows/
├── windows-build.yml          # rewritten: matrix [luminosa,solira] x [stable,beta], cargo build, per-product token secret
│                              #   + runs tools/check_data_dictionary.py (platform-independent, any runner)
└── release.yml                # rewritten: same matrix; beta tag (vX.Y.Z-beta.N) -> prerelease; stable tag -> release;
                               #   publishes installer + .sha256 + exe per (product,channel) with v1-compatible names
```

**Structure Decision**: Single Rust binary crate at the repository root (not a workspace —
scope doesn't warrant it). v1 Python files stay put until `specs/001-v2-remote-upgrade`
retires them. Per-build differences (product bucket, token, watched-file paths, channel) are
**compile-time** constants from `build.rs` plus small per-product / per-channel tables in
`product.rs`; there is exactly one `.exe` per (product, channel), so nothing selects product
or channel at runtime.

## Complexity Tracking

*Constitution Check passes at v1.3.0 — no unjustified violations.* The decisions that drove
the v1.2.0 / v1.3.0 amendments, for the record:

| Decision | Why | Simpler alternative rejected because |
|---|---|---|
| Rust binary (not PyInstaller + Python) — constitution v1.2.0 | Native single binary removes the bundled interpreter, cuts size and cold-start, shrinks the CVE surface on rarely-patched machines, is harder to tamper with — serving Principle V's *intent*. | Python + PyInstaller keeps every downside the principle exists to limit (frozen interpreter, large `_MEI` unpack, `win32*` pulls). |
| Per-interval heartbeat replaces `client_version.json` daily idempotency — v1.2.0 | Fleet "last-seen"/liveness (FR-003, FR-005, SC-001) needs sub-daily cadence. Daily-once moves to config backups (FR-011). | A once-a-day heartbeat cannot tell a healthy machine from one dead 20 h. |
| Channel compiled into the build; product × channel matrix — v1.3.0 | A beta build must be physically incapable of pulling a stable release and vice versa; no config/backend drift. Staged rollout gates every fleet-wide push behind a real-machine beta (Principle VI). | Config-file channel could be edited or lost; backend-assigned cohort needs backend work and softens "device→backend only". |
