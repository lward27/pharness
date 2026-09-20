# PHarness WorkItem Prerequisite Guidance and Recovery

## Summary

Make Repo Mode WorkItem creation recoverable when exact-revision readiness,
capability evidence, Product binding, or context discovery is stale or missing.
The controller supplies typed, ordered prerequisite and remediation projections;
the operator explicitly confirms one corrective step at a time. Preflight remains
read-only and successful recovery never creates a WorkItem automatically.

This milestone is an operator-experience change only. It does not change the
coding loop, prompt packs, inference or execution policies, model qualification,
or Astra's reliability program.

## Implementation contract

- Product Repository summaries expose mutable and context eligibility at the
  registered immutable revision.
- WorkItem preflight retains compatibility blockers and adds typed
  prerequisites plus one server-recommended resolution.
- Context failures distinguish missing Product binding, missing/running/failed
  deterministic discovery, and unavailable Repository identity.
- The wizard keeps ineligible context Repositories visible but disabled, with
  the exact reason and preparation destination.
- Inline recovery supports explicit source-reader/profile verification and
  exact-revision readiness assessment. Structural problems route to the owning
  Repository, onboarding, Product topology, or Settings surface.
- Every inline mutation shows Repository, revision, expected result, operator,
  and reason before execution. Asynchronous readiness is observed through
  bounded polling of the owning Repository only.
- Each successful step reruns authoritative WorkItem preflight and replaces the
  reviewed preflight hash. A stale create response requires a new review.

## Acceptance

- API coverage proves deterministic prerequisite ordering, granular context
  states, compatibility blockers, read-only preflight, and actor/reason
  propagation.
- UI coverage proves early eligibility guidance, disabled context selection,
  one-step confirmation, in-place recovery, request-race protection, structured
  errors, stale-state recovery, and preserved form inputs.
- Real-server browser coverage reproduces the Finance frontend creation failure,
  resolves or removes its context prerequisite, refreshes readiness, and creates
  the WorkItem only after a new explicit confirmation.

