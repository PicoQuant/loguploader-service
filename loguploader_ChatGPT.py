import os
import sys
import glob
import shutil
import zipfile
import datetime
import time
import subprocess
from os.path import basename, abspath
from argparse import ArgumentParser

import nextcloud_client
import settings
import winpath  # Assuming this is a custom module


def has_file_changed(file_path):
    """
    Checks if the specified file has changed since the last check.

    Args:
        file_path (str): Path to the file to check.

    Returns:
        bool: True if the file has changed since the last check, False otherwise.
    """
    last_check_file = os.path.splitext(file_path)[0] + ".lastcheck"
    current_mod_time = os.path.getmtime(file_path)

    try:
        with open(last_check_file, "r") as file:
            last_check_time = float(file.read())
    except (FileNotFoundError, ValueError):
        # If the last check file doesn't exist or is invalid, consider the file as changed
        last_check_time = 0

    # Update the last check time for next iteration
    with open(last_check_file, "w") as file:
        file.write(str(time.time()))

    return current_mod_time > last_check_time


def get_lumi_serial(basepath):
    """
    Retrieves the Luminosa serial number from 'LastOpenSerial.txt'.

    Args:
        basepath (str): Base directory where 'Logs/LastOpenSerial.txt' is located.

    Returns:
        str: The serial number if found, otherwise '0000000'.
    """
    filename = os.path.join(basepath, "Logs", "LastOpenSerial.txt")
    try:
        with open(filename, "r") as fp:
            serial = fp.read().split()[-1]
            print(f"Serial {serial} read from {filename}")
            return serial
    except Exception:
        return "0000000"


def upload_files(
    basepath,
    serialnumber,
    machine_id,
    file_pattern,
    prefix,
    subfolder="",
    delete_original=False,
):
    """
    Uploads files matching 'file_pattern' from 'basepath' to Nextcloud.
    Compresses each file into a zip before uploading.

    Args:
        basepath (str): Base directory where files are located.
        serialnumber (str): Serial number of the device.
        machine_id (str): Machine ID.
        file_pattern (str): Pattern to match files (e.g., '*.xml').
        prefix (str): Prefix to identify the type of files (e.g., 'settings').
        subfolder (str, optional): Subfolder under basepath. Defaults to "".
        delete_original (bool, optional): Whether to delete the original file after uploading. Defaults to False.

    Returns:
        str: Log of upload operations.
    """
    nc = nextcloud_client.Client.from_public_link(settings.public_link)
    if not nc:
        return "Connection failed"

    full_path = os.path.join(basepath, subfolder)
    files = glob.glob(os.path.join(full_path, file_pattern))
    files.sort()
    returntxt = f"{prefix.capitalize()} Directory: {full_path}\n"

    for filename in files:
        # Check if the file is in use
        try:
            os.rename(filename, filename)
        except OSError:
            returntxt += f"File is open: {filename}\n"
            continue

        # For settings files, check if the file has changed
        if prefix in ["settings", "user_settings"] and not has_file_changed(filename):
            continue

        # Generate zip filename with timestamp
        mod_time = os.path.getmtime(filename)
        timestamp = datetime.datetime.fromtimestamp(mod_time).strftime("%Y%m%d%H%M%S")
        base_name = os.path.basename(filename).split(".")[0]
        zip_filename = os.path.join(
            full_path, f"{serialnumber}_{machine_id}_{base_name}_{timestamp}.zip"
        )

        # Compress the file
        with zipfile.ZipFile(zip_filename, "w") as zipObj:
            zipObj.write(
                filename, basename(filename), compress_type=zipfile.ZIP_DEFLATED
            )

        # Upload to Nextcloud
        if nc.drop_file(zip_filename):
            returntxt += f"Uploaded: {zip_filename}\n"
            if delete_original:
                os.remove(filename)  # Delete the original file only if specified
        else:
            returntxt += f"Upload Failed: {zip_filename}\n"

        os.remove(zip_filename)  # Remove the zip file after uploading

    return returntxt or f"No files to upload from {full_path}\n"


def copy_db(basepath):
    """
    Copies 'PQDevice.db' and 'PQDevice.conf' from the installation directory to 'basepath'.

    Args:
        basepath (str): Destination directory for the copied files.

    Returns:
        str: Log of copy operations.
    """
    sourcepath = "C:/Program Files/PicoQuant/Luminosa/"
    files_to_copy = ["PQDevice.db", "PQDevice.conf"]
    returntxt = "Copying database files:\n"

    for filename in files_to_copy:
        source_file = os.path.join(sourcepath, filename)
        destination_file = os.path.join(basepath, f"{filename}.xml")
        try:
            shutil.copy2(source_file, destination_file)
            returntxt += f"Copied {source_file} to {destination_file}\n"
        except Exception as e:
            returntxt += f"Failed to copy {source_file}: {e}\n"

    return returntxt


def init():
    """
    Initializes default directories and retrieves machine ID and serial number.

    Returns:
        tuple: (basepath, serialnumber, current_machine_id)
    """
    if sys.platform == "win32":
        path = winpath.get_common_appdata()
        default_dir = os.path.join(path, "PicoQuant", "Luminosa")
        try:
            current_machine_id = (
                subprocess.check_output("wmic csproduct get uuid")
                .decode()
                .split("\n")[1]
                .strip()
            )
            print(f"Machine ID: {current_machine_id}")
        except Exception:
            current_machine_id = "00000000-0000-0000-0000-000000000000"
    else:
        default_dir = "./"
        current_machine_id = "00000000-0000-0000-0000-000000000000"

    basepath = abspath(default_dir)
    serialnumber = get_lumi_serial(basepath)

    return basepath, serialnumber, current_machine_id


if __name__ == "__main__":
    basepath, serialnumber, current_machine_id = init()

    parser = ArgumentParser()
    parser.add_argument(
        "dir", help="Log Directory", type=str, nargs="?", default=basepath
    )
    args = parser.parse_args()

    if os.path.isdir(abspath(args.dir)):
        basepath = abspath(args.dir)
    else:
        print(f"Directory {args.dir} not found, using default {basepath}")

    print(copy_db(basepath))
    print(
        upload_files(
            basepath,
            serialnumber,
            current_machine_id,
            "*.xml",
            "settings",
            delete_original=False,
        )
    )
    print(
        upload_files(
            basepath,
            serialnumber,
            current_machine_id,
            "*.xml",
            "user_settings",
            subfolder="UserSettings",
            delete_original=False,
        )
    )
    print(
        upload_files(
            basepath,
            serialnumber,
            current_machine_id,
            "LaserPower.log",
            "laser_power",
            delete_original=False,
        )
    )
    print(
        upload_files(
            basepath,
            serialnumber,
            current_machine_id,
            "*.pqlog",
            "log",
            subfolder="Logs",
            delete_original=True,
        )
    )
