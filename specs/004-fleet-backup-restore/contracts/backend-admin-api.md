# Contract — admin API consumed by the archive tool (client side)

Backend side is `specs/003-backend-api-support`; this pins the subset `fleet_backup_pull.py`
depends on. Verified live against `api.picoquant.com` on 2026-09-08 (v2.2.0-beta.2 data).

Base URL: `https://api.picoquant.com` (override: `--api` / `$API_BASE_URL`).
`{product}` ∈ `luminosa` | `solira`.
Auth header on **every** call: `X-ADMIN-API-KEY: <maintainer admin key>`. Nothing else.

## 1. List backups — `GET /api/v2/admin/products/{product}/backups`

Query parameters (all optional):

| Param | Meaning |
|---|---|
| `instrument_serial` | exact serial filter (used only for `--serial` runs) |
| `file_key` | exact file-key filter (not used by this increment) |
| `since` / `until` | RFC3339 bounds on `received_at` |
| `limit` | page size; backend default 100, **max 1000** — the tool requests 1000 |
| `offset` | page offset |

**Success** `200`:

```json
{
  "ok": true,
  "backups": [
    {
      "id": "cd531f95-a3d6-4ba2-b316-e40d4bd7231a",
      "received_at": "2026-09-08T12:52:52.004118Z",
      "product_key": "luminosa",
      "instrument_serial": "unknown",
      "machine_id": "42db0590-55e7-4255-a4c8-421100e69d95",
      "file_key": "settings/lastknowngood.xml",
      "source_path": "C:\\ProgramData\\PicoQuant\\Luminosa\\LastKnownGood.xml",
      "file_mtime": "2026-06-26T09:12:31.158329Z",
      "content_sha256": "b72cde428cef2d296660972714e6767cf970d072358e385d0d0dd77b2df9b84a",
      "size_bytes": 77,
      "agent_version": "2.0.0-beta.2",
      "client_timestamp": "2026-09-08T12:52:51.947598Z"
    }
  ]
}
```

- Rows are **newest first**.
- **Full version history** is returned: multiple rows for the same
  `(instrument_serial, machine_id, file_key)` with distinct `id` / `received_at` /
  `content_sha256` (verified — 2 rows for `pqdevice_conf`).
- Additional fields the backend sends (`storage_backend`, `storage_ref`, `ip_address`,
  `user_agent`, …) are ignored by the tool.
- **Paging**: a full page (`len(backups) == limit`) means "there may be more" → request
  `offset += limit`. A short page ends the sweep.

**Errors → tool behaviour**

| Status | Tool behaviour |
|---|---|
| `401` / `403` | product marked **inaccessible**; logged; run continues with other products (FR-004). Contributes to exit `2` only if it leaves **no** product reachable. |
| `404` (unknown product) | same as 401/403 — product skipped |
| `5xx` / timeout / connection error | up to 3 attempts, backoff 2 s → 4 s; still failing → product's sweep marked failed (exit `1`) |

## 2. Download content — `GET /api/v2/admin/products/{product}/backups/{id}/content`

Returns the raw file bytes. `Content-Type: application/octet-stream`,
`Content-Disposition: attachment; filename="…"`.

The tool streams to `<final>.part`, hashing as it writes, then compares to the row's
`content_sha256`.

**Success** `200`: body is the exact stored bytes; downloaded SHA-256 MUST equal
`content_sha256` (verified byte-exact round trip this session).

**Errors → tool behaviour**

| Status | Tool behaviour |
|---|---|
| `404` | `pruned` — the backend dropped it between the list and this fetch. Logged as a miss, retried next run. Does **not** set exit `1` on its own. |
| `5xx` / timeout / connection error | up to 3 attempts, backoff 2 s → 4 s; still failing → `download_error` for that artifact (exit `1`) |
| digest mismatch on a `200` body | `digest_mismatch`; temp file deleted; not archived; recorded (exit `1`) |

## Not called

`…/backups/latest` (the archive wants every version, not just the newest), any write endpoint,
token minting, the TOTP flow, and the `agent_status` telemetry endpoints. This tool is
read-only against the backend.
