# ASTRA M04D: Bounded gateway diagnostics

Date: 2026-09-06. Source: `f0c7afd54d6f116766659c1960cb72d5d26b9935`. Base main: `49ab3703a26e357be44eb0221817a7f09db8fa8f`.
Status: implemented and locally validated; the live M04D gate remains open.
Authority: [approved M04 program](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).

## Boundary

The existing evaluation API, Kubernetes Job dispatcher, model grants, gateway and
shared stage runner now accept an explicit diagnostic scope. A diagnostic contains
one attempt and one to three declared cases. The controller validates and saves
that scope before dispatch. Unrecognized, duplicate, empty or oversized selections
are rejected; legacy full requests keep their original meaning.

A diagnostic report always sets `gate_passed` and `candidate_safe` to false, even
when all selected cases pass. Its separate `diagnostic.passed` records the bounded
observation. Completion saves the report without creating a policy qualification.
The API checks exact requested cases and report scope; SQLite independently rejects
attaching a qualification to a diagnostic. Duplicate completion cannot replace the
original evidence. Activation still requires the full exact-runtime qualification.

No second execution backend, evaluation service, budget increase, hidden repair,
application source change or production authorization is introduced.

## Declared cases and comparison controls

| Stage | Diagnostic cases | Existing primary | Explicit Kimi K3 control |
| --- | --- | --- | --- |
| Onboarding | `python-contract`, `missing-lock` | `onboarding-minimax-m3-v2` | `m04-control-onboarding-kimi-k3-v2` |
| Plan | `acceptance-boundary`, `failing-baseline` | `planner-kimi-k3-v2` | `m04-control-planner-kimi-k3-v2` |
| Test diagnosis | `assertion-failure`, `passing-control` | `test-diagnosis-nemotron-v2` | `m04-control-test-diagnosis-kimi-k3-v2` |
| Verify | `wrong-endpoint-path`, `valid-implementation-a`, `frontend-semantic-mismatch` | `verifier-glm-5p3-v2` | `m04-control-verifier-kimi-k3-v2` |

Existing target revisions, policies and defaults are unchanged. The separate
`fireworks-kimi-k3@m04-control-v1` target permits the four diagnostic stages, including
Test. Each control clones its primary role's tools, reasoning settings, temperature,
input/output limits and transport retry limit. The model is the named experimental
variable; execution-profile budgets remain the same. Planner already uses Kimi K3,
so its control checks repeatability rather than a different model. Diagnostics cannot
activate any control policy. Full qualification and an explicit policy decision would
still be needed before using one for hosted work.

An optional `reference_evaluation_id` reuses only retained public inputs from a
completed diagnostic with the same runtime, suite hash, cases, prompt contract,
context policy, tools and budget. Report and input hashes must verify. Missing,
redacted or oversized inputs are rejected. The evaluator independently reconstructs
the source and checks its content hash, base commit, task and controller context
before reusing the original observed receipts. Model output and private oracle
material are excluded. Both runs retain the public-input hash for comparison.

Run-specific directories and provider/model binding necessarily differ; this is
identical public fixture evidence and equivalent tools/limits, not a claim that
provider wire requests are byte-identical. The initial native envelope is retained
separately. Full qualification still reruns all original commands and cases.

## Operator request

Read the current registry hash from `/api/inference-policies`, then use the existing
authenticated endpoint `POST /api/inference-policies/{policy}/revisions/v1/qualifications`:

```json
{
  "actor": "lucas",
  "reason": "M04D onboarding boundary canary; no qualification claim",
  "config_hash": "<current registry hash>",
  "attempts": 1,
  "scope": {
    "kind": "diagnostic",
    "case_ids": ["python-contract"]
  }
}
```

For the explicitly selected control policy, use the same case list and add the
completed original evaluation ID as `scope.reference_evaluation_id`. Do not retry
an uncertain POST. Reconcile its saved evaluation and Job first. Each target still
requires the existing fresh 30/30 protocol calibration before dispatch. Run serially,
inspect the first useful boundary failure, and fix that demonstrated issue before
another batch. A failed primary can still provide complete inputs for its control.
A run without complete retained public inputs cannot provide a comparison reference.
A provider failure with intact inputs can still be compared to the control.

## Data compatibility and release

Migration 0055 adds only `inference_evaluations.scope_json`, defaulting old rows to
`full_qualification`. It preserves original reports, policy qualifications and audit
records. Publish and verify the complete immutable release, exercise the real API
migrator on a current database copy, and verify live schema/history preservation
before diagnostic writes. Keep hosted creation and Coding Reliability V2 disabled.

The first accepted schema-55 release becomes the minimum compatible rollback
reader; schema-54 binaries cannot open a later migration set. Active envelope Runs
also need the compatible worker introduced in M04A/B. A failed migration/deployment
requires a compatible forward repair or separately reviewed restoration, not a
blind rollback to source 19. No production Finance change is part of this release.

## Evidence and remaining gates

The [validation record](ASTRA-M04-DIAGNOSTIC-CANARIES-VALIDATION.json) records 627 passing
component tests, one existing live-only test ignored, six-package all-target Clippy,
formatting and five architecture checks. Local tools are Node 26.0.0, Python 3.14.4
and Rust 1.95.0; these results do not qualify the Linux runner images or their declared
Node 24/Python 3.11 environments.

Development checks caught permissive parsing of extra fields on the full-scope
variant, and two historical tests that still asserted schema 54. The final run passes
with strict parsing and all history checks preserved. The schema-55 test also proves
that a schema-54 reader refuses the new database.

Deterministic checks exercise request rejection, diagnostic/non-diagnostic separation,
exact input reuse, source/context mismatch refusal, durable completion and schema-54
history preservation across migration and reopen. The control registry is validated
and role settings compared. These checks are harness evidence, not model scores.

Live request, protocol, dispatch, context, output, usage and failure-class records
must be added after the new runtime is deployed. M04D remains open until those
canaries are evaluated. M04E still needs the connected implementation/failure/one
repair/independent-verification proof. M04F still needs the unchanged full gates
on the frozen release. No old evaluation is rescored or substituted for that proof.
