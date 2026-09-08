# Feature Specification: V1 → V2 Unattended Remote Upgrade Path

**Feature Branch**: `001-v2-remote-upgrade`

**Created**: 2026-09-08

**Status**: Draft

**Input**: User description: "v1→v2 unattended remote-upgrade path an explicit requirement in the spec"

## Overview

The project is being rebuilt as version 2 with a different feature set and a different
implementation. Version 2 will not ship until there is a proven way to move every machine
already running version 1 onto version 2 **without anyone touching the machine**. This
specification defines that upgrade path as a first-class deliverable, independent of whatever
new capabilities version 2 adds.

This spec covers **only** the migration capability. The new v2 feature set is specified
separately.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A fielded v1 machine moves itself to v2 with no site visit (Priority: P1)

A customer instrument runs the version 1 uploader as an unattended background service. The
maintainer publishes version 2. Within a bounded time and without remote-desktop access, a
physical visit, or any customer action, the machine replaces its running v1 software with v2,
resumes its normal upload duties on v2, and reports that it has done so.

**Why this priority**: This is the entire reason the feature exists. If it does not work, v2
cannot be released, because the installed base would be stranded on unmaintainable software.

**Independent Test**: Provision a machine with a current v1 release (including its auto-update
mechanism), publish a v2 build to the release channel, wait for the upgrade trigger, and
confirm the machine is running v2, is still performing uploads, and no operator logged in.

**Acceptance Scenarios**:

1. **Given** a machine running the latest v1 with its auto-update mechanism intact, **When** v2
   is published to the release channel and the upgrade trigger fires, **Then** the machine is
   running v2 and has performed at least one successful upload on v2 without any interactive
   session.
2. **Given** a machine that has just completed the upgrade to v2, **When** the machine reboots
   or the next scheduled cycle runs, **Then** it stays on v2 and does not re-download or
   re-apply the upgrade.
3. **Given** a machine already running v2, **When** the upgrade check runs, **Then** it takes
   no action and records that it is current.
4. **Given** a fleet of machines on several different v1 point releases, **When** v2 is
   published, **Then** every machine that can reach the release channel converges on v2
   regardless of which v1 version it started from.

---

### User Story 2 - A failed or interrupted upgrade leaves a working service (Priority: P1)

An upgrade is attempted on a fielded machine and something goes wrong: the download is cut
off, power is lost mid-install, the new version fails to start, or the new version starts but
cannot perform its job. The machine must not be left with a broken or absent service. It must
end up running a version that works — either the new one or the previous one — and it must try
again later.

**Why this priority**: An unattended upgrade that can brick a customer machine is worse than
no upgrade at all, because recovery then requires exactly the site visit the feature was
meant to avoid.

**Independent Test**: Inject each failure (kill network during download, kill power during
install, ship a v2 build that exits immediately, ship a v2 build that runs but never uploads)
and confirm the machine is left running a working uploader and retries on the next trigger.

**Acceptance Scenarios**:

1. **Given** an upgrade in progress, **When** the download is interrupted before completion,
   **Then** the machine continues running its current version and retries the upgrade on the
   next trigger.
2. **Given** an upgrade in progress, **When** power is lost during installation and the
   machine reboots, **Then** the machine comes back running a working uploader (previous or
   new version) within one trigger cycle.
3. **Given** a downloaded upgrade package, **When** its integrity check fails, **Then** the
   package is rejected, not installed, and the failure is recorded.
4. **Given** v2 has been installed, **When** v2 fails a post-upgrade health check within a
   defined window, **Then** the machine automatically rolls back to the retained v1 version,
   confirms v1 is running and functioning, and records the failed attempt.
5. **Given** a machine that rolled back to v1 after a failed v2 attempt, **When** the next
   upgrade trigger fires and v2 (or a newer v2.x) is still the published version, **Then** the
   machine retries the upgrade rather than staying on v1 indefinitely.
6. **Given** any failed upgrade attempt, **When** the maintainer inspects the machine's
   telemetry, **Then** the failure and its cause are visible without a site visit.

---

### User Story 3 - The maintainer can see which machines have upgraded (Priority: P2)

After publishing v2, the maintainer needs to know how the rollout is going: which machines are
on v2, which are still on v1, and which have tried and failed. This must be answerable from
data the machines send in, not by contacting customers.

**Why this priority**: Without rollout visibility the maintainer cannot tell a slow-but-fine
rollout from a stuck one, cannot decide when it is safe to drop v1 support, and cannot spot
machines that need intervention.

**Independent Test**: Run a mixed set of machines (some upgraded, some not, some failed), then
confirm the maintainer can produce an accurate per-machine version-and-status list purely from
uploaded telemetry.

**Acceptance Scenarios**:

1. **Given** a machine on any version, **When** it completes an upload cycle, **Then** its
   reported data identifies the software version it is currently running.
2. **Given** a machine that attempted and failed to upgrade, **When** the maintainer reviews
   telemetry, **Then** that machine is distinguishable from one that has not yet attempted.
3. **Given** the full fleet, **When** the maintainer checks rollout status, **Then** they can
   determine the count and identity of machines on v1 vs v2 vs failed.

---

### User Story 4 - The upgrade mechanism can upgrade itself (Priority: P2)

Version 2 may need a different upgrade mechanism, different install locations, a different
service identity, or a different scheduled trigger. The release that carries v2 must be able
to rewrite those things **using the v1 mechanism**, so that the machine ends up on v2's new
mechanism and all future upgrades continue to work.

**Why this priority**: If the migration release changes the upgrade plumbing in a way the old
plumbing cannot execute, machines get exactly one more upgrade and are then stranded again —
the original problem, deferred.

**Independent Test**: On a v1 machine, deliver a v2 that intentionally relocates the install
directory / renames the service / replaces the updater and trigger, then confirm the old
mechanism performs the transition and a subsequent v2→v2.x upgrade succeeds through the new
mechanism.

**Acceptance Scenarios**:

1. **Given** a v1 machine, **When** the migration release is applied, **Then** any changed
   install location, service identity, and scheduled trigger are updated to v2's scheme and no
   v1 remnants keep running.
2. **Given** a machine that has migrated to v2's new mechanism, **When** a later v2 update is
   published, **Then** it is delivered and applied through v2's mechanism without manual help.
3. **Given** the migration changes the update trigger, **When** the migration completes,
   **Then** the new trigger is active and the old trigger is removed or disabled.

---

### User Story 5 - Machines without a working v1 updater (Priority: P3)

Some fielded machines may run a v1 build old enough that it has no auto-update mechanism, or
one whose updater is broken. These machines are **explicitly out of scope for unattended
upgrade**. They are handled by a tracked, one-time manual push and must still be counted in
fleet status so the maintainer knows how many remain.

**Why this priority**: These machines exist in unknown numbers and cannot be reached by the
normal path. The bulk of fleet value is delivered by Stories 1–2; this story only needs a
runbook and a way to count the remainder.

**Independent Test**: Provision a machine with a pre-auto-update v1 build, publish v2, and
confirm (a) the machine does not attempt or receive an unattended upgrade, (b) it appears in
fleet status as "manual intervention required", and (c) the documented manual runbook moves
it onto v2.

**Acceptance Scenarios**:

1. **Given** a machine with no functioning update mechanism, **When** v2 is published, **Then**
   the machine takes no unattended action and remains on its working v1 version.
2. **Given** the same machine, **When** the maintainer reviews fleet status, **Then** it is
   listed as requiring manual intervention and is distinguishable from machines that failed an
   unattended attempt.
3. **Given** the documented manual runbook, **When** a maintainer applies it to such a machine,
   **Then** the machine ends up on v2 with v2's update mechanism installed, and future upgrades
   proceed unattended.

---

### Edge Cases

- **Download-time network loss**: partial or corrupt package must never be installed; retry
  later.
- **Integrity check failure**: a package whose checksum/signature does not verify is discarded.
- **Installer/installer-equivalent returns a non-success result**: treated as a failed attempt;
  current version preserved.
- **Service will not stop** (hung, or file locks held): upgrade must either force a safe
  takeover or defer, never leave two versions half-installed.
- **Power loss between "old removed" and "new running"**: next boot must self-heal to a working
  version within one trigger cycle.
- **Disk full / insufficient space** for the package or the install: attempt fails cleanly,
  space is reclaimed, current version keeps running.
- **Machine never reboots** (trigger is boot-based) or reboots rarely: upgrade must still occur
  within the bounded rollout window via some non-boot opportunity.
- **Clock skew / wrong system time**: must not permanently block the upgrade check or cause a
  newer version to look older.
- **Downgrade attempt**: a machine on v2 must not be pulled back to v1 by the release channel
  still listing v1, or by version-comparison quirks (e.g. `2.0` vs `1.11`).
- **v1 device configuration/secrets** (e.g. the upload destination link) must survive the
  upgrade so v2 keeps working without re-provisioning. The migration performs a one-time,
  idempotent translation of v1 config into v2's format/location.
- **v1 config is missing, partial, or malformed** at migration time: the translation must not
  crash the upgrade; it produces the best valid v2 config it can and records what could not be
  translated so the machine surfaces as needing attention rather than silently misconfigured.
- **Migration re-runs** (e.g. a later re-attempt after rollback): the config translation must
  detect that v2 config already exists and not overwrite or corrupt it.
- **Security software** on the customer machine blocks the downloaded package: recorded as a
  failure cause the maintainer can see.
- **Two upgrade runs overlap** (e.g. trigger fires again while one is running): the second must
  no-op or wait, never corrupt the install.
- **Customer has restricted outbound network**: if the release channel host is unreachable,
  the machine keeps running its current version and reports that it could not check.

## Requirements *(mandatory)*

### Functional Requirements

#### Delivery & trigger

- **FR-001**: The system MUST allow a maintainer to publish a v2 release to a channel that
  fielded v1 machines already check, such that no per-machine action is required to begin the
  rollout.
- **FR-002**: Each fielded machine MUST check for a newer version automatically on a recurring
  basis without any interactive session.
- **FR-003**: The upgrade check and application MUST run with sufficient privilege to replace
  the service software and MUST NOT require a logged-in user.
- **FR-004**: The system MUST bound the rollout: **≥ 95% of reachable machines within 14
  days** of a stable publication (SC-001; for the v1→v2 hop, "or next boot" per FR-005).
  `tools/fleet-status.py` MUST report the migrated percentage against this bound; a `stuck`
  count above a set threshold is the signal to ship a v1 bridge release.
- **FR-005**: For **v2→v2.x** updates, the machine MUST NOT depend on a boot event alone:
  v2's updater task carries both a boot trigger and a daily trigger.

  **v1→v2 first hop exception**: the fielded v1 task is `ONSTART`-only and cannot be changed
  remotely, so the first hop is boot-triggered. This is accepted (clarification C1 — the
  Luminosa fleet reboots often enough for the SC-001 window). If a fleet turns out to reboot
  rarely, a **v1 bridge release** (`installer/v1-bridge.iss`, not built) that adds a daily
  trigger ships first. SC-001's window is read as "14 days *or* next boot after publication"
  for the v1→v2 hop.

#### Release channels & staged rollout

- **FR-005a**: There MUST be two release channels: **stable** and **beta**. A machine's
  channel is fixed by the build it runs (compiled in); it is not switchable via config or
  from the backend. CI produces a separate artifact per product per channel.
- **FR-005b**: A `stable` build MUST only ever self-update to a **stable** release; a `beta`
  build MUST only ever self-update to a **beta** release (published as a prerelease). Neither
  crosses to the other channel.
- **FR-005c**: The existing v1 fleet's updater MUST only pick up **stable** v2 releases (it
  already queries "latest", which excludes prereleases) — the automatic v1→v2 rollout is
  therefore always via a stable release. Beta v2 builds reach the beta cohort only by being
  installed by hand.
- **FR-005d**: A stable `vX.Y.Z` MUST NOT be published until the **matching beta build** has
  run at least **7 days** on at least **3 beta instruments** with **zero Sev-1 telemetry**
  over that window. Sev-1 = any of: an upgrade `outcome` of `rollback_failed`; a **crash-loop**
  (≥ 3 service starts within 1 h, or `cycle.ok = false` on ≥ 3 consecutive heartbeats); a
  **stopped service** (no heartbeat for ≥ 3× the cycle interval while the machine is
  otherwise reachable). (Constitution — Build, Release & Distribution.)
- **FR-005e**: The maintainer MUST be able to tell, from fleet telemetry alone, which
  machines are on the beta channel and their health over the beta window, so FR-005d can be
  evaluated without contacting customers.

#### Integrity & safety

- **FR-006**: The system MUST verify the integrity and authenticity of an upgrade package
  before applying it, and MUST refuse to apply a package that fails verification.
- **FR-007**: The system MUST NOT remove or disable the working version until the replacement
  is staged and verified.
- **FR-008**: If applying the upgrade is interrupted at any point, the machine MUST converge to
  a single working version within one subsequent trigger cycle.
- **FR-009**: After applying v2, the system MUST run a health check that confirms the uploader
  is running and able to perform its core duty, within a defined window.
- **FR-010**: If the post-upgrade health check fails, the system MUST automatically roll the
  machine back to the retained previous version, verify that version is running and
  functioning, and record the failed attempt with its cause.
- **FR-010a**: The migration MUST retain, on the device, everything required to restore and run
  the previous version (its binaries and its configuration) until the new version has passed
  its health check. Retained previous-version artifacts MAY be removed only after the new
  version is confirmed healthy.
- **FR-010b**: After a rollback, the machine MUST retry the upgrade on subsequent triggers (not
  remain pinned to the old version) while the newer version is still the published target, and
  MUST report each retry outcome. The system MAY apply a backoff between retries but MUST NOT
  stop retrying on its own.
- **FR-011**: The system MUST prevent a machine from being moved to an older version than it is
  currently running, including across the v1→v2 version-numbering boundary. An automatic
  rollback under FR-010 is not a downgrade for this purpose.
- **FR-012**: Concurrent or overlapping upgrade attempts on one machine MUST NOT corrupt the
  installation; at most one apply operation runs at a time.
- **FR-013**: The upgrade MUST be idempotent: re-running the check or apply on an
  already-current machine makes no changes.

#### Configuration continuity

- **FR-014**: Any device-specific configuration required for the uploader to function MUST
  survive the upgrade with no re-provisioning, via a one-time translation of the v1
  configuration into v2's format and location. **Note**: v2's upload destination and
  credential are compiled into the build (spec 002), and v1's `public_link` is
  Nextcloud-specific and deliberately **not** carried. In practice the translation carries at
  most non-transport settings (e.g. the poll interval → `cycle_interval_secs`) — the
  machinery below matters mostly for the missing/partial/idempotent cases.
- **FR-014a**: The configuration translation MUST be idempotent: if v2 configuration already
  exists (e.g. on a retry after rollback), the translation MUST NOT overwrite or corrupt it.
- **FR-014b**: If the v1 configuration is missing, partial, or malformed, the translation MUST
  NOT abort the upgrade. It MUST produce the best valid v2 configuration it can and record
  which values could not be translated, so the machine surfaces in fleet status as needing
  attention rather than running silently misconfigured.
- **FR-014c**: The original v1 configuration MUST be preserved unmodified until the new version
  passes its health check, so that a rollback also restores working v1 configuration.
- **FR-015**: Any per-device state that prevents duplicate or redundant work MUST degrade
  safely. v2 starts with fresh state (`state.json`); by spec 002 FR-016 the first v2 cycle
  treats every watched file as changed, so the worst case is **one redundant config-file
  backup**, never data loss. v1's `*.lastcheck` / `client_version_last_upload.txt` markers
  are not migrated (they belong to mechanisms v2 does not use).

#### Mechanism migration

- **FR-016**: The migration release MUST be applyable by the v1 upgrade mechanism as it exists
  on already-fielded machines.
- **FR-017**: If v2 changes the update mechanism, install location, service identity, or
  trigger, the migration release MUST perform that transition and leave no v1 component
  running or scheduled.
- **FR-018**: After migration, all subsequent v2 updates MUST be deliverable through v2's
  mechanism without manual intervention.

#### Observability

- **FR-019**: Every machine MUST report, through data it already sends in, the software version
  it is currently running.
- **FR-020**: A machine that attempted an upgrade and failed MUST be distinguishable, from
  maintainer-visible data alone, from a machine that has not yet attempted and from one that
  succeeded.
- **FR-021**: Each upgrade attempt MUST leave a local, retrievable record of what happened and
  why (success, or failure with cause).
- **FR-022**: The maintainer MUST be able to produce a fleet-wide rollout status (per machine:
  current version, last attempt outcome) without contacting customers.

#### Scope decision for un-updatable machines

- **FR-023**: Machines with no working v1 update mechanism are OUT of scope for unattended
  upgrade. The system MUST NOT attempt to reach them through an alternate unattended channel;
  they keep running their working v1 version untouched until a maintainer acts.
- **FR-023a**: Such machines MUST still be identifiable in fleet status as "manual intervention
  required", distinct from machines that attempted an unattended upgrade and failed.
- **FR-023b**: A documented, repeatable manual runbook MUST exist that moves such a machine to
  v2 including installing v2's update mechanism, after which the machine upgrades unattended
  like any other.

### Key Entities *(include if feature involves data)*

- **Release / Upgrade Package**: the published unit a machine downloads to move to a new
  version. Key attributes: version identity, integrity/authenticity proof, the payload that
  performs install + mechanism migration.
- **Version Marker**: the on-device record of which version is currently installed and running;
  the basis for "am I current?" and downgrade prevention.
- **Upgrade Attempt Record**: per-machine, per-attempt log entry — timestamp, from-version,
  to-version, outcome, failure cause. Source of local diagnostics and of uploaded status.
- **Fleet Status View**: the maintainer-side aggregate derived from uploaded telemetry — each
  machine's current version and last attempt outcome.
- **Retained Previous-Version Bundle**: the previous version's binaries plus its unmodified
  configuration, kept on-device from the start of an upgrade until the new version passes its
  health check; the source for automatic rollback (FR-010, FR-010a).
- **Device Configuration (v1 and v2 forms)**: the destination link and other per-machine
  settings. The v1 form is preserved unmodified during upgrade; a one-time idempotent
  translation produces the v2 form (FR-014–FR-014c).
- **Manual-Intervention List**: the set of machines with no working v1 updater, tracked in
  fleet status until the manual runbook has moved them to v2 (FR-023–FR-023b).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After a **stable** v2 release is published, at least 95% of machines that can
  reach the release channel are running v2 within 14 days, with no per-machine manual action.
- **SC-001a**: No stable v2 release is cut without a documented beta record showing ≥ 7 days
  on ≥ 3 beta instruments with zero Sev-1 telemetry (FR-005d). A `stable` build never installs
  a prerelease, and a `beta` build never installs a stable release — 0 cross-channel updates.
- **SC-002**: Zero machines are left without a running, functioning uploader as a result of the
  upgrade (previous or new version always running).
- **SC-003**: 100% of upgrade attempts interrupted by network loss, power loss, or a failed
  new version converge to a working uploader within one subsequent trigger cycle.
- **SC-003a**: 100% of machines whose v2 health check fails are running the previous version
  again within one trigger cycle, and resume retrying the upgrade thereafter.
- **SC-004**: 100% of upgrade packages that fail integrity verification are rejected without
  being applied.
- **SC-005**: For every machine that reports in after publication, the maintainer can state its
  current version and last upgrade outcome within 1 business day, without contacting the
  customer.
- **SC-006**: No machine is moved to an older version at any point during the rollout.
- **SC-007**: After migration, a follow-up v2 update reaches and applies on 95% of migrated
  machines within 14 days through v2's own mechanism (proves the mechanism migrated, not just
  the software).
- **SC-008**: Device configuration re-provisioning required after a successful upgrade: 0
  machines. Machines whose v1 config was missing/partial are flagged for attention, not
  silently broken.
- **SC-008a**: Machines with no working v1 updater: 0 receive an unattended upgrade, 100% are
  visible in fleet status as manual-intervention-required until the runbook is applied.
- **SC-009**: An interrupted upgrade never loses **configuration-backup** data that v2 is
  responsible for: at worst v2's first cycle re-sends one already-current file (spec 002
  FR-016). Unsent v1 **log** files are out of scope — v2 does not collect logs (spec 002
  FR-001) — so they are intentionally not carried across the migration.

## Assumptions

- v2 is distributed through the **same release channel** that v1 machines already check
  (same repository / release feed), so publishing v2 there is what starts the rollout. If v2
  must use a different channel, a v1-compatible bridge release on the old channel is required
  first.
- Fielded machines have intermittent but real outbound internet access to the release channel
  host and to the upload destination.
- The target machines run Windows and the uploader runs as an unattended system service.
- The current v1 auto-update mechanism (a privileged, recurring, boot-triggered task that
  pulls the latest published release, verifies a checksum, and runs the installer silently)
  is representative of what is on most fielded machines.
- Machines reboot at least occasionally; a boot-triggered check alone is not assumed
  sufficient for the FR-004 time bound (hence FR-005).
- The existing per-day, per-machine version/telemetry upload is available as the basis for
  fleet status reporting.
- "v1" for the purposes of unattended upgrade means v1 releases that carry a working
  auto-update mechanism; the pre-auto-update / broken-updater population is explicitly out of
  scope for unattended upgrade and handled by the manual runbook (FR-023 / User Story 5).
- Automatic rollback (FR-010) requires enough free disk on the device to hold both the
  previous-version bundle and the new package simultaneously during an upgrade; machines
  without that headroom fail the attempt cleanly and stay on the previous version.
- v1 configuration is readable by the migration payload (same machine, same privilege), so a
  one-time translation to the v2 format is feasible during the upgrade.
- The maintainer controls the release channel and the upload-destination service.

## Dependencies

- The existing published-release distribution channel and its integrity-proof convention.
- The existing recurring privileged task mechanism on fielded v1 machines.
- The existing telemetry upload that carries per-machine version information.
- Access to the upload destination for post-upgrade health verification.

## Out of Scope

- The new v2 feature set and its implementation (specified separately).
- Changes to what logs/data are collected or how they are uploaded, except where required for
  configuration continuity or health checks.
- A customer-facing UI for the upgrade.
- Upgrades on non-Windows platforms.
