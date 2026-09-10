# Backend Changes Brief — v2 Fleet Agent Support

**Audience**: a coding agent implementing changes in the `api.picoquant.com` backend
repository.
**Client side**: `loguploader-service` v2 (see the *Client contract* appendix and
`specs/002-v2-config-backup-telemetry/spec.md`).
**Goal**: let an unattended fleet of Windows agents (products **Luminosa** and **Solira**)
submit version telemetry and configuration-file backups using a **single non-expiring token
per product**, with backups stored durably and retrievable by admins.

Do **not** break existing behaviour: instrument-bound minted tokens, the TOTP technician
session flow, and current telemetry submissions must keep working exactly as they do now.

---

## Phase 0 — Recon (do this first, adjust everything below to reality)

1. Fetch and skim the live contract:
   ```
   curl -s https://api.picoquant.com/openapi.json | jq '.paths | keys'
   curl -s https://api.picoquant.com/openapi.json | jq '.components.securitySchemes'
   ```
   Known today: OpenAPI 3.1, `info.version = "2"`, the **only** declared security scheme is
   `APIKeyHeader` = `X-ADMIN-API-KEY`. `POST /api/v2/products/{product_key}/telemetry` is
   declared as requiring that scheme, with body `TelemetrySubmitRequest`
   (`measurement_type`, `instrument_serial?`, `system_serial?`, `measured_at?`, `payload`,
   `meta?`; required: `measurement_type`, `payload`).

2. In the repo, locate and read:
   - the app entrypoint (`app.py` / `main.py`) and router registration
   - the telemetry submission handler and its auth dependency (search for `X-TELEMETRY-TOKEN`,
     `X-API-KEY`, `X-ADMIN-API-KEY`, `telemetry_tokens`, `token_hash`)
   - the token model / table definition (`telemetry_tokens`: `kind` in `apikey|session`,
     `token_hash` SHA-256, `instrument_serial`, `expires_at`, `revoked`, `minted_by`, …)
   - config loading (`EXPECTED_API_KEY_TELEMETRY_<PRODUCT>`, `ALLOWED_PRODUCTS`,
     `EXPECTED_ADMIN_API_KEY`, `TELEMETRY_*`)
   - `tlayer.db.apply_additive_migrations` and how tables are first created
   - `telemetry.archive_and_prune` and the daily retention loop
   - the test suite (`tests/test_telemetry.py`, smoke test, fixtures)
   - any existing blob/object-storage abstraction

3. Reconcile: if the stack, table names, config mechanism, or auth flow differ from the
   *Assumptions* in `spec.md`, keep the **contracts** in this brief (paths, fields, headers,
   status codes, env-var names, semantics) and adapt the **implementation notes** to the real
   code. If a contract itself cannot work against the real backend, stop and report why.

---

## Change A — Enable the `solira` product

Low risk, do first.

- Add `solira` (and `luminosa` if missing) to `ALLOWED_PRODUCTS`. Confirm product-key
  validation is purely list membership (`[a-z0-9_]`, used to build env-var names).
- Provision the mint key env var for parity if the code requires one per product:
  `EXPECTED_API_KEY_TELEMETRY_SOLIRA` (falls back to `EXPECTED_API_KEY` if unset — verify).
- No schema change. `product_key` is already a column on `telemetry_records` / `telemetry_tokens`.

**Acceptance**: `POST /api/v2/products/solira/telemetry` behaves exactly like `luminosa`
(same auth, same storage), and `GET /api/v2/admin/products/solira/telemetry` lists records.

---

## Change B — Per-product fleet token

### Semantics (the contract — do not deviate)

- **Header**: `X-TELEMETRY-TOKEN`, same header the instrument-bound tokens use.
- **Scope**: one or more opaque token values **per product**. Any configured value is valid
  for that product's submission and backup endpoints.
- **Never expires**, is **not bound to an instrument**, has **no** `X-ADMIN-API-KEY` powers
  (cannot read, list, revoke, mint, or enroll).
- When a request authenticates via a fleet token:
  - `instrument_serial` in the **request body is required** and is taken as authoritative
    (no "token serial vs body serial" 403 check — that check applies only to instrument-bound
    tokens).
  - the stored record's attribution is `auth_kind = "fleet"`, `submitted_by = null`,
    `token_id = null` (or a synthetic non-DB id — pick one and be consistent).
- **Rotation**: multiple values valid simultaneously. Operator adds the new value, redeploys,
  waits for the client fleet to update, then removes the old value.

### Implementation (recommended: config, no DB)

- New env var per product: `TELEMETRY_FLEET_TOKENS_<PRODUCT>` — comma-separated list of token
  values (or of their lowercase SHA-256 hex digests; match how existing keys are stored —
  the prose docs say tokens are stored **hashed**, so prefer storing/comparing hashes and
  accept plaintext values in the env for convenience, hashing on load).
  - Example: `TELEMETRY_FLEET_TOKENS_LUMINOSA`, `TELEMETRY_FLEET_TOKENS_SOLIRA`.
  - Empty / unset → the product has no fleet token (feature off for it).
- Extend the submission auth dependency: after the existing `X-TELEMETRY-TOKEN` lookup fails
  to find an instrument-bound/session token, check the value (constant-time compare) against
  the configured fleet tokens for the path's `product_key`. On match → authenticate as fleet.
- Token value format: generate with `secrets.token_urlsafe(32)`; document that operators mint
  it out-of-band (no endpoint needed). If the team prefers an endpoint, add
  `POST /api/v2/admin/products/{product_key}/telemetry/fleet-tokens` (admin key) that just
  returns a freshly generated value to paste into config — still not DB-backed.

### Alternative (if the team wants per-token revocation)

Add `kind = "fleet"` to `telemetry_tokens` with `instrument_serial = NULL`,
`expires_at = NULL`; mint via a new admin endpoint; the auth path treats a non-revoked
`kind="fleet"` row for the matching `product_key` as valid. More moving parts; only do this
if per-value revocation without redeploy is a hard requirement.

**Acceptance**:
- A submission to `luminosa` with a configured fleet token and a body `instrument_serial`
  succeeds and stores the body serial.
- The same token used against `GET /api/v2/admin/...` is rejected (401/403).
- The same token used for the `solira` path (different product) is rejected unless also
  configured there.
- An instrument-bound token still works unchanged; a fleet-token request with **no** body
  `instrument_serial` returns `422`.

---

## Change C — Telemetry submission accepts the fleet token

`POST /api/v2/products/{product_key}/telemetry` — no path/shape change, only auth:

- Accept `X-TELEMETRY-TOKEN` that resolves to either an instrument-bound token (as today) or a
  fleet token for `{product_key}` (Change B).
- Keep accepting `X-ADMIN-API-KEY` if it does today (backward compat).
- With a fleet token: require body `instrument_serial`; store it; set `auth_kind="fleet"`.
- Update the OpenAPI: add security scheme `TelemetryTokenHeader`
  (`type: apiKey, in: header, name: X-TELEMETRY-TOKEN`) and set this endpoint's `security` to
  `[{TelemetryTokenHeader: []}, {APIKeyHeader: []}]`.

The v2 client submits its heartbeat here with `measurement_type: "agent_status"` (see
appendix). No server-side special-casing of that type is required — `payload` is open JSON.

**Acceptance**: heartbeat submissions from a fleet token land in `telemetry_records` with the
correct `product_key`, `instrument_serial`, and `payload`, and are returned by the admin list
filtered by `measurement_type=agent_status`.

---

## Change D — Configuration backup endpoint

### `POST /api/v2/products/{product_key}/backup`

- **Auth**: `X-TELEMETRY-TOKEN` (fleet or instrument-bound). Not admin.
- **Content-Type**: `multipart/form-data`.
  - file part **`content`** — the raw file bytes (never base64).
  - text parts:

    | field | type | required | notes |
    |---|---|---|---|
    | `instrument_serial` | string ≤128 | yes | authoritative for fleet tokens |
    | `machine_id` | string ≤128 | yes | OS machine GUID from the client |
    | `file_key` | string ≤128, `[a-z0-9_./-]` | yes | logical id, e.g. `pqdevice_db`, `pqdevice_conf`, `settings/DeviceConfig.xml`, `usersettings/User.xml` |
    | `source_path` | string ≤512 | yes | original absolute path on the machine |
    | `content_sha256` | 64 hex | yes | server recomputes and must match → else `422` |
    | `file_mtime` | ISO-8601 | no | file's own mtime |
    | `agent_version` | string ≤64 | no | v2 build version |
    | `client_timestamp` | ISO-8601 | no | when the client produced the submission |

- **Size limit**: `CONFIG_BACKUP_MAX_BYTES` (default `52428800` = 50 MB). Over → `413`.
- **De-duplication**: if a row already exists with the same
  `(product_key, instrument_serial, file_key, content_sha256)`, do **not** store the bytes
  again; return `200` with `"deduplicated": true` and the existing `id`.
- **Storage**: if the repo already has a blob/object-storage abstraction, use it
  (`storage_backend`, `storage_ref`). Otherwise store bytes in a `config_backup_blobs` table
  (`backup_id` PK/FK, `content BYTEA/BLOB`) so the metadata table stays light. Keep it behind
  one function (`config_backup.store(...)` / `.load(...)`) so it can move to object storage
  later — mirror how `telemetry.archive_and_prune` is described as swappable.
- **Response** `200`:
  ```json
  {
    "ok": true,
    "id": "…",
    "product_key": "luminosa",
    "instrument_serial": "…",
    "file_key": "pqdevice_db",
    "content_sha256": "…",
    "size_bytes": 12345,
    "received_at": "2026-09-08T10:00:01+00:00",
    "deduplicated": false
  }
  ```
- **Errors**: `401` bad token, `404` unknown product, `413` too large, `422` sha mismatch /
  missing required field / bad `file_key`.

### Admin retrieval

- `GET /api/v2/admin/products/{product_key}/backups` — auth `X-ADMIN-API-KEY`.
  Query: `instrument_serial`, `file_key`, `since`, `until`, `limit` (default 100, max 1000),
  `offset`. Returns metadata rows (no content), newest first.
- `GET /api/v2/admin/products/{product_key}/backups/latest?instrument_serial=&file_key=` —
  the single newest row for that pair (404 if none).
- `GET /api/v2/admin/products/{product_key}/backups/{backup_id}/content` — raw bytes,
  `Content-Type: application/octet-stream`, `Content-Disposition: attachment; filename="…"`
  using the basename of `source_path`.

### Retention

- `archive_and_prune` (the daily telemetry loop) MUST NOT read or delete `config_backups` or
  its blobs.
- Optional, default-off pruning of backups:
  - `CONFIG_BACKUP_KEEP_VERSIONS` (default `0` = keep all) — keep newest N per
    `(product_key, instrument_serial, file_key)`.
  - `CONFIG_BACKUP_RETENTION_DAYS` (default `0` = forever).
  - If either is enabled, prune in the same daily loop but as a separate, clearly named step.

**Acceptance**: see the checklist.

---

## Data model

New table **`config_backups`** (create via the same startup path the other tables use;
`apply_additive_migrations` only adds columns to existing tables, so a brand-new table needs
`create_all`/equivalent — verify):

| column | type | notes |
|---|---|---|
| `id` | text/uuid PK | |
| `received_at` | timestamptz | index |
| `product_key` | text | index |
| `instrument_serial` | text | index |
| `machine_id` | text | |
| `file_key` | text | index |
| `source_path` | text | |
| `file_mtime` | timestamptz null | |
| `content_sha256` | text (64 hex) | index |
| `size_bytes` | bigint | |
| `storage_backend` | text | `db` \| `s3` \| … |
| `storage_ref` | text null | object key when external |
| `agent_version` | text null | |
| `client_timestamp` | timestamptz null | |
| `ip_address` | text null | audit |
| `user_agent` | text null | audit |

- Unique index `(product_key, instrument_serial, file_key, content_sha256)` — dedupe.
- Index `(product_key, instrument_serial, file_key, received_at desc)` — "latest" + list.
- Optional `config_backup_blobs(backup_id PK FK, content bytea)` if storing in the DB.

`telemetry_records` / `telemetry_archive`: add nullable `auth_kind` (text) via
`apply_additive_migrations` — values `apikey` \| `session` \| `fleet` \| `admin`. Backfill
not required (nullable). Denormalise it onto the record like `submitted_by` so it survives
token pruning.

`telemetry_tokens`: only if you take Change B's DB alternative — extend the `kind` check
constraint / enum to include `fleet` and allow `instrument_serial` / `expires_at` NULL for
that kind.

---

## Configuration summary (new)

| Env var | Purpose | Default |
|---|---|---|
| `ALLOWED_PRODUCTS` | add `luminosa,solira` | (existing) |
| `EXPECTED_API_KEY_TELEMETRY_SOLIRA` | Solira mint key (parity; optional if only fleet tokens used) | falls back to `EXPECTED_API_KEY` |
| `TELEMETRY_FLEET_TOKENS_LUMINOSA` | comma-separated valid fleet tokens (or their sha256) for Luminosa | unset = off |
| `TELEMETRY_FLEET_TOKENS_SOLIRA` | same, Solira | unset = off |
| `CONFIG_BACKUP_MAX_BYTES` | reject larger backup bodies | `52428800` (50 MB) |
| `CONFIG_BACKUP_STORAGE` | `db` or an object-storage backend id | `db` |
| `CONFIG_BACKUP_KEEP_VERSIONS` | keep newest N per (product,instrument,file); `0` = all | `0` |
| `CONFIG_BACKUP_RETENTION_DAYS` | age-prune backups; `0` = forever | `0` |

---

## Backward compatibility (must all remain true)

- Instrument-bound minted tokens: unchanged (mint, submit, 403 on serial mismatch, revoke).
- TOTP technician enroll / session: unchanged.
- Existing `POST .../telemetry` callers using `X-ADMIN-API-KEY` or an instrument token:
  unchanged responses.
- `archive_and_prune` behaviour for `telemetry_records` / `telemetry_archive`: unchanged.
- `uniharp` and any other current product: unaffected.

---

## Phased execution plan

| Phase | Content | Independently testable? | Unblocks |
|---|---|---|---|
| 0 | Recon + reconcile | n/a | everything |
| 1 | Change A (`solira`) + Change B (fleet token auth, config-based) | yes — unit tests on the auth dependency | — |
| 2 | Change C (telemetry submission accepts fleet token, serial-in-body, `auth_kind`) + OpenAPI security scheme | yes — submission integration tests | **client can start integrating heartbeats** |
| 3 | Change D data model + `POST …/backup` (integrity, dedupe, size, storage fn) | yes — backup integration tests | **client can start integrating backups** |
| 4 | Change D admin retrieval endpoints (list / latest / content) | yes | maintainer fleet visibility |
| 5 | Retention exclusion + optional pruning knobs | yes — retention-loop tests | — |
| 6 | Docs (telemetry markdown + OpenAPI paths/schemas) + full test pass | yes | — |

Ship phases 1–2 first; the client team can integrate against them while 3–6 land.

---

## Test plan

Add to the existing suite (`tests/`), mirroring `test_telemetry.py` style:

- **`test_fleet_token.py`**
  - fleet token submits telemetry for its product with body serial → 200, stored serial =
    body serial, `auth_kind = "fleet"`
  - fleet token without body serial → 422
  - fleet token on the admin list endpoint → 401/403
  - fleet token for the wrong product → 401
  - rotation: two configured values, both work; remove one, only the other works
  - instrument-bound token still works unchanged
- **`test_backup.py`**
  - happy path multipart upload → 200, row present, content retrievable byte-for-byte
  - sha256 mismatch → 422, nothing stored
  - identical re-upload → 200 `deduplicated: true`, no second row / blob
  - body over `CONFIG_BACKUP_MAX_BYTES` → 413
  - unknown product → 404
  - bad token → 401
  - admin list filters by `instrument_serial` + `file_key`; `latest` returns newest;
    `content` returns bytes with a filename
  - `archive_and_prune` run does not touch `config_backups`
  - with `CONFIG_BACKUP_KEEP_VERSIONS=2`, a 3rd version prunes the oldest
- **`test_solira.py`** (or parametrise existing tests): `solira` behaves like `luminosa`.

Local run (SQLite), extending the documented setup:
```
export DATABASE_URL="sqlite+aiosqlite:///./telemetry.db"
export ALLOWED_PRODUCTS="uniharp,luminosa,solira"
export EXPECTED_ADMIN_API_KEY="admin-key"
export TELEMETRY_FLEET_TOKENS_LUMINOSA="test-fleet-luminosa"
export TELEMETRY_FLEET_TOKENS_SOLIRA="test-fleet-solira"
uvicorn app:app --reload
```

---

## Acceptance checklist

- [ ] `solira` accepted everywhere `luminosa` is; `uniharp` unaffected.
- [ ] `TELEMETRY_FLEET_TOKENS_LUMINOSA` / `_SOLIRA` configure 1..n valid non-expiring tokens.
- [ ] Fleet token authenticates `POST …/telemetry` and `POST …/backup` for its product only.
- [ ] Fleet token is rejected by every `X-ADMIN-API-KEY` endpoint.
- [ ] With a fleet token, body `instrument_serial` is required and authoritative; missing → 422.
- [ ] `telemetry_records.auth_kind` recorded (`fleet` for fleet-token submissions).
- [ ] Token rotation works with no downtime (two values valid at once).
- [ ] `POST …/backup` multipart: integrity-checked, size-limited (`413`), de-duplicated.
- [ ] `config_backups` table + indexes created on startup; blob storage behind one function.
- [ ] Admin: list by instrument+file, fetch latest, download raw content with a filename.
- [ ] `archive_and_prune` never touches backups; optional prune knobs default to keep-forever.
- [ ] Instrument-bound tokens, TOTP session flow, existing telemetry submissions: unchanged.
- [ ] OpenAPI updated: `X-TELEMETRY-TOKEN` security scheme; `…/backup` + admin backup paths
      with schemas; `solira` reflected.
- [ ] Telemetry markdown doc updated with the fleet-token section and the backup endpoints.
- [ ] New tests added and the full suite passes.
- [ ] No secrets committed.

---

## Appendix — Client contract (what v2 sends / expects)

From `specs/002-v2-config-backup-telemetry`. The backend must satisfy this exactly.

### Heartbeat (every interval, ~minutes–hourly)

```
POST /api/v2/products/{luminosa|solira}/telemetry
X-TELEMETRY-TOKEN: <that product's fleet token>
Content-Type: application/json

{
  "measurement_type": "agent_status",
  "measured_at": "<client UTC ISO-8601>",
  "instrument_serial": "<serial or 'unknown'>",
  "payload": {
    "machine_id": "<OS machine GUID>",
    "agent_version": "<v2 build version>",
    "product": "luminosa",
    "os": { "version": "...", "build": "...", "arch": "..." },
    "blocked_backups": [
      { "file_key": "pqdevice_db", "reason": "locked|oversize|rejected|absent" }
    ]
  }
}
```
Expected: `200` with `{ ok, id, received_at, ... }`. Client treats any `2xx` as success,
`401` as auth failure (category `authentication`), `413`/`422` as non-retryable, `5xx` /
network as retryable.

### Config backup (per changed file; settings files ≤ once per file per UTC day, `PQDevice.db`/`PQDevice.conf` on every change — spec 002 FR-011a)

```
POST /api/v2/products/{luminosa|solira}/backup
X-TELEMETRY-TOKEN: <that product's fleet token>
Content-Type: multipart/form-data

content=<raw file bytes>
instrument_serial=<serial or 'unknown'>
machine_id=<OS machine GUID>
file_key=<pqdevice_db | pqdevice_conf | settings/<name>.xml | usersettings/<name>.xml>
source_path=<original absolute path>
content_sha256=<hex>
file_mtime=<ISO-8601>
agent_version=<v2 build version>
client_timestamp=<ISO-8601>
```
Expected: `200` with `{ ok, id, deduplicated, received_at, ... }`. Only a `200` records the
file as backed-up (and, for a daily-limited settings file, consumes that UTC day's
allowance); anything else → retry next cycle (except `413`/`422` which the client records as
skip-with-reason and surfaces in the next heartbeat). The backend contract is identical for
both file classes — the once-per-day gate is entirely client-side.

### Luminosa watched files (client side, for reference)

- `C:\Program Files\PicoQuant\Luminosa\PQDevice.db` → `file_key` `pqdevice_db`
- `C:\Program Files\PicoQuant\Luminosa\PQDevice.conf` → `file_key` `pqdevice_conf`
- `C:\ProgramData\PicoQuant\Luminosa\*.xml` → `file_key` `settings/<name>.xml`
- `C:\ProgramData\PicoQuant\Luminosa\UserSettings\*.xml` → `file_key` `usersettings/<name>.xml`
- Logs (`Logs\*.pqlog`, `LaserPower.log`) are **not** sent.

Solira: same layout with `Solira` substituted (pending final confirmation).

### Tokens

- One value per product, non-expiring, shipped in that product's build. The value is injected
  at build time from a GitHub Actions secret named identically to the backend variable —
  `TELEMETRY_FLEET_TOKENS_LUMINOSA` / `TELEMETRY_FLEET_TOKENS_SOLIRA` — never committed.
  Rotation = new build carrying the new value, then retire the old value from the backend's
  `TELEMETRY_FLEET_TOKENS_<PRODUCT>` list. The client sends whatever single value it was built
  with; it does not renew or mint.
