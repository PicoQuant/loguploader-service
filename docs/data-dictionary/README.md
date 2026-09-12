# loguploader-service data dictionary

Semantic data dictionary, **shared across every spec in this repo** that produces output
data: currently the v2 agent's **heartbeat** (spec 002), its **config backup** submission
(spec 002), its **local state** (`state.json`, spec 002), and the v1→v2 updater's
**upgrade-attempt** telemetry (spec 001). On the pattern of the sibling `pm100` app's
`docs/data-dictionary/`; originated by spec 002's FR-036–FR-040, now generalized as
constitution Principle VII ("Semantic Output Schema") — every feature that produces output
data registers its documents here rather than starting a new, separate dictionary.

- Each spec's own `contracts/*.schema.json` — JSON Schema (2020-12) for **structural**
  validation of *that spec's* documents (e.g.
  `specs/002-v2-config-backup-telemetry/contracts/heartbeat-payload.schema.json`,
  `specs/001-v2-remote-upgrade/contracts/upgrade-telemetry.schema.json`). A multipart body
  (the backup submission) has no schema file — its fixed part list lives in that spec's
  `contracts/backend-api.md` instead.
- [`semantic-model.json`](semantic-model.json) — canonical, tool-independent semantic
  concepts (id, datatype, unit, description, aliases, parent concept, examples, confidence),
  **one flat namespace for the whole repo**, so a concept like `identity.machine_id` or
  `doc.agent_version` is defined once and reused by every document that carries it, instead
  of being redefined (and risking drift) per spec.
- [`field-mappings.json`](field-mappings.json) — JSON-pointer-to-semantic-ID mapping for
  every covered document, keyed by a `schema_registry` document id.
- [`schema/`](schema/) — meta-schemas (`semantic-model.schema.json`,
  `field-mappings.schema.json`) validating this dictionary's *own* format — distinct from
  the per-spec structural schemas above, which validate the documents this dictionary
  *describes*.

Semantic IDs are dot-namespaced by domain (`identity.*`, `telemetry.*`, `backup.*`, `time.*`,
`doc.*`) and independent of the field's own name, so they survive a rename or a different
module reusing the same shape. Entries marked `"confidence": "uncertain"` are inferred
rather than fully confirmed against a backend contract or working code, and should be
checked with a domain expert before being relied on (currently just
`telemetry.instrument_log_version` — the exact `.pqlog` header format).

## Getting from a JSON document back to this dictionary

Neither of v2's JSON documents carries a `$schema` URL, unlike pm100's combiner record —
each spec's payload schema documents only the inner `payload` object of its wire body, never
the envelope, and there is no public host to point a URL at anyway. Where a document *can*
self-describe, it does so with a bare version tag instead — the same convention pm100 itself
falls back to for its own telemetry wire submissions (its README: "every telemetry wire
submission's `meta.schema` ... never carries a URL, only the bare version tag"):

- A heartbeat body carries `meta.schema = "v2.heartbeat_payload.v1"` (`src/telemetry.rs`'s
  `HEARTBEAT_SCHEMA_ID`) — the generic `TelemetrySubmitRequest`'s free-form `meta` field
  (`specs/003-backend-api-support/backend-changes.md`), so a raw heartbeat captured anywhere
  (a support ticket, a packet capture) resolves back to this dictionary without first
  knowing "this came from a v2 agent".
- `state.json` has no separate tag field, but doesn't need one: it always lives at one
  fixed, known path, so its existing `schema_version: 1` (already required for spec 002's
  FR-034 empty-if-unreadable-or-newer gate) unambiguously means `v2.local_state.v1` — see
  `doc.schema_version` in `semantic-model.json`.
- The backup submission (`v2.backup_submission.v1`) is `multipart/form-data`, not JSON, to
  an endpoint that only ever accepts this one shape — there is nothing to disambiguate and
  so no tag was added.
- The upgrade-attempt submission (`v2.upgrade_attempt.v1`, spec 001) currently carries **no**
  self-description tag either — it shares the same generic telemetry endpoint and `meta`
  field mechanism the heartbeat uses, so adding one later is a small, additive change
  (`src/upgrade.rs::upgrade_report_cli`), just not yet done. Its `measurement_type:
  "upgrade_attempt"` at least identifies the document *kind*, if not a versioned schema id.

Either way, the lookup goes through `field-mappings.json`'s `schema_registry`, keyed by a
document id:

| Document id | What it is | Self-description | Structural schema |
| --- | --- | --- | --- |
| `v2.heartbeat_payload.v1` | full wire body of `POST .../telemetry`, `measurement_type="agent_status"` (spec 002) | `meta.schema` field, on the wire | `specs/002-v2-config-backup-telemetry/contracts/heartbeat-payload.schema.json` (payload only; the envelope fields — `measurement_type`, `measured_at`, `instrument_serial`, `meta` — are documented in that spec's `contracts/backend-api.md` §1) |
| `v2.backup_submission.v1` | multipart parts of `POST .../backup` (spec 002) | none (single fixed shape, one endpoint) | `specs/002-v2-config-backup-telemetry/contracts/backend-api.md` §2 (no JSON Schema — not a JSON body) |
| `v2.local_state.v1` | `state.json` on disk (spec 002) | `schema_version` field, doubling as this tag | `specs/002-v2-config-backup-telemetry/contracts/local-state.schema.json` |
| `v2.upgrade_attempt.v1` | full wire body of `POST .../telemetry`, `measurement_type="upgrade_attempt"` (spec 001) | none yet (see above) | `specs/001-v2-remote-upgrade/contracts/upgrade-telemetry.schema.json` (payload only; envelope documented alongside the schema's own description) |

```
a field in one of the covered documents  →  field-mappings.json[schema_registry][doc id][pointer]  →  a semantic-model.json id  →  meaning
```

`tools/check_data_dictionary.py` (repo root) enforces that every leaf field of every
document listed in its `DOCUMENT_SOURCES` has an entry here — run it after changing a
schema, `field-mappings.json`, or the fields a document actually sends. To bring a new
spec's document under this dictionary, add one `DocumentSource` entry there and the
corresponding concepts/mappings here — there is no per-spec copy of this tooling to keep in
sync.

## "Same name, different concept" — and the reverse

The collisions below are the ones actually found while building `field-mappings.json`
across both specs; see `semantic-model.json` for the full description of each concept.

| Name(s) | Meaning A | Meaning B |
| --- | --- | --- |
| `serial` (as `instrument_serial`) | `identity.instrument_serial` — **this** agent's instrument, read from `LastOpenSerial.txt`, shared unchanged between the heartbeat and the upgrade-attempt submission | the sibling `pm100` app's `system.serial_number` concept — a *different* instrument, the one being measured by a power meter, not the one running an agent. Same English word, unrelated devices; the two dictionaries never share a concept id for this. |
| `version` | `doc.agent_version` — the **v2 agent's own** build version | `telemetry.instrument_control_version` — the Luminosa/Solira **control-software** version installed now | `telemetry.instrument_log_version` — the control-software version that **last actually ran**, from a `.pqlog` header (may lag the installed version) | `doc.upgrade_from_version` / `doc.upgrade_to_version` — a version-*transition* pair specific to one upgrade attempt, not "the current version" of anything; `to_version` defaults to `doc.agent_version` when the caller doesn't override it, so the two often coincide on a successful attempt but are not the same concept. Five independent fields across two documents — never assume any two of them are equal. |
| `measured_at` (the envelope field name itself) | on `v2.heartbeat_payload.v1`: `time.cycle_started_utc` — cycle-start time | on `v2.upgrade_attempt.v1`: `time.upgrade_attempt_utc` — an installer/updater timestamp, unrelated to any agent cycle. Same field name on the same generic telemetry envelope, two different concepts depending on `measurement_type` — resolve via the document id, never the field name alone. |
| `*_utc` / `*_timestamp` (within one document) | `time.cycle_started_utc` — when the **cycle began** (aliases: `measured_at` on the heartbeat, `cycle.started_utc`, `client_timestamp`, `last_success_utc`, `last_cycle.started_utc` — all the *same* captured instant for a given cycle, confirmed in `src/telemetry.rs` + `src/backup.rs` + `src/cycle.rs`) | `time.cycle_finished_utc` (`last_heartbeat_utc`) — when the cycle **finished**, captured separately, only on a successful heartbeat | `backup.gate_day` (`last_backup_utc_day`, `last_backup_days.<file_key>`) — a **date**, not a timestamp: the once-per-day gate's key, evaluated in UTC. Reading it as "the time of the last backup" is the specific trap this collision calls out. | `received_at` — the backend's own receipt-time field, returned in every submission's `200` response (e.g. spec 002's `contracts/backend-api.md` §1/§2). Deliberately **not** a concept in `semantic-model.json`: it's a field the backend *emits back*, not one v2 submits or persists, so there is no leaf pointer for it to attach to on either document. Named here only because it's easy to confuse with the client-side timestamps above. |
| `reason` (blocked-file) vs. failure category | `telemetry.blocked_backup_reason` — 4 values (`locked`/`absent`/`too_large`/`rejected`), only ever about *this cycle's file outcomes* | `telemetry.last_failure_category` — the full 8-value `FailureCategory` enum, and specifically the most recent **backup-pass** failure, never the heartbeat's own delivery outcome (a heartbeat cannot report its own failure inside its own body) |
| `file_key` (a field) vs. `files` (a map key) | `backup.file_key` as a *value* — the `file_key` part of a backup submission, or a `blocked_backups[].file_key` | the *same* concept as a **map key** — `state.json`'s `files` object is keyed by `file_key`, so the key itself (not a nested field) carries the meaning, documented at `field-mappings.json`'s `/files/*` pointer |

## Is a submitted document identical to what's persisted?

Not the same document at all, by design — there is no local copy of a heartbeat or a backup
submission to compare against. `state.json` only ever stores the *outcome* of a backup
(`last_backup_sha256`, `last_backup_utc_day`, `last_success_utc`) and of the last cycle
(`last_cycle`), never the file content or the heartbeat body itself — see spec 002's
`data-model.md` → "Local Backup State". The one byte-exact round trip that *is* meaningful is
`backup.content_hash`: the same SHA-256 value appears as the wire field `content_sha256`, is
recomputed by the backend, and is stored as `state.json`'s `last_backup_sha256` — verified
end-to-end in spec 002's `quickstart.md` §3.
