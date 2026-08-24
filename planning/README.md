# PHarness documentation map

Last organized: 2026-08-24

PHarness documentation is organized by purpose and lifecycle so an agent can
distinguish current work from shipped behavior and historical context before it
acts.

The current stable release baseline is the annotated tag
`v3-operator-cockpit` at release commit
`597edaf0bb32baf84a23142d61e4c28ac2788191`. Its compiled source revision is
`8c3e2a7985d142cd32b19d6ea6d89fee76d43abc`.

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

No implementation milestone is currently active. The most recent maintenance
milestone, the
[`app.rs` behavior-preserving decomposition](implemented/milestones/app-rs-behavior-preserving-decomposition-milestone-2026-08-21.md), completed at
`v3-decomposed-stable`; the later operator-cockpit release remains the current
stable product baseline. Add the next approved milestone under
[`active/`](active/README.md) before implementation begins.

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
