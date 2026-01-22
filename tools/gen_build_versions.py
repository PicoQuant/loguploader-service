import os
import re


def _read_version(repo_root: str) -> str:
    path = os.path.join(repo_root, "VERSION")
    with open(path, "r", encoding="utf-8") as f:
        v = f.read().strip()
    if not re.match(r"^\d+\.\d+(\.\d+)?$", v):
        raise ValueError(f"Invalid VERSION '{v}'")
    return v


def _to_filevers(version: str) -> tuple[int, int, int, int]:
    parts = [int(p) for p in version.split(".")]
    while len(parts) < 4:
        parts.append(0)
    return tuple(parts[:4])  # type: ignore[return-value]


def write_version_iss(repo_root: str, version: str) -> None:
    path = os.path.join(repo_root, "version.iss")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"#define MyAppVersion \"{version}\"\n")


def write_version_info(repo_root: str, version: str) -> None:
    filevers = _to_filevers(version)
    path = os.path.join(repo_root, "version_info.txt")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(
            "# UTF-8\n"
            "VSVersionInfo(\n"
            "  ffi=FixedFileInfo(\n"
            f"    filevers={filevers},\n"
            f"    prodvers={filevers},\n"
            "    mask=0x3f,\n"
            "    flags=0x0,\n"
            "    OS=0x4,\n"
            "    fileType=0x1,\n"
            "    subtype=0x0,\n"
            "    date=(0, 0)\n"
            "  ),\n"
            "  kids=[\n"
            "    StringFileInfo(\n"
            "      [\n"
            "        StringTable(\n"
            "          '040904B0',\n"
            "          [\n"
            "            StringStruct('CompanyName', 'PicoQuant'),\n"
            "            StringStruct('FileDescription', 'Luminosa Log Uploader Service'),\n"
            f"            StringStruct('FileVersion', '{version}'),\n"
            "            StringStruct('InternalName', 'loguploaderservice'),\n"
            "            StringStruct('OriginalFilename', 'loguploaderservice.exe'),\n"
            "            StringStruct('ProductName', 'Luminosa Log Uploader'),\n"
            f"            StringStruct('ProductVersion', '{version}'),\n"
            "          ]\n"
            "        )\n"
            "      ]\n"
            "    ),\n"
            "    VarFileInfo([VarStruct('Translation', [1033, 1200])])\n"
            "  ]\n"
            ")\n"
        )


def main() -> int:
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    version = _read_version(repo_root)
    write_version_iss(repo_root, version)
    write_version_info(repo_root, version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
