# Living design

This directory is for durable design intent that continues to constrain new
work. It is not a backlog and does not prove implementation.

## Current principles

- [`trusted-autonomy.md`](trusted-autonomy.md) defines the trust-envelope and
  supervised-autonomy principles.
- [`../../ui/AGENTS.md`](../../ui/AGENTS.md) defines current operator-console
  hierarchy, honesty, navigation, and mutation-surface rules.
- [`../../ui/design-qa.md`](../../ui/design-qa.md) describes the code-adjacent
  UI comparison assets.
- [`../../crates/pharness-runhost/src/prompt.rs`](../../crates/pharness-runhost/src/prompt.rs)
  is the actual runtime prompt source. The original planning prompt is retained
  under [`../archive/foundations/`](../archive/foundations/) for history only.

The implemented control-plane and UI designs are indexed in
[`../implemented/README.md`](../implemented/README.md). Superseded V1/V2 UI
concepts are under [`../archive/ui/`](../archive/ui/) and must not be used as
current visual direction.
