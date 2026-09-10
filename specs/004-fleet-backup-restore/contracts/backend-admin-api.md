# Contract — admin API consumed by the archive tool (client side)

Backend side is `specs/003-backend-api-support`; this pins the subset `fleet_backup_pull.py`
depends on. §1/§2 verified live against `api.picoquant.com` on 2026-09-08 (v2.2.0-beta.2
data); §3 (powermeter telemetry, US2) verified live on 2026-09-10.

Base URL: `https://api.picoquant.com` (override: `--api` / `$API_BASE_URL`).
`{product}` ∈ `luminosa` | `solira` for §1/§2; `powermeter` for §3.
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

## 3. List power-meter telemetry — `GET /api/v2/admin/products/powermeter/telemetry` (US2)

Verified live 2026-09-10. Auth header `X-ADMIN-API-KEY`, same as §1/§2.

Query parameters (all optional):

| Param | Meaning |
|---|---|
| `system_serial` | exact PicoQuant-instrument-serial filter (this is what `--serial` passes for powermeter) |
| `instrument_serial` | exact power-meter-device-serial filter (not used by this tool) |
| `measurement_type` | exact type filter (not used — the archive wants every type) |
| `since` / `until` | RFC3339 bounds on `received_at` |
| `limit` | page size; the tool requests `1000` |
| `offset` | page offset |

**Success** `200`:

```json
{
  "ok": true,
  "records": [
    {
      "id": "76d36f96-3c4c-498d-ac51-cd87a317d3a7",
      "received_at": "2026-09-10T11:43:56.455172Z",
      "product_key": "powermeter",
      "instrument_serial": "M01333314",
      "system_serial": "1051032",
      "submitted_by": "tech@picoquant.com",
      "auth_kind": "session",
      "measurement_type": "combiner_power",
      "measured_at": "2026-09-10T11:43:39Z",
      "payload": { "schema": "pm100.combiner_record.v2", "records": [ ... ] }
    }
  ],
  "limit": 1000,
  "offset": 0,
  "total": 5
}
```

- **The list row is the artifact.** `payload` is the full measurement; there is **no**
  `…/telemetry/{id}` content endpoint (verified `404`). The tool stores
  `json.dumps(row, sort_keys=True, indent=2)` as one `records/<measured_at>__<id8>.json`.
- **No `content_sha256`** on the row — the tool records its own SHA-256 of the stored bytes;
  there is nothing external to verify against, so §2's digest-mismatch path does not apply.
- **Paging**: request `limit=1000` + `offset`; stop on an empty/short page or when
  `offset >= total`.
- `system_serial` may be absent → the record is archived under `powermeter/unknown/`.

**Errors → tool behaviour**

| Status | Tool behaviour |
|---|---|
| `401` / `403` / `404` | `powermeter` marked **inaccessible**; logged; the config-backup products still archive. Contributes to exit `2` only if it leaves **no** product reachable. |
| `5xx` / timeout / connection error | up to 3 attempts, backoff 2 s → 4 s; still failing → the `powermeter` sweep is marked failed (exit `1`) |

## Not called

`…/backups/latest` (the archive wants every version, not just the newest), any write endpoint,
token minting, the TOTP flow, and the `agent_status` / `upgrade_attempt` **heartbeat**
telemetry (for `luminosa` / `solira`). This tool is read-only against the backend.
