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

## Versioning

The version is stored in the `VERSION` file (single source of truth).

Before building the Windows EXE / installer, generate the derived version files:

```
python tools/gen_build_versions.py
```

## Nextcloud upload

The service primarily uses the `pyncclient` (`nextcloud_client`) library. If the Nextcloud server rejects uploads via the legacy public share endpoint after an update, the uploader falls back to using:

`public.php/dav/files/<share-token>/<filename>`

This matches the currently working upload path for file-drop shares on our instance.


To create an executable which can be run without installing Python first you need [pyinstaller](https://pyinstaller.readthedocs.io/en/stable/index.html) which can be installed like so:

```
pip install pyinstaller
```

pyinstaller will probably not be found by Windows therefore run it using: 

```
python -m pyinstaller --onefile loguploader.py
```
 
 This should create `dist\loguploader.exe` which can be deployed directly without the need to install anything.

## for building sthe service:

see here: https://metallapan.se/post/windows-service-pywin32-pyinstaller/

python -m PyInstaller --onefile --hidden-import win32timezone --version-file version_info.txt .\loguploaderservice.py

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

## Logs

Service logs are written to the Windows Event Log:

Event Viewer -> Windows Logs -> Application
