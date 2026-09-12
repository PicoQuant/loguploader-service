# Contract — Outbound calls to api.picoquant.com

This is the **client side** of the contract. The backend side and its data model are in
`specs/003-backend-api-support/backend-changes.md`; that contract was verified live against
`api.picoquant.com` **v2.2.0-beta.2** on 2026-09-08.

Base URL: `https://api.picoquant.com` (override: `config.toml` → `api_base_url`).
`{product}` ∈ `luminosa` | `solira` (the build's bucket).
Auth header on every call: `X-TELEMETRY-TOKEN: <compiled-in fleet token>`. Never
`X-ADMIN-API-KEY`. (FR-020)

## 1. Heartbeat — `POST /api/v2/products/{product}/telemetry`

`Content-Type: application/json`

```json
{
  "measurement_type": "agent_status",
  "measured_at": "2026-09-08T08:17:11Z",
  "instrument_serial": "SN-12345",
  "payload": {
    "machine_id": "0f4a...-...-...",
    "agent_version": "2.0.0",
    "product": "luminosa",
    "channel": "stable",
    "serial_source": "file",
    "os": { "version": "10.0.19045", "build": "19045", "arch": "x86_64" },
    "instrument_software": { "version": "1.0.0.5415", "log_version": "1.0.0.2094" },
    "cycle": { "started_utc": "2026-09-08T08:17:10Z", "duration_ms": 812, "ok": true },
    "last_failure_category": null,
    "blocked_backups": [
      { "file_key": "pqdevice_db", "reason": "locked" }
    ],
    "last_backup_days": { "pqdevice_conf": "2026-09-08" }
  },
  "meta": { "schema": "v2.heartbeat_payload.v1" }
}
```

- `instrument_serial`: real serial, or the string `"unknown"` (FR-009a). Required for the
  fleet token — omitting it returns `422 "instrument_serial is required for fleet-token
  submissions"` (verified).
- `payload` must be a non-empty object (backend rejects `{}` with `422`).
- `blocked_backups[].reason` ∈ `locked` | `absent` | `too_large` | `rejected`.
- `meta.schema`: this envelope's self-declared document id in
  `contracts/data-dictionary/field-mappings.json`'s `schema_registry` — a bare version tag,
  not a URL (constitution Principle VII, `contracts/data-dictionary/README.md`). `meta` is
  the generic `TelemetrySubmitRequest`'s optional free-form field
  (`specs/003-backend-api-support/backend-changes.md`), same field pm100 uses for its own
  `meta.schema`.

**Success** `200`:
```json
{ "ok": true, "id": "8b81...", "product_key": "luminosa",
  "instrument_serial": "SN-12345", "measurement_type": "agent_status",
  "received_at": "2026-09-08T08:17:11.560914Z" }
```

**Errors → client category**: `401` → `Auth`; `404` (unknown product) → `RejectedBadRequest`
(config/build error, surface loudly, do not retry blindly); `422` → `RejectedBadRequest`;
`5xx` / timeout / connection refused → `BackendError` / `NoNetwork` (retry next cycle).

## 2. Config backup — `POST /api/v2/products/{product}/backup`

`Content-Type: multipart/form-data`

| Part | Required | Example |
|---|---|---|
| `content` (file) | yes | raw bytes of the file; `Content-Type: application/octet-stream` |
| `instrument_serial` | yes | `SN-12345` or `unknown` |
| `machine_id` | yes | `0f4a...` |
| `file_key` | yes | `pqdevice_conf` / `settings/DeviceConfig.xml` / `usersettings/User.xml` |
| `source_path` | yes | `C:\Program Files\PicoQuant\Luminosa\PQDevice.conf` |
| `content_sha256` | yes | 64 hex; backend recomputes and must match |
| `file_mtime` | no | `2026-09-08T07:55:00Z` |
| `agent_version` | no | `2.0.0` |
| `client_timestamp` | no | cycle start, RFC3339 |

**Success** `200`:
```json
{ "ok": true, "id": "1e86...", "product_key": "luminosa",
  "instrument_serial": "SN-12345", "file_key": "pqdevice_conf",
  "content_sha256": "643f...", "size_bytes": 47,
  "received_at": "...", "deduplicated": false }
```
`deduplicated: true` is returned for byte-identical re-sends and is **also success** — the
client records `{sha, day, ts}` and, for a daily-limited file, advances the once-per-day gate
(verified behaviour). `pqdevice_db` / `pqdevice_conf` are exempt from that gate (FR-011a).

**Errors → client category**: `401` → `Auth`; `413` (over `CONFIG_BACKUP_MAX_BYTES`, backend
default 50 MB) → `TooLarge` (skip, surface, no blind retry — FR-014); `422` (sha mismatch /
missing field / bad `file_key`) → `RejectedBadRequest`; `404` → `RejectedBadRequest`;
`5xx` / network → `BackendError` / `NoNetwork` (retry next cycle).

## Retry policy (both endpoints)

- Per submission: up to 3 attempts, backoff 2 s then 4 s.
- Retry only `NoNetwork` and `BackendError` within a cycle. Never retry a 4xx within a cycle.
- A submission unfinished after 3 attempts is left for the next cycle (heartbeat: latest
  state only, no backlog — FR-006; backup: daily allowance not consumed — FR-012).

## Not called by the agent

The agent never calls the admin endpoints (`/api/v2/admin/...`), token minting, or the TOTP
session flow. Those are for maintainers and use `X-ADMIN-API-KEY`. Fleet visibility and
backup retrieval happen there (`specs/003-backend-api-support`), off-device.
