# Current operational playbooks

Only currently useful, bounded validation procedures belong here. Every command
must be rechecked against the current revision, context, namespace, identities,
and effect boundary before it runs.

## Playbooks

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
