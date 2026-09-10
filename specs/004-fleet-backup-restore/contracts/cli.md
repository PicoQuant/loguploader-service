# Contract — CLI (`tools/fleet_backup_pull.py`)

Invoked by a maintainer or an external scheduler (cron). One command, one job: pull new
config-backup artifacts **and new laser-power measurement records** into the local archive.

## Invocation

```
python tools/fleet_backup_pull.py [options]
```

| Option | Default | Meaning |
|---|---|---|
| `--out DIR` | `./fleet-backups` | archive root |
| `--product {luminosa,solira,powermeter}` | all three | restrict to one product (repeatable). `powermeter` pulls laser-power measurement records; omitting it from an explicit `--product` list skips them |
| `--serial SERIAL` | — | restrict to one serial (repeatable). For `luminosa`/`solira` it filters `instrument_serial`; for `powermeter` it filters `system_serial` |
| `--since ISO` | — | only artifacts with `received_at >= ISO` |
| `--until ISO` | — | only artifacts with `received_at <= ISO` |
| `--api URL` | `$API_BASE_URL` or `https://api.picoquant.com` | backend base URL |
| `--env PATH` | `./.env` | file to read `EXPECTED_ADMIN_API_KEY` (and optional `API_BASE_URL`) from |
| `--timeout SECS` | `60` | per-request HTTP timeout |
| `--quiet` | off | suppress the per-artifact detail lines; still prints the final summary |
| `--rebuild-manifests` | off | force a manifest rebuild-from-filenames for every touched folder before the run |
| `-h`, `--help` | — | usage |

No `--force`, no `--delete`, no `--prune` — the archive is append-only in this increment.

## Credential

The admin key is read from `EXPECTED_ADMIN_API_KEY` in the environment, else from `--env`.
It is sent only as the `X-ADMIN-API-KEY` request header. It is **never** written to the
archive, any `manifest.json`, the lock file, stdout, or stderr. Absent/empty → exit **2**
with a message naming the env var and file checked (not the value).

## Behaviour

1. Acquire `<out>/.fleet-backup.lock` (`O_CREAT|O_EXCL`). If held by a live, non-stale run →
   log `another run in progress; exiting` and **exit 0**.
2. For each in-scope product: page `GET /api/v2/admin/products/{product}/backups` (with any
   `instrument_serial` / `since` / `until` filters). On `401`/`403` → mark the product
   inaccessible, log it, continue.
3. Group rows by `(instrument_serial, machine_id)`; for each machine folder load (or rebuild)
   `manifest.json`.
4. For each row whose `id` is **not** in the manifest: `GET …/backups/{id}/content`, verify
   SHA-256, write `_versions/<rel>/<received_at>__<sha8>.bak` atomically, update the mirrored
   latest if this is the newest version of its `file_key`, append a `ManifestEntry`.
5. Write each touched `manifest.json` atomically.
6. **If `powermeter` is in scope**: page
   `GET /api/v2/admin/products/powermeter/telemetry` (with any `system_serial` / `since` /
   `until` filters). Group rows by `system_serial`. For each system resolve its `_powermeter`
   folder — an existing one from a prior run, else nested under `<product>/<serial>/` if that
   instrument folder exists, else standalone `powermeter/<serial>/`. For each row whose `id`
   is **not** in that folder's `_powermeter/manifest.json`: write the whole row as
   `records/<measured_at>__<id8>.json` atomically, refresh `<measurement_type>.latest.json`,
   append a `PowerRecordEntry`, write the manifest.
7. Print the run summary; release the lock; exit per the table below.

Re-running with no new artifacts downloads nothing and writes nothing (SC-002/SC-003/SC-011).

## Output (stdout)

Human-readable summary, always printed (even with `--quiet`). Shape:

```
fleet-backup-pull  <started_utc> .. <finished_utc>
  luminosa : ok        machines=214  added=3   failed=0
  solira   : SKIPPED    (403 — admin key has no access)
  powermeter: ok        systems=12  records=4  failed=0
totals: machines=214  artifacts added=3  failed=0
```

Per-artifact lines (stderr, suppressed by `--quiet`):

```
+ luminosa/SN-12345/0f4a…/ProgramData/PicoQuant/Luminosa/PQDevice.db  (2026-09-08T12:52:52Z, 40960 B)
! luminosa/SN-67890/1a2b…/…/GUISettings.xml  digest_mismatch (expected 24ab…, got 9f1c…) — not archived
+ powermeter/1051032/combiner_power  (2026-09-10T11:43:56.455172Z, 76d36f96-3c4c-498d-ac51-cd87a317d3a7)
```

## Exit codes

| Code | Meaning | Scheduler action |
|---|---|---|
| `0` | success — artifacts added or nothing new; **or** another run was already in progress | none |
| `1` | partial — the run finished but ≥1 artifact failed download/verification, a sweep failed, or a folder was blocked | look at the named instrument(s) |
| `2` | fatal — admin key missing/invalid, **no** product (incl. powermeter) reachable, or archive root not writable | the job is broken; fix and re-run |

A `pruned` miss (a `404` on content because the backend dropped it between listing and fetch)
is logged but does **not** by itself cause exit `1` — it is retried next run.

## Not in this increment

`restore`, `list` / `inspect`, `verify` (full-archive re-hash), `diff` against the backend,
stale-instrument reporting, prune. Recorded under spec *Deferred*.
