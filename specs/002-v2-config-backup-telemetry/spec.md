# Feature Specification: V2 — Config Backup & Device Telemetry

**Feature Branch**: `002-v2-config-backup-telemetry`

**Created**: 2026-09-08

**Status**: Draft

**Input**: User description: "define the features of v2". v2 stops uploading instrument log
files. It (1) sends device/version telemetry and (2) keeps a daily backup of a per-product set
of device configuration files — uploaded only when they change, at most once per day — to the
PicoQuant backend at `https://api.picoquant.com`. It runs on **Luminosa and Solira**
instruments (Windows only), delivered to the existing fleet via the v1→v2 unattended upgrade.

## Overview

Version 1 zipped and uploaded instrument logs, settings, and diagnostics to a Nextcloud
public-share folder. Version 2 has a narrower purpose and an authenticated transport:

- **Instrument log upload is removed.** `.pqlog`, `LaserPower.log` and similar operational
  logs are no longer collected or sent.
- **Device telemetry** — which machine, which instrument, which software version, which OS —
  is submitted to the backend on a regular heartbeat so the fleet is observable centrally.
- **Configuration backup** — a per-product set of device configuration files is submitted to
  a dedicated backup endpoint on the backend, but only when a file changed since its last
  successful backup, and at most once per calendar day (UTC).
- **Two products, initially**: the same v2 service runs on **Luminosa** and **Solira**
  instruments. Each submission is tagged with the product bucket (`luminosa` or `solira`).
- **Two release channels**: every build is `stable` or `beta`, compiled in. CI produces a
  separate artifact per product per channel. The agent reports its channel in the heartbeat so
  the beta cohort is visible; channel-aware self-update is owned by `specs/001-v2-remote-upgrade`.
- **Transport** is `https://api.picoquant.com`, authenticated with a fleet-wide token
  (`X-TELEMETRY-TOKEN`) per product.
- Platform scope is **Windows only**. v2 runs as an unattended Windows service.

The v1 → v2 unattended remote-upgrade path is specified separately in
`specs/001-v2-remote-upgrade/spec.md` and is a prerequisite for shipping v2.

### Backend support (deployed and verified — `api.picoquant.com` v2.2.0-beta.2, 2026-09-08)

The three backend capabilities this spec depends on were specified in
`specs/003-backend-api-support/backend-changes.md`, implemented, and **verified end-to-end**
against the live backend in this session (heartbeat submit, backup submit incl. dedupe,
missing-serial `422`, admin list/latest/content with byte-exact round trip). The exact
request/response shapes the agent uses are in `contracts/backend-api.md`.

1. **Fleet-wide, non-expiring submission token per product** (`luminosa`; `solira` pending —
   see Open Items). Not instrument-bound; the serial travels in the request body. No
   per-machine provisioning — every v2 build carries its product's token, injected at build
   from a secret (never committed). Rotatable: the backend accepts more than one valid value
   at once (`TELEMETRY_FLEET_TOKENS_<PRODUCT>`). (FR-020–FR-026)
2. **Dedicated backup endpoint** `POST /api/v2/products/{product}/backup` — accepts a file's
   contents + attribution, integrity-checked and de-duplicated by `content_sha256`, retained
   durably (not on the 30-day telemetry prune), latest per `(product, instrument, file)`
   retrievable via admin endpoints. (FR-017, FR-027)
3. **Submission path works with the fleet token, not the admin key** — verified; the agent
   never carries `X-ADMIN-API-KEY`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The fleet is visible from the backend (Priority: P1)

Every machine running v2 submits a telemetry heartbeat to the backend on a regular interval
containing enough to identify the machine, the instrument, the software version, and the OS. A
maintainer can then query the backend and see which machines are alive, what versions are
deployed, and which have gone quiet.

**Why this priority**: Central fleet visibility is the primary reason for v2 and is what makes
the separately-specified upgrade rollout observable. It delivers value even before
configuration backup exists.

**Independent Test**: Install v2 on several machines, wait one heartbeat interval, and confirm
the backend's admin telemetry list, filtered by instrument serial, returns an accurate current
record per machine; power one machine off and confirm its last-seen time identifies it as not
reporting.

**Acceptance Scenarios**:

1. **Given** a v2 machine with network access, **When** a heartbeat interval elapses, **Then**
   the backend has a telemetry record for that instrument containing at least: instrument
   serial, a stable machine identifier, the v2 software version, OS version/build/architecture,
   and a client UTC timestamp.
2. **Given** a machine that reported yesterday, **When** it is powered off today, **Then** the
   backend record shows it last reported yesterday and is distinguishable from a machine that
   reported today.
3. **Given** a machine whose details have not changed, **When** the next interval elapses,
   **Then** it still submits a heartbeat, so "alive" is distinguishable from "stopped".
4. **Given** the backend is unreachable or returns 5xx, **When** the cycle runs, **Then** the
   service records the failure locally with a category, does not crash, and retries next
   cycle; no heartbeat backlog is required (latest state only).
5. **Given** connectivity returns after an outage, **When** the next cycle runs, **Then** a
   heartbeat succeeds.

---

### User Story 2 - Changed configuration files are backed up daily (Priority: P1)

A fixed small set of device configuration files is watched. When one changes, its current
contents are submitted to the backend's backup endpoint. Each file is backed up at most once
per calendar day regardless of how many times it changes, and a file unchanged since its last
successful backup is not re-sent.

**Why this priority**: Configuration backup is the second core purpose of v2. Losing a
device's configuration means an on-site rebuild; a daily off-device copy removes that risk.

**Independent Test**: Modify one watched file, run a cycle, confirm exactly one backup reaches
the backend; modify it again the same day → no second upload; leave it unchanged next day → no
upload; change it the following day → one upload.

**Acceptance Scenarios**:

1. **Given** a watched file changed since its last successful backup, **When** a cycle runs and
   no backup of that file has succeeded today (UTC), **Then** the file's current contents are
   submitted to the backup endpoint and recorded as backed up for today.
2. **Given** a watched file already backed up successfully today, **When** it changes again the
   same day, **Then** no further upload occurs that day.
3. **Given** a watched file unchanged since its last successful backup, **When** a cycle runs,
   **Then** no upload occurs for that file.
4. **Given** a watched file locked/open by the instrument software, **When** a cycle runs,
   **Then** that file is skipped with a recorded reason and retried later; other files are
   unaffected.
5. **Given** an upload that fails, **When** the cycle runs, **Then** the file is not marked
   backed up for today and is retried on the next cycle until it succeeds.
6. **Given** a machine offline for several days during which a file changed once, **When** it
   returns online, **Then** the current contents of that file are backed up once; intermediate
   states never captured are not recoverable and this is accepted.
7. **Given** a watched file whose size exceeds the backup endpoint's limit, **When** a cycle
   runs, **Then** it is skipped with a recorded reason, surfaced in the heartbeat, and not
   retried every cycle without visibility.

---

### User Story 3 - Backups and telemetry are attributable and queryable per instrument (Priority: P2)

Every submission carries the instrument serial, the machine identifier, the software version,
and a UTC timestamp. A maintainer can list, per instrument, the version history and retrieve
the latest backup of each watched file.

**Why this priority**: A backup that cannot be matched to the right instrument is not a usable
backup. It is a property of the data, testable on top of Stories 1–2.

**Independent Test**: Submit telemetry and a backup from two machines; confirm the backend
lets a maintainer retrieve, per instrument serial, the version history and the newest backup
of each file with correct timestamps.

**Acceptance Scenarios**:

1. **Given** any v2 submission, **When** the backend stores it, **Then** the record carries the
   instrument serial, the machine identifier, the v2 version, the source file identity (for
   backups), and a client UTC timestamp.
2. **Given** repeated backups of the same file over time, **When** a maintainer queries the
   backend, **Then** the most recent successful backup for that (instrument, file) pair is
   identifiable and retrievable.
3. **Given** a machine that cannot read its instrument serial, **When** it submits, **Then**
   the submission carries an explicit "unknown" serial marker, is still stored and attributed
   to the machine identifier, and the unknown-serial condition is visible to a maintainer.
4. **Given** a Luminosa machine and a Solira machine, **When** each submits, **Then** the
   record is tagged with the correct product bucket and a maintainer can query each product's
   fleet separately.

---

### User Story 4 - The fleet token is protected and rotatable (Priority: P1)

Each product's submission token is never committed to source and never checked into the
repository; it is injected into the build from a secret. If a token is ever exposed, a
maintainer can rotate it: publish a v2 build carrying the new token, let that product's fleet
update, then retire the old token at the backend — with no window where machines cannot submit.

**Why this priority**: The token is the only thing standing between the open internet and
write access to a product's telemetry bucket. Committing it (Constitution Principle II
violation) or being unable to rotate it turns a leak into a permanent problem.

**Independent Test**: Inspect the repo and a release build — no token in source, token present
in the built binary only because CI injected it; configure the backend with two valid tokens
for a product, confirm machines on either succeed, then retire one and confirm only machines
still carrying the retired token fail.

**Acceptance Scenarios**:

1. **Given** the source repository, **When** inspected, **Then** it contains no submission
   token, no product API key, and no admin key.
2. **Given** a release build produced by CI, **When** built, **Then** the token is supplied
   from a CI secret at build time (the same pattern v1 used for the Nextcloud link).
3. **Given** the backend is configured to accept both an old and a new token, **When** a v2
   fleet update carrying the new token rolls out, **Then** every machine keeps submitting
   successfully throughout the transition.
4. **Given** all machines have moved to the new token, **When** the old token is retired at the
   backend, **Then** no machine is affected.

---

### User Story 5 - Operating the service on a single machine (Priority: P3)

The service runs unattended as a Windows service, survives reboots, records what it does where
a remote maintainer can retrieve it, and never stops itself on a recoverable error.

**Why this priority**: Operability basics carried from v1 — necessary for a shippable product,
lowest risk.

**Acceptance Scenarios**:

1. **Given** the machine reboots, **When** it comes back up, **Then** the service resumes on
   its own with no logon.
2. **Given** any recoverable error (network, locked file, 4xx/5xx, bad config), **When** it
   occurs, **Then** it is logged with a category and the loop continues.
3. **Given** a maintainer retrieves a machine's local records, **When** they read them, **Then**
   they can see recent cycle outcomes: what was checked, what was submitted, and why anything
   failed.

---

### Edge Cases

- **`PQDevice.db` open/locked** by the instrument software — skip with reason, retry next
  cycle; never a partial read.
- **A watched file changes mid-read** — the backend receives a whole, self-consistent copy
  (pre- or post-change), never a torn file; otherwise retried.
- **Instrument serial file missing/unreadable** — submissions proceed with an explicit
  "unknown" serial marker; the condition is surfaced in the heartbeat.
- **Backup body exceeds the endpoint's size limit** — recorded as skipped-oversize, surfaced
  in the heartbeat, not retried blindly every cycle.
- **Submission rejected as malformed** (backend validation failure) — recorded as a
  non-retryable "bad request" category, surfaced in the heartbeat, not retried blindly.
- **Token rejected (401)** — recorded as "authentication"; service keeps running and keeps
  retrying (a fix arrives as a v2 update carrying a valid token).
- **Machine clock wrong** — client timestamp may be off; backend also records receipt time;
  once-per-day logic keyed to UTC, not local time.
- **First run after upgrade** — no backup history: every watched file treated as changed, one
  backup each that day.
- **Watched file deleted** — recorded "absent", not an error; if it reappears changed it is
  backed up again.
- **Backend received the submission but the ack was lost** — a duplicate backup next cycle is
  acceptable; a lost backup is not.
- **`api.picoquant.com` in maintenance / 5xx** — retryable; nothing marked done; service
  continues.
- **Long offline period** — on return: one heartbeat, one backup per changed file, no attempt
  to backfill missed days.
- **Overlapping cycles** — at most one cycle acts at a time; no double submissions, no
  corrupted local state.

## Requirements *(mandatory)*

### Functional Requirements

#### Scope

- **FR-001**: v2 MUST NOT collect or upload instrument operational log files (`.pqlog`,
  `LaserPower.log`, or equivalents). Any v1 mechanism for this is removed.
- **FR-002**: v2's outbound data is limited to (a) device/version heartbeat telemetry and
  (b) backups of the watched configuration file set.

#### Product identity

- **FR-002a**: v2 MUST support the products **Luminosa** and **Solira**, and MUST be
  extensible to further products without a structural change.
- **FR-002b**: Product identity MUST be established by a **separate build per product**: CI
  produces one artifact per product, with the product bucket and that product's fleet token
  compiled in. A single artifact MUST NOT be able to run as the wrong product.
- **FR-002c**: The v1→v2 upgrade (`specs/001-v2-remote-upgrade`) MUST deliver the correct
  per-product build to each machine; a machine MUST NOT be upgradeable to a build for a
  different product than the instrument it serves.
- **FR-002d**: Every submission MUST be tagged with the build's product bucket
  (`luminosa` / `solira`).
- **FR-002e**: Each build MUST also carry a compiled-in **release channel** (`stable` /
  `beta`). CI produces a separate artifact per product per channel (4 builds initially). The
  channel MUST NOT be switchable at runtime, via config, or from the backend.
- **FR-002f**: The heartbeat MUST report the build's channel so a maintainer can identify the
  beta cohort. The health signals v2 provides per heartbeat are: the last cycle's `ok` flag,
  the current `blocked_backups` list, and the most recent submission-failure category. The
  Sev-1 conditions the promotion gate keys on that v2 does **not** send directly — a stopped
  service, a crash-loop — are derived by `specs/001-v2-remote-upgrade`'s fleet-status view
  from heartbeat **absence / gap patterns**. v2's only obligation is to emit an accurate
  heartbeat every cycle while it is running.

#### Telemetry heartbeat

- **FR-003**: Each machine MUST submit a heartbeat telemetry record on a recurring interval,
  including when nothing has changed.
- **FR-004**: The heartbeat MUST include at least: the instrument serial (or explicit unknown
  marker), the stable machine identifier, the v2 software version, the release channel
  (`stable` / `beta`), OS version/build/architecture, and a client UTC timestamp.
- **FR-004a**: The heartbeat MUST also report the **instrument control-software version**
  (Luminosa / Solira), from two independent sources, each nullable:
  1. the file-version resource of `<install_dir>\<Product>.exe` — the version installed now;
  2. the version stamped in the header line of the newest `*.pqlog` under `<data_dir>\Logs\` —
     the version that last actually ran.
  Reading one header line of a `.pqlog` for its version string is **not** log collection —
  FR-001 (no operational-log upload) still holds; the log contents are never sent. Either
  source being unreadable/absent yields `null` for that field and never fails the heartbeat.
- **FR-005**: A maintainer MUST be able to determine, from backend queries alone, each
  machine's last-seen time, current v2 version, and installed instrument-software version.
- **FR-006**: If a heartbeat fails, the machine MUST retry next interval and MUST NOT crash or
  stop; no heartbeat backlog is required (latest state only).
- **FR-007**: Any condition that blocked a backup this cycle (locked file, oversized file,
  rejected submission, absent file) MUST be reflected in the heartbeat so it is visible
  centrally.

#### Configuration backup

- **FR-008**: v2 MUST watch a per-product set of device configuration files. The intent is
  **"everything v1 handled except operational logs"**. Derived from the v1 source
  (`loguploader.py`), the **Luminosa** set is:
  - `C:\Program Files\PicoQuant\Luminosa\PQDevice.db` — device database (`copyDB`)
  - `C:\Program Files\PicoQuant\Luminosa\PQDevice.conf` — device configuration (`copyDB`)
  - `C:\ProgramData\PicoQuant\Luminosa\*.xml` — instrument settings files (`uploadSettings`,
    which in v1 also swept the `.db`/`.conf` copies renamed to `.xml`)
  - `C:\ProgramData\PicoQuant\Luminosa\UserSettings\*.xml` — user settings files
    (`uploadUserSettings`)

  Explicitly **excluded** (operational logs, dropped in v2): `...\Luminosa\Logs\*.pqlog`
  (`uploadlog`) and `...\Luminosa\LaserPower.log` (`uploadLaserPowerLog`).

  The **Solira** set is taken to mirror this layout exactly, with `Solira` substituted for
  `Luminosa` in both the install directory (`C:\Program Files\PicoQuant\Solira\`) and the data
  directory (`C:\ProgramData\PicoQuant\Solira\`), and the serial read from
  `C:\ProgramData\PicoQuant\Solira\Logs\LastOpenSerial.txt`. This is a working assumption to be
  confirmed against a Solira install before the Solira build ships (see Open Items); the
  per-product build boundary (FR-002b) means a wrong Solira path is corrected in the Solira
  build only, with no effect on Luminosa.
- **FR-009**: v2 MUST read each watched file directly from its canonical location above; it
  MUST NOT reproduce v1's copy-into-data-dir-and-rename-to-`.xml` staging. The `.db` and
  `.conf` files are backed up under their real names and extensions.
- **FR-009a**: The instrument serial number MUST be read per product from
  `C:\ProgramData\PicoQuant\<Product>\Logs\LastOpenSerial.txt` (last whitespace-separated
  token, as v1's `getLumiSerial`); if the file is missing or unreadable, submissions carry an
  explicit unknown marker.
- **FR-010**: A watched file MUST be submitted only when its contents changed since its last
  successful backup, determined by content comparison (e.g. a content hash), not modification
  time alone.
- **FR-011**: Each watched file MUST be backed up at most once per calendar day (UTC),
  regardless of how many times it changes that day.
- **FR-012**: A backup MUST be marked done for the day only after the backend confirms
  success; a failed submission MUST be retried next cycle and MUST NOT consume the daily
  allowance.
- **FR-013**: If a watched file is locked, unreadable, or absent at cycle time, it MUST be
  skipped with a recorded reason and retried later; other files MUST still be processed.
- **FR-014**: A watched file exceeding the backup endpoint's size limit MUST be skipped with a
  recorded reason, surfaced per FR-007, and not retried every cycle without visibility.
- **FR-015**: The submission MUST carry a whole, self-consistent copy of the file; if
  consistency for a changing file cannot be assured, it MUST be retried.
- **FR-016**: On first run with no backup history, every watched file MUST be treated as
  changed and backed up once that day.
- **FR-017**: Each backup submission MUST carry: the file contents, a logical file identifier,
  the source path, the file's own modification time, a content hash, and the attribution
  fields of FR-019. The backend MUST retain backups durably (not pruned on the 30-day
  hot-telemetry schedule) and MUST let the latest backup of each (product, instrument, file)
  tuple be retrieved.
- **FR-018**: v2 MUST NOT backfill backups for days it was offline; on return it backs up the
  current contents of each changed file once.

#### Transport & attribution

- **FR-019**: Every submission (heartbeat or backup) MUST carry the product bucket, the
  instrument serial (or unknown marker), the machine identifier, the v2 software version, and
  a client UTC timestamp.
- **FR-020**: All v2 submissions MUST go to `https://api.picoquant.com` over HTTPS, to the
  running instance's product bucket (`luminosa` / `solira`), authenticated with the
  `X-TELEMETRY-TOKEN` header. v2 MUST NOT use `X-ADMIN-API-KEY`.
- **FR-021**: v2 MUST use a fleet-wide token per product (the same value on every machine of
  that product). It MUST NOT attempt per-machine token minting, provisioning, or the
  technician TOTP flow.
- **FR-022**: The instrument serial in the request body is authoritative for the fleet token
  kind; v2 MUST send the serial it determined and MUST NOT assume the backend derives it from
  the token.

#### Token handling

- **FR-023**: No submission token, product API key, or admin key may appear in the source
  repository or its git history. (Constitution Principle II.) A committed example config
  carries only a placeholder.
- **FR-024**: Each product's fleet token MUST be injected into that product's build at build
  time from a **GitHub Actions secret** named the same as the backend variable —
  `TELEMETRY_FLEET_TOKENS_LUMINOSA`, `TELEMETRY_FLEET_TOKENS_SOLIRA` — following the pattern v1
  uses for `PUBLIC_LINK`. The release workflow builds a **product × channel matrix**
  (`luminosa`, `luminosa`-beta, `solira`, `solira`-beta) and passes each build only its
  product's token and its channel. Locally the token is read from a gitignored `.env` (see
  `.env.example`). The build carries exactly one token; if the value contains a
  comma-separated list, v2 uses the first entry. The same token serves both channels of a
  product. The exact in-binary injection mechanism is a planning decision (`/speckit-plan`).
- **FR-024a**: The fleet token is understood to be extractable from a distributed binary; it
  is a *keep-out-of-source* secret, not a confidential one. Its exposure is bounded (write-only
  to one product's bucket) and handled by rotation (FR-025), not by obfuscation.
- **FR-025**: v2 MUST tolerate a token-rotation transition in which the backend accepts both an
  old and a new token for a product: a machine carrying either value submits successfully.
  Rotation is performed by shipping a v2 update with the new token and then retiring the old
  one at the backend.
- **FR-026**: A rejected token (401) MUST be recorded as a distinct "authentication" failure
  category and surfaced in the heartbeat where a still-valid token allows; the service MUST
  keep running and keep retrying.
- **FR-027**: Submission failures MUST be categorized locally and (where possible) in the
  heartbeat at least as: no-network, authentication, rejected-bad-request, too-large,
  backend-error.

#### Operability

- **FR-028**: v2 MUST run unattended as a Windows service and resume automatically after a
  reboot with no interactive logon.
- **FR-029**: The service loop MUST NOT terminate on a recoverable error; each cycle is
  isolated. (Constitution Principle I.)
- **FR-030**: v2 MUST keep a local, retrievable record of recent cycle activity: what was
  checked, what was submitted, and the category of any failure. (Constitution Principle IV.)
- **FR-031**: Overlapping cycles MUST NOT cause double submissions or corrupt local
  backup-state; at most one cycle acts at a time.
- **FR-032**: All once-per-day logic MUST be evaluated in UTC.
- **FR-033**: v2 runs a single work cycle (backup pass + heartbeat) on one recurring
  **cycle interval**. That interval MUST be changeable without a code change, via local
  configuration on the device (consistent with how v1 was configured). There is no separate
  heartbeat vs backup-check cadence — both happen every cycle.
- **FR-034**: Local state that must survive service restarts, reboots, and v2 self-updates:
  the per-file last-successful-backup fingerprint and UTC day, and recent cycle records.

#### Direction of communication

- **FR-035**: v2 communication is device → backend only; v2 does NOT accept inbound
  connections or poll for commands. Remote "back up now" or remote reconfiguration, if wanted
  later, is a separate feature.

### Key Entities *(include if feature involves data)*

- **Heartbeat Telemetry Record**: a device status/version report submitted every interval —
  instrument serial (or unknown), machine identifier, v2 version, OS version/build/arch, client
  UTC timestamp, and any blocked-backup conditions.
- **Watched Configuration File**: one of the fixed set — logical name, canonical source
  location, current content fingerprint, last-successful-backup fingerprint, last-backed-up
  UTC day.
- **Configuration Backup Submission**: the whole contents of one watched file at a point in
  time, plus logical file identifier, source path, file modification time, content hash, and
  attribution (incl. product bucket). At most one per (file, UTC day).
- **Local Backup State**: on-device record of, per watched file, its last successful backup
  fingerprint and UTC day — basis for change detection and the daily limit. Survives restarts,
  reboots, and v2 self-updates.
- **Cycle Record**: per-cycle local log of what was checked, what was submitted, and
  categorized outcomes.
- **Product Identity**: the fixed product (`luminosa` / `solira`) an installation belongs to;
  selects the watched-file set, the product bucket on every submission, and which fleet token
  the build carries. Compiled in; no runtime switch.
- **Release Channel**: the fixed channel (`stable` / `beta`) a build belongs to; compiled in;
  reported in every heartbeat; determines which releases the machine self-updates from
  (`specs/001-v2-remote-upgrade`). No runtime switch.
- **Fleet Token**: a product's `X-TELEMETRY-TOKEN`, identical on every machine of that
  product, non-expiring, injected at build from a secret, rotatable via the accept-two-values
  transition.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of v2 machines with network access have a backend telemetry record no older
  than one cycle interval plus a small margin.
- **SC-002**: A maintainer can determine any reporting machine's current agent version, its
  installed instrument-software version (Luminosa / Solira), and last-seen time from backend
  queries within 1 business day, without contacting the customer.
- **SC-003**: For a watched file that changes on a given UTC day, exactly one backup of that
  file reaches the backend that day — not zero, not more.
- **SC-004**: For a watched file that does not change, zero backup submissions occur.
- **SC-005**: 100% of stored v2 submissions carry the correct product bucket, are attributable
  to a specific machine, and carry a client timestamp; ≥99% also carry a known instrument
  serial (the rest explicitly unknown).
- **SC-006**: Zero submission tokens, product API keys, or admin keys are present in the
  source repository.
- **SC-007**: A per-product token rotation completes with zero machines of that product unable
  to submit at any point during the transition.
- **SC-008**: A machine runs unattended, submitting on schedule, for at least 90 days without
  a human touching it (the non-expiring token makes this the expected steady state).
- **SC-008a**: Every Luminosa machine reports to the `luminosa` bucket and every Solira
  machine to the `solira` bucket — 0 cross-tagged submissions.
- **SC-008b**: Every heartbeat carries the build's channel and the per-cycle health signals
  of FR-002f (last `cycle.ok`, `blocked_backups`, last failure category); a maintainer can
  list the beta cohort and each machine's most recent status from telemetry alone. (Deriving
  "stopped" / "crash-loop" from heartbeat gaps for the promotion gate is `specs/001`.)
- **SC-009**: A transient backend outage of up to 24 hours results in no lost heartbeat state
  and no lost backup of a changed file once connectivity returns.
- **SC-010**: The service sustains continuous unattended operation across reboots and
  recoverable errors for at least 30 days without stopping.
- **SC-011**: Instrument operational log files uploaded by v2: 0.
- **SC-012**: A locked, oversized, or rejected watched file never blocks the backup of the
  other watched files and is visible centrally (in the heartbeat) within one interval.

## Assumptions

- Backend support for `luminosa` (fleet token, backup endpoint, fleet-token submission path)
  is **live and verified** (v2.2.0-beta.2). `solira` must still be added to the backend's
  `ALLOWED_PRODUCTS` with its own `TELEMETRY_FLEET_TOKENS_SOLIRA` before the Solira build
  ships (Open Items).
- The Luminosa watched set is fixed in FR-008 from the v1 source. The Solira set is assumed to
  mirror it with the path segment swapped, pending confirmation (FR-008).
- Machine identifier is obtained as in v1 (OS machine GUID). Instrument serial per FR-009a.
- The single cycle interval default is on the order of v1's cycle (minutes to hourly);
  planning sets it to 30 minutes (`plan.md` / `research.md` D11).
- v2 is delivered to existing machines through `specs/001-v2-remote-upgrade`. Because each
  product's token is fleet-wide and build-injected, that upgrade does not provision anything
  per-machine for v2.
- Windows only; no non-Windows run mode is built.
- The blast radius of a leaked fleet token is write-only access to that one product's
  telemetry/backup bucket (no read — needs the admin key; no mint — needs the product API
  key). Accepted, mitigated by rotation (FR-025).
- The backup endpoint's body size limit is at least large enough for a real-instrument
  `PQDevice.db` (or Solira equivalent); the oversize path (FR-014) covers exceptions.

## Dependencies

- The PicoQuant backend at `https://api.picoquant.com`:
  - `POST /api/v2/products/{product_key}/telemetry` for heartbeats, `product_key` ∈
    {`luminosa`, `solira`}.
  - A dedicated **backup endpoint** for configuration files (new — FR-017).
  - A **per-product fleet-wide non-expiring token**, serial-in-body, rotatable, usable without
    the admin key (new — FR-020, FR-021).
  - Admin/read endpoints for maintainer-side fleet visibility and backup retrieval.
- The v1→v2 unattended remote-upgrade path (`specs/001-v2-remote-upgrade`) — must additionally
  ensure each machine receives the correct **per-product** v2 build (FR-002c); this is a small
  addition to that spec's scope.
- Windows service hosting and a per-machine writable location for local backup state and cycle
  records that survives reboots and v2 self-updates.
- OS machine identifier and the per-product instrument serial source.

## Open Items

- **Confirm the Solira watched-file paths and serial-file location** against a real Solira
  install before the Solira build ships. Working assumption: identical to Luminosa with the
  path segment swapped (FR-008, FR-009a). Not blocking — isolated to the Solira per-product
  build.

## Out of Scope

- Instrument log file collection and upload (removed).
- Non-Windows platforms.
- Backend-side storage, retention policy, dashboards, and admin UI.
- The v1→v2 upgrade mechanism itself (separate spec).
- Inbound control of the agent: remote "back up now", remote reconfiguration, remote
  enable/disable.
- Any sub-daily / real-time streaming of configuration changes.
- Migrating historical v1 uploads out of Nextcloud.
- Per-machine credentials or credential provisioning.
