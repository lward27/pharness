# PHarness documentation map

Last organized: 2026-09-04. Current program: [ASTRA autonomous SDLC](programs/autonomous-sdlc/ASTRA-00-PROGRAM.md).

## Choose an entry point

| Purpose | Read first | Meaning |
| --- | --- | --- |
| Getting started | [README](../README.md), [current baseline](evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md) | What exists, what is deployed, and what is not yet proven |
| Development | [architecture](architecture/README.md), [UI guidance](../ui/AGENTS.md) | Ownership, invariants, and implementation checks |
| Operation | [operations](operations/README.md) | Scoped runbooks and release verification |
| Reference | [product vision](design/product-vision-and-boundaries.md), [product model](design/product-model.md), [stage outcomes](design/stage-outcomes-and-evidence-handoffs.md), [hosted configuration](design/ASTRA-HOSTED-WORKFLOW-CONFIGURATION.md) | Current direction and semantic contracts |
| Roadmap | [ASTRA master and twelve milestones](programs/autonomous-sdlc/ASTRA-00-PROGRAM.md), [active index](active/README.md) | Approved work and acceptance gates |
| Evidence | [evidence](evidence/README.md), [implemented records](implemented/README.md), [archive](archive/README.md) | Dated proof, shipped decisions, and superseded history |

## Current implementation and release

The starting source/configuration baseline is main
`c36b46aceb72f3d7097bc0bdee74810c745f7c0c`, observed live in Argo on 2026-09-04.
The compiled release source is `83a2689c877a3f48688d1d457c34e83474698c46`.
These identities differ intentionally: configuration/evidence commits do not imply
that images were rebuilt. The [Lamina release evidence](evidence/smoke-results/pharness-lamina-operator-console-release-2026-09-04.md)
records its prior acceptance. Refresh the [baseline addendum](evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md)
and milestone evidence before relying on any current-state claim.

Lamina is shipped. The reviewed source-closure fix is already upstream. Both the
yfinance Market endpoint and frontend Market Overview exist on their current main
branches. Earlier Finance campaign text is historical source-only evidence, not
an instruction to recreate those features. Gateway coding qualification and the
continuous hosted lifecycle remain unaccepted.

## Authority and lifecycle

The owner's approved ASTRA decisions govern new work. Executable source, schemas,
tests, and observed immutable deployment evidence establish what currently exists;
when implementation differs from the approved direction, record the gap rather than
claiming the future behavior has shipped. Accepted ADRs and living contracts explain
boundaries; dated evidence and archives must keep their original scope.

Use the numbered program as the single implementation ledger. Keep its documents
in place as status changes, linking from active/implemented indexes. Older active
plans are subordinate references or superseded work, as marked in their headers.
The Repo Mode product/screen contracts describe the shipped legacy boundary;
they are not competing entry points for a new roadmap.

## Long-running execution

Read the master and next eligible milestone; inspect exact source, remote main,
worktree state, and relevant live targets. Implement in isolated `codex/` worktrees,
retain user files, validate meaningful behavior, and record evidence before closing
a gate. Continue independent work when one dependency is externally blocked.
Never equate test success, healthy infrastructure, or elapsed time with acceptance.

## Documentation rules

- New program, assessment, and acceptance Markdown uses `ASTRA-`; existing canonical
  documentation keeps its established filename.
- Use repository-relative links and explicit revisions/dates, not machine-local paths.
- Keep results in `evidence/` and executable operational procedures in `operations/`.
- Mark historical scope and replacement links; do not rewrite dated results.
- Distinguish implemented, deployed, qualified, and accepted behavior.
- Do not store credentials, Secret values, authorization headers, or kubeconfig contents.
- [Presentations](presentations/README.md) support communication, not implementation truth.
