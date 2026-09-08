# Quickstart — validate the v1 → v2 unattended upgrade

Proves: a real current-v1 Luminosa machine moves itself to v2 with nobody logged in, keeps
uploading, and a failed upgrade rolls back to a working v1.

## Prerequisites

- A **Windows 10/11 x64 VM** (snapshots make the failure-injection runs repeatable).
- The **current released v1** installer (`Luminosa Log Uploader Setup.exe` from the latest
  `v0.x` GitHub release).
- A built **v2 Luminosa beta** installer + `.sha256` (`specs/002` → `PQ_PRODUCT=luminosa
  PQ_CHANNEL=beta` build → `installer/v2/luminosa.iss` with `/DPQ_CHANNEL=beta`).
- A GitHub prerelease you control to publish test v2 builds to (or point `update.ps1` at a
  fixture via an env override for local runs).
- `EXPECTED_ADMIN_API_KEY` in `.env` for `tools/fleet-status.py` (the same key also drives
  `tools/fleet_backup_pull.py`, the config-archive tool — `specs/004-fleet-backup-restore`).

## 1. Establish a real v1 machine

```
Luminosa Log Uploader Setup.exe /VERYSILENT
sc query LumiLogUploadService                 # -> RUNNING
schtasks /Query /TN "\PicoQuant\LuminosaLogUploader\AutoUpdate" /V /FO LIST   # -> exists, ONSTART
type "C:\Program Files\Luminosa Log Uploader\VERSION"                          # -> 0.11.x
```
Take a VM snapshot named `v1-clean`.

## 2. Publish v2-beta and let it migrate

Publish the v2 beta installer + `.sha256` as a GitHub **prerelease** `v2.0.0-beta.1`.
On the VM (no interactive login — use `schtasks /Run` to simulate the boot trigger):

```
schtasks /Run /TN "\PicoQuant\LuminosaLogUploader\AutoUpdate"
```

Wait, then assert:

```
type "C:\Program Files\Luminosa Log Uploader\VERSION"         # -> 2.0.0-beta.1
sc query LumiLogUploadService                                 # -> RUNNING (same service name)
& "C:\Program Files\Luminosa Log Uploader\loguploaderservice.exe" version   # product=luminosa channel=beta
type "C:\ProgramData\PicoQuant\LuminosaLogUploader\update\update.log"       # migration steps, health check PASS
dir "C:\ProgramData\PicoQuant\LuminosaLogUploader\rollback\0.11.12"         # bundle retained
```

Then from your workstation:

```
python tools/fleet-status.py --json | jq '.[] | select(.machine_id=="<id>")'
# state: "v2", channel: "beta", last_upgrade.outcome: "ok"
```

Expected: `no operator logged in` throughout; one `agent_status` **and** one `upgrade_attempt`
(`outcome: ok`) record at the backend.

## 3. Idempotency

`schtasks /Run …` again → `update.log` says "already current", nothing changes,
`VERSION` unchanged.

## 4. Failure injection (restore `v1-clean` before each)

| Inject | Expected end state |
|---|---|
| Corrupt the `.sha256` before the run | `integrity_failed` logged, installer not run, still v1, retries next trigger |
| Ship a v2 build whose `once` exits 1 (bad token) | install → `HealthCheck` fails → **auto-rollback to 0.11.12**, `LumiLogUploadService` RUNNING on v1, `upgrade_attempt outcome: health_check_failed` + a `rolled_back` record |
| Kill the VM power during `ssInstall` | on boot: self-heal restores the `pending` bundle → v1 RUNNING within one trigger |
| Fill the disk before the run | `Snapshot` fails cleanly → `install_failed`, still v1 |
| `schtasks /Run` twice concurrently | mutex — second no-ops; install not corrupted |

## 5. The v2→v2.x hop (mechanism migrated)

Publish `v2.0.0-beta.2`. `schtasks /Run …` (the task now also has a `/SC DAILY` trigger from
step 2's install). Assert it upgrades beta→beta through the **v2** `update.ps1`
(`is-newer` used, semver-with-prerelease compare) and health-checks. Proves FR-018 / SC-007.

## 6. Manual-intervention path

On a VM with the AutoUpdate task deleted (`schtasks /Delete …`), publish v2, confirm **no**
change and `fleet-status.py` shows `manual_required` (after adding the machine to
`docs/manual-intervention.json`). Then follow `docs/manual-upgrade-runbook.md` and confirm the
entry clears and the machine self-updates thereafter.

## Done when

- Steps 2, 3, 5 pass; every row of step 4 leaves a **running, working** uploader.
- `fleet-status.py` correctly classifies v1 / v2 / rolled_back / manual_required / stuck.
- No step required an interactive login.
