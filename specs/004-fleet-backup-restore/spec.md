# Feature Specification: Fleet Backup Archive & Restore

**Feature Branch**: `004-fleet-backup-restore`

**Created**: 2026-09-08

**Status**: Draft

**Input**: User description: "a fleet backup pull + restore maintenance tool (spec 004)". The
v2 agent (`specs/002-v2-config-backup-telemetry`) uploads changed device-configuration files
to `api.picoquant.com`, which retains only a limited window of them per instrument/file. A
maintainer needs an off-device tool that (1) runs on a schedule and pulls every new backup
into a permanent local archive, downloading only what it does not already hold, and (2)
restores a chosen instrument's configuration from that archive onto the instrument.

## Overview

The v2 agent is device→backend only and the backend keeps backups only transiently (a bounded
number of versions per file, and/or a time window). Nothing today turns that transient store
into a durable, restorable one.

This feature is the **maintainer side** of configuration backup:

- **Archive** — a scheduled job pulls every config backup the backend currently exposes for
  the fleet into a local archive, organised one folder per instrument serial. It downloads
  only artifacts the archive does not already contain, and it **never deletes** history it has
  already captured, so the archive stays complete even after the backend prunes.
- **Restore** — a maintainer selects an instrument (and, optionally, a point in time or a
  subset of files) and the tool writes those files back to their original locations on the
  target machine, after previewing the change, taking a recoverable copy of what is currently
  there, and confirming.
- **Inspect** — a maintainer can see, per instrument, which files and which versions the
  archive holds (including versions the backend has since dropped), with dates and sizes.

The tool is operated by PicoQuant maintainers/support, off the instrument, using the existing
admin credentials for `api.picoquant.com`. Those credentials are never placed on an
instrument. The agent, the backend, and the upload path are unchanged by this feature.

The seed implementation `tools/fleet_backup_pull.py` already covers a first cut of the pull
half and is the starting point.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The fleet's backups accumulate in a permanent local archive (Priority: P1)

A scheduled job runs the archive pull at a regular interval. Each run fetches every config
backup the backend now exposes for every instrument that it has not already stored locally,
verifies each one, and records it in the archive. Backups the backend has since pruned but
that a previous run captured remain in the archive untouched.

**Why this priority**: The backend is a transient store. Without a durable archive that keeps
accumulating, a device's configuration history is lost on the backend's normal pruning
schedule and an on-site rebuild becomes the only recovery path — the exact outcome v2 exists
to prevent.

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
   left consistent (no partial files).
5. **Given** an instrument that has never appeared before, **When** the pull runs, **Then** a
   new per-serial folder is created for it and all of its available backups are archived.
6. **Given** a downloaded artifact whose content does not match its recorded fingerprint,
   **When** the pull runs, **Then** that artifact is not written into the archive, the failure
   is reported, and the rest of the run continues.

---

### User Story 2 - A maintainer restores an instrument's configuration from the archive (Priority: P1)

A device has lost or corrupted its configuration. A maintainer selects that instrument in the
archive and runs a restore against the target machine. The tool shows exactly which files
would be written where, copies the current on-device versions somewhere recoverable, asks for
confirmation, then writes the archived files to their original locations.

**Why this priority**: A backup that cannot be put back is not a backup. Restore is the point
of the whole exercise.

**Independent Test**: On a machine with known-good configuration, archive it, deliberately
change/delete some of those files, run a restore of the latest archived set, and confirm every
file is byte-identical to what was archived and that the pre-restore state was preserved
somewhere it can be recovered from.

**Acceptance Scenarios**:

1. **Given** an archived instrument and a target machine, **When** a maintainer runs a restore
   in preview mode, **Then** the tool lists every file it would write, its destination path,
   and whether that destination currently exists / differs, and writes nothing.
2. **Given** the maintainer confirms the restore, **When** it runs, **Then** each archived
   file is written to its original location, byte-for-byte, and a recoverable copy of every
   file that was overwritten (or newly created) is kept.
3. **Given** a restore that has been applied, **When** the maintainer decides it was wrong,
   **Then** they can return the machine to its exact pre-restore state from the copy the tool
   kept.
4. **Given** a file in the archived set whose destination directory does not exist on the
   target, **When** the restore runs, **Then** the tool reports it and does not create
   unexpected directory trees without the maintainer's acknowledgement.
5. **Given** the instrument software is running during a restore, **When** the tool detects
   it, **Then** it warns (or refuses, per a flag) rather than overwriting files that may be
   open.
6. **Given** a restore is interrupted partway, **When** the maintainer re-runs it, **Then**
   the operation completes to the same end state and no file is left half-written.

---

### User Story 3 - A maintainer inspects the archive and picks a version to restore (Priority: P2)

Before restoring, a maintainer lists what the archive holds for an instrument: which files,
how many versions of each, their dates and sizes — including versions the backend no longer
has. They can then restore the latest, or a specific earlier version, or the set as it stood
on a given date.

**Why this priority**: "Restore the latest" is the common case, but recovering from a bad
configuration change means going back to a known-good point, which requires history and
selection. It builds on Stories 1–2.

**Acceptance Scenarios**:

1. **Given** an archived instrument, **When** a maintainer lists it, **Then** they see each
   file, the number of archived versions, and each version's date and size.
2. **Given** a file with several archived versions, **When** a maintainer restores "the
   version as of <date>", **Then** the newest version at or before that date is used for each
   file.
3. **Given** a version that exists in the archive but no longer on the backend, **When** a
   maintainer selects it, **Then** it restores from the local copy without needing the
   backend.
4. **Given** an instrument that reads its serial as "unknown", **When** its backups are
   archived, **Then** they are grouped under an explicit "unknown" folder and the maintainer
   can still find and restore them by machine identifier.

---

### User Story 4 - Restore is selective and safe (Priority: P2)

A maintainer restores only part of an instrument's configuration — for example just the device
database, or just the settings files — without touching anything else. The tool never writes a
file that is not in the selected archived set, and it always leaves a way back.

**Acceptance Scenarios**:

1. **Given** a restore scoped to a subset of files, **When** it runs, **Then** only those
   files are written and every other file on the device is untouched.
2. **Given** a restore, **When** it runs, **Then** the tool never deletes a device file that
   is absent from the archived set (it only adds or overwrites within the set).
3. **Given** a dry run, **When** it completes, **Then** the on-device state is provably
   unchanged.
4. **Given** a completed restore, **When** the maintainer inspects the result, **Then** a
   record of what was written, from which archived version, when, and where the pre-restore
   copy is, is available.

---

### User Story 5 - The archive job is safe to run unattended (Priority: P3)

A maintainer runs the pull from an external scheduler (initially a cron job — **the schedule
itself is not part of this feature**). The tool must therefore be safe to run unattended: safe
when runs overlap or fire back-to-back, clear about whether it did anything, and legible
enough that a maintainer can tell from its output and exit status alone whether the fleet is
being archived and whether any instrument has stopped producing backups.

**Acceptance Scenarios**:

1. **Given** the pull is scheduled every N hours, **When** two runs overlap, **Then** the
   second does not corrupt the archive and either waits or exits cleanly.
2. **Given** a run completes, **When** a maintainer reads its output/log, **Then** they can
   see how many artifacts were added, how many instruments were seen, and any errors, with an
   exit status that reflects success/partial/failure.
3. **Given** an instrument that has not produced a new backup in an unusually long time,
   **When** the pull runs, **Then** that gap is surfaced so a maintainer can investigate.
4. **Given** the backend is unreachable for one run, **When** the next run happens, **Then**
   it catches up with no lost artifacts and no duplicates.

---

### Edge Cases

- **Backend prunes between listing and download** — an artifact listed at the start of a run
  is gone before it is fetched: recorded as a miss for this run, retried next run; not an
  archive corruption.
- **The same content under two file identities** (a file renamed on the device) — both
  identities are archived; de-duplication of identical bytes is an optimisation, not a
  requirement.
- **An instrument's serial changes** (hardware swap, serial file fixed) — its later backups
  land under the new serial; the old serial's archive is retained and still restorable.
- **Two machines report the same serial** — their backups are distinguishable by machine
  identifier within the serial folder; a restore targets one machine's set.
- **Archive storage fills up** — the run fails safely (no partial artifacts) and reports the
  condition; it never deletes archived history to make room.
- **Clock skew on the maintainer machine** — "as of <date>" selection uses the backend's
  recorded receipt time, not local time.
- **Restore onto a machine with a newer configuration than the archive** — the tool shows the
  difference in preview; the maintainer decides; the newer state is preserved in the
  pre-restore copy.
- **A watched file is locked/open on the target during restore** — that file is skipped with
  a recorded reason; the rest of the set is restored; the skipped file is retried on request.
- **Partial fleet credentials** — the admin credential grants access to one product only:
  the tool archives that product and clearly reports the other as inaccessible rather than
  failing wholesale.
- **Re-pull after the archive folder was moved/renamed** — the tool re-establishes what is
  already present by content, not by remembered paths, and still avoids re-downloading.

## Requirements *(mandatory)*

### Functional Requirements

#### Scope & operating model

- **FR-001**: The feature MUST provide two capabilities: an **archive pull** (backend → local
  archive) and a **restore** (local archive → instrument). Inspection/listing supports both.
- **FR-002**: The tool MUST run entirely off the instrument. It MUST NOT require any change to
  the v2 agent, the backend, or the upload path, and MUST NOT introduce any inbound path to an
  instrument.
- **FR-003**: The tool MUST authenticate to `api.picoquant.com` with the existing maintainer
  admin credential, supplied from local configuration (the same `.env` the repo already uses
  for admin access) or the environment. The admin credential MUST NOT be written into the
  archive, logs, or any artifact the tool produces, and MUST NOT be distributed to
  instruments.
- **FR-004**: The tool MUST support both products (`luminosa`, `solira`) and be extensible to
  further products without a structural change. If the admin credential grants access to only
  some products, the tool MUST archive those and report the rest as inaccessible.

#### Archive pull

- **FR-005**: The pull MUST be **incremental**: on each run it downloads only backup artifacts
  that are not already present in the local archive, determined by the artifact's content
  fingerprint and identity (product, instrument, file identity, version), not by timestamps
  alone.
- **FR-006**: The pull MUST be safe to run **unattended and repeatedly from an external
  scheduler**. Re-runs with no new backups MUST complete quickly, download nothing, and modify
  no archived file. Configuring the scheduler (the cron job / scheduled task) is the
  operator's responsibility and is **not** delivered by this feature.
- **FR-007**: The local archive MUST be **durable and append-only with respect to history**:
  once an artifact has been archived by a successful run, later runs MUST NOT delete or
  overwrite it, even if the backend no longer exposes it. The archive is the system of record;
  the backend is a transient source.
- **FR-008**: Each archived artifact MUST be **integrity-verified** against its recorded
  fingerprint before being committed to the archive. A mismatch MUST NOT be written, MUST be
  reported, and MUST NOT abort the rest of the run.
- **FR-009**: The archive MUST be organised as **one folder per instrument serial** (with an
  explicit folder for instruments whose serial is unknown), so a whole fleet can be pulled in
  one run and a single instrument located and operated on in isolation.
- **FR-010**: Within an instrument's folder, the archive MUST retain **every version** of each
  configuration file it has ever captured, each identifiable and retrievable, with its backend
  receipt time and size.
- **FR-011**: The archive MUST record, per instrument, a **manifest** of every backup it holds
  — file identity, versions, fingerprints, sizes, receipt times, original source path, and the
  reporting machine identifier — sufficient to drive a restore and to answer "what do we have"
  without contacting the backend.
- **FR-012**: A pull that is **interrupted** MUST leave the archive consistent (no partial
  artifacts, manifest not corrupted) and MUST resume correctly on the next run.
- **FR-013**: The pull MUST NOT attempt to backfill artifacts the backend has already pruned
  and never showed it; it archives what is currently available plus what it already holds.
- **FR-014**: The pull MUST support restricting a run by **instrument** (one or many) and by
  **date range**, in addition to the default "everything".
- **FR-015**: The pull MUST report, per run: artifacts added, instruments seen, per-artifact
  failures, and an overall status distinguishing success / partial / failure, with an exit
  status a scheduler can act on.
- **FR-016**: The pull MUST surface **instruments that have not produced a new backup within a
  configurable window**, so a maintainer can spot an instrument that has stopped backing up.
- **FR-017**: Concurrent or overlapping scheduled runs MUST NOT corrupt the archive; the tool
  MUST serialise or safely decline a second concurrent run.

#### Restore

- **FR-018**: The restore MUST operate **from the local archive** and MUST NOT require the
  backend (so a version the backend has pruned is still restorable).
- **FR-019**: The restore MUST default to a **preview / dry run** that lists every file it
  would write, its destination path, and whether the destination is missing / identical /
  different, and that writes nothing.
- **FR-020**: Applying a restore MUST require an explicit confirmation (interactive prompt or
  an explicit flag) distinct from the preview.
- **FR-021**: Before overwriting or creating any file, the restore MUST take a **recoverable
  copy of the current on-target state** of every path it will touch, such that the maintainer
  can return the machine to its exact pre-restore state.
- **FR-022**: The restore MUST write each archived file to its **original location** (from the
  recorded source path), byte-for-byte identical to the archived version.
- **FR-023**: The restore MUST support selecting **which version** to restore: the latest, a
  specific archived version, or "the set as of <date>" (newest version at or before that date,
  per file).
- **FR-024**: The restore MUST support **restoring a subset** of an instrument's files (e.g. a
  single file, or a named group), touching only the selected files.
- **FR-025**: The restore MUST NOT delete any file on the target that is absent from the
  selected archived set; it only adds or overwrites within the set.
- **FR-026**: If a target file is **locked/open** at restore time, that file MUST be skipped
  with a recorded reason and the rest of the set still restored; the skipped file can be
  retried.
- **FR-027**: If the **instrument software appears to be running**, the restore MUST warn by
  default and MUST support a mode that refuses until it is stopped.
- **FR-028**: A restore that is **interrupted** MUST be safely re-runnable to the same end
  state, with no half-written files.
- **FR-029**: After a restore, the tool MUST record **what was written, from which archived
  version, when, to where, and where the pre-restore copy is kept**.
- **FR-030**: The restore MUST support targeting an **alternate root** (staging directory)
  instead of the live locations, for rehearsal or for preparing a bundle to apply by hand.

#### Inspection

- **FR-031**: The tool MUST list, per instrument, the files and versions the archive holds,
  with dates and sizes, and MUST indicate which versions are also still on the backend.
- **FR-032**: The tool MUST let a maintainer verify an instrument's archived set against the
  backend's current view (what is only local, what is only on the backend, what matches).

### Key Entities *(include if feature involves data)*

- **Backup Artifact**: one version of one configuration file for one instrument — its content,
  content fingerprint, size, backend receipt time, original source path, reporting machine
  identifier, and file identity. Produced by the v2 agent, retained transiently by the
  backend, retained permanently by the archive.
- **Local Archive**: the durable, append-only-with-respect-to-history store on a maintainer
  machine. Organised by product then instrument serial. Holds every artifact ever captured
  plus a per-instrument manifest. The system of record for configuration history.
- **Instrument Folder**: all archived artifacts for one instrument serial (or the "unknown"
  bucket), plus its manifest — the unit that is pulled, inspected, and restored.
- **Manifest**: the per-instrument index of archived artifacts and their metadata; drives
  restore and inspection without the backend.
- **Restore Plan**: the computed set of (archived version → target path) writes for a restore,
  shown in preview and applied on confirmation.
- **Pre-Restore Copy**: the recoverable snapshot of every on-target path a restore touched,
  captured before the restore writes, enabling rollback.
- **Run Report**: the per-pull-run summary — artifacts added, instruments seen, failures,
  overall status, stale-instrument warnings — and the per-restore record.
- **Maintainer Admin Credential**: the existing `api.picoquant.com` admin key, held only on
  maintainer machines, never on an instrument, never written into archive/logs/artifacts.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Any backup artifact that appeared on the backend while at least one scheduled
  pull ran successfully afterward is present in the local archive **permanently** — 0 lost to
  backend pruning.
- **SC-002**: A scheduled pull over a fleet of ~1,000 instruments that finds nothing new
  completes in under 5 minutes and writes 0 bytes into the archive.
- **SC-003**: Re-running a pull re-downloads **0** artifacts the archive already holds.
- **SC-004**: A maintainer can restore any archived instrument to its most recent archived
  configuration within 15 minutes, from the archive alone, without contacting the customer.
- **SC-005**: A restore produces files **byte-identical** to the archived versions at their
  original locations, and the pre-restore state is 100% recoverable afterwards.
- **SC-006**: For any restore, a maintainer can answer "what was changed, from when, and how
  do I undo it" from the tool's records alone.
- **SC-007**: 0 admin credentials appear in the archive, its manifests, the tool's logs, or
  any bundle it produces.
- **SC-008**: A restore scoped to a subset of files changes **only** those files on the
  target — 0 unintended writes or deletions.
- **SC-009**: A pull interrupted at any point leaves the archive usable and, on the next run,
  reaches the same state it would have reached uninterrupted — 0 corrupt or partial artifacts.
- **SC-010**: The archive job runs unattended for at least 90 days with no maintainer
  intervention while instruments keep uploading, and the archive stays complete.
- **SC-011**: A maintainer can list every archived version of a given instrument's file,
  including versions the backend has pruned, and restore any of them.
- **SC-012**: When an instrument stops producing backups, the condition is visible in a pull
  run's output within one configured staleness window.

## Assumptions

- Backend retention of config backups is bounded (a limited number of versions per
  instrument/file and/or a time window — `specs/003-backend-api-support` names the version
  cap). The local archive exists precisely because that retention is not durable.
- The admin retrieval endpoints from `specs/003-backend-api-support` (list backups with
  filters + paging, fetch latest, fetch content by id) are available and stable. If history
  beyond "latest" per file is needed and the list endpoint does not already provide it, that
  is a small addition to spec 003 (see Dependencies).
- The maintainer machine that runs the scheduled pull has durable, backed-up storage for the
  archive; protecting the archive itself (offsite copy, RAID, etc.) is out of scope for this
  tool.
- Restore is applied by a maintainer who is on or has file-system access to the target
  instrument; there is no remote push to a running instrument (Constitution / spec 002
  FR-035). "Restore to an alternate root" covers preparing a set to apply by hand.
- Instruments are Windows; configuration files live at the paths the v2 agent recorded as
  `source_path`. The archive tool itself may run on any platform for the pull; restore runs on
  Windows.
- "The instrument software is running" is detectable well enough (a named process / a locked
  file) to drive the FR-027 warning.
- Identical-bytes de-duplication in the archive is a nice-to-have, not required for
  correctness.
- A single admin credential covers the maintainer's needs; per-maintainer credentials or
  audit-by-identity are out of scope.

## Dependencies

- **`specs/002-v2-config-backup-telemetry`** — produces the backups this feature archives;
  defines the watched-file set, `file_key` scheme, and `source_path`.
- **`specs/003-backend-api-support`** — the admin retrieval API (list / latest / content) and
  the backend retention behaviour this feature compensates for. If per-file version **history**
  (not just "latest") is not already listable via the existing `GET .../backups` endpoint with
  filters + paging, spec 003 gains a small clarification/addition to expose it.
- An external scheduler on the maintainer machine (initially a cron job) to run the pull
  regularly — configured by the operator, explicitly **not shipped or specified by this
  feature**. This feature's obligation is only that the tool is correct and safe to run that
  way.
- Durable local storage for the archive.
- The existing `tools/fleet_backup_pull.py` seed (pull half, first cut).

## Open Items

- **Confirm backend retention parameters** (version cap and/or time window) so the scheduled
  pull interval can be set with margin — a pull interval must be comfortably shorter than the
  shortest retention so no version is pruned between runs.
- **Confirm the admin list endpoint returns full per-file history** (all versions), or agree
  the small spec-003 addition to expose it.
- Decide whether the archive keeps **all** versions forever or applies its own configurable
  retention (default assumed: keep everything).

## Out of Scope

- **The schedule itself.** The pull is triggered by an external scheduler (a cron job for
  now). Setting it up, shipping a cron entry / scheduled-task definition, and choosing the
  interval are the operator's job. This feature only guarantees the tool is safe and correct
  when run that way.
- Any change to the v2 agent, the backend upload path, or the backend's own retention policy.
- Remote push / "restore now" to a running instrument over the network (spec 002 FR-035).
- Protecting the archive storage itself (offsite replication, encryption at rest, RAID).
- A GUI; per-maintainer identity/audit; multi-tenant access control.
- Telemetry-record archival (this feature is configuration **backups** only, not the
  `agent_status` heartbeat history).
- Automated decision-making about *when* to restore an instrument — the tool executes a
  maintainer's decision, it does not make it.
- Migrating the historical v1 Nextcloud uploads into the archive.
