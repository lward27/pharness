# Active implementation plans

This directory contains work that is approved to execute but not yet complete.
An agent should not infer additional active work from the roadmap or archive.

## Current milestone

| Plan | State | Next boundary |
| --- | --- | --- |
| [`app.rs` behavior-preserving decomposition](app-rs-behavior-preserving-decomposition-milestone-2026-08-21.md) | Ready to execute | Rebaseline D0 measurements against `v3-operator-cockpit`, then land one extraction PR at a time |

The decomposition is a maintainability change. It must not alter public routes,
serialized contracts, lifecycle actions, state hashes, policy decisions,
durable events, executor behavior, or external effects.

Before starting a slice, verify the current tagged baseline, recompute the
target module measurements, and run the plan's characterization gates. Stop if
behavioral evidence changes unexpectedly.
