import nextcloud_client
import glob
import os
import zipfile
from os.path import basename
from argparse import ArgumentParser
import settings
import winpath
import sys
import subprocess
import shutil
import datetime
import winreg
import time


def has_file_changed(file_path):

    try:
        # Get the modification time of the file
        current_modification_time = os.path.getmtime(file_path)

        # Generate the last check file name based on the XML file
        last_check_file = os.path.splitext(file_path)[0] + ".lastcheck"

        try:
            # Read the last check time from the file
            with open(last_check_file, "r") as file:
                last_check_time = float(file.read())
        except (FileNotFoundError, ValueError):
            # Return True if the file doesn't exist or if there's an issue reading the time
            # Update the last check time for the next iteration
            with open(last_check_file, "w") as file:
                file.write(str(time.time()))
            return True

        # Compare with the last check time
        if current_modification_time > last_check_time:
            # Update the last check time for the next iteration
            with open(last_check_file, "w") as file:
                file.write(str(time.time()))
            return True
        else:
            return False
    except FileNotFoundError:
        print(f"File not found: {file_path}")
        return False


# --- Upload helpers: retry/backoff and size checks ---
def _get_setting(name, default):
    """Read an optional attribute from settings with a default.
    Falls back to environment variables in ALL_CAPS as strings if present.
    """
    env_name = name.upper()
    if hasattr(settings, name):
        return getattr(settings, name)
    if env_name in os.environ:
        val = os.environ[env_name]
        # Try to cast numerics where appropriate
        try:
            if isinstance(default, int):
                return int(val)
            if isinstance(default, float):
                return float(val)
        except ValueError:
            pass
        return val
    return default


MAX_UPLOAD_SIZE_MB = _get_setting("max_upload_size_mb", 200)  # conservative default
MAX_UPLOAD_ATTEMPTS = _get_setting("max_upload_attempts", 3)
UPLOAD_BACKOFF_SECONDS = _get_setting("upload_backoff_seconds", 2)


def _too_large(path):
    try:
        size_mb = os.path.getsize(path) / (1024 * 1024)
        return size_mb > MAX_UPLOAD_SIZE_MB, size_mb
    except Exception:
        return False, 0.0


def _is_http_409(exc: Exception) -> bool:
    """Best-effort detection of HTTP 409 Conflict from nextcloud_client.

    pyncclient exceptions vary by version; sometimes the status code is only
    present in the string representation.
    """
    for attr in ("status_code", "code"):
        try:
            if getattr(exc, attr) == 409:
                return True
        except Exception:
            pass
    msg = str(exc)
    return " 409" in msg or "HTTP 409" in msg or "409 Client Error" in msg


def _drop_with_retries(nc, path):
    """Try nc.drop_file(path) with retries and backoff. Returns (ok, attempts, last_error)."""
    attempts = 0
    last_error = None
    while attempts < MAX_UPLOAD_ATTEMPTS:
        attempts += 1
        try:
            if nc.drop_file(path):
                return True, attempts, None
        except Exception as e:
            if _is_http_409(e):
                # Nextcloud drop folder: 409 usually means the target name already exists.
                # Treat as success to avoid retrying the same upload forever.
                return True, attempts, "HTTP 409 (already exists)"
            last_error = f"{type(e).__name__}: {e}"
        if attempts < MAX_UPLOAD_ATTEMPTS:
            time.sleep(UPLOAD_BACKOFF_SECONDS)
    return False, attempts, last_error


def getLumiSerial(basepath):
    filename = os.path.join(basepath, "Logs", "LastOpenSerial.txt")
    try:
        fp = open(filename, "r")
        serial = fp.read()
        serial = serial.split()[-1:][0]
        print(f"Serial {serial} read from {filename}")
        return serial
    except:
        return "0000000"


def uploadlog(
    basepath="",
    serialnumber="0000000",
    current_machine_id="00000000-0000-0000-0000-000000000000",
):
    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))
    basepath = os.path.join(basepath, "Logs")

    returntxt = f"LogDir: {basepath}\n"
    nc = nextcloud_client.Client.from_public_link(settings.public_link)
    if nc:
        filepattern = os.path.join(basepath, "*.pqlog")
        logfiles = glob.glob(filepattern)
        logfiles.sort()
        for logfilename in logfiles:
            # try to see if the file is open
            try:
                os.rename(logfilename, logfilename)
            except OSError:
                returntxt = returntxt + f"File is open, skipping: {logfilename}\n"
                continue
            pre, ext = os.path.splitext(os.path.basename(logfilename))
            zipfilename = os.path.join(
                basepath, f"{serialnumber}_{current_machine_id}_{pre}.zip"
            )
            zipObj = zipfile.ZipFile(zipfilename, "w")
            zipObj.write(
                logfilename, basename(logfilename), compress_type=zipfile.ZIP_DEFLATED
            )
            zipObj.close()

            too_big, size_mb = _too_large(zipfilename)
            if too_big:
                returntxt += (
                    f"Skipped (too large {size_mb:.1f} MB > {MAX_UPLOAD_SIZE_MB} MB): {zipfilename}\n"
                )
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
                continue

            ok, attempts, last_error = _drop_with_retries(nc, zipfilename)
            if ok:
                if last_error:
                    returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts}, note={last_error})\n"
                else:
                    returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts})\n"
                try:
                    os.remove(logfilename)
                except Exception:
                    pass
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
            else:
                if last_error:
                    returntxt = (
                        returntxt
                        + f"Upload Failed after {attempts} attempts: {zipfilename} ({last_error})\n"
                    )
                else:
                    returntxt = returntxt + f"Upload Failed after {attempts} attempts: {zipfilename}\n"
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
    else:
        returntxt = returntxt + f"Connection failed"
    return returntxt


def uploadLaserPowerLog(
    basepath="",
    serialnumber="0000000",
    current_machine_id="00000000-0000-0000-0000-000000000000",
):

    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))

    returntxt = f"LaserPower.log Dir: {basepath}\n"
    nc = nextcloud_client.Client.from_public_link(settings.public_link)
    if nc:
        filepattern = os.path.join(basepath, "LaserPower.log")
        logfiles = glob.glob(filepattern)
        logfiles.sort()
        for logfilename in logfiles:
            # try to see if the file is open
            try:
                os.rename(logfilename, logfilename)
            except OSError:
                returntxt = returntxt + f"File is open, skipping: {logfilename}\n"
                continue

            # Get the file modification time (Unix timestamp)
            mod_time = os.path.getmtime(logfilename)

            # Convert the timestamp to a readable date format (e.g., YYYY-MM-DD)
            mod_time_str = datetime.datetime.fromtimestamp(mod_time).strftime(
                "%Y%m%d%H%M%S"
            )
            pre, ext = os.path.splitext(os.path.basename(logfilename))
            zipfilename = os.path.join(
                basepath,
                f"{serialnumber}_{current_machine_id}_{pre}_{mod_time_str}.zip",
            )
            zipObj = zipfile.ZipFile(zipfilename, "w")
            zipObj.write(
                logfilename, basename(logfilename), compress_type=zipfile.ZIP_DEFLATED
            )
            zipObj.close()

            too_big, size_mb = _too_large(zipfilename)
            if too_big:
                returntxt += (
                    f"Skipped (too large {size_mb:.1f} MB > {MAX_UPLOAD_SIZE_MB} MB): {zipfilename}\n"
                )
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
                continue

            ok, attempts, last_error = _drop_with_retries(nc, zipfilename)
            if ok:
                if last_error:
                    returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts}, note={last_error})\n"
                else:
                    returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts})\n"
                try:
                    os.remove(logfilename)
                except Exception:
                    pass
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
            else:
                if last_error:
                    returntxt = (
                        returntxt
                        + f"Upload Failed after {attempts} attempts: {zipfilename} ({last_error})\n"
                    )
                else:
                    returntxt = returntxt + f"Upload Failed after {attempts} attempts: {zipfilename}\n"
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
    else:
        returntxt = returntxt + f"Connection failed"
    return returntxt


def uploadSettings(
    basepath="",
    serialnumber="0000000",
    current_machine_id="00000000-0000-0000-0000-000000000000",
):
    import datetime as dt

    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))
    basepath = os.path.join(basepath, "")
    returntxt = f"SettingsDir: {basepath}\n"

    nc = nextcloud_client.Client.from_public_link(settings.public_link)
    if nc:
        filepattern = os.path.join(basepath, "*.xml")
        settingsFiles = glob.glob(filepattern)
        settingsFiles.sort()
        for settingsFileName in settingsFiles:
            if has_file_changed(settingsFileName):
                pre, ext = os.path.splitext(os.path.basename(settingsFileName))
                timestamp = dt.datetime.now().strftime("%Y%m%d%H%M%S")
                zipfilename = os.path.join(
                    basepath,
                    f"{serialnumber}_{current_machine_id}_{pre}_{timestamp}.zip",
                )
                zipObj = zipfile.ZipFile(zipfilename, "w")
                zipObj.write(
                    settingsFileName,
                    basename(settingsFileName),
                    compress_type=zipfile.ZIP_DEFLATED,
                )
                zipObj.close()

                too_big, size_mb = _too_large(zipfilename)
                if too_big:
                    returntxt += (
                        f"Skipped (too large {size_mb:.1f} MB > {MAX_UPLOAD_SIZE_MB} MB): {zipfilename}\n"
                    )
                    try:
                        os.remove(zipfilename)
                    except Exception:
                        pass
                    continue

                ok, attempts, last_error = _drop_with_retries(nc, zipfilename)
                if ok:
                    if last_error:
                        returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts}, note={last_error})\n"
                    else:
                        returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts})\n"
                else:
                    if last_error:
                        returntxt = (
                            returntxt
                            + f"Upload Failed after {attempts} attempts: {zipfilename} ({last_error})\n"
                        )
                    else:
                        returntxt = returntxt + f"Upload Failed after {attempts} attempts: {zipfilename}\n"
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
    else:
        returntxt = returntxt + f"Connection failed"
    return returntxt


def uploadUserSettings(
    basepath="",
    serialnumber="0000000",
    current_machine_id="00000000-0000-0000-0000-000000000000",
):
    import datetime as dt

    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))
    basepath = os.path.join(basepath, "UserSettings")

    returntxt = f"UserSettingsDir: {basepath}\n"
    nc = nextcloud_client.Client.from_public_link(settings.public_link)
    if nc:
        filepattern = os.path.join(basepath, "*.xml")
        settingsFiles = glob.glob(filepattern)
        settingsFiles.sort()
        for settingsFileName in settingsFiles:
            if has_file_changed(settingsFileName):
                pre, ext = os.path.splitext(os.path.basename(settingsFileName))
                timestamp = dt.datetime.now().strftime("%Y%m%d%H%M%S")
                zipfilename = os.path.join(
                    basepath,
                    f"{serialnumber}_{current_machine_id}_UserSettings_{pre}_{timestamp}.zip",
                )
                zipObj = zipfile.ZipFile(zipfilename, "w")
                zipObj.write(
                    settingsFileName,
                    basename(settingsFileName),
                    compress_type=zipfile.ZIP_DEFLATED,
                )
                zipObj.close()

                too_big, size_mb = _too_large(zipfilename)
                if too_big:
                    returntxt += (
                        f"Skipped (too large {size_mb:.1f} MB > {MAX_UPLOAD_SIZE_MB} MB): {zipfilename}\n"
                    )
                    try:
                        os.remove(zipfilename)
                    except Exception:
                        pass
                    continue

                ok, attempts, last_error = _drop_with_retries(nc, zipfilename)
                if ok:
                    if last_error:
                        returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts}, note={last_error})\n"
                    else:
                        returntxt = returntxt + f"Uploaded: {zipfilename} (attempts={attempts})\n"
                else:
                    if last_error:
                        returntxt = (
                            returntxt
                            + f"Upload Failed after {attempts} attempts: {zipfilename} ({last_error})\n"
                        )
                    else:
                        returntxt = returntxt + f"Upload Failed after {attempts} attempts: {zipfilename}\n"
                try:
                    os.remove(zipfilename)
                except Exception:
                    pass
    else:
        returntxt = returntxt + f"Connection failed"
    return returntxt


def copyDB(
    basepath="",
):
    import datetime as dt

    sourcepath = "C:/Program Files/PicoQuant/Luminosa/"

    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))

    returntxt = f"DBDir: {basepath}\n"

    source_file = os.path.join(sourcepath, "PQDevice.db")
    destination_file = os.path.join(basepath, "PQDevice.db.xml")

    shutil.copy2(source_file, destination_file)
    returntxt += f"File copied from {source_file} to {destination_file}\n"

    source_file = os.path.join(sourcepath, "PQDevice.conf")
    destination_file = os.path.join(basepath, "PQDevice.conf.xml")

    shutil.copy2(source_file, destination_file)
    returntxt += f"File copied from {source_file} to {destination_file}\n"

    return returntxt


def get_machine_guid_windows():
    """Retrieve the MachineGUID from the Windows registry."""
    try:
        import winreg
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography")
        machine_guid, _ = winreg.QueryValueEx(key, "MachineGuid")
        winreg.CloseKey(key)
        return machine_guid
    except Exception as e:
        print(f"Failed to retrieve MachineGUID on Windows: {e}")
        return "00000000-0000-0000-0000-000000000000"  # Fallback GUID

def get_machine_guid_mac():
    """Retrieve the hardware UUID on macOS using ioreg."""
    try:
        # Run the ioreg command to get the hardware UUID
        output = subprocess.check_output(
            ["ioreg", "-rd1", "-c", "IOPlatformExpertDevice"],
            text=True
        )
        for line in output.splitlines():
            if "IOPlatformUUID" in line:
                # Extract and return the UUID
                return line.split("=")[1].strip().strip('"')
    except Exception as e:
        print(f"Failed to retrieve hardware UUID on macOS: {e}")
        return "00000000-0000-0000-0000-000000000000"  # Fallback UUID

def get_machine_guid_linux():
    """Retrieve the machine ID from /etc/machine-id on Linux."""
    try:
        with open("/etc/machine-id", "r") as file:
            machine_id = file.read().strip()
            return machine_id
    except Exception as e:
        print(f"Failed to retrieve machine ID on Linux: {e}")
        return "00000000-0000-0000-0000-000000000000"  # Fallback UUID


def init():
    """Initialize platform-specific configurations and retrieve machine ID."""
    if sys.platform == "win32":  # Windows-specific logic
        path = os.getenv("PROGRAMDATA")  # Common app data folder
        default_dir = os.path.join(path, "PicoQuant", "Luminosa")
        current_machine_id = get_machine_guid_windows()
    elif sys.platform == "darwin":  # macOS-specific logic
        default_dir = os.path.expanduser("~/Library/Application Support/Luminosa")
        current_machine_id = get_machine_guid_mac()
    elif sys.platform.startswith("linux"):  # Linux-specific logic
        default_dir = os.path.expanduser("~/.luminosa")  # Typical location for app data in Linux
        current_machine_id = get_machine_guid_linux()
    else:
        default_dir = "./"
        current_machine_id = "00000000-0000-0000-0000-000000000000"

    # Create the basepath and simulate serial number retrieval
    basepath = os.path.abspath(default_dir)
    serialnumber = getLumiSerial(basepath)

    return [default_dir, serialnumber, current_machine_id]


if __name__ == "__main__":
    [defaultDir, serialnumber, current_machine_id] = init()

    parser = ArgumentParser()
    parser.add_argument(
        "dir", help="Log Directory", type=str, nargs="?", default=defaultDir
    )
    args = parser.parse_args()

    if os.path.isdir(os.path.abspath(args.dir)):
        basepath = os.path.abspath(args.dir)
    else:
        basepath = os.path.abspath(defaultDir)

    print(f"trying to find {basepath}")
    if not os.path.isdir(basepath):
        basepath = os.path.dirname(os.path.realpath(__file__))
        print(f"Directory not found defaulting to {basepath}")

        print(f"Luminosa Serial Number: {serialnumber}")

    print(copyDB(basepath))
    print(uploadSettings(basepath, serialnumber, current_machine_id))
    print(uploadUserSettings(basepath, serialnumber, current_machine_id))
    print(uploadLaserPowerLog(basepath, serialnumber, current_machine_id))
    print(uploadlog(basepath, serialnumber, current_machine_id))
