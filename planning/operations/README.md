# Current operational playbooks

Only currently useful, bounded validation procedures belong here. Every command
must be rechecked against the current revision, context, namespace, identities,
and effect boundary before it runs.

## Playbooks

- [Minisforum Windows local-model setup](ASTRA-MINISFORUM-WINDOWS-LOCAL-MODELS.md)
  prepares LM Studio for the existing PHarness gateway and separates initial setup from qualification.
- [`release-verification-smoke-playbook.md`](runbooks/release-verification-smoke-playbook.md)
  exercises read-only release verification and durable completion evidence.
- [`tekton-e2e-smoke.md`](runbooks/tekton-e2e-smoke.md) describes the bounded
  inert Tekton fixture.
- [`tekton-executor-smoke-playbook.md`](runbooks/tekton-executor-smoke-playbook.md)
  describes the reviewed console and CLI flow for that fixture.

Older manual API/CLI smoke sequences were superseded by the environment-ready
WorkItem flow and operator cockpit. They remain available under
[`../archive/runbooks/`](../archive/runbooks/) but are not current run
instructions.

Before an effectful smoke, state the exact cluster context, namespace,
application/repository target, expected mutation, success criteria, and cleanup
ownership. Never copy credentials into a playbook or command transcript.

## Starting a release observation window

After exact Argo and running-image verification, confirm that the expected
availability metrics are positive and fresh, and inspect their underlying sample
timestamps. Samples used to start the window must follow the verified rollout;
a range-query timestamp alone can still select an older pre-readiness sample.
Then start the complete stated observation period. Retain failed or interrupted
windows separately; never trim the first point, reuse a partial window as complete,
or substitute direct health checks for telemetry. The [sourcee508 observation
record](../evidence/autonomous-sdlc/ASTRA-M04-RELEASE-WINDOW-START.md) documents the
observed timing problem and the added start check.
