# Specification Quality Checklist: Fleet Backup Archive

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

- **Scope narrowed 2026-09-08**: only the archive **pull** (US1) is in this increment.
  Restore, inspection, and unattended-operation niceties are recorded under *Deferred — later
  increments*. FR-010 / FR-011 (keep every version, keep a complete manifest) are retained so
  the deferred work has its data.
- **Clarified 2026-09-08** (3 questions): archive is append-only with no prune/retention this
  increment; overlapping runs skip and exit 0; archive layout is
  `<product>/<serial>/<machine-id>/…` (one folder = one physical machine).
- **Extended 2026-09-10 — US2**: the pull also archives the `powermeter` product's laser-power
  measurement records (every `measurement_type`), keyed by `system_serial`. Clarified (3
  questions): spec-and-implement together; whole `powermeter` product, not just
  `combiner_power`; nest under `<product>/<serial>/_powermeter/` when that instrument folder
  exists, else standalone `powermeter/<serial>/`. Resolved by live check: the telemetry list
  row carries the full `payload` (no content sub-endpoint, no backend hash), and `powermeter`
  telemetry *is* on the backend prune loop — so archiving it is load-bearing. `agent_status` /
  `upgrade_attempt` heartbeats stay out of scope.
- The one remaining Open Item (confirm backend retention parameters with the backend team)
  does not block planning — it only sets an operator-guidance number, not tool behaviour. The
  earlier open item about the admin list endpoint returning full history was resolved by live
  verification.
- The seed tool `tools/fleet_backup_pull.py` is named as a starting point — a factual pointer,
  not a design constraint. `api.picoquant.com` / `.env` are the existing external contract
  (specs 002/003), i.e. domain context.
- The scheduler (cron job) is explicitly out of scope per the user.
- Items marked incomplete require spec updates before `/speckit-plan`.
