# PHarness documentation map

Last organized: 2026-08-26

PHarness documentation is organized by purpose and lifecycle so an agent can
distinguish current work from shipped behavior and historical context before it
acts.

The current stable release baseline is GitOps commit
`bbe5c613958881b6237c5173d0dd7458eed7669c`. Its compiled source revision is
`78fb3eb77cf082a9385a0bca1c4c2b06ed618f18`; immutable artifact provenance is
recorded in the completed Node milestone and dated release evidence.

## Start here

| Directory | Meaning | Use it when |
| --- | --- | --- |
| [`active/`](active/README.md) | Approved work that has not finished | Selecting or continuing the next implementation slice |
| [`architecture/`](architecture/README.md) | Current structural maps and architecture entry points | Understanding ownership, dependencies, and invariants |
| [`design/`](design/README.md) | Living product and interaction principles | Evaluating how a proposed change should behave |
| [`implemented/`](implemented/README.md) | Shipped milestones and capability decision records | Understanding why existing behavior was built |
| [`operations/`](operations/README.md) | Current, bounded validation playbooks | Running a reviewed smoke or release verification |
| [`evidence/`](evidence/README.md) | Dated assessments, evaluations, and observed results | Comparing a claim with recorded evidence |
| [`presentations/`](presentations/README.md) | Demo scripts, slides, and presentation sources | Preparing external communication, not implementation truth |
| [`archive/`](archive/README.md) | Superseded plans, old versions, and retained scratch notes | Historical research only |

## Current work

The active milestone is the
[`Finance Metadata Reliability Campaign`](active/PHarness-Finance-Metadata-Reliability-Campaign.md).
It deliberately keeps every change single-repository while recording the
backend-to-frontend merge order and pinned context needed to inform a later
DeliveryPlan design.

The approved
[`Repo Mode V1 product contract`](design/repo-mode-v1-product-contract.md) and
[`Repo Mode V1 screen contract`](design/repo-mode-v1-screen-contract.md) remain
the design authorities for the shipped V1. Future work must begin from a new
reviewed active milestone instead of replaying either completed plan.

## Source-of-truth order

When documents disagree, use this order:

1. Executable code, tests, database migrations, schemas, and deployed immutable
   release provenance.
2. Accepted ADRs under [`../docs/adr/`](../docs/adr/) and current architecture
   maps under [`architecture/`](architecture/README.md).
3. The current active milestone and its explicit behavior invariants.
4. Implemented capability records and current operational playbooks.
5. Dated evidence.
6. Archived material.

An archived document is never authorization to implement, deploy, or mutate an
external system.

## Long-horizon agent workflow

1. Read this index and [`active/README.md`](active/README.md).
2. Resolve the exact Git revision and inspect the current working tree before
   relying on line counts, routes, or screenshots in a document.
3. Read only the architecture, design, implemented records, and evidence needed
   for the current slice.
4. Preserve the stable characterization fixture and external-effect boundaries.
5. Keep one implementation milestone active at a time.
6. When a milestone ships, move it to `implemented/milestones/`, record its
   source/release provenance, and update this index.
7. When a plan is superseded without shipping, move it to `archive/` and name
   its replacement in the archive index.

## Documentation rules

- Use dated filenames for evidence and milestone snapshots.
- Use durable, repository-relative links; do not commit machine-local absolute
  paths.
- Put executable operational commands only in `operations/` and state their
  scope, prerequisites, external effects, and cleanup behavior.
- Keep observed results in `evidence/`; do not rewrite them into current truth.
- Do not duplicate documents across lifecycle directories. Link to them.
- Never store credentials, Secret values, authorization headers, or kubeconfig
  contents in documentation.

## Code-adjacent design sources

Some design sources intentionally remain beside the code they govern:

- [`../ui/AGENTS.md`](../ui/AGENTS.md) contains the current operator-console
  interaction invariants used by implementation agents.
- [`../ui/design-qa.md`](../ui/design-qa.md) and its adjacent images support the
  UI comparison harness and must retain their code-relative paths.
- [`../docs/adr/0001-local-first-cluster-native.md`](../docs/adr/0001-local-first-cluster-native.md)
  is the accepted architecture decision record.
