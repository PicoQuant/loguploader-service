# loguploader



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
