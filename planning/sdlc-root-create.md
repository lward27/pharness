# SDLC Root Create

## Decisions

- Add public create surfaces for the SDLC chain roots:
  - `POST /api/observations`
  - `POST /api/incidents`
  - `POST /api/remediation-plans`

- Add matching CLI commands:
  - `pharness-cli observations create`
  - `pharness-cli incidents create`
  - `pharness-cli remediation-plans create`

- Use real API surfaces instead of a smoke-only seed path.
  - Codex, UI, smoke scripts, and future cluster workers should all exercise the same machine-facing contract.

- Inherit parent identity instead of allowing mismatched child scopes.
  - `Incident` inherits session, run, and resource identity from its parent `Observation` unless explicit resource metadata is provided.
  - `RemediationPlan` inherits session, run, and resource identity from its parent `Incident` unless explicit resource metadata is provided.

- Audit every root creation event.
  - `observation.created`
  - `incident.created`
  - `remediation_plan.created`

## Backlog

- Add create surfaces for `ApprovalGate` only if operators need to create standalone gates outside WorkPlan or ChangeSet lifecycle flows.

- Add schema-level request examples to the API documentation once the endpoint shapes settle.

- Add a UI-backed root-chain creation flow after the read-heavy UI is wired.

- Decide whether external controllers should supply their own stable IDs or let Pharness generate IDs by default.

- Add stronger domain validation for `Observation.kind` and `Observation.source` after more controller producers exist.
