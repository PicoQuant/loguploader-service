# Contract — Retained Previous-Version Bundle (on-device)

Location: `C:\ProgramData\PicoQuant\LuminosaLogUploader\rollback\<from-version>\`
(under the dir the fielded updater already owns).

## Contents

```
rollback\0.11.12\
├── loguploaderservice.exe     # exact bytes of the previous binary
├── VERSION                    # "0.11.12"
├── update.ps1                 # the previous updater (so rollback also restores the mechanism)
├── settings.py                # previous config, IF it existed (absent file => absent here)
└── manifest.json
```

`manifest.json`:

```json
{
  "from": "0.11.12",
  "to": "2.0.0",
  "created_utc": "2026-09-08T10:00:00Z",
  "service_name": "LumiLogUploadService",
  "install_dir": "C:\\Program Files\\Luminosa Log Uploader",
  "outcome": "pending" | "ok" | "rolled_back" | "rollback_failed",
  "healthcheck_utc": "…" | null
}
```

## Invariants

- The bundle is **complete before** any file in the install dir is replaced (installer
  `Snapshot`, step `ssInstall`). If it cannot be written, the upgrade aborts with the old
  version untouched (FR-007, FR-010a).
- `outcome` transitions: `pending` → (`ok` | `rolled_back` | `rollback_failed`).
- `Rollback` restores every file present in the bundle to `install_dir` (and `update.ps1` to
  `install_dir\updater\`), re-points/re-registers `service_name` at the restored exe, starts
  it, verifies `RUNNING`.
- Retention: kept while `outcome = pending`; after `ok`, kept 30 days then deleted by the v2
  agent (bundles with `created_utc` older than 30 days); after `rolled_back` /
  `rollback_failed`, kept indefinitely as an attempt record (it is small — 4 files).
- Self-heal on boot: if `install_dir\VERSION` is a v2 version but the v2 service will not
  reach `RUNNING` and a bundle with `outcome = pending` exists, run `Rollback` from it.

## Disk headroom (Assumptions)

An upgrade needs room for: the downloaded installer + the new payload + this bundle,
concurrently. Machines without headroom fail `Snapshot` cleanly and stay on the previous
version (`install_failed`, telemetry sent).
