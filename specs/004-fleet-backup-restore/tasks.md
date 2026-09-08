---
description: "Task list for Fleet Backup Archive (spec 004, US1 only)"
---

# Tasks: Fleet Backup Archive

**Input**: Design documents from `specs/004-fleet-backup-restore/` — plan.md, spec.md,
research.md, data-model.md, contracts/{cli,backend-admin-api}.md, quickstart.md
**Constitution**: v1.3.0 — gate **PASS** (Principle III is the core; I / VI are N/A — not a
service, nothing deployed to instruments).

**Tests**: INCLUDED. `plan.md` (Testing) + `research.md` (D9) + `quickstart.md` (§6) specify a
concrete `unittest` suite with named cases, HTTP faked, no network. Test tasks are listed in
the US1 phase; write them alongside or before the code they cover.

**Scope**: US1 (the archive pull) only. Restore, inspection, and stale-instrument detection are
**Deferred** (spec). This increment **evolves the existing seed** `tools/fleet_backup_pull.py`
— it is not a new file.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files / disjoint functions, no incomplete dependency)
- **[Story]**: US1 from spec.md; Setup / Foundational / Polish carry no story label
- All paths are repo-root-relative. One script + one test file (see `plan.md` → Project Structure).

---

## Phase 1: Setup

**Purpose**: reshape the seed so the rest is testable and matches the contracts.

- [X] T001 Restructure `tools/fleet_backup_pull.py`: no side effects at import; keep every unit of logic as a module-level pure function; make `class Api` the single HTTP seam; `main(argv=None)` stays thin. Delete the seed's now-obsolete pieces — the re-hash-the-archive incremental check, the `--all` discovery flag, `--all-versions` (the archive always pulls every version) — and any docstring/example that no longer matches `contracts/cli.md`.
- [X] T002 [P] Rewrite the `argparse` setup in `tools/fleet_backup_pull.py` to match `contracts/cli.md` exactly: `--out` (default `./fleet-backups`), `--product` (repeatable, choices luminosa/solira), `--serial` (repeatable), `--since`, `--until`, `--api`, `--env` (default `./.env`), `--timeout` (default 60), `--quiet`, `--rebuild-manifests`, `-h/--help`. No `--force`, `--delete`, `--prune`, positional serials, `--all`, or `--all-versions`.
- [X] T003 [P] Add `Config` (dataclass) + `load_config(args)` in `tools/fleet_backup_pull.py`: resolve the admin key from `EXPECTED_ADMIN_API_KEY` (env → `--env` file via a minimal `KEY=VALUE` parser), `api_base_url` (`--api` → `$API_BASE_URL` → `.env` → `https://api.picoquant.com`), `timeout`, `out` root. Missing/empty key → return a value that makes `main()` exit **2** with a message naming the env var and file checked — **never the value** (FR-003).
- [X] T004 [P] Create `tools/test_fleet_backup_pull.py`: `unittest` scaffold, a `FakeApi` (canned pages + `{id: bytes}` map, per-product `raise ApiAuthError`), and a `tmp_archive()` helper (temp dir, auto-cleaned).

---

## Phase 2: Foundational (blocking prerequisites for US1)

**⚠️ US1 work starts only after this phase.**

- [X] T005 [P] `rel_path(source_path: str, file_key: str) -> PurePosixPath` in `tools/fleet_backup_pull.py` per `data-model.md`: `\`→`/`, strip a leading `X:/` drive, strip leading `/`, drop `.`/`..` segments; if nothing usable remains, split `file_key` on `/`.
- [X] T006 [P] `version_filename(received_at: str, content_sha256: str) -> str` in `tools/fleet_backup_pull.py`: `received_at` with `:`→`-`, then `__` + first 8 hex of the digest + `.bak`.
- [X] T007 `write_atomic(path, data: bytes)` + `commit_artifact(dest_dir, rel, row, data) -> ManifestEntry | Failure` in `tools/fleet_backup_pull.py`: write `<final>.part` in the destination dir while hashing, compare to `row.content_sha256`; on mismatch delete the temp + return `Failure(digest_mismatch)`; on match `os.replace` to the `_versions/<rel>/<version_filename>` path; **refuse to overwrite an existing `_versions/...` file** (append-only, Constitution III); on `OSError` return `Failure(write_error)`. (FR-008, FR-012, SC-004, SC-005)
- [X] T008 Manifest layer in `tools/fleet_backup_pull.py` per `data-model.md`: `ManifestEntry` + `Manifest` types; `load_manifest(machine_dir) -> Manifest`; `save_manifest(machine_dir, m)` (atomic `manifest.json.tmp` → `os.replace`); `rebuild_manifest_from_disk(machine_dir) -> Manifest` (scan `_versions/**/*.bak`, parse `received_at` + `sha8` from filenames, `is_latest` = newest `received_at` per `rel_path`, `id=None`, drop unreadable). Missing / unreadable / `schema_version` > 1 → rebuild, never raise. (FR-011, FR-012)
- [X] T009 Lock in `tools/fleet_backup_pull.py`: `acquire_lock(root) -> Lock | LOCK_HELD`: `os.open(<root>/.fleet-backup.lock, O_CREAT|O_EXCL|O_WRONLY)`, write `{pid,host,started_utc}`; on `FileExistsError` read it — younger than `STALE_LOCK_SECS` (21600) and (where checkable) pid alive → return `LOCK_HELD`; else reclaim with a `log` warning. `release_lock(lock)` unlinks; callers use `try/finally`. (FR-017)
- [X] T010 `Api.__init__(base_url, admin_key, timeout)` + `Api._get(path, params=None) -> bytes` in `tools/fleet_backup_pull.py`: `urllib.request` with `X-ADMIN-API-KEY` header and the timeout; ≤3 attempts, backoff 2 s → 4 s, retrying only transport errors and HTTP ≥ 500; raise `ApiAuthError` on 401/403/404-product, `ApiNotFound` on a 404 content fetch. Never log the key. (`contracts/backend-admin-api.md`, research D7)
- [X] T011 [P] `Api.list_backups(product, *, instrument_serial=None, since=None, until=None)` generator in `tools/fleet_backup_pull.py`: `GET /api/v2/admin/products/{product}/backups` with `limit=1000` + `offset`, yield each row dict, stop on a short page. (FR-014)
- [X] T012 [P] `Api.download(product, backup_id) -> bytes` in `tools/fleet_backup_pull.py`: `GET .../backups/{id}/content`; 404 → `ApiNotFound` (caller records `pruned`).
- [X] T013 [P] `RunReport` + `ProductResult` types in `tools/fleet_backup_pull.py` per `data-model.md`, with `RunReport.exit_code` → **2** if a fatal precondition or no product reachable, else **1** if `artifacts_failed > 0`, else **0**. A `pruned` miss increments a separate counter that does **not** raise the exit code. (FR-015, SC-008)

**Checkpoint**: `python -c "import tools.fleet_backup_pull"` succeeds; helpers unit-testable in isolation.

---

## Phase 3: User Story 1 — The fleet's backups accumulate in a permanent local archive (Priority: P1) 🎯 MVP

**Goal**: a scheduled run downloads every backend backup the archive does not already hold,
verifies it, files it under `<product>/<serial>/<machine-id>/`, and never deletes captured
history.

**Independent Test**: run the pull twice with nothing new between → second run downloads
nothing, changes no file. Add a backend backup → exactly that artifact appears. Drop a
backend backup the archive holds → the archived copy stays, run still succeeds.

### Tests for User Story 1 (`tools/test_fleet_backup_pull.py`)

- [X] T014 [P] [US1] `tools/test_fleet_backup_pull.py::test_incremental_skip_by_manifest_id`: a row whose `id` is already a `ManifestEntry` → 0 `Api.download` calls, no file/manifest write (SC-002, SC-003).
- [X] T015 [P] [US1] `tools/test_fleet_backup_pull.py::test_rel_path_derivation`: `C:\ProgramData\PicoQuant\Luminosa\LastKnownGood.xml` → `ProgramData/PicoQuant/Luminosa/LastKnownGood.xml`; drive-less, `..`-containing, and empty/garbage `source_path` → `file_key` fallback.
- [X] T016 [P] [US1] `tools/test_fleet_backup_pull.py::test_commit_artifact`: good bytes → `_versions/<rel>/<name>.bak` written, no `.part` left, `ManifestEntry` returned; bad bytes → nothing written, `Failure(digest_mismatch)`; pre-existing `.bak` → not overwritten.
- [X] T017 [P] [US1] `tools/test_fleet_backup_pull.py::test_manifest_roundtrip_and_rebuild`: `save`→`load` equal; a corrupt file, a `schema_version: 2` file, and a missing file each → rebuilt from `_versions/` filenames; an unreadable `.bak` is dropped from the rebuild.
- [X] T018 [P] [US1] `tools/test_fleet_backup_pull.py::test_lock`: fresh lock present → `acquire_lock` returns `LOCK_HELD` and `main()` exits 0 writing nothing; a lock older than `STALE_LOCK_SECS` → reclaimed with a warning.
- [X] T019 [P] [US1] `tools/test_fleet_backup_pull.py::test_product_inaccessible`: `FakeApi` raises `ApiAuthError` for `solira` → `solira` `SKIPPED`, `luminosa` still archived, exit 0; both raise → exit 2 (FR-004).
- [X] T020 [P] [US1] `tools/test_fleet_backup_pull.py::test_paging`: `FakeApi` serves 2 full pages + a short page → every row processed exactly once.
- [X] T021 [P] [US1] `tools/test_fleet_backup_pull.py::test_append_only`: given a manifest that already holds the newest version, a run that re-lists an older version it also holds does not rewrite or delete any `_versions/` file (FR-007, SC-001).
- [X] T022 [P] [US1] `tools/test_fleet_backup_pull.py::test_admin_key_redaction`: run end-to-end against `FakeApi` with a sentinel admin key; capture stdout + stderr + the bytes of every file the run wrote; assert the sentinel appears in none of them (SC-006).
- [X] T023 [P] [US1] `tools/test_fleet_backup_pull.py::test_exit_codes`: nothing new → 0; one injected `download_error` → 1; no product reachable → 2; a `pruned`-only run → 0.

### Implementation for User Story 1 (`tools/fleet_backup_pull.py`)

- [X] T024 [US1] `discover_and_group(api, products, *, serials, since, until) -> (groups, product_results)`: one `list_backups` sweep per in-scope product (with the `serials`/date filters), catch `ApiAuthError` → mark that `ProductResult` inaccessible and continue; group rows into `{(product, serial, machine_id): [rows]}` sorted **ascending by `received_at`** within each `(…, file_key)` (FR-009, FR-013).
- [X] T025 [US1] `pull_machine(api, product, serial, machine_id, rows, out_root, report, *, rebuild) -> None`: resolve `<out_root>/<product>/<serial>/<machine-id>/`; `load_manifest` (or `rebuild_manifest_from_disk` if `--rebuild-manifests`); for each row whose `id` ∉ manifest (fallback `(rel_path, content_sha256)` when the manifest was rebuilt) → `api.download` → `commit_artifact`; on success append the `ManifestEntry`; after the loop, for the newest `received_at` per `file_key` set `is_latest` and refresh the mirrored latest file at `<machine-dir>/<rel_path>` (same verify-then-`os.replace`); `save_manifest`; fold counts into `report`. `ApiNotFound` on a download → count `pruned`, continue (FR-005, FR-007, FR-010, FR-018).
- [X] T026 [US1] `main(argv=None)`: `load_config`; if the key is missing or `--out` is not creatable/writable → print the reason, exit **2**; `acquire_lock(out)` → `LOCK_HELD` → `log "another run in progress; exiting"`, exit **0**; `try`: `discover_and_group` → loop `pull_machine`; `finally`: `release_lock`; `print_summary(report, quiet)`; `sys.exit(report.exit_code)`.
- [X] T027 [US1] `print_summary(report, quiet)` + the per-artifact detail lines, byte-for-byte per `contracts/cli.md`: `+ <product>/<serial>/<machine>/<rel>  (<received_at>, <n> B)` and `! …  <category> (expected …, got …) — not archived` to **stderr** (gated by `quiet`); the `fleet-backup-pull … / <product> : ok|SKIPPED … / totals: …` block to **stdout** always (FR-015, SC-008).

**Checkpoint**: `quickstart.md` steps 1–2 pass against the real backend — first run populates
`<out>/luminosa/<serial>/<machine-id>/…`, second run is a no-op.

---

## Phase 4: Polish & Cross-Cutting

- [X] T028 [P] Update `README.MD` → "Pulling backups off the backend": new flag set (no `--all` / `--all-versions`), the `<product>/<serial>/<machine-id>/` layout, `manifest.json` as the record + **append-only, no prune this increment**, exit codes 0/1/2, and the "run it from cron; the schedule is your job" note.
- [X] T029 [P] Verify `.gitignore` ignores the default archive (`/fleet-backups` — present) and `**/.fleet-backup.lock`; add the lock pattern.
- [X] T030 `python -m unittest discover -s tools -p 'test_*.py'` — full green; fix findings. Confirm no test performs real network I/O.
- [X] T031 Ran `quickstart.md` §1–§3 + §5 against `https://api.picoquant.com` (2026-09-08): first pull → 8 artifacts / 3 machines / exit 0; second pull → added=0 / exit 0; byte-exact — backend `…/{id}/content` == `manifest.content_sha256` == archived `.bak` == mirrored latest (`b72cde42…`, 77 B); concurrent second run → `another run in progress` / exit 0. §4 (survives a real backend prune) covered by unit tests `test_append_only_and_mirrored_latest` + `test_pruned_download_is_not_a_failure`; the operational SC-001 check waits for a real backend prune.
- [X] T032 [P] Redaction audit: `grep -R "$(admin key)"` over the produced archive + a captured run log → 0 hits (SC-006); confirm `Api` never passes the key to `log`/`print`.

---

## Dependencies & Execution Order

### Phase order

- **Setup (P1)** → **Foundational (P2)** → **US1 (P3)** → **Polish (P4)**.
- Foundational blocks US1 entirely. There is only one user story, so US1 *is* the MVP.

### Within-phase

- Setup: T002, T003, T004 parallel after T001 (T001 sets the file's shape).
- Foundational: T005, T006 parallel; T007 needs T006; T008 needs T006; T009 independent;
  T010 independent; T011, T012 need T010; T013 independent.
- US1 tests (T014–T023) are all `[P]` — different test methods, and they exercise Foundational
  units plus `pull_machine` (T025) / `main` (T026), so land T024–T027 first or stub them.
- US1 impl: T024 → T025 → T026 → T027 (sequential; same file, layered).
- Polish: T028, T029, T032 parallel; T030 after all impl; T031 after T030.

### Parallel example — Foundational

```
# after T005/T006:
Task: T007  write_atomic + commit_artifact
Task: T008  manifest load/save/rebuild
Task: T009  lock
Task: T010  Api._get + retry
Task: T013  RunReport + exit_code
```

## Implementation Strategy

### MVP = US1 (the whole increment)

1. Phase 1 Setup → 2. Phase 2 Foundational → 3. Phase 3 US1 →
4. **Validate**: `quickstart.md` steps 1–2 (populate + no-op re-run) against the real backend →
5. ship.

### Notes

- The tool is copied/updated with the repo; there is no install or distribution step.
- Every write is `.part` → `os.replace`; no code path deletes or overwrites an archived
  `_versions/` file (Constitution III — verified by T016/T021).
- The admin key lives only in `Config` and the request header (Constitution II — verified by
  T022/T032).
- Commit after each task or logical group.
