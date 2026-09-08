# Feature Specification: Backend API Support for the v2 Fleet Agent

**Feature Branch**: `003-backend-api-support`

**Created**: 2026-09-08

**Status**: Draft

**Input**: User request: "create documentation that allows an agent to add the functionality
to the backend. This documentation / agent commands is a feature."

## Overview

The v2 fleet agent (`specs/002-v2-config-backup-telemetry`) depends on three capabilities that
`https://api.picoquant.com` does not have today:

1. A **per-product, non-expiring, fleet-wide submission token** (not instrument-bound; serial
   supplied in the request body; multiple valid values for rotation).
2. A **configuration-file backup endpoint** with durable storage and admin retrieval, exempt
   from the 30-day telemetry pruning.
3. **`solira`** enabled as a product bucket alongside `luminosa`.

This feature's deliverable is **the documentation an implementing agent follows to make those
backend changes**: [`backend-changes.md`](./backend-changes.md). It is a self-contained
implementation brief — recon steps, exact API/data-model/config contracts, a phased execution
plan, and an acceptance checklist — written to be handed to a coding agent working in the
backend repository.

This repository (`loguploader-service`) holds the **client** side. The backend lives in a
separate repository; this feature produces only the brief, not backend code.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An agent implements the backend changes from the brief alone (Priority: P1)

A coding agent is given `backend-changes.md` and access to the backend repository. It can
establish current state, make the changes in the described order, and verify each one, without
needing to come back for missing information about the intended contract.

**Why this priority**: The brief exists to unblock the v2 agent work. If it is ambiguous or
incomplete, the backend work stalls or diverges from what the client expects.

**Independent Test**: Give the brief and backend repo to an agent (or reviewer) unfamiliar
with this conversation; confirm they can produce a plan and identify every endpoint, field,
table column, and env var to add, and every acceptance test to write, with no open questions
about the intended behaviour.

**Acceptance Scenarios**:

1. **Given** the brief, **When** an agent reads it, **Then** it can list every new/changed
   endpoint with method, path, auth, request fields, and response fields.
2. **Given** the brief, **When** an agent reads it, **Then** it can list every new table
   column and every new environment variable with its default.
3. **Given** the brief, **When** an agent finishes, **Then** the "Acceptance checklist"
   section can be run item by item to confirm done.
4. **Given** the brief, **When** the backend's actual structure differs from the brief's
   assumptions, **Then** the brief's recon phase tells the agent to reconcile and how.

### User Story 2 - The backend and client agree on one contract (Priority: P1)

The request/response shapes, header names, error codes, and the fleet-token semantics in the
brief match exactly what `specs/002-v2-config-backup-telemetry` requires of the agent.

**Why this priority**: Two agents working from one shared contract only converge if the
contract is truly shared.

**Acceptance Scenarios**:

1. **Given** both specs, **When** compared, **Then** every backend dependency listed in spec
   002 (fleet token, backup endpoint, serial-in-body, no-admin-key, `solira` bucket) is
   specified in the brief with a concrete shape.
2. **Given** the brief's "Client contract" appendix, **When** the v2 agent implements against
   it, **Then** no field or error code is left to guess.

### Edge Cases

- The brief's assumed backend stack (FastAPI + async SQLAlchemy + env-var config, tables
  `telemetry_records` / `telemetry_tokens` / `technicians`) is wrong or out of date — recon
  phase must catch this before any change.
- The live OpenAPI only declares `X-ADMIN-API-KEY` as a security scheme, but the prose docs
  describe `X-TELEMETRY-TOKEN` — the brief must tell the agent to determine which is real in
  code and reconcile the OpenAPI.
- The backend already stores blobs somewhere (object storage) — the brief must not force
  DB-blob storage if a storage abstraction already exists.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The deliverable MUST be a single Markdown document in this feature directory that
  an agent can execute against the backend repository.
- **FR-002**: The document MUST begin with a recon phase: which files to read, the command to
  fetch and inspect the live OpenAPI, and how to reconcile differences from its assumptions.
- **FR-003**: The document MUST specify, for each new or changed endpoint: method, path,
  authentication, required and optional request fields with types and limits, success
  response fields, and error status codes.
- **FR-004**: The document MUST specify the fleet-token model: per-product, non-expiring,
  not instrument-bound, serial taken from the request body, several values valid at once for
  rotation, configured without a database migration where possible, and never equal to the
  admin key.
- **FR-005**: The document MUST specify the backup storage: new table/columns, a content
  integrity check, de-duplication of identical content, a size limit, and exclusion from the
  telemetry retention/prune loop.
- **FR-006**: The document MUST specify admin retrieval: list backups by instrument and file,
  fetch the latest, and download raw content.
- **FR-007**: The document MUST list every new environment variable with a default and every
  new table column with a type.
- **FR-008**: The document MUST give a phased execution plan where each phase is independently
  testable and ordered so earlier phases unblock client development.
- **FR-009**: The document MUST include an acceptance checklist and a test plan (which test
  files, which cases).
- **FR-010**: The document MUST include a "Client contract" appendix restating exactly what
  `specs/002-v2-config-backup-telemetry` sends and expects, so both sides share one contract.
- **FR-011**: The document MUST preserve backward compatibility: existing instrument-bound
  tokens, the TOTP session flow, and current telemetry submissions keep working unchanged.
- **FR-012**: The document MUST NOT contain real secrets, tokens, or keys.

### Key Entities

- **Backend Change Brief** (`backend-changes.md`): the deliverable. Sections: recon, the three
  changes with contracts, data model, configuration, backward compatibility, phased plan, test
  plan, acceptance checklist, client-contract appendix.

## Success Criteria *(mandatory)*

- **SC-001**: An agent or reviewer unfamiliar with this conversation can, from the brief alone,
  enumerate 100% of new/changed endpoints, fields, columns, and env vars.
- **SC-002**: Every backend dependency in `specs/002-v2-config-backup-telemetry` maps to a
  concrete contract in the brief (0 unspecified).
- **SC-003**: The phased plan has each phase independently testable; the client can start
  integrating after phase 2 (fleet-token auth) without waiting for backups.
- **SC-004**: Following the brief changes nothing about existing instrument-bound tokens, the
  TOTP flow, or current telemetry submissions (backward compatible).
- **SC-005**: Zero secrets present in the document.

## Assumptions

- The backend is a Python async web service (FastAPI/Starlette + `uvicorn`), using async
  SQLAlchemy, env-var configuration, and startup-time additive migrations
  (`tlayer.db.apply_additive_migrations`), with tables `telemetry_records`,
  `telemetry_archive`, `telemetry_tokens`, `technicians`, and a daily `archive_and_prune`
  loop — inferred from the telemetry API prose docs. The brief's recon phase verifies this.
- The implementing agent has access to the backend repository and can run its test suite.
- `X-TELEMETRY-TOKEN` handling largely exists already (per the prose docs) even though the
  published OpenAPI only declares `X-ADMIN-API-KEY`.

## Dependencies

- `specs/002-v2-config-backup-telemetry` — the client requirements the brief must satisfy.
- Read access to the live API at `https://api.picoquant.com/openapi.json`.
- The backend repository (separate from this repo).

## Out of Scope

- Writing or running the backend code (done by the implementing agent in the backend repo).
- Backend hosting, deployment, and secret management.
- Any client-side change (covered by spec 002).
