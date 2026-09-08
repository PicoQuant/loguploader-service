# Implementation Plan: Fleet Backup Archive

**Branch**: `v2-specs` (feature dir `specs/004-fleet-backup-restore`) | **Date**: 2026-09-08 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/004-fleet-backup-restore/spec.md`

## Summary

An off-device maintainer command that turns the backend's transient config-backup store into a
permanent local archive. Run from an external scheduler (a cron job — not part of this
feature), each invocation sweeps `api.picoquant.com`'s admin backup list for every product it
can see, and downloads every backup artifact the local archive does not already hold. It
verifies each artifact against its recorded digest, writes it atomically, records it in a
per-machine manifest, and **never deletes or overwrites** history it already captured — so the
archive stays complete even after the backend prunes.

Archive layout is `<root>/<product>/<serial>/<machine-id>/…` (one folder per physical machine
— clarified 2026-09-08), with the newest content of each file mirrored at its original
Windows path and every version kept under `_versions/`. The per-machine `manifest.json` is
both the record of what is held and the **incremental cursor** — an artifact is "already
archived" iff its backend id is in the manifest, so a no-op run does no file hashing.

Technical approach: a single self-contained **Python 3** script (`tools/fleet_backup_pull.py`,
evolving the existing seed), standard library only (`urllib`, `hashlib`, `json`, `argparse`,
`pathlib`), no third-party dependency. Concurrency-safe via a lock file (second run logs and
exits 0). Restore, inspection, and stale-instrument detection are out of this increment
(spec → *Deferred*).

## Technical Context

**Language/Version**: Python 3.9+ (stdlib only; `from __future__ import annotations` for
typing on 3.9). Runs on any OS for the pull (network + local file writes).

**Primary Dependencies**: none beyond the Python standard library. `urllib.request` for HTTP,
`hashlib` for SHA-256, `json`, `argparse`, `pathlib`, `os` (atomic `os.replace`), `sys`.
Rationale in `research.md` (Constitution V — the seed is already stdlib-only; `requests` in
`requirements.txt` belongs to the v1 service, not this tool).

**Storage**: the local archive — a plain directory tree on a maintainer machine, chosen with
`--out` (default `./fleet-backups`). One `manifest.json` per machine folder. No database.
Durable/off-site protection of the archive itself is the operator's responsibility (spec
Assumptions).

**Testing**: `unittest` (stdlib) with the HTTP layer injected as a fake — no real network in
the test run. Covers: manifest-as-cursor incremental skip, atomic write + digest-mismatch
rejection, archive-path derivation from `source_path`, manifest round-trip + corruption
fallback, lock contention → exit 0, per-product inaccessible handling, paging. Plus
`quickstart.md`: a real run against `api.picoquant.com` with the admin key from `.env`
(already exercised in this repo's session against `v2.0.0-beta.2` data).

**Target Platform**: the tool runs anywhere Python 3.9+ runs. The *data* it archives comes
from Windows instruments; the archive mirrors Windows paths but the tool does not need
Windows.

**Project Type**: single maintenance script under `tools/`, not a service. No packaging, no
install step — `python tools/fleet_backup_pull.py …`.

**Performance Goals**: SC-002 — a no-new-work run over ~1,000 instruments in under 5 minutes,
0 bytes written. Achieved by paging the admin list (few hundred rows/page) and checking each
row's backend id against the in-memory manifest set — no local file I/O on the skip path.

**Constraints**: append-only w.r.t. history (never delete/overwrite archived artifacts); every
artifact digest-verified before commit; every write atomic (`.tmp` → `os.replace`); the admin
credential never written to the archive, its manifests, logs, or stdout; safe to run
unattended and overlapping (lock → exit 0).

**Scale/Scope**: order 10²–10³ instruments across 2 products; a handful of small config files
each (KB–low MB); a few versions per file per year. Archive growth is modest and unbounded is
acceptable (no prune this increment — clarified). Script target < ~600 LOC.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Constitution v1.3.0. Result: **PASS.**

| Principle | Assessment |
|---|---|
| I. Never Crash the Service Loop | **N/A / intent honoured.** Not a service. The pull nonetheless treats failure as normal: bounded retry + backoff on 5xx/network, a bad artifact is logged and skipped (not fatal), a bad product is reported and skipped. A run always terminates with a meaningful exit code. |
| II. Single Source of Truth for Version & Config | **PASS.** No secret in source: the admin key is read from the gitignored `.env` (same one the repo already uses) or the environment, and FR-003 forbids it from appearing in the archive/manifests/logs/output. No version-stamped artifacts here. |
| III. Non-Destructive Local File Handling | **PASS (this is the point).** The archive is append-only: a captured artifact is never deleted or overwritten (FR-007). Downloads are written to a temp file, digest-verified, then atomically renamed (FR-008, FR-012). Oversize/locked/rejected conditions are recorded, never forced. |
| IV. Observable by Default | **PASS.** Every run prints a report — artifacts added, machines seen, per-artifact failures, per-product accessibility — and returns an exit code a scheduler acts on (0 success/idle/locked, 1 partial, 2 fatal). Per-artifact and per-error lines go to stderr/log. |
| V. Minimal Dependencies, Self-Contained Delivery | **PASS.** Python standard library only; no new third-party dependency. One file, no build, no install. |
| VI. Remote Upgradeability Is Non-Negotiable | **N/A.** Nothing is deployed to an instrument. The tool is copied/updated by maintainers with the repo. |

**Gate outcome**: **PASS** — proceed to Phase 0.

### Post-Design Constitution Re-check (after Phase 1)

No violations. Confirmations:
- **III** — `data-model.md` defines the archive as write-once per artifact; the only writer,
  `commit_artifact`, refuses to touch an existing path and only ever `os.replace`s a verified
  temp file. `manifest.json` is written the same way.
- **II** — the admin key flows from `Config` (env/.env) into an `Authorization`-style header
  and nowhere else; `RunReport` and manifests have no field for it; a redaction test asserts
  it never appears in captured output.
- **IV** — `RunReport` is the single structured summary; `contracts/cli.md` fixes the exit-code
  meanings.
- **V** — `contracts/` shows only stdlib types on the wire; no dependency added.

## Project Structure

### Documentation (this feature)

```text
specs/004-fleet-backup-restore/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── cli.md                 # the command's own arguments, output, exit codes
│   └── backend-admin-api.md   # the admin-API subset this tool consumes (client side)
└── tasks.md             # /speckit-tasks output (next)
```

### Source Code (repository root)

```text
tools/
├── fleet_backup_pull.py        # the tool — evolves the existing seed:
│                               #   + <machine-id> folder level (clarified layout)
│                               #   + manifest.json as the incremental cursor (id-keyed)
│                               #   + lock file (overlap -> log + exit 0)
│                               #   + bounded retry/backoff, per-product accessibility
│                               #   + RunReport + 0/1/2 exit codes
│                               #   pure helpers kept at module scope for unit testing
└── test_fleet_backup_pull.py   # unittest, HTTP layer faked, no network
```

**Structure Decision**: One script under `tools/`, matching the repo's existing maintenance
scripts and the user's ask ("a python script"). No package, no entry point, no dependency
file. The HTTP client is a thin seam (`class Api` with a single `_get`) so tests inject a fake
without a network. Everything that has logic worth testing (path derivation, the
already-archived check, the run summary, retry decisions) is a module-level pure function.

## Complexity Tracking

*Constitution Check passes with no violations.* No entries.

## Notes / decisions carried into Phase 0

- **Discovery**: one paged sweep of `GET /admin/products/{product}/backups` per product yields
  every row with its `instrument_serial` + `machine_id`; group client-side into machine
  folders. `--serial` adds the `instrument_serial` filter; `--since/--until` pass through.
- **"Already archived"** = backend row `id` present in the target machine's `manifest.json`.
  Missing/corrupt manifest → rebuild it by scanning that folder's `_versions/` filenames
  (which embed the digest) before the run proceeds.
- **Layout**: `<root>/<product>/<serial>/<machine-id>/<mirrored source_path>` for the newest
  content of each file; `…/<machine-id>/_versions/<mirrored source_path>/<received_at>__<sha8>.bak`
  for every version (including the newest, so history is complete in one place).
- **Restore / inspection / stale-instrument detection**: explicitly out of this plan (spec
  *Deferred*). `manifest.json` is designed now to carry everything those increments will need.
