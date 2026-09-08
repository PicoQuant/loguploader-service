#!/usr/bin/env python3
"""Pull v2 config-backup artifacts from api.picoquant.com to a local tree.

Maintenance / disaster-recovery tool. Talks to the admin API with the key from `.env`
(`EXPECTED_ADMIN_API_KEY`). Downloads land under one folder per instrument serial, so you
can pull a whole fleet at once and later restore a single device by copying its folder back.

Layout (per serial)::

    <out>/<product>/<serial>/
        manifest.json                       # every backup record for this instrument
        ProgramData/PicoQuant/Luminosa/ChromophoreList.xml     # newest content, mirrored
        Program Files/PicoQuant/Luminosa/PQDevice.db           #   from `source_path`
        _versions/ProgramData/.../ChromophoreList.xml/
            2026-09-08T12-52-51Z_b72cde42.bak                  # older versions (--all-versions)

Every download is sha256-verified against the record. Re-runs are incremental: a file
already on disk with the right digest is skipped.

Examples::

    # everything for two instruments
    python tools/fleet_backup_pull.py SN-12345 SN-67890

    # the whole Luminosa fleet, all historical versions, into a dated folder
    python tools/fleet_backup_pull.py --all --all-versions --out backups/2026-09-08

    # just what changed in the last week for Solira
    python tools/fleet_backup_pull.py --product solira --all --since 2026-09-01

No third-party dependencies (stdlib only).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_API = "https://api.picoquant.com"
PAGE = 1000  # admin list max


# --------------------------------------------------------------------------- env


def load_env(path: Path) -> dict[str, str]:
    env: dict[str, str] = {}
    if not path.is_file():
        return env
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        env[k.strip()] = v.strip().strip('"').strip("'")
    return env


# --------------------------------------------------------------------------- http


class Api:
    def __init__(self, base_url: str, admin_key: str, product: str):
        self.base = base_url.rstrip("/")
        self.key = admin_key
        self.product = product

    def _get(self, path: str, params: dict | None = None):
        url = f"{self.base}{path}"
        if params:
            url += "?" + urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
        req = urllib.request.Request(url, headers={"X-ADMIN-API-KEY": self.key})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            body = e.read().decode("utf-8", "replace")[:300]
            raise SystemExit(f"HTTP {e.code} for {path}: {body}") from None
        except urllib.error.URLError as e:
            raise SystemExit(f"cannot reach {self.base}: {e.reason}") from None

    def list_backups(self, *, instrument_serial=None, file_key=None, since=None, until=None):
        """Yield every metadata row (paginates on limit/offset)."""
        offset = 0
        while True:
            payload = json.loads(
                self._get(
                    f"/api/v2/admin/products/{self.product}/backups",
                    {
                        "instrument_serial": instrument_serial,
                        "file_key": file_key,
                        "since": since,
                        "until": until,
                        "limit": PAGE,
                        "offset": offset,
                    },
                )
            )
            rows = payload.get("backups", payload if isinstance(payload, list) else [])
            if not rows:
                return
            yield from rows
            if len(rows) < PAGE:
                return
            offset += PAGE

    def download(self, backup_id: str) -> bytes:
        return self._get(f"/api/v2/admin/products/{self.product}/backups/{backup_id}/content")


# ------------------------------------------------------------------------- layout


def mirror_relpath(source_path: str, file_key: str) -> Path:
    """Turn a Windows `source_path` into a restore-friendly relative path.

    `C:\\ProgramData\\PicoQuant\\Luminosa\\LastKnownGood.xml`
        -> ProgramData/PicoQuant/Luminosa/LastKnownGood.xml
    Falls back to the (normalised) `file_key` if `source_path` is unusable.
    """
    sp = (source_path or "").replace("\\", "/").strip()
    sp = re.sub(r"^[A-Za-z]:/", "", sp)  # drop drive
    sp = sp.lstrip("/")
    parts = [p for p in sp.split("/") if p not in ("", ".", "..")]
    if parts:
        return Path(*parts)
    return Path(*[p for p in file_key.split("/") if p not in ("", ".", "..")])


def version_tag(row: dict) -> str:
    ts = (row.get("received_at") or row.get("client_timestamp") or "unknown").replace(":", "-")
    ts = ts.split(".")[0].rstrip("Z") + "Z"
    sha8 = (row.get("content_sha256") or "0" * 8)[:8]
    return f"{ts}_{sha8}"


# --------------------------------------------------------------------------- pull


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def write_verified(dest: Path, data: bytes, expected_sha: str | None) -> None:
    got = hashlib.sha256(data).hexdigest()
    if expected_sha and got != expected_sha:
        raise SystemExit(f"sha256 mismatch for {dest.name}: expected {expected_sha}, got {got}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".tmp")
    tmp.write_bytes(data)
    tmp.replace(dest)


def pull_serial(api: Api, serial: str, out_root: Path, *, all_versions: bool,
                since: str | None, until: str | None) -> dict:
    rows = sorted(
        api.list_backups(instrument_serial=serial, since=since, until=until),
        key=lambda r: r.get("received_at", ""),
    )
    serial_dir = out_root / api.product / safe(serial)
    stats = {"serial": serial, "records": len(rows), "downloaded": 0, "skipped": 0, "files": 0}
    if not rows:
        print(f"  {serial}: no backups")
        return stats

    # group by file_key; newest last (rows are ascending by received_at)
    by_key: dict[str, list[dict]] = {}
    for r in rows:
        by_key.setdefault(r.get("file_key", "?"), []).append(r)

    for file_key, versions in sorted(by_key.items()):
        rel = mirror_relpath(versions[-1].get("source_path", ""), file_key)
        latest = versions[-1]

        # newest content at its mirrored path
        dest = serial_dir / rel
        if dest.is_file() and latest.get("content_sha256") and sha256_file(dest) == latest["content_sha256"]:
            stats["skipped"] += 1
        else:
            write_verified(dest, api.download(latest["id"]), latest.get("content_sha256"))
            stats["downloaded"] += 1
            print(f"  {serial}: {rel}  ({latest.get('size_bytes', '?')} B)")
        stats["files"] += 1

        # history
        older = versions if all_versions else []
        for r in older:
            vdest = serial_dir / "_versions" / rel / f"{version_tag(r)}.bak"
            if vdest.is_file() and r.get("content_sha256") and sha256_file(vdest) == r["content_sha256"]:
                stats["skipped"] += 1
                continue
            write_verified(vdest, api.download(r["id"]), r.get("content_sha256"))
            stats["downloaded"] += 1

    manifest = serial_dir / "manifest.json"
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(
        json.dumps({"serial": serial, "product": api.product, "backups": rows}, indent=2),
        encoding="utf-8",
    )
    return stats


def safe(name: str) -> str:
    """A filesystem-safe folder name for a serial (they are normally clean already)."""
    return re.sub(r"[^A-Za-z0-9._-]", "_", name) or "unknown"


def discover_serials(api: Api, since: str | None, until: str | None) -> list[str]:
    seen: set[str] = set()
    for r in api.list_backups(since=since, until=until):
        s = r.get("instrument_serial")
        if s:
            seen.add(s)
    return sorted(seen)


# --------------------------------------------------------------------------- main


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("serials", nargs="*", help="instrument serial(s) to pull; omit with --all")
    ap.add_argument("--all", action="store_true", help="discover and pull every instrument with backups")
    ap.add_argument("--product", default="luminosa", choices=["luminosa", "solira"])
    ap.add_argument("--out", default="fleet-backups", type=Path, help="output root (default: ./fleet-backups)")
    ap.add_argument("--all-versions", action="store_true", help="also download historical versions, not just the newest")
    ap.add_argument("--since", help="ISO date/time lower bound (received_at)")
    ap.add_argument("--until", help="ISO date/time upper bound (received_at)")
    ap.add_argument("--api", default=None, help=f"API base URL (default: $API_BASE_URL or {DEFAULT_API})")
    ap.add_argument("--env", default=Path(".env"), type=Path, help="path to .env (default: ./.env)")
    args = ap.parse_args(argv)

    env = load_env(args.env)
    admin_key = os.environ.get("EXPECTED_ADMIN_API_KEY") or env.get("EXPECTED_ADMIN_API_KEY")
    if not admin_key:
        print(f"error: EXPECTED_ADMIN_API_KEY not in {args.env} or the environment", file=sys.stderr)
        return 2
    base = args.api or os.environ.get("API_BASE_URL") or env.get("API_BASE_URL") or DEFAULT_API

    api = Api(base, admin_key, args.product)

    serials = list(args.serials)
    if args.all:
        print(f"discovering {args.product} instruments at {base} ...")
        serials = sorted(set(serials) | set(discover_serials(api, args.since, args.until)))
    if not serials:
        print("nothing to do: give one or more serials, or --all", file=sys.stderr)
        return 2

    print(f"pulling {len(serials)} instrument(s) -> {args.out.resolve()}")
    totals = {"downloaded": 0, "skipped": 0, "files": 0, "records": 0}
    for s in serials:
        st = pull_serial(api, s, args.out, all_versions=args.all_versions, since=args.since, until=args.until)
        for k in totals:
            totals[k] += st.get(k, 0)

    print(
        f"\ndone: {totals['files']} files across {len(serials)} instrument(s) "
        f"({totals['downloaded']} downloaded, {totals['skipped']} up-to-date, "
        f"{totals['records']} backup records)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
