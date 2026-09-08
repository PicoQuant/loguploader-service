# Phase 1 Data Model — V1 → V2 Unattended Remote Upgrade

Entities are on-device files, GitHub release assets, and one telemetry record type. No
database.

## Version Marker

The authoritative on-disk record of the installed version (Constitution II).

| Field | Location | Notes |
|---|---|---|
| version string | `C:\Program Files\Luminosa Log Uploader\VERSION` | plain text, e.g. `0.11.12` / `2.0.0` / `2.1.0-beta.3` |

- Written **atomically with** the binary swap (temp file + rename inside the installer).
- The rewritten `update.ps1` reads it and asks the installed exe (`is-newer <remote>`) whether
  to act — the exe owns semver-with-prerelease comparison (research D8).
- Downgrade prevention (FR-011): `is-newer` returns "act" only when remote is strictly newer.
  An automatic rollback restores the previous `VERSION` and is not a downgrade.

## Release / Upgrade Package (GitHub release + assets)

| Attribute | Value |
|---|---|
| tag | `v2.0.0` (stable) / `v2.0.0-beta.3` (`prerelease: true`) |
| installer asset | `<Product> Log Uploader[ Beta] Setup.exe` (keeps the `…Setup.exe` token the fielded updater globs) |
| integrity asset | `<installer name>.sha256` |
| (optional) signature | Authenticode on the installer + exe (research D9 / C2) |
| service exe asset | `loguploaderservice.exe` (Luminosa) / `pquploader-solira[-beta].exe` |

Composition rules — research D12:

| Tag | prerelease? | assets |
|---|---|---|
| `v2.0.0` | no | **Luminosa only** (the migration release) |
| `v2.0.z`, `v2.y.*` | no | Luminosa + Solira |
| `v*-beta.N` | yes | the `-beta` artifacts for products in beta |

## Retained Previous-Version Bundle

`C:\ProgramData\PicoQuant\LuminosaLogUploader\rollback\<from-version>\`

| File | Purpose |
|---|---|
| `loguploaderservice.exe` | the previous binary |
| `VERSION` | previous version string |
| `update.ps1` | previous updater |
| `settings.py` | previous config (if it existed) |
| `manifest.json` | `{ from, to, created_utc, service_name, install_dir, outcome }` |

Lifecycle:

```
upgrade start ──▶ Snapshot writes bundle
health check PASS ──▶ manifest.outcome = "ok"; keep 30 days; v2 agent deletes when created_utc > 30d
health check FAIL ──▶ Rollback consumes bundle (restores files); manifest.outcome = "rolled_back"; manifest kept as attempt record
```

FR-010a: the bundle MUST exist and be complete before the binary is replaced; retained
artifacts MAY be removed only after the new version is confirmed healthy.

## Upgrade Attempt Record

Two representations of the same event.

**Local** — `C:\ProgramData\PicoQuant\LuminosaLogUploader\upgrade\attempts.log`
(append-only, size-capped): one JSON line per attempt —
`{ attempt_utc, from_version, to_version, channel, phase, outcome, cause, health_ms }`.
`phase ∈ { download, verify, install, health_check, rollback, done }`.

**Uploaded** — `measurement_type: "upgrade_attempt"` telemetry (contract:
`contracts/upgrade-telemetry.schema.json`), one POST per terminal outcome. Distinguishes, in
`Fleet Status View`, a machine that **tried and rolled back** from one that **never tried**
(FR-020) and one that **succeeded**.

`outcome` enum: `ok` · `integrity_failed` · `install_failed` · `health_check_failed` ·
`rolled_back` · `rollback_failed` (the last is the only true Sev-1).

## Fleet Status View (derived, maintainer-side)

Produced by `tools/fleet-status.py` from uploaded telemetry + the manual-intervention list.
Not stored. Per machine:

> Companion tool: `tools/fleet_backup_pull.py` (`specs/004-fleet-backup-restore`) — same admin
> key, queries the backup list instead of telemetry, and archives each machine's config
> locally. When this view shows `stuck` / `manual_required`, archive that machine first
> (`--serial <SN>`).

| Column | Source |
|---|---|
| `machine_id`, `instrument_serial` | any telemetry record |
| `current_version`, `channel` | newest `agent_status` (v2) or `client_version` (v1) |
| `last_seen` | newest record's `received_at` |
| `last_upgrade` | newest `upgrade_attempt` `outcome` + `attempt_utc` |
| `state` | `v1` \| `v2` \| `migrating` \| `rolled_back` \| `manual_required` \| `stuck` |

`state` rules: `manual_required` = on the list; `rolled_back` = last `upgrade_attempt` is a
failure and `current_version` is v1; `stuck` = `current_version` is v1, a stable v2 exists,
> N days since publication, and no `upgrade_attempt` seen; `migrating` = an `upgrade_attempt`
with a non-terminal `phase` seen more recently than any `agent_status`.

## Manual-Intervention List

`docs/manual-intervention.md` (human-readable) backed by a small `manual-intervention.json`
the tool reads: `[{ machine_id | instrument_serial, reason, added_utc, cleared_utc? }]`.
An entry is added when a machine is known to lack a working updater and cleared by the runbook
(FR-023a, FR-023b).

## Device Configuration (v1 and v2 forms)

| Form | Location | Migration handling |
|---|---|---|
| v1 | `C:\Program Files\Luminosa Log Uploader\settings.py` | preserved unmodified until health check passes; in the rollback bundle (FR-014c) |
| v2 | `C:\Program Files\Luminosa Log Uploader\config.toml` | written by `SeedConfig`: v2 defaults + `service_interval_seconds → cycle_interval_secs` if present; idempotent (FR-014a); untranslated v1 keys noted in the `upgrade_attempt` payload (FR-014b) |

Nothing device-specific is required for v2 to run (token + endpoint are compiled in) — see
research D13.
