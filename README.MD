# loguploader



```
git pull https://github.com/snizzleorg/loguploader-service.git
cd lloguploader-service
pip install -r requirements.txt
```
then create a file `settings.py` with the public link for the nextcloud dropbox folder:
```
public_link=https://<nexcloud-address>
```


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

python -m PyInstaller --onefile --hidden-import win32timezone .\loguploaderservice.py

## Installing the service
