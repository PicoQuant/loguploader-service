# loguploader

> **v2 (Rust agent) is documented at the bottom of this file:**
> [V2 — device telemetry & config backup](#v2--device-telemetry--config-backup).
> Everything above the v2 heading describes the legacy v1 (Python/Nextcloud) service,
> which stays in the tree until the v1→v2 fleet migration (`specs/001-v2-remote-upgrade`)
> completes.

```
git clone https://github.com/PicoQuant/loguploader-service.git
cd loguploader-service
pip install -r requirements.txt
```
then create a file `settings.py` with the public link for the nextcloud dropbox folder:
```
public_link = "https://nc.example.com/index.php/s/<share-token>"
```

You can start from `settings.py.example`.

`settings.py` is intentionally gitignored. For CI/builds, the public link can be embedded into the installer/EXE by providing a GitHub Actions secret `PUBLIC_LINK`.

## Versioning

The version is stored in the `VERSION` file (single source of truth).

Before building the Windows EXE / installer, generate the derived version files:

```
python tools/gen_build_versions.py
```

Do not edit `version.iss` / `version_info.txt` manually.

## CI builds and GitHub Releases

- The workflow `.github/workflows/windows-build.yml` produces build artifacts for each run.
- The workflow `.github/workflows/release.yml` creates a GitHub Release when you push a tag `v*` and uploads:
  - the installer (`Output/*.exe`)
  - a checksum file (`Output/*.sha256`)
  - the service EXE (`dist/loguploaderservice.exe`)

To build a fully self-contained installer without requiring runtime configuration on the target machine:

- Add a GitHub Actions secret named `PUBLIC_LINK` (the Nextcloud share link)
- CI will generate a `settings.py` during the build and bundle it into the EXE.

## Release helper

To publish a new release (commit + tag + push) you can use:

```
chmod +x tools/release.sh
tools/release.sh 0.11.10
```

This updates `VERSION`, regenerates derived version files, commits, pushes `main`, creates tag `v<version>`, and pushes the tag.

## Nextcloud upload

The service primarily uses the `pyncclient` (`nextcloud_client`) library. If the Nextcloud server rejects uploads via the legacy public share endpoint after an update, the uploader falls back to using:

`public.php/dav/files/<share-token>/<filename>`

This matches the currently working upload path for file-drop shares on our instance.


## for building the service:

To create an executable which can be run without installing Python first you need [pyinstaller](https://pyinstaller.readthedocs.io/en/stable/index.html) which can be installed like so:

```
pip install pyinstaller
```

see here: https://metallapan.se/post/windows-service-pywin32-pyinstaller/
```
python -m PyInstaller --onefile --hidden-import win32timezone --version-file version_info.txt .\loguploaderservice.py
```

This should create `dist\loguploader.exe` which can be deployed directly without the need to install anything.

## Installing the service

Run the following as Administrator:

```
loguploaderservice.exe install
loguploaderservice.exe start
```

To stop/remove:

```
loguploaderservice.exe stop
loguploaderservice.exe remove
```

For console debugging:

```
loguploaderservice.exe debug
```

## Auto-update (Windows)

The installer installs an updater script to:

- `{app}\updater\update.ps1`

and creates a Scheduled Task that runs at boot (with a delay) and updates from the latest GitHub Release:

- Task name: `\PicoQuant\LuminosaLogUploader\AutoUpdate`
- Trigger: at startup
- Action: runs `powershell.exe ... updater\update.ps1`

Updater logs:

- `C:\ProgramData\PicoQuant\LuminosaLogUploader\update\update.log`

Installer task creation logs:

- `C:\ProgramData\PicoQuant\LuminosaLogUploader\update\installer_task.log`

Troubleshooting:

- If the task is missing, run (as Administrator):

```
schtasks /Query /TN "\PicoQuant\LuminosaLogUploader\AutoUpdate" /V /FO LIST
```

- If task creation fails, check `installer_task.log` for the `schtasks` command and exit code.

## Logs

Service logs are written to the Windows Event Log:

Event Viewer -> Windows Logs -> Application

---

# V2 — device telemetry & config backup

v2 (`pquploader`) is a single self-contained **Rust** Windows service. It replaces v1's
Nextcloud log upload with two things on a fixed cycle (default 30 min):

1. a **telemetry heartbeat** (`POST /api/v2/products/{product}/telemetry`) — machine id,
   instrument serial, agent version, release channel, OS — so the fleet is visible centrally;
2. a **config backup** (`POST /api/v2/products/{product}/backup`) of a per-product set of
   device configuration files, uploaded only when a file's content changed and at most once
   per UTC day.

It never uploads instrument logs, never accepts inbound connections, and never crashes its
loop. Spec: `specs/002-v2-config-backup-telemetry/`.

## Per-product build

There is one binary per **product** (`luminosa`, `solira`) × **channel** (`stable`, `beta`).
The product bucket, channel, and fleet token are compiled in from `build.rs`:

```
# reads PQ_PRODUCT + PQ_CHANNEL + the matching TELEMETRY_FLEET_TOKENS_<PRODUCT> from .env
PQ_PRODUCT=luminosa PQ_CHANNEL=stable cargo build --release
#  -> target/x86_64-pc-windows-msvc/release/pquploader.exe
#     CI renames it to pquploader-luminosa.exe  (see contracts/cli.md for all 4 names)
```

A **release** build fails if the product/channel is invalid or the token is empty. A debug
build tolerates a missing token (it just logs `authentication` failures).

- `VERSION` is the single source of truth; `build.rs` stamps the binary + Windows version
  resource. Do not hand-edit derived files.
- CI: `.github/workflows/windows-build.yml` builds the full product × channel matrix and runs
  a "no fleet token in source" gate.

## Runtime config

Optional `config.toml` **next to the exe** (see `config.toml.example`). Resolution is one
path: compiled default → `config.toml` → hard default. Keys: `cycle_interval_secs`,
`api_base_url`, `backup_max_bytes`, `http_timeout_secs`. Watched-file paths and the channel
are **not** configurable.

## Install / operate

```
pquploader-luminosa.exe install        # registers service PQUploaderLuminosa (auto-delayed,
                                        # LocalSystem) + the Event Log source
sc start PQUploaderLuminosa
pquploader-luminosa.exe once            # run one cycle, print the CycleRecord JSON, exit
pquploader-luminosa.exe debug           # run the loop in the foreground, Ctrl-C to stop
pquploader-luminosa.exe version --json  # product / channel / version / backend / token present?
pquploader-luminosa.exe uninstall [--purge]   # --purge also removes the v2agent state dir
```

The per-`(product,channel)` Inno Setup scripts are in `installer/v2/` and keep the
`…Setup.exe` asset name the v1 updater's glob matches (Constitution VI).

## Where it keeps things

`C:\ProgramData\PicoQuant\<Product>\v2agent\`:

| File | Purpose |
|---|---|
| `state.json` | per-file last-successful-backup sha256 + UTC day, last heartbeat, last cycle summary. Atomic write; survives reboot and v2 self-update (schema in `contracts/local-state.schema.json`). |
| `cycles.log` | rolling (~5 × 1 MiB) per-cycle `CycleRecord` lines for remote support. |

Cycle summaries also go to the Windows Event Log, source
`PicoQuant <Product> LogUploader` (Event Viewer → Windows Logs → Application).

## Fleet token rotation (FR-025, SC-007)

The `X-TELEMETRY-TOKEN` is fleet-wide per product, non-expiring, and **write-only** to that
one product's bucket. It is extractable from a distributed binary — treat it as
keep-out-of-source, not confidential. To rotate with **no submission gap**:

1. Add the new value to the backend's `TELEMETRY_FLEET_TOKENS_<PRODUCT>` (comma-separated —
   the backend now accepts **both** old and new).
2. Set the GitHub Actions secret `TELEMETRY_FLEET_TOKENS_<PRODUCT>` to `new,old` (the build
   compiles in the **first** entry → new).
3. Ship a v2 release; let that product's fleet auto-update (watch the heartbeat `agent_version`
   climb in the backend fleet view).
4. Once every machine reports the new build, **remove the old value** from the backend.

No machine is unable to submit at any point. See also
`specs/002-v2-config-backup-telemetry/quickstart.md`.

## Tests

```
cargo test        # unit + integration (mock backend, no real network)
```

End-to-end validation against the real backend is `specs/002-v2-config-backup-telemetry/quickstart.md`.

## Fleet backup archive (maintenance)

`tools/fleet_backup_pull.py` turns the backend's *transient* config-backup store into a
**permanent local archive**. It downloads every backup `api.picoquant.com` currently exposes
that the archive does not already hold, verifies it, files it, and never deletes captured
history — so the archive stays complete after the backend prunes. Spec:
`specs/004-fleet-backup-restore/`. Standard library only; run it from cron (the schedule is
your job, not the tool's).

```
python tools/fleet_backup_pull.py                              # whole fleet, all products, into ./fleet-backups
python tools/fleet_backup_pull.py --out backups/2026-09-08     # into a dated root
python tools/fleet_backup_pull.py --product luminosa --serial SN-12345   # narrow
python tools/fleet_backup_pull.py --since 2026-09-01           # only recent artifacts
```

Auth: `EXPECTED_ADMIN_API_KEY` from the environment or `--env` (`./.env`); it is never written
to the archive, its manifests, or any output.

Layout — one folder per physical machine:

```
<out>/<product>/<serial>/<machine-id>/
    manifest.json                                     # what this folder holds + the cursor
    ProgramData/PicoQuant/Luminosa/LastKnownGood.xml  # newest version, mirrored from source_path
    _versions/…/LastKnownGood.xml/2026-09-08T12-52-52Z__b72cde42.bak   # every version
```

Re-runs are incremental (an artifact already in `manifest.json` is skipped, no hashing) and
append-only — **no prune / retention in this increment**. Overlapping runs: the second logs
"another run in progress" and exits 0. If a run reports `skipped=N` with a `#` line about an
"old archive layout", delete that archive root (an earlier version used a different layout)
and re-run into a fresh one.

**Exit codes**: `0` success / nothing new / another run already in progress · `1` partial
(≥1 artifact failed) · `2` fatal (no admin key, no product reachable, archive not writable).

Tests: `python -m unittest discover -s tools -p 'test_*.py'` (no network).
