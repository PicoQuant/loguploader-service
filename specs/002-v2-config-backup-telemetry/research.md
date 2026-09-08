# Phase 0 Research — V2 Config Backup & Device Telemetry

All Technical Context unknowns resolved. One item (Principle V) is a governance decision, not
a technical unknown — tracked in `plan.md` Complexity Tracking + Next Actions.

## D1. Implementation language — Rust

- **Decision**: Rust, stable toolchain, edition 2021, target `x86_64-pc-windows-msvc`,
  single binary crate at repo root. MSRV pinned in `Cargo.toml`.
- **Rationale**: user preference; a native single `.exe` with no interpreter or runtime DLLs
  best serves Constitution Principle V's intent (small, self-contained, low CVE surface,
  tamper-resistant, fast cold start on unattended machines). Strong Windows-service and
  HTTP/TLS crate ecosystem.
- **Alternatives considered**:
  - *Python + PyInstaller (v1 status quo)* — matches the current constitution letter, but
    keeps the frozen-interpreter fragility, large `_MEI` unpack, and `win32*` runtime pulls
    the principle exists to avoid.
  - *Go* — also a single static binary, simpler build, good `x/sys/windows/svc`. Rejected
    only on user preference; would be an acceptable fallback. Slightly larger binaries, no
    other material downside at this scale.
  - *C# / .NET AOT* — native-AOT single file is viable, but the toolchain and trimming story
    is heavier than Rust for a ~2k-LOC agent.

## D2. Windows service integration — `windows-service`

- **Decision**: `windows-service` crate for the SCM control handler, status reporting, and
  the `service_dispatcher`. `main.rs` exposes `run` (SCM entrypoint), `debug` (run the loop
  in the console), `install` / `uninstall` (register/remove the service via the crate's
  service-manager API), and `once` (run a single cycle and exit, for tests/support).
- **Rationale**: de-facto standard, thin wrapper over `advapi32`, no transitive bloat, lets
  us report `STOP_PENDING`/`STOPPED` promptly (mirrors v1's responsive stop).
- **Alternatives**: raw `windows`/`windows-sys` FFI (more code, no benefit); `sc.exe` shelling
  for install (v1-style) — kept as the *fallback* install path documented in the installer,
  but the crate API is cleaner and testable.
- **Service account**: LocalSystem. Needs read on `C:\Program Files\PicoQuant\<Product>\`,
  read `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`, write
  `C:\ProgramData\PicoQuant\<Product>\v2agent\`.

## D3. HTTP client + TLS — `ureq` + `rustls` + `webpki-roots`

- **Decision**: `ureq` (blocking, no async runtime) with the `rustls` backend and
  `webpki-roots` for a compiled-in trust store.
- **Rationale**: the agent is a slow periodic loop — an async runtime (tokio via `reqwest`)
  is dead weight. `ureq` is tiny, blocking, ergonomic. `rustls` + `webpki-roots` keeps the
  binary self-contained (no dependency on the machine's cert store state, no OpenSSL DLLs).
- **Alternatives**:
  - *`reqwest` blocking* — pulls `tokio` and `hyper` even in blocking mode; heavier.
  - *native-tls / SChannel* — uses the OS cert store (no bundled roots) but ties TLS behaviour
    to the machine's Windows update state; rejected for predictability.
  - *`attohttpc`* — comparable to `ureq`; `ureq` chosen for maturity and docs.
- **Timeouts**: connect 10 s, read 60 s. **Retry**: max 3 attempts per submission, backoff
  2 s → 4 s, only for `NoNetwork` / `BackendError` (5xx); never retry 4xx within a cycle.

## D4. Multipart body — hand-rolled

- **Decision**: a ~30-line `multipart.rs` that writes `multipart/form-data` (random boundary,
  text parts + one binary `content` part) to a `Vec<u8>`, passed to `ureq`'s `send_bytes`
  with the boundary content-type.
- **Rationale**: the backup endpoint needs exactly one file part + a handful of text fields
  (see `contracts/backend-api.md`). A dedicated `multipart` crate is more surface than the
  feature warrants (Principle V). Bodies are small (≤ backend's 50 MB cap; realistically a
  few MB) so building in memory is fine.
- **Alternatives**: `multipart` crate (extra dep); `reqwest::multipart` (needs reqwest).

## D5. Config model

- **Compile-time constants** (from `build.rs` via `cargo:rustc-env`, consumed with `env!`):
  - `PQ_PRODUCT` = `luminosa` | `solira`
  - `PQ_FLEET_TOKEN` = first entry of `TELEMETRY_FLEET_TOKENS_<PRODUCT>` (CI secret / local `.env`)
  - `PQ_VERSION` = contents of the repo `VERSION` file
  - Build fails loudly if `PQ_PRODUCT` is unset/invalid or the token is empty in a
    *release* build (a debug build may allow an empty token and just log auth failures).
- **Runtime config** — `config.toml` next to the `.exe` (optional; all keys have defaults):
  - `cycle_interval_secs` (default 1800), `api_base_url` (default
    `https://api.picoquant.com`), `backup_max_bytes` (default 52428800), `http_timeout_secs`.
  - Watched-file paths are **not** user-config — they come from `product.rs` (FR-008).
- **Resolution order**: compiled default → `config.toml` value if present → hard default.
- **Rationale**: mirrors v1's "single resolution path" (Principle II); keeps the token out of
  any runtime-readable file on the machine (it's in the binary, unavoidably — FR-024a).
- **Alternatives**: environment variables at runtime (rejected — service env is awkward to
  set and the token should not sit in the service environment block); registry (heavier).

## D6. Instrument serial + machine id — `winreg` + file read

- **Machine id**: `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` via `winreg`
  (`KEY_WOW64_64KEY`). Fallback: `00000000-0000-0000-0000-000000000000` (as v1).
- **Instrument serial**: read `C:\ProgramData\PicoQuant\<Product>\Logs\LastOpenSerial.txt`,
  take the last whitespace-separated token (v1 `getLumiSerial`). Missing/unreadable →
  `"unknown"` marker (FR-009a); the condition is surfaced in the heartbeat.
- **Alternatives**: WMI for serial (heavier, and the instrument serial ≠ hardware serial).

## D7. Time / UTC day — `time`

- **Decision**: `time` crate (`OffsetDateTime::now_utc()`), format timestamps as RFC 3339;
  "UTC day" = `date().to_string()` (YYYY-MM-DD) of `now_utc()`.
- **Rationale**: small, no C deps, sufficient. `chrono` also fine; `time` is lighter.
- **Edge**: a cycle straddling midnight uses the day value captured once at cycle start, so a
  file is not double-sent across the boundary (spec Edge Cases).

## D8. Content change detection — SHA-256

- **Decision**: `sha2` over the whole file bytes; compare to the stored
  `last_backup_sha256` in `state.json`. `content_sha256` sent to the backend is the same
  digest (backend verifies — `contracts/backend-api.md`).
- **Rationale**: content-based, not mtime (FR-010); the digest doubles as the wire integrity
  field and the dedupe key the backend already uses.
- **Torn-read safety (FR-015)**: open the file with read + FILE_SHARE_READ only; read fully;
  if the OS reports a sharing violation or the size changes between `metadata()` and EOF,
  treat as `FileLocked` and retry next cycle.

## D9. Local state persistence — atomic JSON

- **Decision**: `state.json` at `C:\ProgramData\PicoQuant\<Product>\v2agent\`, written
  atomically (write `state.json.tmp` in the same dir, `fsync`, rename). `serde_json`,
  pretty-printed, schema in `contracts/local-state.schema.json`.
- **Rationale**: outside the install directory so a v2 self-update (spec 001) preserves it
  (FR-034); atomic rename survives power loss mid-write (Principle I intent). Human-readable
  for support.
- **Contents**: schema version, per-`file_key` `{ last_backup_sha256, last_backup_utc_day,
  last_success_rfc3339 }`, `last_heartbeat_rfc3339`, `last_cycle_summary`.
- **Corruption handling**: unreadable/invalid `state.json` → log, start from empty state
  (first-run semantics, FR-016: everything re-backed-up once that day). Never crash.

## D10. Logging / observability — Event Log + rolling file

- **Decision**: dual sink. `eventlog` crate registers an Event Log source
  (`PicoQuant <Product> LogUploader`) and writes one Info entry per cycle summary + Warn/Error
  per failure (Principle IV). A rolling `cycles.log` (size-capped, e.g. 5 × 1 MB) under the
  ProgramData dir holds the detailed per-file `CycleRecord` lines a maintainer retrieves
  (FR-030).
- **Rationale**: Event Log is the field diagnostic channel (Principle IV); the file log keeps
  structured detail without flooding the Event Log.
- **Alternatives**: `tracing` (nice, but more than needed); ETW (overkill).

## D11. Cycle scheduling + single-flight

- **Decision**: one unified cycle every `cycle_interval_secs` (default **1800 s / 30 min**)
  does both the heartbeat and the backup pass. The loop sleeps in ≤1 s steps so stop is
  responsive (v1 pattern). A process-local flag / `Mutex` guarantees no overlapping cycle
  (FR-031); the service is single-instance by SCM so no cross-process lock is needed, but
  `state.json` writes are still atomic.
- **Rationale**: 30 min gives fleet "last-seen" resolution well within SC-001/SC-002 while
  staying gentle on the backend and on locked-file retries; configurable for tuning.
- **Alternatives**: separate heartbeat vs backup intervals (more config, no real benefit —
  both are cheap and want similar cadence).

## D12. Build-time secret injection

- **Decision**: CI sets `PQ_PRODUCT` and passes the matching
  `TELEMETRY_FLEET_TOKENS_<PRODUCT>` GitHub Actions secret as an env var to `cargo build`;
  `build.rs` reads them and emits `cargo:rustc-env=PQ_FLEET_TOKEN=...` (first comma-separated
  entry) and `cargo:rustc-env=PQ_PRODUCT=...`. The release workflow runs the build once per
  product (matrix). Locally, `build.rs` also loads a gitignored `.env` if present.
- **Rationale**: mirrors v1's `PUBLIC_LINK` pattern (Principle II); one artifact per product
  (FR-002b); token never in source or git history (FR-023) — verified by a CI grep gate.
- **Rotation (FR-025)**: ship a new build with the new token value; the backend accepts old
  + new during the transition; retire the old value after the fleet has updated.

## D13. Testing approach

- **Unit** (`cargo test`, no network): change detection (D8), once-per-UTC-day gate incl.
  midnight boundary (D7/D11), failure categorisation (`error.rs`), product/path resolution
  (`product.rs`), multipart formatting (D4), `state.json` load/save + corruption recovery.
- **Integration**: `mockito` server asserts the exact heartbeat JSON and backup multipart
  shapes, and drives 200 / 401 / 413 / 422 / 5xx / connection-refused to check the agent's
  categorisation, retry, and daily-allowance behaviour.
- **Manual E2E** (`quickstart.md`): `once` subcommand against the real `api.picoquant.com`
  with a test token from `.env`, then verify via admin queries — the flow already proven by
  this repo's `test_submission.sh`.
