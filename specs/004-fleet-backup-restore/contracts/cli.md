# Contract — CLI (`tools/fleet_backup_pull.py`)

Invoked by a maintainer or an external scheduler (cron). One command, one job: pull new
config-backup artifacts into the local archive.

## Invocation

```
python tools/fleet_backup_pull.py [options]
```

| Option | Default | Meaning |
|---|---|---|
| `--out DIR` | `./fleet-backups` | archive root |
| `--product {luminosa,solira}` | both | restrict to one product (repeatable) |
| `--serial SERIAL` | — | restrict to one instrument serial (repeatable); passes the `instrument_serial` filter to the backend |
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
6. Print the run summary; release the lock; exit per the table below.

Re-running with no new artifacts downloads nothing and writes nothing (SC-002/SC-003).

## Output (stdout)

Human-readable summary, always printed (even with `--quiet`). Shape:

```
fleet-backup-pull  <started_utc> .. <finished_utc>
  luminosa : ok        machines=214  added=3   failed=0
  solira   : SKIPPED    (403 — admin key has no access)
totals: machines=214  artifacts added=3  failed=0
```

Per-artifact lines (stderr, suppressed by `--quiet`):

```
+ luminosa/SN-12345/0f4a…/ProgramData/PicoQuant/Luminosa/PQDevice.db  (2026-09-08T12:52:52Z, 40960 B)
! luminosa/SN-67890/1a2b…/…/GUISettings.xml  digest_mismatch (expected 24ab…, got 9f1c…) — not archived
```

## Exit codes

| Code | Meaning | Scheduler action |
|---|---|---|
| `0` | success — artifacts added or nothing new; **or** another run was already in progress | none |
| `1` | partial — the run finished but ≥1 artifact failed download or verification | look at the named instrument(s) |
| `2` | fatal — admin key missing/invalid, no product reachable, or archive root not writable | the job is broken; fix and re-run |

A `pruned` miss (a `404` on content because the backend dropped it between listing and fetch)
is logged but does **not** by itself cause exit `1` — it is retried next run.

## Not in this increment

`restore`, `list` / `inspect`, `verify` (full-archive re-hash), `diff` against the backend,
stale-instrument reporting, prune. Recorded under spec *Deferred*.
