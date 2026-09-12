#!/usr/bin/env python3
"""Unit tests for check_data_dictionary.py — no network, no real schema files touched.

    python -m unittest discover -s tools -p 'test_*.py'
"""

from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_data_dictionary as cdd  # noqa: E402

REAL_CONTRACTS = Path(__file__).resolve().parent.parent / "specs" / "002-v2-config-backup-telemetry" / "contracts"
REAL_HEARTBEAT_SCHEMA = REAL_CONTRACTS / "heartbeat-payload.schema.json"
REAL_LOCAL_STATE_SCHEMA = REAL_CONTRACTS / "local-state.schema.json"
REAL_FIELD_MAPPINGS = REAL_CONTRACTS / "data-dictionary" / "field-mappings.json"
REAL_SEMANTIC_MODEL = REAL_CONTRACTS / "data-dictionary" / "semantic-model.json"


class Base(unittest.TestCase):
    def setUp(self):
        import tempfile

        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)

    def tearDown(self):
        self._tmpdir.cleanup()

    def write(self, name: str, data) -> Path:
        p = self.tmp / name
        p.write_text(json.dumps(data), encoding="utf-8")
        return p

    def run_main(self, **overrides):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = cdd.main(**overrides)
        return code, out.getvalue(), err.getvalue()


class TestRealDictionary(Base):
    """The checked-in dictionary itself must pass — this is the actual CI gate."""

    def test_real_files_pass(self):
        code, out, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertIn("OK", out)

    def test_real_semantic_model_entries_are_well_formed(self):
        semantic_model = json.loads(REAL_SEMANTIC_MODEL.read_text(encoding="utf-8"))
        errors = cdd.check_semantic_model(semantic_model)
        self.assertEqual(errors, [])


class TestMissingMapping(Base):
    def test_unmapped_pointer_fails_naming_it(self):
        heartbeat_schema = self.write(
            "heartbeat.schema.json",
            {
                "type": "object",
                "properties": {"machine_id": {"type": "string"}, "agent_version": {"type": "string"}},
            },
        )
        local_state_schema = self.write("local_state.schema.json", {"type": "object", "properties": {}})
        semantic_model = self.write(
            "semantic-model.json",
            {"identity.machine_id": {"id": "identity.machine_id", "datatype": "string",
                                      "description": "d", "confidence": "confirmed"}},
        )
        field_mappings = self.write(
            "field-mappings.json",
            {
                "schema_registry": {
                    "v2.heartbeat_payload.v1": {"/payload/machine_id": "identity.machine_id"},
                    "v2.backup_submission.v1": {p: "identity.machine_id" for p in cdd.BACKUP_SUBMISSION_POINTERS},
                    "v2.local_state.v1": {},
                }
            },
        )
        code, out, err = self.run_main(
            heartbeat_schema_path=heartbeat_schema,
            local_state_schema_path=local_state_schema,
            field_mappings_path=field_mappings,
            semantic_model_path=semantic_model,
        )
        self.assertEqual(code, 1)
        self.assertIn("/payload/agent_version", err)
        self.assertNotIn("OK", out)


class TestDanglingConceptId(Base):
    def test_mapping_to_undefined_concept_fails_naming_it(self):
        heartbeat_schema = self.write(
            "heartbeat.schema.json", {"type": "object", "properties": {"machine_id": {"type": "string"}}}
        )
        local_state_schema = self.write("local_state.schema.json", {"type": "object", "properties": {}})
        semantic_model = self.write("semantic-model.json", {})
        field_mappings = self.write(
            "field-mappings.json",
            {
                "schema_registry": {
                    "v2.heartbeat_payload.v1": {"/payload/machine_id": "identity.does_not_exist"},
                    "v2.backup_submission.v1": {p: "identity.does_not_exist" for p in cdd.BACKUP_SUBMISSION_POINTERS},
                    "v2.local_state.v1": {},
                }
            },
        )
        code, _, err = self.run_main(
            heartbeat_schema_path=heartbeat_schema,
            local_state_schema_path=local_state_schema,
            field_mappings_path=field_mappings,
            semantic_model_path=semantic_model,
        )
        self.assertEqual(code, 1)
        self.assertIn("identity.does_not_exist", err)


class TestMalformedConceptEntry(Base):
    def test_missing_required_field_is_reported(self):
        errors = cdd.check_semantic_model({"identity.machine_id": {"id": "identity.machine_id", "datatype": "string"}})
        self.assertTrue(any("missing field" in e for e in errors))

    def test_bad_datatype_is_reported(self):
        errors = cdd.check_semantic_model(
            {"identity.machine_id": {"id": "identity.machine_id", "datatype": "float",
                                      "description": "d", "confidence": "confirmed"}}
        )
        self.assertTrue(any("invalid datatype" in e for e in errors))

    def test_bad_confidence_is_reported(self):
        errors = cdd.check_semantic_model(
            {"identity.machine_id": {"id": "identity.machine_id", "datatype": "string",
                                      "description": "d", "confidence": "maybe"}}
        )
        self.assertTrue(any("invalid confidence" in e for e in errors))


class TestLeafPointerWalk(Base):
    def test_map_wildcard_and_array_wildcard(self):
        schema = {
            "type": "object",
            "properties": {
                "files": {"type": "object", "additionalProperties": {"type": "object",
                                                                       "properties": {"sha": {"type": "string"}}}},
                "items_list": {"type": "array", "items": {"type": "object",
                                                           "properties": {"k": {"type": "string"}}}},
                "nullable": {"type": ["string", "null"]},
            },
        }
        pointers = sorted(cdd._leaf_pointers(schema, ""))
        self.assertEqual(pointers, ["/files/*/sha", "/items_list/*/k", "/nullable"])


if __name__ == "__main__":
    unittest.main()
