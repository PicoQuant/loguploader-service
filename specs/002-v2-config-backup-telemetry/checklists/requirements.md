# Specification Quality Checklist: V2 — Config Backup & Device Telemetry

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-09-08
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs) — API surface is a stated
      external dependency, not an implementation choice; no language/framework named
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

**Resolved during specification (user, 2026-09-08):**
- Instrument logs are no longer uploaded (FR-001).
- Transport: PicoQuant Telemetry API, `https://api.picoquant.com` (FR-020).
- Auth: single fleet-wide **non-expiring** token per product, serial-in-body, injected at
  build from a secret, rotatable; no per-machine provisioning (FR-020–FR-026, User Story 4).
- Config backup goes to a **dedicated backup endpoint** (to be added to the API), not JSON
  telemetry documents (FR-017).
- Two products initially: **Luminosa** and **Solira**, via **separate per-product builds**
  (FR-002a–FR-002d); the v1→v2 upgrade must deliver the right build per machine.
- Platform: Windows only; unattended service; delivered via `specs/001-v2-remote-upgrade`.

**Live API check (`https://api.picoquant.com/docs`, 2026-09-08):** no non-expiring token
today (mint TTL capped at 7 days), no backup endpoint today, telemetry submission currently
appears to require `X-ADMIN-API-KEY` in the published spec — all three are covered by the
"backend changes this spec depends on" list and must be confirmed before `/speckit-plan`
finalizes the transport design.

**Watched-file set (FR-008)** — resolved for Luminosa from the v1 source (`loguploader.py`):
`PQDevice.db` + `PQDevice.conf` under `C:\Program Files\PicoQuant\Luminosa\`, plus
`C:\ProgramData\PicoQuant\Luminosa\*.xml` and `...\UserSettings\*.xml`. Logs
(`Logs\*.pqlog`, `LaserPower.log`) excluded. "Everything but the logs."

**Deferred (tracked in spec "Open Items", does not block planning):**
1. Confirm the **Solira** paths/filenames and serial-file location against a real Solira
   install before the Solira build ships. Working assumption: Luminosa layout with `Solira`
   swapped in. Isolated to the Solira per-product build.

**Implementation:** planned as a compiled single-binary **Rust** Windows service
(`plan.md` + research/data-model/contracts). Constitution amended to **v1.3.0** — Principle V
allows a compiled binary; Build section adds the staged-rollout / beta-channel mandate.

**Amendment 2026-09-08 — release channels** (user decision): added FR-002e/FR-002f, `channel`
in FR-004 + the heartbeat schema, SC-008b. Build carries a compiled-in `stable`/`beta`
channel; CI matrix is product × channel (4 artifacts); the agent reports its channel in every
heartbeat. The channel-aware updater + beta→stable promotion gate live in `specs/001`.
