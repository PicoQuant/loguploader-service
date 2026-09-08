#!/usr/bin/env python3
"""Unit tests for fleet_backup_pull.py — HTTP faked, no network. Spec 004 US1.

    python -m unittest discover -s tools -p 'test_*.py'
"""

from __future__ import annotations

import base64
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fleet_backup_pull as fbp  # noqa: E402


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def make_row(rid, *, content=b"data", serial="SN-1", machine="M-1", file_key="pqdevice_conf",
             source=r"C:\Program Files\PicoQuant\Luminosa\PQDevice.conf",
             received="2026-09-08T12:00:00Z", product="luminosa", corrupt_sha=False):
    """A backup-list row. `content` bytes are carried in `_content_b64` for the FakeApi;
    the real tool ignores unknown keys."""
    digest = sha(content if not corrupt_sha else b"something else")
    return {
        "id": rid,
        "product_key": product,
        "instrument_serial": serial,
        "machine_id": machine,
        "file_key": file_key,
        "source_path": source,
        "content_sha256": digest,
        "size_bytes": len(content),
        "received_at": received,
        "file_mtime": "2026-06-01T00:00:00Z",
        "agent_version": "2.0.0-beta.3",
        "_content_b64": base64.b64encode(content).decode(),
    }


class FakeApi:
    """Serves canned pages + content extracted from the rows; records download calls."""

    def __init__(self, pages_by_product=None, *, auth_errors=(), list_errors=(), drop_ids=()):
        self.pages = pages_by_product or {}          # product -> list[list[row]]
        self.auth_errors = set(auth_errors)
        self.list_errors = set(list_errors)
        self.drop_ids = set(drop_ids)                # ids the backend has "pruned"
        self.download_calls: list[str] = []
        self._content = {}
        for pages in self.pages.values():
            for page in pages:
                for r in page:
                    self._content[r["id"]] = base64.b64decode(r["_content_b64"])

    def list_backups(self, product, *, instrument_serial=None, since=None, until=None):
        if product in self.auth_errors:
            raise fbp.ApiAuthError(403)
        if product in self.list_errors:
            raise fbp.ApiError("boom")
        for page in self.pages.get(product, []):
            for r in page:
                if instrument_serial and r.get("instrument_serial") != instrument_serial:
                    continue
                yield {k: v for k, v in r.items() if k != "_content_b64"}

    def download(self, product, backup_id):
        self.download_calls.append(backup_id)
        if backup_id in self.drop_ids or backup_id not in self._content:
            raise fbp.ApiNotFound()
        return self._content[backup_id]


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="fbp_test_"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def machine_dir(self, product="luminosa", serial="SN-1", machine="M-1"):
        return self.tmp / product / serial / machine

    def run_pull(self, api, products=("luminosa",), serials=(), rebuild=False):
        report = fbp.RunReport(started_utc=fbp.now_iso())
        groups, results = fbp.discover_and_group(api, list(products), list(serials), None, None)
        report.products = [results[p] for p in products]
        for (product, serial, machine_id), rows in sorted(groups.items()):
            results[product].machines_seen += 1
            fbp.pull_machine(api, product, serial, machine_id, rows, self.tmp,
                             results[product], rebuild=rebuild, quiet=True)
        report.finished_utc = fbp.now_iso()
        return report


# --------------------------------------------------------------------------- T015


class TestRelPathAndFilename(Base):
    def test_rel_path_derivation(self):
        self.assertEqual(
            str(fbp.rel_path(r"C:\ProgramData\PicoQuant\Luminosa\LastKnownGood.xml", "settings/x.xml")),
            "ProgramData/PicoQuant/Luminosa/LastKnownGood.xml")
        self.assertEqual(
            str(fbp.rel_path("ProgramData/PicoQuant/Luminosa/x.xml", "k")),
            "ProgramData/PicoQuant/Luminosa/x.xml")
        self.assertEqual(str(fbp.rel_path(r"C:\a\..\b\c.xml", "k")), "a/b/c.xml")
        self.assertEqual(str(fbp.rel_path("", "settings/chromophorelist.xml")),
                         "settings/chromophorelist.xml")
        self.assertTrue(str(fbp.rel_path("", "")))  # never empty

    def test_version_filename(self):
        self.assertEqual(
            fbp.version_filename("2026-09-08T12:52:52.004118Z", "b72cde42" + "0" * 56),
            "2026-09-08T12-52-52Z__b72cde42.bak")
        self.assertEqual(
            fbp.version_filename("2026-09-08T12:52:52Z", "aa" * 32),
            "2026-09-08T12-52-52Z__aaaaaaaa.bak")


# --------------------------------------------------------------------------- T016


class TestCommitArtifact(Base):
    def test_good_bytes_written_no_part_left(self):
        md = self.machine_dir()
        r = make_row("id-1", content=b"hello")
        e = fbp.commit_artifact(md, fbp.rel_path(r["source_path"], r["file_key"]), r, b"hello")
        self.assertIsInstance(e, fbp.ManifestEntry)
        self.assertEqual((md / e.version_file).read_bytes(), b"hello")
        self.assertFalse(list(md.rglob("*.part")))

    def test_digest_mismatch_not_written(self):
        md = self.machine_dir()
        r = make_row("id-1", content=b"hello")
        result = fbp.commit_artifact(md, fbp.rel_path(r["source_path"], r["file_key"]), r, b"TAMPERED")
        self.assertIsInstance(result, fbp.Failure)
        self.assertEqual(result.category, fbp.FAILURE_DIGEST)
        self.assertFalse(list(md.rglob("*.bak")))

    def test_existing_version_not_overwritten(self):
        md = self.machine_dir()
        r = make_row("id-1", content=b"v1")
        rel = fbp.rel_path(r["source_path"], r["file_key"])
        e1 = fbp.commit_artifact(md, rel, r, b"v1")
        (md / e1.version_file).write_bytes(b"SENTINEL")
        e2 = fbp.commit_artifact(md, rel, r, b"v1")
        self.assertIsInstance(e2, fbp.ManifestEntry)
        self.assertEqual((md / e1.version_file).read_bytes(), b"SENTINEL")


# --------------------------------------------------------------------------- T017


class TestManifest(Base):
    def test_roundtrip(self):
        md = self.machine_dir()
        md.mkdir(parents=True)
        m = fbp.Manifest(product="luminosa", instrument_serial="SN-1", machine_id="M-1")
        r = make_row("id-1", content=b"x")
        m.artifacts.append(fbp.commit_artifact(md, fbp.rel_path(r["source_path"], r["file_key"]), r, b"x"))
        fbp.save_manifest(md, m)
        back = fbp.load_manifest(md, "luminosa", "SN-1", "M-1")
        self.assertEqual(back.ids(), {"id-1"})
        self.assertEqual(back.schema_version, fbp.SCHEMA_VERSION)

    def test_corrupt_future_missing_all_rebuild(self):
        md = self.machine_dir()
        r = make_row("id-1", content=b"payload")
        fbp.commit_artifact(md, fbp.rel_path(r["source_path"], r["file_key"]), r, b"payload")
        mp = md / "manifest.json"

        for bad in ("{ not json", json.dumps({"schema_version": 2, "artifacts": []})):
            mp.write_text(bad, encoding="utf-8")
            rebuilt = fbp.load_manifest(md, "luminosa", "SN-1", "M-1")
            self.assertEqual(len(rebuilt.artifacts), 1)
            self.assertTrue(rebuilt.artifacts[0].is_latest)
            self.assertIsNone(rebuilt.artifacts[0].id)

        mp.unlink()
        self.assertEqual(len(fbp.load_manifest(md, "luminosa", "SN-1", "M-1").artifacts), 1)


# --------------------------------------------------------------------------- T018


class TestLock(Base):
    def test_contention_returns_lock_held(self):
        first = fbp.acquire_lock(self.tmp)
        self.assertIsInstance(first, fbp.Lock)
        self.assertIs(fbp.acquire_lock(self.tmp), fbp.LOCK_HELD)
        fbp.release_lock(first)
        third = fbp.acquire_lock(self.tmp)
        self.assertIsInstance(third, fbp.Lock)
        fbp.release_lock(third)

    def test_stale_lock_reclaimed(self):
        self.tmp.mkdir(parents=True, exist_ok=True)
        lp = self.tmp / ".fleet-backup.lock"
        lp.write_text(json.dumps({"pid": 999999, "host": "old", "started_utc": "2000-01-01T00:00:00Z"}))
        old = time.time() - fbp.STALE_LOCK_SECS - 10
        os.utime(lp, (old, old))
        got = fbp.acquire_lock(self.tmp)
        self.assertIsInstance(got, fbp.Lock)
        fbp.release_lock(got)


# --------------------------------------------------------------------------- T014, T020, T021, T024, T025


class TestPull(Base):
    def test_incremental_skip_by_manifest_id(self):
        r1 = make_row("id-1", content=b"conf v1")
        api = FakeApi({"luminosa": [[r1]]})
        self.run_pull(api)
        self.assertEqual(api.download_calls, ["id-1"])
        self.run_pull(api)
        self.assertEqual(api.download_calls, ["id-1"], "no re-download on the second run")

    def test_incremental_skip_by_disk_when_manifest_rebuilt(self):
        r1 = make_row("id-1", content=b"conf v1")
        api = FakeApi({"luminosa": [[r1]]})
        self.run_pull(api)
        (self.machine_dir() / "manifest.json").unlink()  # force rebuild (ids lost)
        self.run_pull(api, rebuild=True)
        self.assertEqual(api.download_calls, ["id-1"], "on-disk .bak short-circuits the download")

    def test_paging(self):
        def grp(prefix, n, ext):
            return [make_row(f"{prefix}{i}", file_key=f"settings/{prefix}{i}.{ext}",
                             source=fr"C:\ProgramData\PicoQuant\Luminosa\{prefix}{i}.{ext}",
                             content=f"{prefix}{i}".encode()) for i in range(n)]
        a, b, c = grp("a", fbp.PAGE, "xml"), grp("b", fbp.PAGE, "xml"), grp("c", 1, "xml")
        api = FakeApi({"luminosa": [a, b, c]})
        self.run_pull(api)
        want = {r["id"] for r in a + b + c}
        self.assertEqual(set(api.download_calls), want)
        self.assertEqual(len(api.download_calls), len(want), "each row fetched exactly once")

    def test_append_only_and_mirrored_latest(self):
        old, new = b"old", b"new"
        r_old = make_row("id-old", content=old, received="2026-09-01T00:00:00Z")
        r_new = make_row("id-new", content=new, received="2026-09-08T00:00:00Z")
        api = FakeApi({"luminosa": [[r_old, r_new]]})
        self.run_pull(api)
        md = self.machine_dir()
        baks = sorted((md / "_versions").rglob("*.bak"))
        self.assertEqual(len(baks), 2)
        older = next(b for b in baks if b.name.startswith("2026-09-01"))
        older_bytes = older.read_bytes()
        mirror = md / "Program Files" / "PicoQuant" / "Luminosa" / "PQDevice.conf"
        self.assertEqual(mirror.read_bytes(), new)

        self.run_pull(api)  # re-run: older version must be untouched
        self.assertEqual(older.read_bytes(), older_bytes)
        self.assertEqual(len(list((md / "_versions").rglob("*.bak"))), 2)

    def test_machine_id_folder_separates_same_serial(self):
        r1 = make_row("id-1", machine="MACH-A", content=b"m1")
        r2 = make_row("id-2", machine="MACH-B", content=b"m2")
        api = FakeApi({"luminosa": [[r1, r2]]})
        self.run_pull(api)
        self.assertTrue((self.tmp / "luminosa" / "SN-1" / "MACH-A" / "manifest.json").is_file())
        self.assertTrue((self.tmp / "luminosa" / "SN-1" / "MACH-B" / "manifest.json").is_file())

    def test_pruned_download_is_not_a_failure(self):
        r = make_row("id-gone", content=b"x")
        api = FakeApi({"luminosa": [[r]]}, drop_ids=["id-gone"])
        report = self.run_pull(api)
        self.assertEqual(report.products[0].pruned, 1)
        self.assertEqual(report.products[0].artifacts_failed, 0)
        self.assertEqual(report.exit_code, 0)

    def test_stale_layout_file_blocks_machine_cleanly(self):
        # a pre-machine-id ("seed") archive left <serial> as a *file*
        (self.tmp / "luminosa" / "SN-1").parent.mkdir(parents=True, exist_ok=True)
        (self.tmp / "luminosa" / "SN-1").write_bytes(b"stale seed mirror")
        api = FakeApi({"luminosa": [[make_row("id-1", content=b"conf")]]})
        report = self.run_pull(api)
        pr = report.products[0]
        self.assertEqual(pr.skipped_machines, 1)
        self.assertEqual(pr.artifacts_added, 0)
        self.assertEqual(pr.artifacts_failed, 0)
        self.assertEqual(report.exit_code, 1)
        self.assertEqual(api.download_calls, [])  # nothing attempted for that machine


# --------------------------------------------------------------------------- T019


class TestProductAccess(Base):
    def _report(self, api, products):
        report = fbp.RunReport(started_utc=fbp.now_iso())
        groups, results = fbp.discover_and_group(api, list(products), [], None, None)
        report.products = [results[p] for p in products]
        for (product, serial, machine_id), rows in sorted(groups.items()):
            results[product].machines_seen += 1
            fbp.pull_machine(api, product, serial, machine_id, rows, self.tmp,
                             results[product], rebuild=False, quiet=True)
        return report, results

    def test_one_inaccessible_other_still_archived(self):
        api = FakeApi({"luminosa": [[make_row("id-1", content=b"x")]]}, auth_errors=["solira"])
        report, results = self._report(api, ["luminosa", "solira"])
        self.assertTrue(results["luminosa"].accessible)
        self.assertFalse(results["solira"].accessible)
        self.assertEqual(results["luminosa"].artifacts_added, 1)
        self.assertEqual(report.exit_code, 0)

    def test_no_product_reachable_is_fatal(self):
        api = FakeApi({}, auth_errors=["luminosa", "solira"])
        report, _ = self._report(api, ["luminosa", "solira"])
        self.assertEqual(report.exit_code, 2)

    def test_list_error_is_partial(self):
        api = FakeApi({}, list_errors=["luminosa"])
        report, _ = self._report(api, ["luminosa"])
        self.assertEqual(report.exit_code, 1)


# --------------------------------------------------------------------------- T022, T023, T026, T027


class TestMainEndToEnd(Base):
    def _fake_main(self, api, argv, key="K"):
        real = fbp.Api
        fbp.Api = lambda *a, **k: api
        if key is not None:
            os.environ["EXPECTED_ADMIN_API_KEY"] = key
        else:
            os.environ.pop("EXPECTED_ADMIN_API_KEY", None)
        out, err = io.StringIO(), io.StringIO()
        try:
            with redirect_stdout(out), redirect_stderr(err):
                code = fbp.main(argv)
        finally:
            fbp.Api = real
            os.environ.pop("EXPECTED_ADMIN_API_KEY", None)
        return code, out.getvalue(), err.getvalue()

    def _argv(self, sub):
        return ["--out", str(self.tmp / sub), "--product", "luminosa", "--env", str(self.tmp / "no-env")]

    def test_nothing_new_exit_0_and_idempotent(self):
        api = FakeApi({"luminosa": [[make_row("id-1", content=b"conf")]]})
        code, out, _ = self._fake_main(api, self._argv("arch"))
        self.assertEqual(code, 0)
        self.assertIn("added=1", out)
        code2, out2, _ = self._fake_main(api, self._argv("arch"))
        self.assertEqual(code2, 0)
        self.assertIn("added=0", out2)

    def test_missing_key_is_exit_2(self):
        code, out, err = self._fake_main(FakeApi({}), self._argv("a"), key=None)
        self.assertEqual(code, 2)
        self.assertIn("EXPECTED_ADMIN_API_KEY", err)

    def test_download_error_is_exit_1(self):
        r_ok = make_row("id-ok", content=b"conf")
        r_bad = make_row("id-bad", file_key="settings/x.xml",
                         source=r"C:\ProgramData\PicoQuant\Luminosa\x.xml",
                         content=b"xdata", corrupt_sha=True)  # digest won't match
        api = FakeApi({"luminosa": [[r_ok, r_bad]]})
        code, out, _ = self._fake_main(api, self._argv("a"))
        self.assertEqual(code, 1)
        self.assertIn("failed=1", out)

    def test_lock_held_exits_0_without_writing(self):
        api = FakeApi({"luminosa": [[make_row("id-1", content=b"x")]]})
        arch = self.tmp / "a"
        arch.mkdir(parents=True)
        (arch / ".fleet-backup.lock").write_text(
            json.dumps({"pid": os.getpid(), "host": "h", "started_utc": fbp.now_iso()}))
        code, out, err = self._fake_main(api, ["--out", str(arch), "--product", "luminosa",
                                               "--env", str(self.tmp / "no-env")])
        self.assertEqual(code, 0)
        self.assertIn("another run in progress", err)
        self.assertEqual(api.download_calls, [])
        self.assertFalse((arch / "luminosa").exists())

    def test_admin_key_never_leaks(self):
        sentinel = "SENTINEL-ADMIN-KEY-9f8e7d6c5b4a"
        api = FakeApi({"luminosa": [[make_row("id-1", content=b"conf-bytes")]]})
        arch = self.tmp / "arch"
        code, out, err = self._fake_main(api, ["--out", str(arch), "--product", "luminosa",
                                               "--env", str(self.tmp / "no-env")], key=sentinel)
        self.assertEqual(code, 0)
        self.assertNotIn(sentinel, out)
        self.assertNotIn(sentinel, err)
        for f in arch.rglob("*"):
            if f.is_file():
                self.assertNotIn(sentinel.encode(), f.read_bytes(), f"leak in {f}")


if __name__ == "__main__":
    unittest.main()
