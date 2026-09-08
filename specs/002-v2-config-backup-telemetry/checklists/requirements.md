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

**/speckit-analyze 2026-09-08 — MEDIUM findings remediated (F1–F4):**
- F1: FR-033 + Assumptions + SC-001 → one **cycle interval**, not two (matches plan D11).
- F2: "Current backend state" / "Backend changes" → rewritten as "Backend support (deployed
  and verified — v2.2.0-beta.2)". `solira` enablement remains the only open backend item.
- F3: 4-variant artifact naming (`pquploader-<product>[-beta].exe`, `… Setup.exe`) written
  into `contracts/cli.md`; T036/T044/T045 reference it. Flagged for spec 001: the v1 updater
  must become product-aware (stable release contains both products' installers).
- F4: FR-002f + SC-008b now define the per-heartbeat health signals (`cycle.ok`,
  `blocked_backups`, `last_failure_category`); "stopped/crash-loop" is derived in spec 001
  from heartbeat gaps. Added `last_failure_category` to the heartbeat schema + data-model.
- LOW findings F5–F10 left as-is (benign / acknowledged in the docs).
