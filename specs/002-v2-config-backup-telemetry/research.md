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

## D12. Build-time constant injection (product, channel, token, version)

- **Decision**: CI sets `PQ_PRODUCT` and `PQ_CHANNEL` and passes the matching
  `TELEMETRY_FLEET_TOKENS_<PRODUCT>` GitHub Actions secret as an env var to `cargo build`;
  `build.rs` emits `cargo:rustc-env=` for `PQ_PRODUCT`, `PQ_CHANNEL` (default `stable`),
  `PQ_FLEET_TOKEN` (first comma-separated entry), `PQ_VERSION` (repo `VERSION`). The release
  workflow runs a **product × channel matrix** (4 builds). Locally, `build.rs` also loads a
  gitignored `.env` if present.
- **Rationale**: mirrors v1's `PUBLIC_LINK` pattern (Principle II); one artifact per
  (product, channel) (FR-002b, FR-002e); token never in source or git history (FR-023) —
  verified by a CI grep gate. Channel compiled in (not config/backend) so a `beta` build is
  physically unable to install a stable release and vice versa (constitution v1.3.0).
- **Rotation (FR-025)**: ship a new build with the new token value; the backend accepts old
  + new during the transition; retire the old value after the fleet has updated. The same
  token serves both channels of a product.

## D14. Release channels — stable / beta

- **Decision**: two channels, **compiled into the build** (`PQ_CHANNEL`). The agent's only
  channel responsibility is to report it in the heartbeat (`data-model.md`). Which release a
  machine self-updates from is decided by the updater in `specs/001-v2-remote-upgrade`:
  a `stable` build → GitHub `/releases/latest` (excludes prereleases, unchanged from v1);
  a `beta` build → newest release including prereleases. Beta releases are published with
  `prerelease: true`; beta tags are `vX.Y.Z-beta.N`.
- **Rationale**: the existing v1 fleet already only sees stable (its updater uses `/latest`),
  so the automatic v1→v2 rollout is inherently gated. The beta cohort is a handful of
  internal instruments seeded with beta builds by hand. The constitution (v1.3.0 Build
  section) requires ≥ 7 days on ≥ 3 beta instruments with 0 Sev-1 telemetry before a stable
  `vX.Y.Z` is cut — the heartbeat channel field makes that measurable.
- **Alternatives**: channel in `config.toml` (editable / can be lost); backend-assigned
  cohort (needs backend work, softens "device→backend only"). Rejected per the user's call.

## D15. Semantic data dictionary — hand-authored, schema-validated coverage

> **Superseded in part by D16 (2026-09-12):** the dictionary's *location* moved from this
> spec's own `contracts/data-dictionary/` to a repo-wide `docs/data-dictionary/`. Left below
> unedited as the accurate record of the original per-spec decision and its reasoning, which
> still holds for everything except location (hand-authored, schema-validated, code-independent).

- **Decision**: the FR-036–FR-040 semantic dictionary lives at
  `contracts/data-dictionary/` as three hand-authored files, mirroring `pm100`'s
  `docs/data-dictionary/` exactly:
  - `README.md` — how the pieces fit, plus the FR-039 "same name, different concept" table.
  - `semantic-model.json` — a flat map of dot-namespaced concept id → `{ datatype, unit,
    description, aliases, parent, examples, confidence }`. Namespaces: `identity.*`
    (machine id, instrument serial), `telemetry.*` (channel, cycle health, failure
    categories), `backup.*` (file key, content hash, daily-limit gate), `time.*`
    (client vs. receipt vs. gate-day timestamps — the FR-039 collision), `config.*`
    (build/runtime config), `doc.*` (envelope-level fields shared by every submission —
    `product`, `agent_version`).
  - `field-mappings.json` — a `schema_registry` keyed by a document id
    (`v2.heartbeat_payload.v1`, `v2.backup_submission.v1`, `v2.local_state.v1`) → map of
    JSON pointer → semantic id. The backup submission is `multipart/form-data`, not JSON,
    so its "pointers" are just its part names (`/instrument_serial`, `/content_sha256`, …)
    treated as a flat one-level document, same convention pm100 uses for its non-JSON parts.
  - Structural JSON Schemas for the *format* of a `semantic-model.json` entry and of a
    `field-mappings.json` registry go in `contracts/semantic-model.schema.json` and
    `contracts/field-mappings.schema.json` (Phase 1 output) — these validate the dictionary's
    own shape, distinct from `heartbeat-payload.schema.json` / `local-state.schema.json`,
    which validate the documents the dictionary describes.
- **Rationale**: pm100's dictionary is deliberately code-independent, hand-curated prose +
  data, not generated — semantic meaning ("this is the gate-day key, not a timestamp") isn't
  reliably derivable from field names or types alone, and forcing generation would just
  produce confident-sounding `description` strings with no real `confidence: "uncertain"`
  signal. Keeping the three files matches a pattern already proven to work for a sibling
  PicoQuant app and keeps `field-mappings.json` trivially diffable in review.
- **Coverage enforcement (SC-013 — 0 unmapped fields)**: a small script,
  `tools/check_data_dictionary.py` (same tools/ location as `fleet_backup_pull.py`), walks
  every leaf JSON pointer in `heartbeat-payload.schema.json` and `local-state.schema.json`
  plus the fixed backup-submission part list from `contracts/backend-api.md`, and fails if
  any pointer is absent from `field-mappings.json` or maps to an id missing from
  `semantic-model.json`. Run in CI alongside `cargo test`; not a Rust dependency (Principle
  V) — it only reads the schema/dictionary JSON already checked into the repo.
- **Sync discipline (FR-040)**: the script only catches *missing* mappings, not *stale*
  descriptions of a field whose meaning changed — that half stays a human review step
  whenever `data-model.md` or a schema file changes, called out explicitly in the PR
  checklist rather than automated.
- **Alternatives considered**:
  - *Generate the dictionary from Rust doc-comments / serde derive attributes* — would drift
    less from the code, but collapses "shape" and "meaning" back into one source, which is
    exactly what FR-036 says not to do (a doc-comment describes a struct field, not whether
    two same-named fields across documents are the same concept).
  - *Skip the meta-schemas, freeform JSON* — cheaper, but then a malformed entry (missing
    `confidence`, wrong nesting) only surfaces when a human reads it; the meta-schema catches
    it the same cycle the dictionary is edited.

## D16. Consolidate the data dictionary to a repo-wide location (2026-09-12)

- **Decision**: move `contracts/data-dictionary/{README.md,semantic-model.json,field-mappings.json}`
  and `contracts/{semantic-model,field-mappings}.schema.json` to repo-root
  `docs/data-dictionary/{README.md,semantic-model.json,field-mappings.json,schema/*.schema.json}`,
  and register `specs/001-v2-remote-upgrade`'s `upgrade_attempt` telemetry document
  (`v2.upgrade_attempt.v1`) into the same `semantic-model.json`/`field-mappings.json`, reusing
  concepts already defined for spec 002 (`identity.machine_id`, `identity.instrument_serial`,
  `telemetry.channel`, `doc.agent_version`, `telemetry.measurement_type`) rather than
  redefining them under a second, spec-001-local dictionary.
- **Rationale**: constitution Principle VII (added the same day as D15) makes this dictionary
  a project-wide obligation, not a spec-002 one — "every feature that produces output data."
  Scoping what that would mean for specs 001/003/004 (a separate exercise) found spec 001's
  upgrade-telemetry document reuses several spec-002 concepts verbatim; keeping D15's
  per-spec layout would have meant either duplicating those concept definitions (risking
  silent drift between two `identity.machine_id` entries) or spec 001 depending on spec
  002's `contracts/` directory, which is a worse coupling than a shared, spec-independent
  location. `docs/` (not `specs/002.../contracts/`) also matches where the sibling `pm100`
  app's own dictionary already lives, for a consistent place a PicoQuant engineer would look.
- **Mechanics**: `tools/check_data_dictionary.py` was generalized from two hardcoded schema
  paths to a `DOCUMENT_SOURCES: list[DocumentSource]` table (`doc_id` +
  optional `fixed_pointers` + optional `schema_path`/`schema_prefix`) — covering a new
  spec's document is one list entry, not a rewrite of the walking logic. Confirmed via the
  checker's own first failing run that `upgrade-telemetry.schema.json`'s `config_notes` is an
  array of strings (leaf pointer `/payload/config_notes/*`, one item), not a single
  array-shaped field — caught the same way FR-037's "check against working code, not just
  the field name" discipline was meant to catch such things.
- **Alternatives considered**:
  - *Leave spec 002's dictionary where it is; give spec 001 its own* — simpler in isolation,
    but the concept-duplication risk above is exactly what a semantic dictionary exists to
    prevent; doing it to the dictionary itself would be a poor precedent.
  - *A dictionary per spec, cross-referencing shared concepts by pointing at spec 002's file*
    — avoids duplication but makes every consuming spec's dictionary depend on spec 002's
    directory continuing to exist at that path; a plain repo-root shared location has no such
    directional dependency.

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
- **Data dictionary coverage** (D15): `tools/check_data_dictionary.py`, run in CI, not
  `cargo test` (it validates docs/JSON, not Rust behaviour).
