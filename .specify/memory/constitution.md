<!--
Sync Impact Report
==================
Version change: 1.4.0 → 1.4.1
Rationale: PATCH — factual correction only, no principle text changed. Principle VII's
rationale paragraph pointed at specs/002-v2-config-backup-telemetry/contracts/data-dictionary/
as "where this is implemented"; that dictionary was consolidated the same day to a repo-wide
docs/data-dictionary/ (also now covering specs/001-v2-remote-upgrade's upgrade-telemetry
document), making the pointer stale within hours of the v1.4.0 amendment. Updated the pointer
only.

Modified principles: VII (rationale pointer only; requirement text unchanged)
Added principles: none
Modified sections: none
Added sections: none
Removed sections: none

Templates / files requiring updates:
  ✅ .specify/memory/constitution.md (this file)
  ✅ docs/data-dictionary/ (the actual move; this amendment only follows it)

Deferred TODOs: none

Prior report (1.3.0 → 1.4.0)
-----------------------------
Version change: 1.3.0 → 1.4.0
Rationale: Add a new core principle requiring a semantic data dictionary for every feature
that produces output data (an API response, a wire submission, or a persisted file/document),
alongside its structural schema. A structural schema (JSON Schema or equivalent) says shape;
the dictionary says meaning — a field's meaning is not reliably derivable from its name or
type and gets rediscovered (or silently misread) by every future reader without it. Pattern
already implemented for v2's heartbeat/backup/local-state documents in
specs/002-v2-config-backup-telemetry/contracts/data-dictionary/ (semantic-model.json +
field-mappings.json + README.md), enforced by tools/check_data_dictionary.py, itself modeled
on the sibling pm100 app's docs/data-dictionary/. MINOR bump: new principle added, nothing
removed or redefined.

Modified principles: none (I–VI unchanged)

Added principles:
  VII. Semantic Output Schema

Modified sections:
  Development Workflow — added a review-gate bullet: a change to an external-facing
    document's fields MUST update its semantic data dictionary in the same change

Added sections: none
Removed sections: none

Templates / files requiring updates:
  ✅ .specify/memory/constitution.md (this file)
  ✅ specs/002-v2-config-backup-telemetry/{spec,plan,research,data-model,tasks}.md +
     contracts/data-dictionary/ + tools/check_data_dictionary.py — the concrete
     implementation this principle generalizes from (already done, precedes this amendment)
  ⚠ specs/001-v2-remote-upgrade, specs/003-backend-api-support, specs/004-fleet-backup-restore
    — each defines output documents (upgrade-telemetry payload, backend API responses, the
    fleet-backup archive's manifest/JSON files) not yet covered by a semantic dictionary;
    bringing them into compliance is future work, not required retroactively by this
    amendment alone (see Governance: amendments are not automatically retroactive without a
    separate compliance pass)

Deferred TODOs:
  - Decide whether existing specs 001/003/004 need their own data-dictionary follow-up work,
    and if so, track it as a task under each spec rather than in this constitution.

Prior report (1.2.0 → 1.3.0)
----------------------------
Version change: 1.2.0 → 1.3.0
Rationale: Add a staged-rollout mandate to the Build, Release & Distribution section: a v2
release reaches the full fleet only after a beta period on a limited cohort, with an explicit
promotion gate. Operationalizes Principle VI's "tested on a real Windows install" for a fleet
of unknown machines. MINOR bump: new mandate added to an existing section, no principle
removed or redefined.

Modified sections:
  Build, Release & Distribution — added: stable vs beta channels (channel compiled into the
    build; separate per-product-per-channel artifacts); beta releases are GitHub prereleases;
    a stable release MUST NOT be cut until the matching beta has run >= 7 days on >= 3 beta
    instruments with zero Sev-1 telemetry

Added principles: none
Removed sections: none

Templates / files requiring updates:
  ✅ .specify/memory/constitution.md (this file)
  ✅ specs/001-v2-remote-upgrade/spec.md — channel-aware updater + the promotion gate as FRs
  ✅ specs/002-v2-config-backup-telemetry/{spec,plan,tasks,data-model,research}.md +
     contracts/ — build carries a channel constant; heartbeat reports it; CI matrix is
     product x channel

Deferred TODOs:
  - RATIFICATION_DATE remains 2026-09-08.

Prior report (1.1.0 → 1.2.0)
----------------------------
Version change: 1.1.0 → 1.2.0
Rationale: Realign the delivery guidance with the v2 decision to build the agent as a
compiled single-binary Windows service (Rust) instead of a packaged Python interpreter.
Principle V is generalized from "PyInstaller EXE + Python" to "a single self-contained
executable, compiled-language preferred"; the Build section and Principle IV's success-marker
wording follow. MINOR bump: this is not a loosening — a Rust binary meets every requirement
Principle V imposes (self-contained, no runtime on target, minimal justified deps, YAGNI) and
arguably raises the bar; an implementation-specific prescription is replaced with a broader,
tool-agnostic standard. No principle removed or redefined incompatibly; v1's PyInstaller build
still complies.

Modified principles:
  V. Minimal Dependencies, Self-Contained Delivery — generalized to any compiled
     self-contained executable; native binary preferred over a packaged interpreter
  IV. Observable by Default — success/failure marker made tool-agnostic (v1 `Uploaded:`
     prefix; v2 structured `cycles.log` + backend heartbeat)

Modified sections:
  Build, Release & Distribution — build step is tool-agnostic (v1 PyInstaller / v2
     `cargo build` per product); per-product builds with compiled-in bucket + fleet token;
     telemetry is a recurring heartbeat, the once-per-UTC-day rule now governs config backups
  Development Workflow — submission-path testing is backend-agnostic (v1 Nextcloud /
     v2 `api.picoquant.com`)

Added principles: none
Removed sections: none

Templates / files requiring updates:
  ✅ .specify/memory/constitution.md (this file)
  ✅ specs/002-v2-config-backup-telemetry/plan.md — Constitution Check gate now passes;
     Complexity Tracking rows for Principle V / Build section are resolved by this amendment
  ⚠ .github/workflows/*.yml — still PyInstaller; rewritten for Rust during v2 implementation

Deferred TODOs:
  - RATIFICATION_DATE remains 2026-09-08. If the team considers an earlier adoption date
    authoritative, amend with a PATCH bump.

Prior report (v1.0.0 → 1.1.0)
-----------------------------
Version change: 1.0.0 → 1.1.0
Rationale: Added a new non-negotiable principle (VI) guaranteeing that any deployed
version can be replaced in place, remotely, by a later version. Triggered by the
decision to build a feature- and implementation-divergent v2 that must not strand
the installed v1 fleet. MINOR bump: new principle, no existing principle removed or
redefined.

Modified principles: none (I–V unchanged)

Added principles:
  VI. Remote Upgradeability Is Non-Negotiable

Added sections: none (Build, Release & Distribution expanded with a fleet-migration
bullet cross-referencing Principle VI)

Removed sections: none

Templates / files requiring updates:
  ✅ .specify/memory/constitution.md (this file)
  ⚠ README.MD — consider linking to this constitution
  ⚠ No CLAUDE.md present; principles here are the runtime guidance source
  ⚠ Upcoming v2 spec/plan MUST include an explicit v1→v2 unattended migration path

Deferred TODOs:
  - RATIFICATION_DATE remains 2026-09-08 (constitution first authored). If the team
    considers an earlier adoption date authoritative, amend with a PATCH bump.

Prior report (v1.0.0)
---------------------
Version change: (unversioned template) → 1.0.0
Rationale: Initial ratification of a concrete project constitution, replacing the
unfilled scaffold. MAJOR bump establishes the baseline governance document.
Modified principles:
  [PRINCIPLE_1_NAME] → I. Never Crash the Service Loop
  [PRINCIPLE_2_NAME] → II. Single Source of Truth for Version & Configuration
  [PRINCIPLE_3_NAME] → III. Non-Destructive Local File Handling
  [PRINCIPLE_4_NAME] → IV. Observable by Default
  [PRINCIPLE_5_NAME] → V. Minimal Dependencies, Self-Contained Delivery
Added sections: Build, Release & Distribution; Development Workflow; Governance.
Removed sections: none.
-->

# LogUploader Service Constitution

## Core Principles

### I. Never Crash the Service Loop

The Windows service main loop MUST NOT terminate because of a recoverable error.
Every upload cycle MUST be wrapped so that any exception is caught, logged, and the
loop continues to the next interval. Network calls, file operations, and external
process invocations MUST assume failure is normal: they MUST use bounded retries
with backoff and MUST degrade gracefully (e.g. fall back from `pyncclient` to the
`public.php/dav` endpoint) rather than abort the cycle.

Rationale: the service runs unattended on customer machines with no operator. A
single unhandled exception that stops the service means logs silently stop being
collected until someone notices, which can be months.

### II. Single Source of Truth for Version & Configuration

The `VERSION` file is the only authoritative version. Derived artifacts
(`version.iss`, `version_info.txt`) MUST be generated by `tools/gen_build_versions.py`
and MUST NOT be hand-edited. Releases MUST go through `tools/release.sh` so that
version bump, tag, and push stay consistent.

Secrets and deployment-specific configuration (the Nextcloud `public_link`) MUST
NOT be committed. `settings.py` stays gitignored; CI injects configuration via the
`PUBLIC_LINK` GitHub Actions secret. Code MUST read configuration through a single
resolution path (`settings` attribute → environment variable → documented default).

Rationale: version drift between the EXE, installer, and Release breaks the
auto-updater; a leaked share link exposes every customer's uploaded logs.

### III. Non-Destructive Local File Handling

A local source file MUST only be deleted after its upload is confirmed successful.
Files that appear open or locked MUST be skipped and retried on a later cycle, never
forced. Zip archives MUST be retained locally according to the configured retention
window (`keep_local_zip_days`) and cleaned up only by the age-based cleanup routine.
Oversized files MUST be skipped with a logged reason, not truncated or partially sent.

Rationale: these are diagnostic logs from scientific instruments. Losing a log to a
premature delete or a half-written upload destroys data that cannot be regenerated.

### IV. Observable by Default

Every cycle MUST record, to the Windows Event Log, the resolved working directory or
paths, the instrument serial number, the machine ID, and a per-item outcome for each
upload or backup attempt including attempt count and any fallback note. Outcomes MUST
be machine-detectable — a stable success/failure marker in a structured line (v1: the
`Uploaded:` prefix; v2: structured `cycles.log` records plus the per-interval backend
heartbeat). Failures MUST record the error type and message. Detailed per-item
records MAY live in a size-capped local file that a remote maintainer can retrieve,
so long as a cycle summary still reaches the Event Log.

Rationale: the Event Log is the diagnostic channel that is always present in the
field. Structured, greppable output — locally and, for v2, queryable on the backend —
is what makes remote troubleshooting possible without a site visit.

### V. Minimal Dependencies, Self-Contained Delivery

The service ships as a **single self-contained executable** that runs on a stock
Windows machine with no separate runtime, interpreter, or framework to install, and
no dependency on libraries beyond the operating system's own. A compiled-language
build (the v2 agent is Rust) is preferred over a packaged-interpreter bundle. Every
new third-party dependency — library, build tool, or language runtime — MUST be
justified in the plan or the PR against using the standard library or a dependency
already present. The codebase stays small and direct; speculative abstraction and
features not required by a current need (YAGNI) MUST be rejected in review.

Rationale: every dependency is another thing that can break the build or introduce a
CVE onto a customer machine that is rarely patched. A native single binary carries
less of that risk than a frozen interpreter — no multi-megabyte unpack at start, no
bundled runtime version to drift, a smaller attack surface, and it is harder to
tamper with in the field.

### VI. Remote Upgradeability Is Non-Negotiable

Any deployed version MUST be replaceable in place by a later version without
physical or interactive access to the customer machine. The mechanism that performs
that replacement — currently the `\PicoQuant\LuminosaLogUploader\AutoUpdate`
scheduled task running `updater/update.ps1` against the latest GitHub Release — is a
binding contract with every machine already in the field.

- A new major version MUST NOT be published until an unattended upgrade path from
  every currently-deployed version to it has been tested on a real Windows install.
- The migration surface — service name, install location, scheduled-task name and
  trigger, updater script location, and Release asset naming — MUST NOT change in a
  way the already-deployed updater cannot follow. If it must change, the release
  that changes it MUST first be installable by the OLD updater and MUST itself
  rewrite the updater and task to the new contract.
- The upgrade MUST be idempotent and MUST roll forward safely if interrupted by
  power loss or network failure: a partially applied upgrade MUST leave a working
  service running some valid version.
- A change that is otherwise an improvement but that strands any fielded machine is
  rejected. Losing the ability to remote-upgrade a fielded machine is a Sev-1
  regression.

Rationale: machines sit at customer sites with no operator and usually no inbound
network access. If we cannot push a fix remotely, we cannot fix anything at all. A
v2 that is better in every other way but cannot replace the installed v1 has failed
its single most important requirement.

### VII. Semantic Output Schema

Every feature that produces output data — an API response, a wire submission, or a
persisted file/document — MUST ship a semantic data dictionary alongside its
structural schema. The dictionary MUST define, for every field: a dot-namespaced
semantic concept id independent of the field's own name, its datatype, unit where
applicable, a description of what it *means* (not what shape it has), known
aliases, its parent concept where nested, at least one real example, and a
confidence marker (`confirmed` / `uncertain`) — `uncertain` unless checked against
a live contract or working code rather than inferred from a name or type alone.
The dictionary MUST also map every field of every covered document to a concept id,
so a maintainer can resolve any field back to its meaning without re-deriving it
from source. A structural schema (JSON Schema or equivalent) states *shape*; this
dictionary states *meaning*; neither substitutes for the other, and a schema or
data-model change that adds, renames, or reinterprets a field without a matching
dictionary update is an incomplete change, not a follow-up.

Rationale: a field's meaning — "this is a gate key, not a timestamp"; "these two
same-named fields across two documents are not the same concept" — is not
reliably derivable from its name or type, and gets silently rediscovered, or
silently misread, by every future reader without it being written down once,
deliberately, in one place. Implemented as a repo-wide dictionary at
`docs/data-dictionary/` (`semantic-model.json` + `field-mappings.json` + `README.md`),
covering v2's heartbeat, backup submission, and local state (spec 002) plus the
v1→v2 updater's upgrade-attempt telemetry (spec 001) — one shared dictionary, not
one per feature, so a concept is defined once and reused. Enforced by
`tools/check_data_dictionary.py`.

## Build, Release & Distribution

- Windows builds are produced by `.github/workflows/windows-build.yml`; releases by
  `.github/workflows/release.yml`, triggered by pushing a `v*` tag. The build step
  compiles the service (v1: PyInstaller; v2: `cargo build`) — the workflow files and
  the artifacts they produce are the contract, not the build tool.
- Every release MUST ship a silent-capable **installer**, a `.sha256` checksum, and
  the service executable, under asset names the deployed updater can consume
  (Principle VI). Distribution is always via the installer, never a bare executable.
- The installer MUST register the `\PicoQuant\LuminosaLogUploader\AutoUpdate`
  scheduled task and install `updater/update.ps1`. Changes to the updater or task
  registration MUST be verified on a real Windows install before release.
- v2 produces one build and one installer **per product** (`luminosa`, `solira`),
  each with that product's bucket and fleet submission token compiled in from a CI
  secret (never committed). A machine MUST NOT be able to run or be upgraded to a
  build for the wrong product.
- Device/version telemetry MUST be sent on a recurring heartbeat (v1: the once-daily
  `client_version.json`; v2: a per-interval `agent_status` submission). Any
  once-per-UTC-day idempotency requirement applies to the v2 configuration-file
  backups: a given file uploads at most once per UTC day, and only when it changed.
- **Staged rollout (v2).** A release reaches the full fleet only after a beta
  period. v2 builds carry their channel — `stable` or `beta` — compiled in; CI
  produces a separate artifact per product per channel. Beta releases are published
  as GitHub prereleases; a `stable` build self-updates only from stable releases, a
  `beta` build only from prereleases. A stable `vX.Y.Z` MUST NOT be cut until the
  matching beta build has run at least 7 days on at least 3 beta instruments with
  zero Sev-1 telemetry (failed upgrade, crash-loop, or stopped service). The beta
  cohort is seeded by installing beta builds by hand; the existing v1 fleet only
  ever auto-updates to v2 from a stable release.
- Backward compatibility of the upload path and share-token handling MUST be
  preserved unless a MAJOR constitution amendment and a migration note accompany
  the change.
- The v1→v2 transition is governed by Principle VI. Until telemetry shows the
  installed base has moved off v1, every v2 release MUST remain reachable by the
  v1 updater (compatible asset names and installer behavior), or ship as a v1-
  compatible bridge release that upgrades the updater and scheduled task first.
  The v2 spec and plan MUST document this migration path before implementation.

## Development Workflow

- Changes MUST be made on a branch and merged via pull request; the default branch
  is `main`.
- The reviewer MUST confirm the change respects every principle above, or that a
  deviation is explicitly justified in the PR description.
- Changes to the upload/submission path MUST be exercised against the real backend
  before merge (v1: a Nextcloud file-drop share, `test_public_webdav_put.py` /
  `testnextcloud_upload.py`; v2: `api.picoquant.com` with a test fleet token, see
  `specs/002-v2-config-backup-telemetry/quickstart.md`).
- Release commits follow the existing convention (`chore(release): X.Y.Z`) and are
  produced by the release helper, not by hand.
- User-facing behavior changes MUST be reflected in `README.MD`.
- A change that adds, renames, or reinterprets a field of an external-facing
  document (an API response, a wire submission, or a persisted file) MUST update
  that document's semantic data dictionary in the same change (Principle VII), and
  MUST pass its coverage check (e.g. `tools/check_data_dictionary.py`) before merge.

## Governance

This constitution supersedes other process conventions for this repository. When a
practice and this document conflict, this document wins until amended.

Amendments MUST be made by editing this file, updating the version and dates below,
and prepending a Sync Impact Report describing the change. Versioning follows
semantic versioning:

- MAJOR: a principle is removed or redefined in a backward-incompatible way, or a
  governance rule is materially loosened.
- MINOR: a new principle or section is added, or existing guidance is materially
  expanded.
- PATCH: clarifications, wording, and non-semantic refinements.

Every pull request MUST be checked for compliance with the principles here.
Unavoidable complexity or a principle deviation MUST be called out and justified in
the PR; unjustified violations block merge. This file is the runtime development
guidance source for the project.

**Version**: 1.4.1 | **Ratified**: 2026-09-08 | **Last Amended**: 2026-09-12
