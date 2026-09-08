# Feature Specification: Fleet Backup Archive

**Feature Branch**: `004-fleet-backup-restore`

**Created**: 2026-09-08

**Status**: Draft — scoped to the archive pull only (restore & inspection deferred)

**Input**: User description: "a fleet backup pull + restore maintenance tool (spec 004)".
Narrowed on 2026-09-08: **only the scheduled archive pull is in scope now.** Restore,
version inspection, and unattended-operation niceties are deferred to a later increment of
this feature (kept in *Deferred — later increments* below).

## Overview

The v2 agent (`specs/002-v2-config-backup-telemetry`) uploads changed device-configuration
files to `api.picoquant.com`. The backend keeps those backups only transiently — a bounded
number of versions per instrument/file and/or a limited time window. Nothing today turns that
transient store into a durable one, so a device's configuration history is lost on the
backend's normal pruning schedule.

This increment delivers the **archive pull**: an off-device job, run from an external
scheduler, that pulls every config backup the backend currently exposes for the fleet into a
permanent local archive — downloading only artifacts the archive does not already hold, and
never deleting history it has already captured. The archive becomes the system of record for
configuration history; the backend is treated as a transient source.

The tool is operated by PicoQuant maintainers/support, off the instrument, using the existing
admin credential for `api.picoquant.com` (never placed on an instrument). The agent, the
backend, and the upload path are unchanged. `tools/fleet_backup_pull.py` is a first cut of
this pull and the starting point.

Restoring an archived configuration onto an instrument is the natural next step and stays in
this feature's scope overall, but is **not** part of this increment.

## Clarifications

### Session 2026-09-08

- Q: Should the local archive keep every version forever, or apply its own retention? → A: Keep every version forever — append-only, no prune command or retention config in this increment.
- Q: What should a run do when a previous run is still in progress (overlapping schedule)? → A: Detect the lock, log "already running", exit 0 without doing anything (the next scheduled run catches up).
- Q: One archive folder per serial, or per (serial + machine identifier)? → A: Per (serial, machine id) — `<product>/<serial>/<machine-id>/…`, always. One folder maps to exactly one physical machine.
- (Resolved by investigation, not a question) The admin endpoint `GET /api/v2/admin/products/{product}/backups` already returns full per-file **version history** (all versions, newest first), verified live. FR-010 / SC-009 need no `specs/003-backend-api-support` change.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The fleet's backups accumulate in a permanent local archive (Priority: P1)

A job runs the archive pull at a regular interval (triggered by an external scheduler — see
*Out of Scope*). Each run fetches every config backup the backend now exposes for every
instrument that it has not already stored locally, verifies each one, and records it in the
archive. Backups the backend has since pruned but that a previous run captured remain in the
archive untouched.

**Why this priority**: The backend is a transient store. Without a durable archive that keeps
accumulating, a device's configuration history is lost on the backend's pruning schedule and
an on-site rebuild becomes the only recovery path — the exact outcome v2 exists to prevent.
This is also the prerequisite for every later increment (you cannot restore what you did not
keep).

**Independent Test**: Run the pull twice with no new backups in between and confirm the second
run downloads nothing and changes no archived file. Submit a new backup from an instrument,
run the pull, and confirm exactly that one artifact is added. Remove a backup from the backend
(simulating a prune) that the archive already holds, run the pull, and confirm the archived
copy is still present and unchanged.

**Acceptance Scenarios**:

1. **Given** an archive that already holds every backup the backend currently exposes,
   **When** the pull runs, **Then** it downloads nothing, reports "nothing new", and modifies
   no archived file.
2. **Given** an instrument has uploaded a new configuration file since the last run, **When**
   the pull runs, **Then** the new artifact is downloaded, integrity-checked, and added to
   that instrument's folder, and nothing else changes.
3. **Given** a backup that a prior run archived is no longer returned by the backend, **When**
   the pull runs, **Then** the archived copy is retained unchanged and the run still succeeds.
4. **Given** a pull is interrupted partway (process killed, network drop), **When** it is run
   again, **Then** it resumes, every already-archived artifact is skipped, and the archive is
   left consistent (no partial files, manifest not corrupted).
5. **Given** a machine that has never appeared before, **When** the pull runs, **Then** a new
   `<product>/<serial>/<machine-id>/` folder is created for it and all of its available backups
   are archived.
6. **Given** a downloaded artifact whose content does not match its recorded fingerprint,
   **When** the pull runs, **Then** that artifact is not written into the archive, the failure
   is reported, and the rest of the run continues.
7. **Given** a run over the whole fleet, **When** it finishes, **Then** it reports artifacts
   added, instruments seen, per-artifact failures, and an overall success / partial / failure
   status with an exit code a scheduler can act on.
8. **Given** the backend is unreachable for one run, **When** the next run happens, **Then**
   it catches up with no lost artifacts and no duplicates.

---

### Edge Cases

- **Backend prunes between listing and download** — an artifact listed at the start of a run
  is gone before it is fetched: recorded as a miss for this run, retried next run; not an
  archive corruption.
- **The same content under two file identities** (a file renamed on the device) — both
  identities are archived; de-duplication of identical bytes is an optimisation, not a
  requirement.
- **An instrument's serial changes** (hardware swap, serial file fixed) — the machine
  identifier is unchanged, so a new `<serial>/<machine-id>/` folder appears (new serial, same
  machine id) and the machine's earlier `<old-serial>/<machine-id>/` folder is retained.
- **Two machines report the same serial** — each gets its own `<serial>/<machine-id>/` folder;
  their file histories never interleave.
- **An instrument whose serial is "unknown"** — archived under `<product>/unknown/<machine-id>/`,
  so multiple unknown-serial machines stay separate.
- **A machine that cannot read its machine identifier** (all-zero fallback) — archived under
  that fallback id; the manifest's serial + receipt times still disambiguate if two such
  machines exist.
- **Archive storage fills up** — the run fails safely (no partial artifacts, manifest intact)
  and reports the condition; it never deletes archived history to make room.
- **The archive folder was moved or renamed** between runs — the tool re-establishes what is
  already present from the archive's own contents/manifest, not from remembered absolute
  paths, and still avoids re-downloading.
- **Partial fleet credentials** — the admin credential grants access to one product only: the
  tool archives that product and clearly reports the other as inaccessible rather than failing
  wholesale.
- **Clock skew on the maintainer machine** — ordering and "since last run" use the backend's
  recorded receipt time, not the local clock.

## Requirements *(mandatory)*

### Functional Requirements

#### Scope & operating model

- **FR-001**: This increment MUST provide the **archive pull** (backend → local archive) only.
  It MUST NOT require any change to the v2 agent, the backend, or the upload path, and MUST NOT
  introduce any inbound path to an instrument.
- **FR-002**: The tool MUST run entirely off the instrument, from a maintainer machine.
- **FR-003**: The tool MUST authenticate to `api.picoquant.com` with the existing maintainer
  admin credential, supplied from local configuration (the `.env` the repo already uses for
  admin access) or the environment. The admin credential MUST NOT be written into the archive,
  its manifests, the tool's logs, or any artifact it produces, and MUST NOT be distributed to
  instruments.
- **FR-004**: The tool MUST support both products (`luminosa`, `solira`) and be extensible to
  further products without a structural change. If the admin credential grants access to only
  some products, the tool MUST archive those and report the rest as inaccessible.

#### Archive pull

- **FR-005**: The pull MUST be **incremental**: on each run it downloads only backup artifacts
  not already present in the local archive, determined by the artifact's content fingerprint
  and identity (product, instrument, file identity, version), not by timestamps alone.
- **FR-006**: The pull MUST be safe to run **unattended and repeatedly from an external
  scheduler**. Re-runs with no new backups MUST complete quickly, download nothing, and modify
  no archived file. Configuring the scheduler is the operator's responsibility and is **not**
  delivered by this feature.
- **FR-007**: The local archive MUST be **durable and append-only with respect to history**:
  once an artifact has been archived by a successful run, later runs MUST NOT delete or
  overwrite it, even if the backend no longer exposes it. The archive is the system of record;
  the backend is a transient source. This increment ships **no local retention or prune** —
  every version is kept indefinitely (a prune command may be added in a later increment).
- **FR-008**: Each archived artifact MUST be **integrity-verified** against its recorded
  fingerprint before being committed to the archive. A mismatch MUST NOT be written, MUST be
  reported, and MUST NOT abort the rest of the run.
- **FR-009**: The archive MUST be organised as **one folder per (instrument serial, machine
  identifier)** — `<product>/<serial>/<machine-id>/…`, with an explicit `unknown` serial
  segment when the serial is not known. Every such folder therefore maps to exactly one
  physical machine, so a whole fleet is pulled in one run and a single machine can be located
  and operated on in isolation even when two machines share a serial (or both report
  `unknown`).
- **FR-010**: Within a machine's folder, the archive MUST retain **every version** of each
  configuration file it has ever captured, each identifiable and retrievable, with its backend
  receipt time and size. (Restoring a chosen version is a later increment; keeping them is
  this one.)
- **FR-011**: The archive MUST record, per machine folder, a **manifest** of every backup it
  holds — serial, machine identifier, file identity, versions, fingerprints, sizes, receipt
  times, original source path — so that "what do we have" can be answered without contacting
  the backend, and so a later restore increment has what it needs.
- **FR-012**: A pull that is **interrupted** MUST leave the archive consistent (no partial
  artifacts, manifest not corrupted) and MUST resume correctly on the next run.
- **FR-013**: The pull MUST NOT attempt to backfill artifacts the backend pruned before it
  ever saw them; it archives what is currently available plus what it already holds.
- **FR-014**: The pull MUST support restricting a run by **instrument** (one or many) and by
  **date range**, in addition to the default "everything".
- **FR-015**: The pull MUST report, per run: artifacts added, instruments seen, per-artifact
  failures, and an overall status distinguishing success / partial / failure, with an exit
  status a scheduler can act on.
- **FR-016**: A single admin credential is assumed sufficient; the backend being unreachable
  or returning errors for part of a run MUST be handled as a partial run (retry next run),
  not a crash.
- **FR-017**: If two runs overlap (scheduler fires again before the previous finished), the
  second MUST detect that a run is in progress, log "already running", and **exit 0 without
  touching the archive**. It MUST NOT wait, queue, or error. The next scheduled run catches up
  (the pull is incremental).

### Key Entities *(include if feature involves data)*

- **Backup Artifact**: one version of one configuration file for one instrument — its content,
  content fingerprint, size, backend receipt time, original source path, reporting machine
  identifier, and file identity. Produced by the v2 agent, retained transiently by the
  backend, retained permanently by the archive.
- **Local Archive**: the durable store on a maintainer machine, organised
  `<product>/<serial>/<machine-id>/…`. Holds every artifact ever captured plus a per-machine
  manifest. Append-only with respect to history; no local retention/prune in this increment.
  The system of record for configuration history.
- **Machine Folder**: all archived artifacts for one physical machine (one `serial` +
  `machine-id`), plus its manifest — the unit that is pulled and, later, restored.
- **Manifest**: the per-machine index of archived artifacts and their metadata (serial,
  machine id, file identity, versions, fingerprints, sizes, receipt times, source path); lets
  inspection and (later) restore work without the backend.
- **Run Report**: the per-run summary — artifacts added, instruments seen, failures, overall
  status — and the exit status derived from it.
- **Maintainer Admin Credential**: the existing `api.picoquant.com` admin key, held only on
  maintainer machines, never on an instrument, never written into archive/logs/artifacts.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Any backup artifact that appeared on the backend while at least one pull ran
  successfully afterward is present in the local archive **permanently** — 0 lost to backend
  pruning.
- **SC-002**: A pull over a fleet of ~1,000 instruments that finds nothing new completes in
  under 5 minutes and writes 0 bytes into the archive.
- **SC-003**: Re-running a pull re-downloads **0** artifacts the archive already holds.
- **SC-004**: A pull interrupted at any point leaves the archive usable and, on the next run,
  reaches the same state it would have reached uninterrupted — 0 corrupt or partial artifacts.
- **SC-005**: Every archived artifact is byte-identical to what the backend served, verified
  by fingerprint at archive time — 0 silently corrupted artifacts.
- **SC-006**: 0 admin credentials appear in the archive, its manifests, the tool's logs, or
  its output.
- **SC-007**: The pull runs unattended for at least 90 days while instruments keep uploading,
  and the archive stays complete with no maintainer intervention.
- **SC-008**: From a run's output/exit status alone, a maintainer can tell whether it
  succeeded, partially succeeded, or failed, and how many artifacts were added.
- **SC-009**: For every machine folder, the archive holds **every version** of each file it
  ever captured (not just the latest), each retrievable — the foundation the later restore
  increment builds on.

## Assumptions

- Backend retention of config backups is bounded (a limited number of versions per
  instrument/file and/or a time window — `specs/003-backend-api-support` names the version
  cap). The local archive exists precisely because that retention is not durable.
- The admin retrieval endpoints from `specs/003-backend-api-support` (list backups with
  filters + paging, fetch content by id) are available and stable. **Verified**: the list
  endpoint returns full per-file version history (all versions, newest first).
- The archive keeps every version indefinitely; there is no local retention/prune in this
  increment (see Clarifications).
- The maintainer machine that runs the pull has durable, backed-up storage for the archive;
  protecting the archive itself (offsite copy, RAID) is out of scope.
- The archive tool may run on any platform (the pull is network + file writes only).
- A single admin credential covers the maintainer's needs; per-maintainer credentials or
  audit-by-identity are out of scope.
- Identical-bytes de-duplication in the archive is a nice-to-have, not required for
  correctness.

## Dependencies

- **`specs/002-v2-config-backup-telemetry`** — produces the backups this feature archives;
  defines the watched-file set, `file_key` scheme, and `source_path`.
- **`specs/003-backend-api-support`** — the admin retrieval API (list / content) and the
  backend retention behaviour this feature compensates for. The existing `GET .../backups`
  endpoint (filters + paging) already returns full per-file version history — no spec-003
  change needed.
- An external scheduler on the maintainer machine (initially a cron job) to run the pull
  regularly — configured by the operator, **not shipped or specified by this feature**.
- Durable local storage for the archive.
- The existing `tools/fleet_backup_pull.py` seed (first cut of the pull).

## Open Items

- **Confirm backend retention parameters** (version cap and/or time window) with the backend
  team, so operator guidance can state a safe maximum pull interval — the interval must be
  comfortably shorter than the shortest retention so no version is pruned between runs. Not
  blocking: the tool's behaviour does not change, only the guidance number.

## Out of Scope

- **The schedule itself.** The pull is triggered by an external scheduler (a cron job for
  now). Setting it up, shipping a cron entry / scheduled-task definition, and choosing the
  interval are the operator's job. This feature only guarantees the tool is safe and correct
  when run that way.
- Any change to the v2 agent, the backend upload path, or the backend's own retention policy.
- Protecting the archive storage itself (offsite replication, encryption at rest, RAID).
- A GUI; per-maintainer identity/audit; multi-tenant access control.
- Telemetry-record (`agent_status` heartbeat) archival — this feature is configuration
  **backups** only.
- Migrating historical v1 Nextcloud uploads into the archive.

## Deferred — later increments (same feature area, not now)

These were specified in the original draft and remain the intended direction; they are cut
from the current increment at the user's request and will return as their own scoped work:

- **Restore** — write a chosen archived version of an instrument's configuration back to its
  original locations on the target machine, with a dry-run preview, a recoverable pre-restore
  copy, subset selection, "as of <date>" version selection, lock/running-software safety, and
  a restore-to-alternate-root staging mode.
- **Inspection** — list, per instrument, the files and versions the archive holds (including
  versions the backend has pruned) with dates and sizes; diff an instrument's archived set
  against the backend's current view.
- **Unattended-operation niceties** — surfacing instruments that have stopped producing
  backups within a staleness window; richer run reporting / alerting hooks.

FR-010 and FR-011 (keep every version; keep a complete manifest) are retained in this
increment specifically so the deferred restore/inspection work has the data it needs.
