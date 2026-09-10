# Contract — `updater/update.ps1` (rewritten, channel-aware)

> **Status 2026-09-10**: the channel-aware selection + verify + apply path (steps 1–8, 10) is
> implemented and Pester-tested (`tests/updater/`). **Not yet built**: step 6 Authenticode
> (no cert — C2), `SelfHealOnBoot`, and the installer-side snapshot/health/rollback that
> step 9 assumes — step 9 currently only restarts the exe already on disk. See
> `tasks.md` → "Amendment 2026-09-10".

Runs as SYSTEM from the scheduled task `\PicoQuant\LuminosaLogUploader\AutoUpdate`
(`/SC ONSTART /DELAY 0000:30`, plus `/SC DAILY` once the machine is on v2 — research D10).
Replaces the v1 `update.ps1` at the same path so the task's `/TR` never changes.

## Inputs (discovered, not arguments)

| Input | Source |
|---|---|
| install root | the script's own parent-of-parent dir (`Split-Path $PSScriptRoot`) |
| current version | `<InstallRoot>\VERSION` |
| product + channel | `& <InstallRoot>\<exe> version --json` → `{ version, product, channel }` — for a v1 machine the exe has no `version --json`; the script falls back to `product=luminosa`, `channel=stable` |
| exe name | `loguploaderservice.exe` (Luminosa) or `pquploader-<product>[-beta].exe` — probe both |
| repo | `PicoQuant/loguploader-service` (constant) |

## Behaviour

1. Resolve product + channel.
2. Query GitHub:
   - `channel = stable` → `GET /repos/…/releases/latest` (excludes prereleases — unchanged from v1).
   - `channel = beta` → `GET /repos/…/releases?per_page=20`, pick the highest semver **including** prereleases.
3. From the chosen release, pick the asset matching **this machine's product**:
   `^<Product> Log Uploader( Beta)? Setup\.exe$` (v1 machines: the migration release is
   Luminosa-only, so the first `*Setup*.exe` is correct — research D12). Pick its `.sha256`.
4. **Decide whether to act**: `& <exe> is-newer <remoteVersion>` → act iff exit 0. On a v1
   machine (no `is-newer`), fall back to `[Version]` compare, then string compare (v1 logic).
5. Download installer + `.sha256` to `%ProgramData%\PicoQuant\LuminosaLogUploader\update\`.
6. **Verify**: SHA-256 must match. If a code-signing cert is in use (C2):
   `Get-AuthenticodeSignature` status `Valid` **and** thumbprint ∈ pinned set. Fail →
   record `integrity_failed`, do not run, exit 0.
7. `& <InstallRoot>\<current-exe> stop` (best effort).
8. `Start-Process <installer> -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-' -Wait`.
   The installer performs snapshot → install → health check → rollback-on-fail → report
   (`contracts/installer-cli.md`) and **exits 0 in every case it reached a working state**.
9. If the installer exit code is non-zero (should not happen): `& <current-exe> start` to
   restore the prior service, log, exit 1 (the fielded v1 updater would `throw` here — the v2
   updater catches and restores).
10. Otherwise `& <InstallRoot>\<exe> start` as a backstop (idempotent) and exit 0.

## Outputs

- **Exit code**: `0` = handled (up-to-date, applied, or applied-then-self-rolled-back);
  `1` = could not reach a working state (last-resort; also emits an Event Log error).
- **Log**: `%ProgramData%\PicoQuant\LuminosaLogUploader\update\update.log` (append, timestamped)
  — same path as v1.
- **Event Log**: one Information entry per run summary, Warning/Error on any failure
  (Constitution IV).
- The `upgrade_attempt` telemetry POST is done by the **installer**, not the updater (the
  updater has no token).

## Non-goals

- The updater never handles the fleet token, never calls admin endpoints, never writes config.
- The updater does not roll back — that is the installer's job (it has the snapshot). The
  updater's step 9 is only "restart the exe that is still on disk".
