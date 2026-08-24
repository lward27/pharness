# Current architecture

This directory contains structural maps that describe the current system. Line
counts and dependency measurements are snapshots and must be recomputed before
they are used as an implementation gate.

## Architecture sources

- [`app-module-dependency-graph-2026-08-21.md`](app-module-dependency-graph-2026-08-21.md)
  records the API application's extracted module graph and intended dependency
  direction.
- [`../../docs/adr/0001-local-first-cluster-native.md`](../../docs/adr/0001-local-first-cluster-native.md)
  records the accepted local-first, cluster-native architecture decision.
- [`../active/app-rs-behavior-preserving-decomposition-milestone-2026-08-21.md`](../active/app-rs-behavior-preserving-decomposition-milestone-2026-08-21.md)
  defines the target module ownership and non-negotiable behavior invariants
  for the active decomposition.

For runtime truth, inspect the crate graph, Axum router composition, migrations,
Helm chart, and current immutable release values directly.
