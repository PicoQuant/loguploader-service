# Phase 0 Research — Fleet Backup Archive

No open NEEDS CLARIFICATION items in Technical Context (the `/speckit-clarify` session and a
live API check resolved them). This file records the design decisions and the alternatives
weighed.

## D1. Language & runtime — Python 3.9+, standard library only

- **Decision**: a single Python 3 script, `tools/fleet_backup_pull.py`, stdlib only
  (`urllib.request`, `hashlib`, `json`, `argparse`, `pathlib`, `os`, `sys`, `time`).
- **Rationale**: the user asked for "a python script"; the repo already ships Python tooling;
  the seed `tools/fleet_backup_pull.py` is already stdlib-only. Zero third-party dependencies
  means a maintainer can run it on any box with a stock Python — no venv, no `pip install`
  (Constitution V).
- **Alternatives**:
  - *`requests`* (already in `requirements.txt`) — nicer API, but that requirement belongs to
    the v1 service; adding a runtime dep to a DR tool that must "just run" is the wrong trade.
  - *A compiled tool (Rust, matching the v2 agent)* — overkill for an occasional off-device
    maintenance job with no distribution or tamper concerns.
- **Version floor**: 3.9 (repo's v1 targeted 3.10; 3.9 widens where it can run).
  `from __future__ import annotations` keeps `X | None` typing usable on 3.9.

## D2. Backend access — the existing admin retrieval API

- **Decision**: consume only `GET /api/v2/admin/products/{product}/backups` (list, with
  `instrument_serial` / `since` / `until` / `limit` / `offset`) and
  `GET /api/v2/admin/products/{product}/backups/{id}/content` (raw bytes). Auth header
  `X-ADMIN-API-KEY`. Contract: `contracts/backend-admin-api.md`.
- **Rationale**: **verified live** that the list endpoint returns *full per-file version
  history* (all versions, newest first — two rows for `pqdevice_conf` with distinct
  `received_at` + `content_sha256`). So FR-010 / SC-009 need no `specs/003-backend-api-support`
  change. `…/backups/latest` is not needed (the archive wants all versions).
- **Discovery**: one paged sweep of the list per product yields every row with its
  `instrument_serial` and `machine_id`; the tool groups client-side. Fewer requests than
  "list serials, then list per serial". `--serial` narrows via the `instrument_serial` filter.
- **Alternatives**: deriving the instrument set from the `agent_status` telemetry list —
  rejected, the backups list already carries everything and is the authoritative source of
  what there is to archive.

## D3. "Already archived" — the manifest is the incremental cursor

- **Decision**: each machine folder holds a `manifest.json` listing every archived artifact by
  its backend `id` (plus `file_key`, `content_sha256`, `size_bytes`, `received_at`,
  `source_path`, `machine_id`, `instrument_serial`). An artifact from the backend list is
  "already archived" **iff its `id` is in that manifest**. The skip path does **no** local
  file hashing (SC-002, SC-003).
- **Rationale**: backend `id` is a stable UUID, unique per stored artifact. Reading one small
  JSON per machine and doing set membership is O(rows) with no disk I/O on the common no-op
  run. The manifest travels with the folder, so a moved/renamed archive still self-identifies
  (spec edge case).
- **Corruption / missing manifest**: rebuilt before the run by scanning that folder's
  `_versions/<path>/<received_at>__<sha8>.bak` filenames (digest embedded) and the mirrored
  latest files; entries whose digest cannot be reconciled are dropped and will re-download.
  Never a crash (FR-012).
- **Alternatives**:
  - *Scan + hash the archive every run* — simplest to reason about, but tens of thousands of
    small hashes on a 1,000-instrument fleet every run; fails the SC-002 spirit and wastes I/O.
  - *A separate top-level index / SQLite* — one more moving part and a corruption surface;
    per-folder JSON keeps the unit of pull/restore self-contained.

## D4. Archive layout

- **Decision**:
  ```
  <root>/<product>/<serial>/<machine-id>/
      manifest.json
      <mirrored source_path>                         # newest version of each file
      _versions/<mirrored source_path>/
          <received_at>__<sha8>.bak                  # every version (incl. newest)
  ```
  `<serial>` is the literal `unknown` when the serial is not known. `<mirrored source_path>` is
  the agent's recorded Windows `source_path` with the drive stripped
  (`C:\ProgramData\PicoQuant\Luminosa\LastKnownGood.xml` →
  `ProgramData/PicoQuant/Luminosa/LastKnownGood.xml`); if `source_path` is unusable, fall back
  to the (already-normalised) `file_key`.
- **Rationale**: one folder per physical machine (clarified) so same-serial / unknown-serial
  machines never interleave. Mirroring `source_path` makes the newest set directly
  restore-mappable in the deferred restore increment. `_versions/` keeps the full history in
  one predictable place; embedding `received_at` + `sha8` in the filename makes it
  self-describing and lets the manifest be rebuilt from filenames alone.
- **`received_at` in a filename**: colons replaced (`2026-09-08T12-52-52Z`); `sha8` = first 8
  hex of `content_sha256`. Collision within one file+second is not credible; the manifest is
  authoritative regardless.
- **Alternatives**: a flat `<id>.bak` per artifact + manifest-only structure — loses the
  restore-friendly mirrored tree and human browsability for no real gain.

## D5. Integrity & atomicity

- **Decision**: download each artifact to `<final>.part` in the destination directory, compute
  SHA-256 while writing, compare to the row's `content_sha256`; on mismatch delete the temp
  and record a failure; on match `os.replace(tmp, final)` (atomic on the same filesystem).
  `manifest.json` is written the same way (`manifest.json.tmp` → `os.replace`). A partial or
  interrupted run therefore never leaves a half-file or a torn manifest (FR-008, FR-012, SC-004,
  Constitution III).
- **Rationale**: `os.replace` is atomic on POSIX and Windows for same-volume paths; the temp
  lives in the destination dir so the rename stays same-volume.
- **Existing files**: `commit_artifact` refuses to overwrite an already-present archive path
  (append-only). The "newest" mirrored file is the one exception — it is updated to the newest
  version — but only via the same verify-then-`os.replace`, and the prior newest already
  exists under `_versions/`, so nothing is lost.

## D6. Concurrency — a lock file, skip on contention

- **Decision**: at start, create `<root>/.fleet-backup.lock` exclusively (`O_CREAT|O_EXCL`)
  containing the pid + start time. If it already exists and is fresh, log
  `"another run in progress; exiting"` and **exit 0** (clarified — no wait, no queue, no
  error). A stale lock (pid gone / older than a generous threshold, e.g. 6 h) is reclaimed
  with a warning. The lock is released (file removed) in a `finally`.
- **Rationale**: cron-friendly; the pull is incremental so the next scheduled run catches up.
- **Alternatives**: `fcntl`/`msvcrt` advisory locks — platform-split code for no benefit over
  an exclusive-create sentinel; waiting/queuing — rejected in clarify.

## D7. HTTP robustness

- **Decision**: `urllib.request` with an explicit timeout (connect+read, e.g. 60 s). Per
  request: up to 3 attempts, backoff 2 s → 4 s, retry only on a transport error or HTTP
  `>=500`. A `401`/`403` on a product's list → that product is marked **inaccessible**, logged,
  and skipped (FR-004); it does not fail the run. A `404` on a specific artifact's content
  (pruned between list and fetch) → recorded as a miss for this run, retried next run (spec
  edge case).
- **Rationale**: matches the v2 agent's retry posture (research D3 of spec 002) and
  Constitution I's "assume failure is normal".

## D8. Run report & exit codes

- **Decision**: one summary line-set at the end — per product: accessible?, machines seen,
  artifacts added, artifacts failed; plus totals. Exit code:
  - **0** — success (artifacts added or nothing new) **or** another run was in progress.
  - **1** — partial: the run completed but ≥1 artifact failed download/verification.
  - **2** — fatal: admin key missing/invalid, no product reachable at all, or the archive root
    is not writable.
  Detail lines (per artifact added, per failure, per inaccessible product) go to stderr.
- **Rationale**: SC-008 — the scheduler and a glancing maintainer both act on the exit code;
  `1` vs `2` distinguishes "look at one instrument" from "the job is broken".

## D9. Testing approach

- **Unit** (`unittest`, no network): inject a fake `Api` returning canned pages / bytes.
  Cover — manifest-as-cursor skip (id present → 0 downloads); digest mismatch → not written +
  failure recorded + run continues; `mirror_relpath` for normal / drive-less / unusable
  `source_path`; manifest write+reload round trip and rebuild-from-filenames fallback; lock
  present → exit 0; product `403` → inaccessible, other product still archived; paging across
  >1 page; exit-code selection.
- **Redaction test**: run with a known admin key, capture stdout+stderr+every file the run
  wrote, assert the key string appears nowhere (SC-006).
- **Manual E2E** (`quickstart.md`): a real run against `api.picoquant.com` with the `.env`
  admin key; re-run shows 0 downloads; a byte-exact spot check of one archived file vs the
  backend `…/content`. (Already demonstrated this session against `v2.0.0-beta.2` data.)
