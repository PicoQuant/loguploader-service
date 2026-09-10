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

## 3b. Laser-power measurement records (US2)

The default run (step 1) already pulled `powermeter`. Check one system's records:

```
python tools/fleet_backup_pull.py --out ./fleet-backups --product powermeter          # first pull
python tools/fleet_backup_pull.py --out ./fleet-backups --product powermeter --quiet  # re-run: records=0
```

Expected:
- first run summary line `powermeter: ok  systems=<n>  records=<m>`, exit `0`
- a folder per system — `./fleet-backups/powermeter/<system_serial>/` (standalone) or
  `./fleet-backups/<product>/<system_serial>/_powermeter/` when that instrument was also
  archived by step 1 — each with `manifest.json`, `records/<measured_at>__<id8>.json` per
  record, and `<measurement_type>.latest.json`
- the re-run shows `records=0` and changes no file

Byte-exact check against the backend list row:

```
set -a; . ./.env; set +a
API=https://api.picoquant.com ; ADMIN="$EXPECTED_ADMIN_API_KEY"
SERIAL=<system_serial> ; ID=<record id from a manifest entry>

curl -s -H "X-ADMIN-API-KEY: $ADMIN" \
  "$API/api/v2/admin/products/powermeter/telemetry?system_serial=$SERIAL&limit=1000" \
| python3 -c "import sys,json; d=json.load(sys.stdin); \
r=[x for x in d['records'] if x['id']=='$ID'][0]; \
f=json.load(open([p for p in __import__('glob').glob('./fleet-backups/**/$SERIAL/**/records/*$( echo $ID | cut -c1-8 )*.json', recursive=True)][0])); \
print('match:', r==f)"
```

Expected: `match: True`. (Demonstrated 2026-09-10 — system `1051032`, record
`76d36f96…`, archived JSON == backend list row.)

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
For **US2**: power-record filename; append-only commit; incremental skip by `id`; paging;
nest-vs-standalone location + location pinned once chosen; `<measurement_type>.latest.json`
mirror; manifest rebuild from `records/*.json`; `unknown` system serial; `powermeter`
inaccessible → skipped; end-to-end + key redaction.

## Done when

- Step 1 produces a populated archive; step 2 adds and changes nothing.
- Step 3's three digests match.
- Step 3b: power records archived, re-run adds nothing, one record byte-matches the backend.
- Step 4: an archived version survives a backend prune.
- Step 5: the second concurrent run exits 0 and touches nothing.
- `python -m unittest discover -s tools` passes.
- The admin key value appears in no file the tool wrote and in no line it printed.
