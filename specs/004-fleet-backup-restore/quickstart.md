# Quickstart — validate the fleet backup archive

Proves the archive pull works end to end: a real run against `api.picoquant.com` fills a local
archive, a second run does nothing, and an archived file is byte-identical to the backend's
copy.

## Prerequisites

- Python 3.9+ (no packages to install — standard library only).
- `EXPECTED_ADMIN_API_KEY` in `./.env` (already present in this repo for admin access), or in
  the environment. `.env` is gitignored.
- Network access to `https://api.picoquant.com`, with the admin key granting at least
  `luminosa` access.

## 1. First pull

```
python tools/fleet_backup_pull.py --out ./fleet-backups
```

Expected:
- exit `0`
- a summary like `luminosa : ok  machines=<n>  added=<m>  failed=0`
- a tree under `./fleet-backups/luminosa/<serial>/<machine-id>/` containing, per machine:
  - `manifest.json`
  - the newest version of each file mirrored at its original path
    (`ProgramData/PicoQuant/Luminosa/LastKnownGood.xml`, …)
  - `_versions/<same path>/<received_at>__<sha8>.bak` for every version

See `contracts/cli.md` for all options and exit codes, `data-model.md` for the layout and
manifest shape.

## 2. Second pull — nothing new

```
python tools/fleet_backup_pull.py --out ./fleet-backups
```

Expected: exit `0`, summary shows `added=0` for every product, and **no file under
`./fleet-backups` changes** (check with `git status`-style diff or mtimes). This is SC-002 /
SC-003.

## 3. Byte-exact spot check

Pick any archived file and confirm it matches the backend:

```
set -a; . ./.env; set +a
API=https://api.picoquant.com ; ADMIN="$EXPECTED_ADMIN_API_KEY"

# from a manifest.json entry: its `id` and `content_sha256`
ID=<backup id> ; SHA=<content_sha256>
curl -s -H "X-ADMIN-API-KEY: $ADMIN" -o /tmp/dl.bin "$API/api/v2/admin/products/luminosa/backups/$ID/content"
sha256sum /tmp/dl.bin        # == $SHA
sha256sum "./fleet-backups/luminosa/<serial>/<machine-id>/_versions/<rel>/<received_at>__<sha8>.bak"   # == $SHA
```

Expected: all three digests equal. (Demonstrated this session — `LastKnownGood.xml`,
`b72cde42…`, disk == backend metadata == downloaded content, 77 bytes.)

## 4. Append-only after a backend prune (simulated)

- Note a file the archive holds several versions of.
- Have the backend drop the oldest version (or wait for its retention to do so).
- Re-run the pull.

Expected: exit `0`; the pruned version's `_versions/…/*.bak` is **still present and
unchanged** in the archive (FR-007 / SC-001); the run does not try to re-fetch it.

## 5. Overlap safety

```
# start a long run (e.g. against a large fleet) and, while it runs, start a second:
python tools/fleet_backup_pull.py --out ./fleet-backups &
python tools/fleet_backup_pull.py --out ./fleet-backups
```

Expected: the second prints `another run in progress; exiting` and exits `0`. `./fleet-backups`
is not corrupted (FR-017).

## 6. Failure-path checks (`unittest`, no network)

```
python -m unittest discover -s tools -p 'test_*.py'
```

Covers, without touching the real backend: manifest-as-cursor incremental skip; digest
mismatch → not archived + run continues; `rel(source_path)` derivation; manifest round-trip +
rebuild-from-filenames; lock contention → exit 0; a product returning `403` → skipped, the
other still archived; paging; exit-code selection; admin-key redaction from all output.

## Done when

- Step 1 produces a populated archive; step 2 adds and changes nothing.
- Step 3's three digests match.
- Step 4: an archived version survives a backend prune.
- Step 5: the second concurrent run exits 0 and touches nothing.
- `python -m unittest discover -s tools` passes.
- The admin key value appears in no file the tool wrote and in no line it printed.
