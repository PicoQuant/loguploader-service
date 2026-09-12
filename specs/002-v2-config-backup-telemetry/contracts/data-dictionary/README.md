# v2 agent data dictionary

Semantic data dictionary for every document the v2 agent submits or persists: the
**heartbeat** it POSTs every cycle, the **config backup** it POSTs per changed file, and the
**local state** (`state.json`) it keeps on disk between cycles. On the pattern of the sibling
`pm100` app's `docs/data-dictionary/` (spec `002-v2-config-backup-telemetry` FR-036–FR-040).

- [`../heartbeat-payload.schema.json`](../heartbeat-payload.schema.json) and
  [`../local-state.schema.json`](../local-state.schema.json) — JSON Schema (2020-12) for
  **structural** validation of those two documents. The backup submission is
  `multipart/form-data`, not JSON, so it has no schema file — its fixed part list is in
  [`../backend-api.md`](../backend-api.md) §2.
- [`semantic-model.json`](semantic-model.json) — canonical, tool-independent semantic
  concepts (id, datatype, unit, description, aliases, parent concept, examples, confidence).
- [`field-mappings.json`](field-mappings.json) — JSON-pointer-to-semantic-ID mapping for each
  of the three documents, keyed by a `schema_registry` document id.

Semantic IDs are dot-namespaced by domain (`identity.*`, `telemetry.*`, `backup.*`, `time.*`,
`doc.*`) and independent of the field's own name, so they survive a rename or a different v2
module reusing the same shape. Entries marked `"confidence": "uncertain"` are inferred rather
than fully confirmed against `specs/003-backend-api-support` or working code, and should be
checked with a domain expert before being relied on (currently just
`telemetry.instrument_log_version` — the exact `.pqlog` header format).

## Getting from a JSON document back to this dictionary

Unlike pm100's combiner record, none of v2's three documents carry a `$schema`
self-description field — `heartbeat-payload.schema.json` documents only the inner `payload`
object of the wire body, and `state.json` has no such field at all. So the lookup always goes
through `field-mappings.json`'s `schema_registry`, keyed by a document id:

| Document id | What it is | Structural schema |
| --- | --- | --- |
| `v2.heartbeat_payload.v1` | the full wire body of `POST .../telemetry` (envelope + `payload`) | `../heartbeat-payload.schema.json` (payload only; the three envelope fields — `measurement_type`, `measured_at`, `instrument_serial` — are documented in `../backend-api.md` §1) |
| `v2.backup_submission.v1` | the multipart parts of `POST .../backup` | `../backend-api.md` §2 (no JSON Schema — not a JSON body) |
| `v2.local_state.v1` | `state.json` on disk | `../local-state.schema.json` |

```
a field in one of the three documents  →  field-mappings.json[schema_registry][doc id][pointer]  →  a semantic-model.json id  →  meaning
```

`tools/check_data_dictionary.py` (repo root) enforces that every leaf field of all three
documents has an entry here (SC-013) — run it after changing a schema, `field-mappings.json`,
or the fields a document actually sends.

## "Same name, different concept" — and the reverse

The collisions below are the ones actually found while building `field-mappings.json`
(FR-039); see `semantic-model.json` for the full description of each concept.

| Name(s) | Meaning A | Meaning B |
| --- | --- | --- |
| `serial` (as `instrument_serial`) | `identity.instrument_serial` — **this** agent's instrument, read from `LastOpenSerial.txt` | the sibling `pm100` app's `system.serial_number` concept — a *different* instrument, the one being measured by a power meter, not the one running an agent. Same English word, unrelated devices; the two dictionaries never share a concept id for this. |
| `version` | `doc.agent_version` — the **v2 agent's own** build version | `telemetry.instrument_control_version` — the Luminosa/Solira **control-software** version installed now | `telemetry.instrument_log_version` — the control-software version that **last actually ran**, from a `.pqlog` header (may lag the installed version). Three independent, individually-nullable fields (FR-004a) — never assume any two of them are equal. |
| `*_utc` / `*_timestamp` | `time.cycle_started_utc` — when the **cycle began** (aliases: `measured_at`, `cycle.started_utc`, `client_timestamp`, `last_success_utc`, `last_cycle.started_utc` — all the *same* captured instant for a given cycle, confirmed in `src/telemetry.rs` + `src/backup.rs` + `src/cycle.rs`) | `time.cycle_finished_utc` (`last_heartbeat_utc`) — when the cycle **finished**, captured separately, only on a successful heartbeat | `backup.gate_day` (`last_backup_utc_day`, `last_backup_days.<file_key>`) — a **date**, not a timestamp: the once-per-day gate's key, evaluated in UTC. Reading it as "the time of the last backup" is the specific trap FR-039 calls out. |
| `reason` (blocked-file) vs. failure category | `telemetry.blocked_backup_reason` — 4 values (`locked`/`absent`/`too_large`/`rejected`), only ever about *this cycle's file outcomes* | `telemetry.last_failure_category` — the full 8-value `FailureCategory` enum, and specifically the most recent **backup-pass** failure, never the heartbeat's own delivery outcome (a heartbeat cannot report its own failure inside its own body) |
| `file_key` (a field) vs. `files` (a map key) | `backup.file_key` as a *value* — the `file_key` part of a backup submission, or a `blocked_backups[].file_key` | the *same* concept as a **map key** — `state.json`'s `files` object is keyed by `file_key`, so the key itself (not a nested field) carries the meaning, documented at `field-mappings.json`'s `/files/*` pointer |

## Is a submitted document identical to what's persisted?

Not the same document at all, by design — there is no local copy of a heartbeat or a backup
submission to compare against. `state.json` only ever stores the *outcome* of a backup
(`last_backup_sha256`, `last_backup_utc_day`, `last_success_utc`) and of the last cycle
(`last_cycle`), never the file content or the heartbeat body itself — see `data-model.md` →
"Local Backup State". The one byte-exact round trip that *is* meaningful is
`backup.content_hash`: the same SHA-256 value appears as the wire field `content_sha256`, is
recomputed by the backend, and is stored as `state.json`'s `last_backup_sha256` — verified
end-to-end in `quickstart.md` §3.
