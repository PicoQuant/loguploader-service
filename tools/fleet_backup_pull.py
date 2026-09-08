#!/usr/bin/env python3
"""Pull v2 config-backup artifacts from api.picoquant.com into a permanent local archive.

Maintainer / disaster-recovery tool (spec 004). Runs off the instrument, from an external
scheduler (a cron job — the schedule is the operator's job, not this tool's). Each run sweeps
the admin backup list for every product it can see and downloads every artifact the local
archive does not already hold, verifies it, files it, and **never deletes or overwrites**
history it already captured — so the archive stays complete even after the backend prunes.

Archive layout (one folder per physical machine)::

    <out>/<product>/<serial>/<machine-id>/
        manifest.json                                   # what this folder holds + the cursor
        ProgramData/PicoQuant/Luminosa/LastKnownGood.xml # newest version, mirrored from source_path
        _versions/ProgramData/PicoQuant/Luminosa/LastKnownGood.xml/
            2026-09-08T12-52-52Z__b72cde42.bak          # every version (incl. newest)

Auth: the admin key from `EXPECTED_ADMIN_API_KEY` (env or `--env` file). It is never written
to the archive, the manifests, the lock file, or any line this tool prints.

Exit codes: 0 = success / nothing new / another run already in progress; 1 = partial (>=1
artifact failed); 2 = fatal (no admin key, no product reachable, archive not writable).

No third-party dependencies (Python standard library only).
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import re
import socket
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

DEFAULT_API = "https://api.picoquant.com"
PAGE = 1000                 # admin list max page size
SCHEMA_VERSION = 1          # manifest.json schema
STALE_LOCK_SECS = 6 * 3600  # a lock older than this is reclaimed
MAX_ATTEMPTS = 3
BACKOFF_SECS = (2, 4)
PRODUCTS = ("luminosa", "solira")


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log(msg: str) -> None:
    """Diagnostics to stderr. Never receives the admin key."""
    print(f"[fleet-backup-pull] {msg}", file=sys.stderr)


# --------------------------------------------------------------------------- config


def parse_env_file(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return out
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = v.strip().strip('"').strip("'")
    return out


@dataclass
class Config:
    admin_key: str
    api_base_url: str
    timeout: float
    out: Path
    products: list[str]
    serials: list[str]
    since: str | None
    until: str | None
    quiet: bool
    rebuild_manifests: bool
    key_error: str | None = None  # set instead of admin_key when the key is missing -> exit 2


def load_config(args: argparse.Namespace) -> Config:
    env = parse_env_file(args.env)
    key = os.environ.get("EXPECTED_ADMIN_API_KEY") or env.get("EXPECTED_ADMIN_API_KEY") or ""
    base = (
        args.api
        or os.environ.get("API_BASE_URL")
        or env.get("API_BASE_URL")
        or DEFAULT_API
    ).rstrip("/")
    key_error = None
    if not key:
        key_error = (
            f"EXPECTED_ADMIN_API_KEY is not set in the environment or {args.env} "
            f"(the value is never printed)."
        )
    return Config(
        admin_key=key,
        api_base_url=base,
        timeout=float(args.timeout),
        out=Path(args.out),
        products=list(args.product) if args.product else list(PRODUCTS),
        serials=list(args.serial) if args.serial else [],
        since=args.since,
        until=args.until,
        quiet=bool(args.quiet),
        rebuild_manifests=bool(args.rebuild_manifests),
        key_error=key_error,
    )


# --------------------------------------------------------------------------- http


class ApiError(Exception):
    pass


class ApiAuthError(ApiError):
    def __init__(self, code: int):
        super().__init__(f"HTTP {code}")
        self.code = code


class ApiNotFound(ApiError):
    pass


class Api:
    def __init__(self, base_url: str, admin_key: str, timeout: float = 60.0):
        self.base = base_url.rstrip("/")
        self._key = admin_key
        self.timeout = timeout

    def _get(self, path: str, params: dict | None = None, *, content: bool = False) -> bytes:
        url = self.base + path
        if params:
            clean = {k: v for k, v in params.items() if v is not None and v != ""}
            url += "?" + urllib.parse.urlencode(clean)
        last: Exception | None = None
        for attempt in range(1, MAX_ATTEMPTS + 1):
            try:
                req = urllib.request.Request(url, headers={"X-ADMIN-API-KEY": self._key})
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    return resp.read()
            except urllib.error.HTTPError as e:
                if e.code in (401, 403):
                    raise ApiAuthError(e.code) from None
                if e.code == 404:
                    if content:
                        raise ApiNotFound() from None
                    raise ApiAuthError(404) from None  # unknown product
                if e.code >= 500:
                    last = e
                else:
                    raise ApiError(f"HTTP {e.code} for {path}") from None
            except urllib.error.URLError as e:
                last = e
            if attempt < MAX_ATTEMPTS:
                time.sleep(BACKOFF_SECS[attempt - 1])
        raise ApiError(f"{path}: {last}")

    def list_backups(self, product: str, *, instrument_serial: str | None = None,
                     since: str | None = None, until: str | None = None):
        offset = 0
        while True:
            payload = json.loads(
                self._get(
                    f"/api/v2/admin/products/{product}/backups",
                    {
                        "instrument_serial": instrument_serial,
                        "since": since,
                        "until": until,
                        "limit": PAGE,
                        "offset": offset,
                    },
                )
            )
            rows = payload.get("backups", []) if isinstance(payload, dict) else list(payload)
            for row in rows:
                yield row
            if len(rows) < PAGE:
                return
            offset += PAGE

    def download(self, product: str, backup_id: str) -> bytes:
        return self._get(
            f"/api/v2/admin/products/{product}/backups/{backup_id}/content", content=True
        )


# --------------------------------------------------------------------------- layout


def safe_segment(name: str) -> str:
    """Filesystem-safe path segment for a serial / machine id (normally already clean)."""
    return re.sub(r"[^A-Za-z0-9._@+-]", "_", name) or "unknown"


def rel_path(source_path: str, file_key: str) -> PurePosixPath:
    """Windows `source_path` -> restore-friendly relative path (drive stripped).

    `C:\\ProgramData\\PicoQuant\\Luminosa\\LastKnownGood.xml`
        -> ProgramData/PicoQuant/Luminosa/LastKnownGood.xml
    Falls back to `file_key` split on '/' when `source_path` is unusable.
    """
    sp = (source_path or "").replace("\\", "/").strip()
    sp = re.sub(r"^[A-Za-z]:/", "", sp).lstrip("/")
    parts = [p for p in sp.split("/") if p not in ("", ".", "..")]
    if not parts:
        parts = [p for p in (file_key or "").split("/") if p not in ("", ".", "..")]
    if not parts:
        parts = ["_unknown"]
    return PurePosixPath(*parts)


def version_filename(received_at: str, content_sha256: str) -> str:
    ts = (received_at or "unknown").split(".")[0].replace(":", "-").rstrip("Z") + "Z"
    sha8 = (content_sha256 or "0" * 8)[:8]
    return f"{ts}__{sha8}.bak"


# --------------------------------------------------------------------------- write


FAILURE_DIGEST = "digest_mismatch"
FAILURE_DOWNLOAD = "download_error"
FAILURE_WRITE = "write_error"
FAILURE_PRUNED = "pruned"


@dataclass
class Failure:
    category: str
    detail: str = ""


def write_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".part")
    tmp.write_bytes(data)
    os.replace(tmp, path)


@dataclass
class ManifestEntry:
    id: str | None
    file_key: str
    source_path: str
    rel_path: str
    content_sha256: str
    size_bytes: int
    received_at: str
    file_mtime: str | None
    agent_version: str | None
    version_file: str      # POSIX path relative to the machine folder
    is_latest: bool
    archived_utc: str

    @staticmethod
    def from_dict(d: dict) -> "ManifestEntry":
        return ManifestEntry(
            id=d.get("id"),
            file_key=d.get("file_key", ""),
            source_path=d.get("source_path", ""),
            rel_path=d.get("rel_path", ""),
            content_sha256=d.get("content_sha256", ""),
            size_bytes=int(d.get("size_bytes", 0) or 0),
            received_at=d.get("received_at", ""),
            file_mtime=d.get("file_mtime"),
            agent_version=d.get("agent_version"),
            version_file=d.get("version_file", ""),
            is_latest=bool(d.get("is_latest", False)),
            archived_utc=d.get("archived_utc", ""),
        )


def commit_artifact(machine_dir: Path, rel: PurePosixPath, row: dict, data: bytes) -> ManifestEntry | Failure:
    """Verify `data` against the row digest and file it under `_versions/`. Append-only."""
    expected = row.get("content_sha256") or ""
    got = hashlib.sha256(data).hexdigest()
    if expected and got != expected:
        return Failure(FAILURE_DIGEST, f"expected {expected[:8]}, got {got[:8]}")

    vf = version_filename(row.get("received_at", ""), got)
    version_rel = PurePosixPath("_versions", *rel.parts, vf)
    vpath = machine_dir.joinpath(*version_rel.parts)
    try:
        if not vpath.exists():  # append-only: never overwrite an archived version
            write_atomic(vpath, data)
    except OSError as e:
        return Failure(FAILURE_WRITE, str(e))

    return ManifestEntry(
        id=row.get("id"),
        file_key=row.get("file_key", ""),
        source_path=row.get("source_path", ""),
        rel_path=str(rel),
        content_sha256=got,
        size_bytes=int(row.get("size_bytes", len(data)) or len(data)),
        received_at=row.get("received_at", ""),
        file_mtime=row.get("file_mtime"),
        agent_version=row.get("agent_version"),
        version_file=str(version_rel),
        is_latest=False,
        archived_utc=now_iso(),
    )


# --------------------------------------------------------------------------- manifest


@dataclass
class Manifest:
    product: str
    instrument_serial: str
    machine_id: str
    artifacts: list[ManifestEntry] = field(default_factory=list)
    schema_version: int = SCHEMA_VERSION
    updated_utc: str = ""

    def ids(self) -> set[str]:
        return {e.id for e in self.artifacts if e.id}


def _manifest_path(machine_dir: Path) -> Path:
    return machine_dir / "manifest.json"


def load_manifest(machine_dir: Path, product: str, serial: str, machine_id: str) -> Manifest:
    p = _manifest_path(machine_dir)
    try:
        d = json.loads(p.read_text(encoding="utf-8"))
        if int(d.get("schema_version", 0)) == SCHEMA_VERSION:
            return Manifest(
                product=d.get("product", product),
                instrument_serial=d.get("instrument_serial", serial),
                machine_id=d.get("machine_id", machine_id),
                artifacts=[ManifestEntry.from_dict(e) for e in d.get("artifacts", [])],
                schema_version=SCHEMA_VERSION,
                updated_utc=d.get("updated_utc", ""),
            )
        log(f"{p}: schema_version {d.get('schema_version')} != {SCHEMA_VERSION}; rebuilding from disk")
    except FileNotFoundError:
        pass
    except (OSError, ValueError) as e:
        log(f"{p}: unreadable ({e}); rebuilding from disk")
    return rebuild_manifest_from_disk(machine_dir, product, serial, machine_id)


def save_manifest(machine_dir: Path, m: Manifest) -> None:
    m.updated_utc = now_iso()
    body = json.dumps(
        {
            "schema_version": SCHEMA_VERSION,
            "product": m.product,
            "instrument_serial": m.instrument_serial,
            "machine_id": m.machine_id,
            "updated_utc": m.updated_utc,
            "artifacts": [dataclasses.asdict(e) for e in m.artifacts],
        },
        indent=2,
    )
    write_atomic(_manifest_path(machine_dir), body.encode("utf-8"))


_VF_RE = re.compile(r"^(?P<ts>.+?)__(?P<sha8>[0-9a-f]{8})\.bak$")


def rebuild_manifest_from_disk(machine_dir: Path, product: str, serial: str, machine_id: str) -> Manifest:
    """Reconstruct a manifest from `_versions/**/*.bak` filenames (ids are unknown)."""
    m = Manifest(product=product, instrument_serial=serial, machine_id=machine_id)
    versions_root = machine_dir / "_versions"
    if not versions_root.is_dir():
        return m
    newest: dict[str, str] = {}  # rel_path -> newest ts
    for bak in versions_root.rglob("*.bak"):
        mt = _VF_RE.match(bak.name)
        if not mt:
            continue
        try:
            if bak.stat().st_size < 0:  # touch it; unreadable -> skip
                continue
            bak.read_bytes()  # ensure readable
        except OSError:
            log(f"{bak}: unreadable; omitted from rebuild")
            continue
        rel = PurePosixPath(*bak.parent.relative_to(versions_root).parts)
        rel_s = str(rel)
        ts = mt.group("ts")
        newest[rel_s] = max(newest.get(rel_s, ""), ts)
        m.artifacts.append(
            ManifestEntry(
                id=None,
                file_key="",
                source_path="",
                rel_path=rel_s,
                content_sha256="",
                size_bytes=bak.stat().st_size,
                received_at=ts,
                file_mtime=None,
                agent_version=None,
                version_file=str(PurePosixPath("_versions", *rel.parts, bak.name)),
                is_latest=False,
                archived_utc="",
            )
        )
    for e in m.artifacts:
        e.is_latest = newest.get(e.rel_path) == e.received_at
    return m


# --------------------------------------------------------------------------- lock


LOCK_HELD = object()


@dataclass
class Lock:
    path: Path


def _pid_alive(pid) -> bool:
    try:
        pid = int(pid)
    except (TypeError, ValueError):
        return True  # unknown -> assume alive (safer: skip)
    if os.name == "posix":
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
    return True  # Windows: cannot cheaply check -> assume alive


def acquire_lock(root: Path):
    root.mkdir(parents=True, exist_ok=True)
    lp = root / ".fleet-backup.lock"
    body = json.dumps(
        {"pid": os.getpid(), "host": socket.gethostname(), "started_utc": now_iso()}
    ).encode("utf-8")
    try:
        fd = os.open(lp, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        try:
            os.write(fd, body)
        finally:
            os.close(fd)
        return Lock(lp)
    except FileExistsError:
        try:
            info = json.loads(lp.read_text(encoding="utf-8"))
            age = time.time() - lp.stat().st_mtime
        except (OSError, ValueError):
            info, age = {}, STALE_LOCK_SECS + 1
        if age < STALE_LOCK_SECS and _pid_alive(info.get("pid")):
            return LOCK_HELD
        log(f"reclaiming stale lock (age {int(age)}s, pid {info.get('pid')})")
        try:
            lp.unlink()
        except OSError:
            return LOCK_HELD
        return acquire_lock(root)


def release_lock(lock) -> None:
    if isinstance(lock, Lock):
        try:
            lock.path.unlink()
        except OSError:
            pass


# --------------------------------------------------------------------------- report


@dataclass
class ProductResult:
    product: str
    accessible: bool = True
    reason: str | None = None
    sweep_failed: bool = False
    machines_seen: int = 0
    artifacts_added: int = 0
    artifacts_failed: int = 0
    pruned: int = 0


@dataclass
class RunReport:
    products: list[ProductResult] = field(default_factory=list)
    started_utc: str = ""
    finished_utc: str = ""
    fatal: bool = False
    lock_held: bool = False

    @property
    def machines_seen(self) -> int:
        return sum(p.machines_seen for p in self.products)

    @property
    def artifacts_added(self) -> int:
        return sum(p.artifacts_added for p in self.products)

    @property
    def artifacts_failed(self) -> int:
        return sum(p.artifacts_failed for p in self.products)

    @property
    def exit_code(self) -> int:
        if self.fatal:
            return 2
        if self.lock_held:
            return 0
        # every product denied by auth (permanent misconfiguration) and none merely transient
        auth_denied = [p for p in self.products if not p.accessible and not p.sweep_failed]
        if self.products and len(auth_denied) == len(self.products):
            return 2
        if self.artifacts_failed > 0 or any(p.sweep_failed for p in self.products):
            return 1
        return 0


# --------------------------------------------------------------------------- pull


def discover_and_group(api, products: list[str], serials: list[str],
                       since: str | None, until: str | None):
    groups: dict[tuple[str, str, str], list[dict]] = {}
    results: dict[str, ProductResult] = {p: ProductResult(product=p) for p in products}
    for product in products:
        pr = results[product]
        try:
            rows: list[dict] = []
            for s in (serials or [None]):
                rows.extend(api.list_backups(product, instrument_serial=s, since=since, until=until))
        except ApiAuthError as e:
            pr.accessible = False
            pr.reason = f"{e.code} — admin key has no access"
            continue
        except ApiError as e:
            pr.accessible = False
            pr.sweep_failed = True
            pr.reason = f"list failed: {e}"
            continue
        for r in rows:
            key = (
                product,
                r.get("instrument_serial") or "unknown",
                r.get("machine_id") or "unknown",
            )
            groups.setdefault(key, []).append(r)
    for key in groups:
        groups[key].sort(key=lambda r: (r.get("file_key", ""), r.get("received_at", "")))
    return groups, results


def pull_machine(api, product: str, serial: str, machine_id: str, rows: list[dict],
                 out_root: Path, pr: ProductResult, *, rebuild: bool, quiet: bool) -> None:
    machine_dir = out_root / product / safe_segment(serial) / safe_segment(machine_id)
    if rebuild:
        m = rebuild_manifest_from_disk(machine_dir, product, serial, machine_id)
    else:
        m = load_manifest(machine_dir, product, serial, machine_id)
    known_ids = m.ids()

    latest_by_key: dict[str, tuple[str, Path]] = {}  # file_key -> (received_at, bak path)
    added_entries: list[ManifestEntry] = []

    for row in rows:
        file_key = row.get("file_key", "")
        rel = rel_path(row.get("source_path", ""), file_key)
        received_at = row.get("received_at", "")
        rid = row.get("id")

        vf = version_filename(received_at, row.get("content_sha256", ""))
        vpath = machine_dir.joinpath("_versions", *rel.parts, vf)

        if rid and rid in known_ids:
            action = "have"
        elif vpath.exists():
            action = "have"  # stale/rebuilt manifest but the bytes are on disk already
        else:
            try:
                data = api.download(product, rid)
            except ApiNotFound:
                pr.pruned += 1
                _detail(quiet, f"~ {product}/{serial}/{machine_id}/{rel}  pruned (gone from backend)")
                continue
            except ApiError as e:
                pr.artifacts_failed += 1
                _detail(quiet, f"! {product}/{serial}/{machine_id}/{rel}  download_error ({e}) — not archived")
                continue
            result = commit_artifact(machine_dir, rel, row, data)
            if isinstance(result, Failure):
                pr.artifacts_failed += 1
                _detail(quiet, f"! {product}/{serial}/{machine_id}/{rel}  {result.category} ({result.detail}) — not archived")
                continue
            added_entries.append(result)
            pr.artifacts_added += 1
            _detail(quiet, f"+ {product}/{serial}/{machine_id}/{rel}  ({received_at}, {result.size_bytes} B)")
            action = "added"

        prev = latest_by_key.get(file_key)
        if prev is None or received_at >= prev[0]:
            latest_by_key[file_key] = (received_at, vpath)

    # merge new entries; recompute is_latest per file_key across the whole manifest
    by_id = {e.id: e for e in m.artifacts if e.id}
    for e in added_entries:
        if e.id and e.id in by_id:
            continue
        m.artifacts.append(e)
        if e.id:
            by_id[e.id] = e

    newest_per_key: dict[str, str] = {}
    for e in m.artifacts:
        key = e.file_key or e.rel_path
        newest_per_key[key] = max(newest_per_key.get(key, ""), e.received_at)
    for e in m.artifacts:
        e.is_latest = newest_per_key.get(e.file_key or e.rel_path) == e.received_at

    # refresh each mirrored latest from its _versions/*.bak
    for file_key, (_, bak) in latest_by_key.items():
        if not bak.exists():
            continue
        rel = PurePosixPath(*bak.parent.relative_to(machine_dir / "_versions").parts)
        mirror = machine_dir.joinpath(*rel.parts)
        try:
            data = bak.read_bytes()
            if not mirror.exists() or mirror.read_bytes() != data:
                write_atomic(mirror, data)
        except OSError as e:
            log(f"could not refresh mirrored latest {mirror}: {e}")

    if added_entries or not _manifest_path(machine_dir).exists():
        try:
            save_manifest(machine_dir, m)
        except OSError as e:
            log(f"could not write {_manifest_path(machine_dir)}: {e}")


def _detail(quiet: bool, line: str) -> None:
    if not quiet:
        print(line, file=sys.stderr)


# --------------------------------------------------------------------------- output


def print_summary(report: RunReport) -> None:
    print(f"fleet-backup-pull  {report.started_utc} .. {report.finished_utc}")
    for p in report.products:
        if not p.accessible:
            print(f"  {p.product:<9}: SKIPPED   ({p.reason})")
        else:
            print(
                f"  {p.product:<9}: ok        machines={p.machines_seen}  "
                f"added={p.artifacts_added}  failed={p.artifacts_failed}"
                + (f"  pruned={p.pruned}" if p.pruned else "")
            )
    print(
        f"totals: machines={report.machines_seen}  "
        f"artifacts added={report.artifacts_added}  failed={report.artifacts_failed}"
    )


# --------------------------------------------------------------------------- main


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--out", default="fleet-backups", help="archive root (default: ./fleet-backups)")
    ap.add_argument("--product", action="append", choices=list(PRODUCTS),
                    help="restrict to one product (repeatable; default: all)")
    ap.add_argument("--serial", action="append", help="restrict to one instrument serial (repeatable)")
    ap.add_argument("--since", help="only artifacts with received_at >= this ISO time")
    ap.add_argument("--until", help="only artifacts with received_at <= this ISO time")
    ap.add_argument("--api", help=f"backend base URL (default: $API_BASE_URL or {DEFAULT_API})")
    ap.add_argument("--env", default=Path(".env"), type=Path, help="file to read EXPECTED_ADMIN_API_KEY from")
    ap.add_argument("--timeout", default=60, type=float, help="per-request HTTP timeout (s)")
    ap.add_argument("--quiet", action="store_true", help="suppress per-artifact lines; still print the summary")
    ap.add_argument("--rebuild-manifests", action="store_true",
                    help="rebuild every touched manifest.json from _versions/ filenames first")
    return ap


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    cfg = load_config(args)
    report = RunReport(started_utc=now_iso())

    if cfg.key_error:
        log(cfg.key_error)
        report.fatal = True
    else:
        try:
            cfg.out.mkdir(parents=True, exist_ok=True)
            probe = cfg.out / ".fleet-backup.wtest"
            probe.write_text("", encoding="utf-8")
            probe.unlink()
        except OSError as e:
            log(f"archive root not writable: {cfg.out} ({e})")
            report.fatal = True

    if not report.fatal:
        lock = acquire_lock(cfg.out)
        if lock is LOCK_HELD:
            log("another run in progress; exiting")
            report.lock_held = True
        else:
            try:
                api = Api(cfg.api_base_url, cfg.admin_key, cfg.timeout)
                groups, results = discover_and_group(
                    api, cfg.products, cfg.serials, cfg.since, cfg.until
                )
                report.products = [results[p] for p in cfg.products]
                for (product, serial, machine_id), rows in sorted(groups.items()):
                    results[product].machines_seen += 1
                    pull_machine(
                        api, product, serial, machine_id, rows, cfg.out,
                        results[product], rebuild=cfg.rebuild_manifests, quiet=cfg.quiet,
                    )
            finally:
                release_lock(lock)

    report.finished_utc = now_iso()
    if report.fatal:
        print("fleet-backup-pull: fatal — see message above", file=sys.stderr)
    elif report.lock_held:
        print(f"fleet-backup-pull  {report.started_utc}  another run in progress — nothing done")
    else:
        if not report.products:
            report.products = [ProductResult(product=p) for p in cfg.products]
        print_summary(report)
    return report.exit_code


if __name__ == "__main__":
    raise SystemExit(main())
