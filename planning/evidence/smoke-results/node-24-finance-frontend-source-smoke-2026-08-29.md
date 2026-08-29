# Node 24 finance-frontend source smoke — 2026-08-29

This record freezes the acceptance evidence for the PHarness Node 24
EnvironmentProfile and finance-frontend Repo Mode source smoke. Runtime state
and provider configuration may change later; revalidate before relying on
these values operationally.

## Accepted PHarness release

- Compiled source revision: `4bb2d96b650687021e04b51c4c8f7ada07c5b59a`
- GitOps release revision: `39521a27a917aca79e5a097e09f6e3edb2f4b7de`
- Runtime: `registry.lucas.engineering/pharness-runtime@sha256:15ce2ee3f8f1711dbd74331fe40732d76ed06087e7d55b03cf09203ebdea4063`
- UI: `registry.lucas.engineering/pharness-ui@sha256:b3d0940a4fc91448ce3465385bc37ea76bf110b2bd457aa009c7163b734b80d2`
- Python runner: `registry.lucas.engineering/pharness-python-runner@sha256:55f996cc4117d6588d2122cce158a9fa2bb76df80b51677e101a425df7880077`
- Node runner: `registry.lucas.engineering/pharness-node-runner@sha256:6d6e2e68a68976793578df200901c4188275c92b9a12bee6286114d11c360d66`

All four artifacts were built from the same full source SHA. Registry
inspection confirmed Linux/AMD64 and exact OCI source/revision labels. The
buildctl endpoint used by the Tekton build tasks resolved to the dedicated
`lucas-desktop` host; the Tekton orchestration Pods remained on the Kubernetes
worker. Argo reported `Synced/Healthy` at the exact release revision, API/UI
compiled revisions matched, live Pod image IDs matched the declared runtime
and UI digests, and the four PHarness Deployments were ready with zero
restarts.

## Repository onboarding

- Repository: `lward27/finance-frontend`
- Node prerequisite PR: `#3`
- Canonical onboarding PR: `#4`
- Onboarding merge revision: `9279324693292da04775d5852eab17883d4507f3`
- EnvironmentProfile: `node-24`
- Preparation strategy: `node_npm_ci`
- Dependency lock: `npm_package_lock`
- Lifecycle scripts: denied
- Coding readiness: ready

The contract declared only `src/**`, `tests/**`, and `README.md` writable and
selected `npm test`, `npm run lint`, and `npm run build` as typed acceptance
commands. Node preparation completed before model turn zero, used the exact
lock, ignored lifecycle scripts, and did not modify tracked source.

## Supervised WorkItem

- WorkItem: `witem_01a04aeb6fd57691967dfff496708cf4`
- WorkPlan: `wplan_01a04aec918c71b3a21a8f08d530f0c7`
- Workspace: `ws_01a04aed4b657850a373ada26a2d907a`
- Final Builder Run: `run_01a04b1e011e77328ffd6dadbfb2cf61`
- Final Tester Run: `run_01a04b395d2272329bc6dad62214d9b3`
- Verifier Run: `run_01a04b3a5ac073d1a7a428a5e767f136`
- ChangeSet: `cset_01a04b3ec09e7e91877d2a97f75d7a67`
- SourceDeliveryIntent: `srcintent_01a04b41824a7eb1b0f6cfb12e6697e1`
- Source PR: `lward27/finance-frontend#5`
- Approved PR head: `e7858c4059ec6ae8cb5181781ffbc326a55ba9df`
- Merge commit: `28e553510b1e0b82e3105c754a7019145829ab55`
- Raw-evidence hold: `rethold_1787969268747544661`

The WorkItem implemented a shared ticker-search matcher, replaced duplicated
filtering in Explorer, Compare Stocks, and Ticker Table, added Node tests, and
updated README guidance. The approved ChangeSet patch and live PR diff shared
the exact SHA-256:

`34c0a027533317c41ab197ad92cdc826d92d0df1062ef32c20e5681b3063f569`

## Stage and acceptance evidence

- Discover: succeeded
- Plan: succeeded
- Implement: succeeded
- Test: succeeded
- Verify: succeeded
- Source Delivery: succeeded
- Release: inapplicable
- Observe: inapplicable
- `npm test`: 31 tests passed
- `npm run lint`: zero errors and one pre-existing warning
- `npm run build`: passed

The Verifier approved the change against controller-sealed Implement and Test
evidence with no contradictions or risks. PHarness recorded fresh passing
provider-check observations before and after merge and bound merge provenance
to the exact approved head and required-set hash.

## Characterized recovery behavior

The live smoke exposed several harness defects without losing the durable
workspace or transcript:

1. Correction preparation initially treated the existing tracked diff as a
   preparation mutation.
2. Tester copying initially stripped executable modes from
   `node_modules/.bin`.
3. A follow-up copy fix attempted to preserve permissions on the mounted
   workspace root.
4. The WorkItem exhausted its normal attempt count even though the final Tester
   failure occurred before any model turn.

The accepted release preserves the exact tracked correction state, copies
workspace children without changing the mount root, and permits one explicit
state-hashed retry of a zero-turn Tester or Verifier startup failure without
consuming another WorkItem attempt. The retried Tester and resumed Verifier
then completed on the preserved workspace.

## Operational closeout

The earlier blocked duplicate WorkItem
`witem_01a04a9469397a52916658fda46f4ccf` had no active Run or
SourceDeliveryIntent and was cancelled on 2026-08-29 after the accepted
replacement closed. It remains in History and was not deleted.

The compatibility cancellation endpoint does not carry the Repo Mode action
rail's state hash and is not surfaced by the new UI. A server-derived,
state-hashed cancel action remains a follow-up UX/control-plane improvement.

## Deterministic validation

- `cargo fmt --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- UI production build and Vitest
- Helm lint, values-schema validation, rendering, and Kubernetes dry-run
- Rendered-manifest scan proving no PHarness `:latest` references
- Linux/AMD64 platform plus OCI revision/source inspection for all four images
- Fresh isolated model, Python-profile, Node-profile, source-writer, and
  source-observer capability checks
- Live Argo, Deployment, Pod image-ID, WorkItem outcome, provider-observation,
  merge-provenance, and retention-hold checks

## Follow-up boundary

Use Finance for a bounded reliability campaign before adding workloads or
another language profile. The next architecture entry point remains
[PHarness Data Model Convergence](../../active/PHarness-Data-Model-Convergence-Entry-Point.md).
