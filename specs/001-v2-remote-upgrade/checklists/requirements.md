# Specification Quality Checklist: V1 → V2 Unattended Remote Upgrade Path

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-09-08
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
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

- All three clarifications resolved (user, 2026-09-08):
  1. **Failed v2 health check → automatic rollback to retained v1** (Option A).
     Encoded in FR-010, FR-010a (retain previous-version bundle + config until v2 healthy),
     FR-010b (keep retrying after rollback), US2 scenarios 4–5, SC-003a, Key Entities
     "Retained Previous-Version Bundle".
  2. **Machines with no working v1 updater → out of scope for unattended upgrade; tracked
     manual runbook + counted in fleet status** (Option A).
     Encoded in FR-023, FR-023a, FR-023b, User Story 5 (rewritten), SC-008a.
  3. **v1 config → one-time migration translates it into a new v2 format/location** (Option B).
     Encoded in FR-014, FR-014a (idempotent), FR-014b (missing/partial handling),
     FR-014c (preserve v1 config for rollback), Edge Cases, Key Entities, SC-008.
- Spec is ready for `/speckit-clarify` (optional) or `/speckit-plan`.

**Amendment 2026-09-08 — release channels & staged rollout** (user decision):
added FR-005a–FR-005e and SC-001a. Channel (`stable`/`beta`) is compiled into the build; a
`stable` build only self-updates from stable releases, a `beta` build only from prereleases;
the existing v1 fleet only auto-updates from stable. A stable `vX.Y.Z` MUST NOT be cut until
the matching beta ran ≥7 days on ≥3 beta instruments with 0 Sev-1 telemetry. Mirrored in
constitution v1.3.0 (Build section) and spec 002 (heartbeat carries the channel).

**/speckit-plan 2026-09-08** — plan + research + data-model + contracts + quickstart written.
Constitution Check PASS (v1.3.0). Key design: migration logic inside the v2 installer
`[Code]`; **only Luminosa has a v1 fleet** (Solira greenfield); Luminosa v2 keeps every v1
identifier (AppId/dir/exe/service/task) for near-zero migration surface; snapshot →
health-check (`loguploaderservice.exe once` → `heartbeat.ok`) → auto-rollback; failed attempts
reported via `measurement_type: "upgrade_attempt"` (no backend change). Clarifications
resolved: C1 reboot cadence OK (boot-triggered hop accepted, no v1 bridge); C2 no
code-signing cert (SHA-256 + release access control as v1; Authenticode switch left dormant).
Ripple: spec 002 `contracts/cli.md` — Luminosa binary/service names differ from the
`pquploader-<product>` scheme (small amendment pending, spec 001 T050).

**/speckit-analyze 2026-09-08 — A1–A8 remediated:**
- A1 (HIGH): SC-009 reworded — "pending file" scoped to config-backup data; unsent v1 logs
  are intentionally not migrated (v2 has no logs).
- A2 (HIGH): FR-005 split — daily trigger for v2→v2.x; **v1→v2 first hop is boot-triggered**
  (fielded task immutable), accepted via C1, bridge release as fallback.
- A3: FR-004 now names the 95%/14-day bound and requires `fleet-status.py` to report
  migrated-% + a `stuck`-threshold bridge-release signal (T035).
- A4: added spec 002 **T045a** — build `src/upgrade.rs` (`version --json` / `is-newer` /
  `upgrade-report`) during 002's implementation, not only spec 001's.
- A5: `release.yml` is **owned by spec 001 T048**; spec 002 T045 defers to it.
- A6: FR-014 notes v1's `public_link` (Nextcloud) is deliberately not carried.
- A7: FR-015 rewritten — v2 starts fresh, worst case one redundant backup (spec 002 FR-016).
- A8: FR-005d + T037 define crash-loop (≥3 starts/1h or cycle.ok=false ×3) and stopped
  service (no heartbeat ≥3× cycle interval).
- LOW A9–A12 partly folded: T048 grep gate now covers `updater/` `installer/` `src/`.
