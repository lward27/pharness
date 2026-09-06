# ASTRA M09: Bind the production decision to reviewable material

Status: source implemented and locally validated; not deployed and not an accepted production gate.

The first M09 slice adds typed material for one Finance production proposal, its complete GitOps file diff, and the human decision over those hashes. Source `b71e267d452343203fcc45b1cc1efc9e9397779a` starts from main `04dbaeab51d9de939f0743d587c3aa3d10c21a8f` and incorporates the unmerged M08 dependency `06ead42711492377dba03da67cd9d7ec2dfa29a8` in an isolated worktree.

## Implemented boundaries

The proposal records the original WorkItem, operation, pipeline and deployment; workflow/source/build identities; candidate and preceding image digests; separate staging and production-baseline evidence references; configuration and rollback-compatibility fingerprints; and the recorded rollback permission. Changing any of that material invalidates the existing decision.

The production target is limited to the existing yfinance or Finance frontend Kustomization. The original file must already contain the recorded baseline digest. The only allowed diff replaces it with the candidate digest. No-op changes, mutable image tags, extra edits and another namespace are rejected. Existing production files omit `namespace`, so native Argo and Deployment evidence must also establish `apps-prod`; an absent YAML key is not proof of deployment scope.

Approval binds both the proposal and full GitOps plan, including the base revision and original blob identity. It retains the existing production maximum of 30 minutes from the recorded approval instant. Future, expired, overlong and mismatched decisions fail. Repeated validation never renews the time window. The proposal may wait for a human without granting an execution budget; the later controller must preserve its existing operation and recovery limits.

## Validation and limits

[Validation](ASTRA-M09-PRODUCTION-DECISION-MATERIAL-VALIDATION.json) records 212 passing core-package tests, including six new production boundary tests. These cover both application targets; every bound proposal field; changed GitOps base, blob or complete diff; namespace and image restrictions; decision time boundaries; replay stability; and rejection of unrelated/legacy schemas. Six explicit live-reader tests remain ignored. Core Clippy, formatting and architecture checks passed.

These are structural contract checks. A hash is not evidence of native origin, and an operator-name string is not proof of a human decision. This slice adds no decision endpoint, worker authority, network request, GitOps mutation, database migration or enabled production path. It cannot close M09. The current hosted controller still stops after verified staging.

## Next integration and acceptance

1. Load and validate original controller-sealed source, build, staging and production-baseline artifacts; collect fresh application-scoped evidence before admission. Check current configuration and rollback compatibility rather than trusting submitted hashes.
2. Persist the authenticated operator decision separately from tool or automatic lifecycle approvals. Bind it to the exact proposal and plan; refuse worker-authenticated or self-declared decisions. Serialize approval, pause/cancel and dispatch through the existing single-writer boundary.
3. Revalidate the current source, exact GitOps base/diff, staging evidence, production target and baseline immediately before one recorded production effect. Recover lost acknowledgments from the original external operation rather than writing again. Observe Argo auto-sync and the same digest without a rebuild.
4. Complete the ten-minute production observation and one bounded, compatible rollback for confirmed release regression. Missing telemetry alone does not authorize rollback, and a recovered service leaves the failed WorkItem failed. Demonstrate recovery in staging, then obtain genuine human approvals for M11.

Production yfinance still needs a compatible, verified baseline for the current behavior checks. Preparing a baseline correction is separate from executing it and does not count as an autonomous Finance maintenance result. All M09 acceptance gates remain open.
