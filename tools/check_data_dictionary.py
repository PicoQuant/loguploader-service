#!/usr/bin/env python3
"""Check this repo's shared semantic data dictionary for full field coverage (SC-013).

Spec: originated by specs/002-v2-config-backup-telemetry/spec.md FR-036-FR-040,
plan.md/research.md D15; generalized to a repo-wide dictionary (docs/data-dictionary/)
under constitution Principle VII, covering every spec's output documents, not just one.

Walks the structural JSON Schemas that describe what each covered document looks
like (a spec's own contracts/*.schema.json) and combines that with any fixed,
non-JSON part lists (e.g. a multipart body) to get the full set of fields those
documents can carry. Every one of those fields MUST resolve, via
docs/data-dictionary/field-mappings.json, to a concept defined in
docs/data-dictionary/semantic-model.json — this script is that check, not a
general JSON Schema validator. It has no third-party dependency (stdlib only),
matching Constitution V: it never links into the agent binary, it only reads the
contract/dictionary JSON already checked into the repo.

To cover a new document: add one entry to DOCUMENT_SOURCES below.

Exit 0: every field is mapped and every mapping resolves. Exit 1: prints each
offending pointer/id and exits non-zero (for CI, tools/test_check_data_dictionary.py).
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator

REPO_ROOT = Path(__file__).resolve().parent.parent
DICTIONARY_DIR = REPO_ROOT / "docs" / "data-dictionary"
SEMANTIC_MODEL = DICTIONARY_DIR / "semantic-model.json"
FIELD_MAPPINGS = DICTIONARY_DIR / "field-mappings.json"

SPEC_002_CONTRACTS = REPO_ROOT / "specs" / "002-v2-config-backup-telemetry" / "contracts"
SPEC_001_CONTRACTS = REPO_ROOT / "specs" / "001-v2-remote-upgrade" / "contracts"
SPEC_004_CONTRACTS = REPO_ROOT / "specs" / "004-fleet-backup-restore" / "contracts"

# contracts/backend-api.md SS2 (spec 002) — the backup submission is
# multipart/form-data, not JSON, so there is no schema file to walk; the part
# list is fixed and hardcoded here (kept in sync with backend-api.md by a
# human, per FR-040).
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


@dataclass(frozen=True)
class DocumentSource:
    """One document this dictionary must cover end to end.

    `fixed_pointers`: pointers not derivable from a JSON Schema (envelope
    fields wrapping a schema'd sub-object, or an entire non-JSON body).
    `schema_path` + `schema_prefix`: if set, every leaf pointer of the schema
    at `schema_path` is walked and prefixed with `schema_prefix` before being
    added to this document's required-pointer set.
    """

    doc_id: str
    fixed_pointers: tuple[str, ...] = ()
    schema_path: Path | None = None
    schema_prefix: str = ""


DOCUMENT_SOURCES: list[DocumentSource] = [
    DocumentSource(
        doc_id="v2.heartbeat_payload.v1",
        # heartbeat-payload.schema.json only describes the `payload` object
        # (see its own "description"); these envelope fields wrap it on the
        # wire per contracts/backend-api.md SS1 and are not JSON-Schema'd
        # separately. `/meta/schema` is the document's self-description tag
        # (doc.schema_id) — see semantic-model.json.
        fixed_pointers=("/measurement_type", "/measured_at", "/instrument_serial", "/meta/schema"),
        schema_path=SPEC_002_CONTRACTS / "heartbeat-payload.schema.json",
        schema_prefix="/payload",
    ),
    DocumentSource(
        doc_id="v2.backup_submission.v1",
        fixed_pointers=tuple(BACKUP_SUBMISSION_POINTERS),
    ),
    DocumentSource(
        doc_id="v2.local_state.v1",
        schema_path=SPEC_002_CONTRACTS / "local-state.schema.json",
    ),
    DocumentSource(
        doc_id="v2.upgrade_attempt.v1",
        # Same generic telemetry endpoint as the heartbeat, envelope fields
        # not separately schema'd; no `meta` field is sent for this one.
        fixed_pointers=("/measurement_type", "/measured_at", "/instrument_serial"),
        schema_path=SPEC_001_CONTRACTS / "upgrade-telemetry.schema.json",
        schema_prefix="/payload",
    ),
    DocumentSource(
        doc_id="v2.fleet_archive_manifest.v1",
        schema_path=SPEC_004_CONTRACTS / "manifest.schema.json",
    ),
    DocumentSource(
        doc_id="v2.fleet_archive_power_manifest.v1",
        schema_path=SPEC_004_CONTRACTS / "power-manifest.schema.json",
    ),
]

REQUIRED_CONCEPT_FIELDS = {"id", "datatype", "description", "confidence"}
VALID_DATATYPES = {
    "string",
    "enum",
    "integer",
    "boolean",
    "object",
    "iso8601-datetime",
    "date",
    "binary",
}
VALID_CONFIDENCE = {"confirmed", "uncertain"}


def _type_names(schema: dict) -> list[str]:
    t = schema.get("type")
    if t is None:
        return []
    return t if isinstance(t, list) else [t]


def _resolve_ref(ref: str, root: dict) -> dict:
    """Resolve a local `$ref` of the form `#/$defs/<name>` against `root`.

    Only local $defs refs are supported — every schema this tool walks is a
    single self-contained file, never a multi-file $ref chain.
    """
    if not ref.startswith("#/"):
        raise ValueError(f"unsupported $ref (not a local pointer): {ref!r}")
    node: Any = root
    for segment in ref[2:].split("/"):
        node = node[segment]
    return node


def _leaf_pointers(schema: dict, prefix: str, root: dict | None = None) -> Iterator[str]:
    """Yield every leaf JSON pointer under `schema`, rooted at `prefix`.

    An object with `properties` recurses per property. An object with only
    `additionalProperties` (a map, e.g. state.json's `files`) recurses once
    under a `*` wildcard segment. An array recurses under `*` into `items`.
    A `$ref` (local `#/$defs/<name>` only) resolves against `root` — the
    top-level schema passed to the first call — before continuing. Anything
    else (string/integer/boolean/enum, or a `[type, "null"]` leaf) is a leaf:
    yield `prefix` itself.
    """
    if root is None:
        root = schema
    if "$ref" in schema:
        yield from _leaf_pointers(_resolve_ref(schema["$ref"], root), prefix, root)
        return
    types = _type_names(schema)
    if "object" in types or "properties" in schema:
        props = schema.get("properties")
        if props:
            for name, subschema in props.items():
                yield from _leaf_pointers(subschema, f"{prefix}/{name}", root)
            return
        additional = schema.get("additionalProperties")
        if isinstance(additional, dict):
            yield from _leaf_pointers(additional, f"{prefix}/*", root)
            return
        # object with no properties and no (schema) additionalProperties: leaf
        yield prefix
        return
    if "array" in types:
        items = schema.get("items")
        if isinstance(items, dict):
            yield from _leaf_pointers(items, f"{prefix}/*", root)
            return
        yield prefix
        return
    yield prefix


def _load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def required_pointers(sources: list[DocumentSource] = DOCUMENT_SOURCES) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for src in sources:
        pointers = list(src.fixed_pointers)
        if src.schema_path is not None:
            schema = _load_json(src.schema_path)
            pointers.extend(f"{src.schema_prefix}{p}" for p in _leaf_pointers(schema, ""))
        result[src.doc_id] = pointers
    return result


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
    sources: list[DocumentSource] = DOCUMENT_SOURCES,
    field_mappings_path: Path = FIELD_MAPPINGS,
    semantic_model_path: Path = SEMANTIC_MODEL,
) -> int:
    required = required_pointers(sources)
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
