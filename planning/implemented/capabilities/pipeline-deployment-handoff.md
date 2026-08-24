# Pipeline To Deployment Handoff

## Decisions

- A completed PipelineIntent can create one proposed DeploymentIntent only when
  its authored `deployment_handoff` declares the exact environment, namespace,
  and Argo CD Application. The executor never infers a deployment target from
  build output or model text.
- The handoff requires satisfied terminal PipelineRunAnalysis evidence. A
  successful build without that evidence remains an approved PipelineIntent
  and does not advance the delivery chain.
- The generated DeploymentIntent is always `proposed`. Approval, Argo CD
  observation, and any future sync operation remain separate decisions.
- Replays are idempotent: an existing downstream DeploymentIntent is retained
  and recorded as such rather than duplicated or rewritten.

## Backlog

- Add a DeploymentContract with an explicit Argo sync operation schema before
  introducing any deployment mutation executor or Argo CD `Application` patch
  permission.
- Add a reviewed policy for whether a failed PipelineRun should create a
  diagnostic deployment handoff. The default remains no handoff.
- Surface the declared handoff and its creation audit directly in the operator
  console inspector.
