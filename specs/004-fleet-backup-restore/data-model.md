# Phase 1 Data Model — Fleet Backup Archive

Mostly in-memory value types plus the persisted JSON shapes (the config-backup
`manifest.json`, the US2 `_powermeter/manifest.json`, the lock file) and the archive's
directory layout itself. No database. Wire shapes are the backend's, defined in
`specs/003-backend-api-support`; the fields this tool relies on are pinned in
`contracts/backend-admin-api.md`.

## BackendBackupRow (wire — one row from `GET /admin/products/{product}/backups`)

The tool reads these fields; it ignores any others.

| Field | Type | Use |
|---|---|---|
| `id` | string (UUID) | **archive identity** — "already archived" ⇔ `id` in the machine manifest (D3) |
| `product_key` | `"luminosa"` \| `"solira"` | which product tree |
| `instrument_serial` | string | `<serial>` path segment; literal `"unknown"` allowed |
| `machine_id` | string | `<machine-id>` path segment (always present; all-zero GUID fallback) |
| `file_key` | string `[a-z0-9_./-]` | fallback path if `source_path` is unusable; grouping key for "versions of a file" |
| `source_path` | string (Windows path) | mirrored into the archive tree (drive stripped) |
| `content_sha256` | 64 hex | integrity check on download; `sha8` in the version filename |
| `size_bytes` | int | report / sanity |
| `received_at` | RFC3339 string | version ordering; `received_at` segment in the version filename; staleness (later increment) |
| `file_mtime` | RFC3339 string \| null | carried into the manifest for the restore increment |
| `agent_version` | string \| null | carried into the manifest (informational) |

Ordering: the backend returns rows **newest first**. The tool sorts ascending by `received_at`
per `(serial, machine_id, file_key)` so the last one processed is the newest → becomes the
mirrored "latest".

## BackendPowerRow (wire — one row from `GET /admin/products/powermeter/telemetry`) — US2

The whole row is the artifact (there is no per-record content endpoint). The tool stores it
verbatim and reads these fields for grouping / the manifest:

| Field | Type | Use |
|---|---|---|
| `id` | string (UUID) | **archive identity** — "already archived" ⇔ `id` in the `_powermeter` manifest |
| `product_key` | `"powermeter"` | (always powermeter here) |
| `system_serial` | string \| null/absent | the PicoQuant instrument — grouping key + `<serial>` folder; absent → `unknown` |
| `instrument_serial` | string | the **power-meter device** serial (e.g. Thorlabs `M01333314`) — manifest only |
| `measurement_type` | string | e.g. `combiner_power`; groups the `<measurement_type>.latest.json` mirror |
| `received_at` | RFC3339 string | ordering within a `measurement_type`; part of the record filename fallback |
| `measured_at` | RFC3339 string | preferred timestamp in the record filename |
| `payload` | object | the measurement itself — stored, never interpreted |
| `submitted_by`, `auth_kind`, `meta`, … | any | stored verbatim as part of the row; not read |

Response envelope: `{ "ok": true, "records": [...], "limit": int, "offset": int, "total": int }`.
Paging: request `limit=1000` + `offset`; stop on an empty/short page or when `offset >= total`.
`--serial` → the `system_serial` query filter; `--since` / `--until` pass through.

## Archive layout (on disk)

```
<root>/
├── .fleet-backup.lock                      # present only while a run holds it
├── <product>/
│   └── <serial>/                           # literal "unknown" if not known
│       ├── <machine-id>/
│       │   ├── manifest.json
│       │   ├── <rel(source_path)>          # newest version, e.g.
│       │   │                               #   ProgramData/PicoQuant/Luminosa/LastKnownGood.xml
│       │   └── _versions/
│       │       └── <rel(source_path)>/
│       │           ├── 2026-09-08T12-52-52Z__b72cde42.bak     # every version, incl. newest
│       │           └── 2026-09-07T09-10-00Z__a1b2c3d4.bak
│       └── _powermeter/                    # US2 — present when this instrument has power records
│           ├── manifest.json               # kind: "powermeter"
│           ├── combiner_power.latest.json  # newest record of each measurement_type
│           └── records/
│               └── 2026-09-10T11-43-39Z__76d36f96.json        # every measurement record (full row)
└── powermeter/                             # US2 — standalone tree for systems with no instrument folder
    └── <system_serial>/                    # literal "unknown" if absent
        ├── manifest.json
        ├── combiner_power.latest.json
        └── records/
            └── 2026-09-10T11-43-39Z__76d36f96.json
```

### `rel(source_path)` — path derivation (pure function, unit-tested)

1. `source_path.replace("\\", "/")`, strip a leading `X:/` drive, strip leading `/`.
2. Drop `.`/`..` segments.
3. If nothing usable remains, use `file_key` split on `/` instead.

`C:\ProgramData\PicoQuant\Luminosa\LastKnownGood.xml` → `ProgramData/PicoQuant/Luminosa/LastKnownGood.xml`.

### version filename

`<received_at with ':' → '-'>__<first 8 of content_sha256>.bak`
e.g. `2026-09-08T12-52-52Z__b72cde42.bak`. Self-describing; lets the manifest be rebuilt from
filenames alone.

## Manifest (`<machine-id>/manifest.json`) — persisted

Written atomically (`manifest.json.tmp` → `os.replace`). The record of what the folder holds
**and** the incremental cursor. Structural schema: `contracts/manifest.schema.json`; semantic
dictionary: `docs/data-dictionary/` (`v2.fleet_archive_manifest.v1`, added 2026-09-12,
constitution Principle VII), both cross-checked against `tools/fleet_backup_pull.py`'s
`Manifest`/`ManifestEntry` dataclasses.

| Field | Type | Notes |
|---|---|---|
| `schema_version` | int | currently `1`; unknown/greater → rebuild from filenames, log |
| `product` | string | |
| `instrument_serial` | string | |
| `machine_id` | string | |
| `updated_utc` | RFC3339 string | last successful run that touched this folder |
| `artifacts` | array of `ManifestEntry` | every archived version, any order |

**ManifestEntry**

| Field | Type | Notes |
|---|---|---|
| `id` | string (UUID) | backend id — the dedupe key |
| `file_key` | string | |
| `source_path` | string | original Windows path |
| `rel_path` | string | `rel(source_path)` — where it lives in the tree |
| `content_sha256` | 64 hex | |
| `size_bytes` | int | |
| `received_at` | RFC3339 string | |
| `file_mtime` | RFC3339 string \| null | for the restore increment |
| `agent_version` | string \| null | informational |
| `version_file` | string | path of the `_versions/…/*.bak` relative to the machine folder |
| `is_latest` | bool | true for the newest version of its `file_key` (the mirrored copy) |
| `archived_utc` | RFC3339 string | when this run wrote it |

**Rebuild rule** (missing / corrupt / higher `schema_version`): scan `_versions/**/*.bak`,
parse `received_at` + `sha8` from each filename, reconstruct entries; set `is_latest` on the
newest `received_at` per `rel_path`; entries whose `.bak` cannot be read are omitted (they
re-download). `id` is unknown for rebuilt entries → store `id: null`; the run then treats
`(rel_path, content_sha256)` as the fallback dedupe key for that folder until the next clean
manifest write restores ids.

### power record filename — US2

`<measured_at (or received_at) with ':' → '-'>__<first 8 of the backend id>.json`
e.g. `2026-09-10T11-43-39Z__76d36f96.json`. One file per record; the file holds the full
backend row (`json.dumps(row, sort_keys=True, indent=2)`).

## Power Manifest (`_powermeter/manifest.json`) — persisted — US2

Written atomically. `kind: "powermeter"` distinguishes it from the config-backup manifest.
Structural schema: `contracts/power-manifest.schema.json`; semantic dictionary:
`docs/data-dictionary/` (`v2.fleet_archive_power_manifest.v1`), both cross-checked against
`PowerManifest`/`PowerRecordEntry`.

| Field | Type | Notes |
|---|---|---|
| `schema_version` | int | currently `1` |
| `kind` | `"powermeter"` | guards against loading a config-backup manifest by mistake |
| `system_serial` | string | the PicoQuant instrument (or `"unknown"`) |
| `location` | `"luminosa"` \| `"solira"` \| `"powermeter"` | which tree this folder lives in — **pinned**, never recomputed |
| `updated_utc` | RFC3339 string | last successful run that touched this folder |
| `records` | array of `PowerRecordEntry` | every archived record, any order |

**PowerRecordEntry**

| Field | Type | Notes |
|---|---|---|
| `id` | string (UUID) | backend id — the dedupe key |
| `received_at` | RFC3339 string | ordering within a `measurement_type` |
| `measured_at` | RFC3339 string | |
| `measurement_type` | string | e.g. `combiner_power` |
| `instrument_serial` | string \| null | the power-meter **device** serial |
| `system_serial` | string | the PicoQuant instrument |
| `record_file` | string | POSIX path of `records/*.json` relative to the `_powermeter` folder |
| `content_sha256` | 64 hex | SHA-256 of the **stored JSON bytes** (own integrity ref — no backend hash exists) |
| `is_latest` | bool | true for the newest `received_at` of its `measurement_type` (the `<type>.latest.json` mirror) |
| `archived_utc` | RFC3339 string | when this run wrote it |

**Location resolution** (`resolve_power_dir`): (1) if a `_powermeter/manifest.json` already
exists under `luminosa/<serial>/`, `solira/<serial>/`, or `powermeter/<serial>/` → use it;
(2) else, first sighting → `<product>/<serial>/_powermeter/` if that instrument folder exists,
otherwise `powermeter/<serial>/`. The result is stored as `location` and never revisited.

**Rebuild rule** (missing / corrupt / wrong `kind` / higher `schema_version`): read every
`records/*.json`, take `id` / `received_at` / `measured_at` / `measurement_type` /
`system_serial` / `instrument_serial` straight from the file (the backend `id` is *inside*
the record, unlike the `.bak` case), recompute `content_sha256` from the bytes, set
`is_latest` on the newest `received_at` per `measurement_type`; unreadable files are omitted
(they re-download).

## RunReport (in-memory, printed at the end)

| Field | Type |
|---|---|
| `products` | array of `ProductResult` |
| `started_utc`, `finished_utc` | RFC3339 |
| `machines_seen` | int |
| `artifacts_added` | int |
| `artifacts_failed` | int |
| `exit_code` | `0` \| `1` \| `2` (see `contracts/cli.md`) |

**ProductResult**: `product`, `accessible: bool`, `reason: str?` (why not), `machines_seen`,
`artifacts_added`, `artifacts_failed`, `pruned`, `skipped_machines`. The `powermeter` sweep
reuses this type — `machines_seen` counts **systems**, `artifacts_added` counts **records**
(the summary line prints `systems=` / `records=` for it).

### Exit-code derivation

- any fatal precondition (no admin key / archive root unwritable / **no** product reachable)
  → **2**
- else if `artifacts_failed > 0` → **1**
- else → **0** (this includes "nothing new" and "lock held, another run in progress")

## Lock file (`<root>/.fleet-backup.lock`) — persisted

Created with `O_CREAT | O_EXCL`. Body: JSON `{ "pid": int, "host": str, "started_utc": str }`.
Removed in a `finally`. On contention: if the lock is younger than `STALE_LOCK_SECS`
(default 21600 = 6 h) and — where checkable — its pid is alive, the new run logs and **exits
0**; otherwise it reclaims the lock with a warning.

## FailureCategory (per-artifact, for the report + logs)

`download_error` (transport / 5xx after retries) · `digest_mismatch` · `pruned` (404 on
content — gone between list and fetch) · `write_error` (archive not writable) · `internal`.

None abort the run; each is counted into `artifacts_failed` (→ exit 1) except `pruned`, which
is a benign miss retried next run and does **not** set exit 1 on its own.
