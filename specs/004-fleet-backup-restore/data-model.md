# Phase 1 Data Model — Fleet Backup Archive

Mostly in-memory value types plus two persisted JSON shapes (`manifest.json`, the lock file)
and the archive's directory layout itself. No database. Wire shapes are the backend's, defined
in `specs/003-backend-api-support`; the fields this tool relies on are pinned in
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

## Archive layout (on disk)

```
<root>/
├── .fleet-backup.lock                      # present only while a run holds it
└── <product>/
    └── <serial>/                           # literal "unknown" if not known
        └── <machine-id>/
            ├── manifest.json
            ├── <rel(source_path)>           # newest version, e.g.
            │                                #   ProgramData/PicoQuant/Luminosa/LastKnownGood.xml
            └── _versions/
                └── <rel(source_path)>/
                    ├── 2026-09-08T12-52-52Z__b72cde42.bak     # every version, incl. newest
                    └── 2026-09-07T09-10-00Z__a1b2c3d4.bak
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
**and** the incremental cursor.

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
`artifacts_added`, `artifacts_failed`.

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
