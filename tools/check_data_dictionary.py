#!/usr/bin/env python3
"""Check the v2 agent's semantic data dictionary for full field coverage (SC-013).

Spec: specs/002-v2-config-backup-telemetry/spec.md FR-036-FR-040, plan.md/research.md D15.

Walks the structural JSON Schemas that describe what v2 submits/persists
(heartbeat payload, local state) and combines that with the fixed, non-JSON
backup-submission part list (contracts/backend-api.md SS2) to get the full set
of fields v2's wire/persisted documents can carry. Every one of those fields
MUST resolve, via field-mappings.json, to a concept defined in
semantic-model.json — this script is that check, not a general JSON Schema
validator. It has no third-party dependency (stdlib only), matching
Constitution V: it never links into the agent binary, it only reads the
contract/dictionary JSON already checked into the repo.

Exit 0: every field is mapped and every mapping resolves. Exit 1: prints each
offending pointer/id and exits non-zero (for CI, tools/test_check_data_dictionary.py).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, Iterator

CONTRACTS_DIR = Path(__file__).resolve().parent.parent / "specs" / "002-v2-config-backup-telemetry" / "contracts"
DICTIONARY_DIR = CONTRACTS_DIR / "data-dictionary"

HEARTBEAT_SCHEMA = CONTRACTS_DIR / "heartbeat-payload.schema.json"
LOCAL_STATE_SCHEMA = CONTRACTS_DIR / "local-state.schema.json"
SEMANTIC_MODEL = DICTIONARY_DIR / "semantic-model.json"
FIELD_MAPPINGS = DICTIONARY_DIR / "field-mappings.json"

# The heartbeat-payload.schema.json file only describes the `payload` object
# (see its own "description"); these three envelope fields wrap it on the
# wire per contracts/backend-api.md SS1 and are not JSON-Schema'd separately.
HEARTBEAT_ENVELOPE_POINTERS = ["/measurement_type", "/measured_at", "/instrument_serial"]

# contracts/backend-api.md SS2 — the backup submission is multipart/form-data,
# not JSON, so there is no schema file to walk; the part list is fixed and
# hardcoded here (kept in sync with backend-api.md by a human, per FR-040).
BACKUP_SUBMISSION_POINTERS = [
    "/content",
    "/instrument_serial",
    "/machine_id",
    "/file_key",
    "/source_path",
    "/content_sha256",
    "/file_mtime",
    "/agent_version",
    "/client_timestamp",
]

REQUIRED_CONCEPT_FIELDS = {"id", "datatype", "description", "confidence"}
VALID_DATATYPES = {"string", "enum", "integer", "boolean", "object", "iso8601-datetime", "date", "binary"}
VALID_CONFIDENCE = {"confirmed", "uncertain"}


def _type_names(schema: dict) -> list[str]:
    t = schema.get("type")
    if t is None:
        return []
    return t if isinstance(t, list) else [t]


def _leaf_pointers(schema: dict, prefix: str) -> Iterator[str]:
    """Yield every leaf JSON pointer under `schema`, rooted at `prefix`.

    An object with `properties` recurses per property. An object with only
    `additionalProperties` (a map, e.g. state.json's `files`) recurses once
    under a `*` wildcard segment. An array recurses under `*` into `items`.
    Anything else (string/integer/boolean/enum, or a `[type, "null"]` leaf)
    is a leaf: yield `prefix` itself.
    """
    types = _type_names(schema)
    if "object" in types or "properties" in schema:
        props = schema.get("properties")
        if props:
            for name, subschema in props.items():
                yield from _leaf_pointers(subschema, f"{prefix}/{name}")
            return
        additional = schema.get("additionalProperties")
        if isinstance(additional, dict):
            yield from _leaf_pointers(additional, f"{prefix}/*")
            return
        # object with no properties and no (schema) additionalProperties: leaf
        yield prefix
        return
    if "array" in types:
        items = schema.get("items")
        if isinstance(items, dict):
            yield from _leaf_pointers(items, f"{prefix}/*")
            return
        yield prefix
        return
    yield prefix


def _load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def required_pointers(
    heartbeat_schema_path: Path = HEARTBEAT_SCHEMA,
    local_state_schema_path: Path = LOCAL_STATE_SCHEMA,
) -> dict[str, list[str]]:
    heartbeat_schema = _load_json(heartbeat_schema_path)
    local_state_schema = _load_json(local_state_schema_path)

    heartbeat_pointers = list(HEARTBEAT_ENVELOPE_POINTERS)
    for p in _leaf_pointers(heartbeat_schema, ""):
        heartbeat_pointers.append(f"/payload{p}")

    local_state_pointers = list(_leaf_pointers(local_state_schema, ""))

    return {
        "v2.heartbeat_payload.v1": heartbeat_pointers,
        "v2.backup_submission.v1": list(BACKUP_SUBMISSION_POINTERS),
        "v2.local_state.v1": local_state_pointers,
    }


def check_semantic_model(semantic_model: dict) -> list[str]:
    """Structural sanity check of semantic-model.json entries (a light stand-in
    for full JSON-Schema validation against semantic-model.schema.json — see
    module docstring for why this script doesn't pull in a jsonschema dep)."""
    errors = []
    for concept_id, entry in semantic_model.items():
        if not isinstance(entry, dict):
            errors.append(f"semantic-model.json: {concept_id!r} is not an object")
            continue
        missing = REQUIRED_CONCEPT_FIELDS - entry.keys()
        if missing:
            errors.append(f"semantic-model.json: {concept_id!r} missing field(s) {sorted(missing)}")
        if entry.get("id") != concept_id:
            errors.append(f"semantic-model.json: {concept_id!r} has id {entry.get('id')!r} (must match its own key)")
        if entry.get("datatype") not in VALID_DATATYPES:
            errors.append(f"semantic-model.json: {concept_id!r} has invalid datatype {entry.get('datatype')!r}")
        if entry.get("confidence") not in VALID_CONFIDENCE:
            errors.append(f"semantic-model.json: {concept_id!r} has invalid confidence {entry.get('confidence')!r}")
    return errors


def check_coverage(required: dict[str, list[str]], field_mappings: dict, semantic_model: dict) -> list[str]:
    errors = []
    registry = field_mappings.get("schema_registry", {})
    for doc_id, pointers in required.items():
        doc_mapping = registry.get(doc_id)
        if doc_mapping is None:
            errors.append(f"field-mappings.json: missing schema_registry entry for {doc_id!r}")
            continue
        for pointer in pointers:
            if pointer not in doc_mapping:
                errors.append(f"{doc_id}: {pointer!r} has no entry in field-mappings.json")
                continue
            concept_id = doc_mapping[pointer]
            if concept_id not in semantic_model:
                errors.append(f"{doc_id}: {pointer!r} maps to undefined concept {concept_id!r}")
    return errors


def main(
    heartbeat_schema_path: Path = HEARTBEAT_SCHEMA,
    local_state_schema_path: Path = LOCAL_STATE_SCHEMA,
    field_mappings_path: Path = FIELD_MAPPINGS,
    semantic_model_path: Path = SEMANTIC_MODEL,
) -> int:
    required = required_pointers(heartbeat_schema_path, local_state_schema_path)
    field_mappings = _load_json(field_mappings_path)
    semantic_model = _load_json(semantic_model_path)

    errors = check_semantic_model(semantic_model)
    errors += check_coverage(required, field_mappings, semantic_model)

    if errors:
        print(f"Data dictionary check FAILED ({len(errors)} issue(s)):", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    total = sum(len(p) for p in required.values())
    print(f"Data dictionary check OK: {total} fields across {len(required)} documents, all mapped.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
