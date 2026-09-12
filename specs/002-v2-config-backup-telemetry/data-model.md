# Phase 1 Data Model — V2 Config Backup & Device Telemetry

Entities are mostly in-memory value types plus one persisted file (`state.json`) and two wire
payloads. No database on the client. Field names below are the canonical names; JSON schemas
for the persisted and wire shapes are in `contracts/`.

## BuildIdentity (compile-time)

| Field | Type | Source | Notes |
|---|---|---|---|
| `product` | enum `Luminosa` \| `Solira` | `build.rs` → `env!("PQ_PRODUCT")` | one build per product (FR-002b) |
| `channel` | enum `Stable` \| `Beta` | `build.rs` → `env!("PQ_CHANNEL")`, default `stable` | one build per channel (FR-002e); reported in every heartbeat (FR-002f) |
| `bucket` | `&'static str` | derived: `"luminosa"` / `"solira"` | goes on every submission (FR-002d, FR-019) |
| `fleet_token` | `&'static str` | `env!("PQ_FLEET_TOKEN")` | first entry of the CI secret list; same token both channels (FR-024) |
| `version` | `&'static str` | `env!("PQ_VERSION")` = `VERSION` file | FR-004; Principle II |
| `install_dir` | `PathBuf` | table in `product.rs` | `C:\Program Files\PicoQuant\<Product>\` |
| `data_dir` | `PathBuf` | table in `product.rs` | `C:\ProgramData\PicoQuant\<Product>\` |

**Validation**: a *release* build fails if `product` is unset/unknown, `channel` is invalid,
or `fleet_token` is empty. A machine cannot run as the wrong product or channel — there is no
runtime switch. Channel-aware self-update selection is `specs/001-v2-remote-upgrade`.

## WatchedFileSpec (per-product, static table)

The set for a product, from `product.rs` (FR-008). Each entry:

| Field | Type | Notes |
|---|---|---|
| `file_key` | `&'static str` | logical id sent to the backend, `[a-z0-9_./-]` |
| `root` | enum `InstallDir` \| `DataDir` | which product root it lives under |
| `rel` | `&'static str` | path or glob relative to `root` |
| `kind` | enum `Fixed` \| `Glob` | `Glob` expands to 0..n concrete files at cycle time |
| `daily_limit` | `bool` | `true` → once-per-UTC-day gate (FR-011); `false` → send on every change (FR-011a) |

**Luminosa entries** (Solira: same with `Solira` root paths):

| file_key | root | rel | daily_limit |
|---|---|---|---|
| `pqdevice_db` | InstallDir | `PQDevice.db` | `false` |
| `pqdevice_conf` | InstallDir | `PQDevice.conf` | `false` |
| `settings/<name>.xml` | DataDir | `*.xml` (glob; `file_key` = `settings/` + filename) | `true` |
| `usersettings/<name>.xml` | DataDir | `UserSettings\*.xml` (glob; `file_key` = `usersettings/` + filename) | `true` |

The backend accepts `file_key` only in `[a-z0-9_./-]` (spec 003). The real on-disk
filename (`ChromophoreList.xml`, `GUI Settings.xml`, …) is therefore normalised for the
`file_key` — ASCII-lowercased, any other character → `-` — by `watchset::sanitize_key_component`.
The untouched name still travels in the `source_path` part.

Explicitly excluded: `Logs\*.pqlog`, `LaserPower.log` (FR-001).

## ResolvedWatchedFile (per cycle, in-memory)

One concrete file discovered this cycle.

| Field | Type | Notes |
|---|---|---|
| `file_key` | `String` | for globs, includes the real filename |
| `abs_path` | `PathBuf` | canonical location (FR-009) |
| `read_result` | enum | `Ok(bytes, sha256, mtime)` \| `Locked` \| `Absent` \| `TooLarge(size)` |
| `daily_limit` | `bool` | carried from the `WatchedFileSpec` (FR-011a) |

## LocalBackupState (persisted — `state.json`)

Atomic-write JSON at `<data_dir>\v2agent\state.json`. Survives reboot + v2 self-update
(FR-034). Schema: `contracts/local-state.schema.json`.

| Field | Type | Notes |
|---|---|---|
| `schema_version` | int | currently `1`; unknown/greater → treat as empty, log |
| `files` | map `file_key` → `FileBackupState` | |
| `last_heartbeat_utc` | RFC3339 string \| null | informational |
| `last_cycle` | `CycleSummary` \| null | last run's outcome, for support |

**FileBackupState**

| Field | Type | Notes |
|---|---|---|
| `last_backup_sha256` | 64 hex string | change-detection baseline (FR-010) |
| `last_backup_utc_day` | `YYYY-MM-DD` | once-per-day gate (FR-011); recorded for every file but ignored for `daily_limit == false` (FR-011a) |
| `last_success_utc` | RFC3339 string | |

**State transitions for one file within a cycle**

```
missing in state  ──(file present, readable)──▶ send backup ──(200)──▶ record {sha, day, ts}
                  └─(file absent)─────────────▶ no-op, note "absent" in heartbeat

have state, sha == last_backup_sha256 ─────────▶ skip (unchanged)                (FR-010)
have state, sha != last, daily_limit, day == today ─▶ skip (already backed up today) (FR-011)
have state, sha != last, !daily_limit           ─▶ send backup ──(200)──▶ update {sha, day, ts}  (FR-011a)
have state, sha != last, day <  today   ─────────▶ send backup ──(200)──▶ update {sha, day, ts}
send backup ──(4xx bad-request / too-large)────▶ do NOT update; mark blocked; surface in heartbeat; no blind retry (FR-014, FR-027)
send backup ──(401 / 5xx / no network)─────────▶ do NOT update; retry next cycle (FR-012)
```

`last_backup_utc_day` is only advanced on a confirmed `200`. For a daily-limited file, a file
changing twice in a day is captured once (first successful send); later same-day changes wait
for tomorrow (accepted, spec US2 scenario 2 / Edge Cases). `PQDevice.db` / `PQDevice.conf`
(`daily_limit == false`) are sent on every same-day change (FR-011a).

## HeartbeatPayload (wire — `POST .../telemetry`)

Full request contract in `contracts/backend-api.md`; `payload` object schema in
`contracts/heartbeat-payload.schema.json`.

Envelope: `measurement_type = "agent_status"`, `measured_at` = cycle-start RFC3339 UTC,
`instrument_serial` = serial or `"unknown"`, `payload = { ... }`.

`payload` fields:

| Field | Type | Notes |
|---|---|---|
| `machine_id` | string | MachineGuid, or all-zero GUID fallback |
| `agent_version` | string | `PQ_VERSION` |
| `product` | string | bucket |
| `channel` | enum `stable` \| `beta` | `PQ_CHANNEL`; feeds spec 001's promotion gate (FR-002f) |
| `serial_source` | enum `file` \| `unknown` | FR-009a / US3 scenario 3 |
| `os` | object `{ version, build, arch }` | from `GetVersionEx`/`RtlGetVersion` + arch |
| `instrument_software` | object `{ version, log_version }` (each string \| null) | Luminosa/Solira version: `version` from `<install_dir>\<Product>.exe` file-version resource, `log_version` from the newest `*.pqlog` header (FR-004a) |
| `cycle` | object `{ started_utc, duration_ms, ok }` | last cycle health (FR-002f) |
| `last_failure_category` | `FailureCategory` \| null | most recent submission failure this cycle, or null (FR-002f, FR-027) |
| `blocked_backups` | array of `{ file_key, reason }` | reason ∈ `locked` \| `absent` \| `too_large` \| `rejected` (FR-007) |
| `last_backup_days` | map `file_key` → `YYYY-MM-DD` | optional; helps the maintainer see freshness |

## BackupSubmission (wire — `POST .../backup`, multipart/form-data)

Full contract in `contracts/backend-api.md`.

| Part | Type | Required | Notes |
|---|---|---|---|
| `content` | file part (octet-stream) | yes | whole file bytes (FR-015) |
| `instrument_serial` | text | yes | serial or `"unknown"` — authoritative for fleet token (FR-022) |
| `machine_id` | text | yes | |
| `file_key` | text | yes | from the watched-file table |
| `source_path` | text | yes | `abs_path` |
| `content_sha256` | text (64 hex) | yes | backend verifies + dedupes |
| `file_mtime` | text RFC3339 | no | file's own mtime |
| `agent_version` | text | no | `PQ_VERSION` |
| `client_timestamp` | text RFC3339 | no | cycle start |

Response of interest: `200` `{ id, deduplicated, size_bytes, received_at }`. `deduplicated:
true` is still success — record `{sha, day, ts}` (and, for a daily-limited file, advance the
gate).

## CycleRecord / CycleSummary (local log + state)

**CycleRecord** — one per cycle, written to `cycles.log` and summarised into Event Log:

| Field | Type |
|---|---|
| `started_utc`, `finished_utc` | RFC3339 |
| `product`, `instrument_serial`, `machine_id` | string |
| `heartbeat` | `{ ok: bool, category: FailureCategory? }` |
| `files` | array of `{ file_key, action, category? }` where action ∈ `sent` \| `deduplicated` \| `unchanged` \| `skipped_today` \| `blocked` \| `retry_later` |
| `duration_ms` | int |

**CycleSummary** (persisted subset in `state.json.last_cycle`): `started_utc`, `ok`,
`counts { sent, unchanged, blocked, retry_later }`, `heartbeat_ok`.

## FailureCategory (enum)

`NoNetwork` · `Auth` (401) · `RejectedBadRequest` (422/400) · `TooLarge` (413) ·
`BackendError` (5xx / unexpected) · `FileLocked` · `FileAbsent` · `Internal`

Retryable next cycle: `NoNetwork`, `Auth`, `BackendError`, `FileLocked`.
Not retried blindly (surfaced, wait for change / new build): `RejectedBadRequest`, `TooLarge`,
`FileAbsent`. (FR-006, FR-012, FR-014, FR-026, FR-027)

## Semantic Data Dictionary (`contracts/data-dictionary/`)

Meaning-layer companion to the structural schemas above (FR-036–FR-040, D15). Three files,
on the `pm100/docs/data-dictionary` pattern:

### `semantic-model.json`

Flat map, concept id → entry:

| Entry field | Type | Notes |
|---|---|---|
| `id` | string | dot-namespaced, e.g. `identity.instrument_serial`, `time.gate_day` |
| `datatype` | string | `string` \| `enum` \| `integer` \| `boolean` \| `object` \| `iso8601-datetime` \| `date` |
| `unit` | string \| null | e.g. `milliseconds`, `bytes`; null where not a measured quantity |
| `description` | string | one or two sentences, meaning not shape |
| `aliases` | array of string | other field names this concept appears under across documents |
| `parent` | string \| null | enclosing concept id for nested fields (e.g. `telemetry.os.*` → `telemetry.os`) |
| `examples` | array | at least one real (or realistic) value |
| `confidence` | enum `confirmed` \| `uncertain` | `uncertain` per FR-037 unless checked against `specs/003-backend-api-support` or code |

**Namespaces** (FR-037): `identity.*`, `telemetry.*`, `backup.*`, `time.*`, `config.*`,
`doc.*` (envelope-level fields shared by every submission: `product`, `agent_version`).

Format validated by `contracts/semantic-model.schema.json`.

### `field-mappings.json`

```json
{
  "schema_registry": {
    "v2.heartbeat_payload.v1": { "/instrument_serial": "identity.instrument_serial", "...": "..." },
    "v2.backup_submission.v1": { "/file_key": "backup.file_key", "...": "..." },
    "v2.local_state.v1": { "/files/*/last_backup_sha256": "backup.content_hash", "...": "..." }
  }
}
```

- Keys under each document id are JSON pointers into that document (or, for the
  multipart `v2.backup_submission.v1`, its flat part names) → a `semantic-model.json` id.
- `*` in a pointer denotes "any key at this position" (used for `state.json`'s `files` map,
  keyed by `file_key`).
- Every leaf pointer of `heartbeat-payload.schema.json`, `local-state.schema.json`, and the
  backup-submission part list in `contracts/backend-api.md` MUST appear here (SC-013),
  enforced by `tools/check_data_dictionary.py` (D15).

Format validated by `contracts/field-mappings.schema.json`.

### `README.md`

Explains the two files above, how to navigate from a schema file back to the dictionary, and
carries the FR-039 "same name, different concept" table (`serial`, `version`, `*_utc` /
`*_timestamp`) plus any other collision found while authoring.
