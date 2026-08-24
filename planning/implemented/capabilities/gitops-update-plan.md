# GitOps Update Plan

`POST /api/pipeline-intents/:pipeline_intent_id/gitops-update-plan` prepares a
durable, review-only Kustomize image update after a completed PipelineIntent
has satisfied terminal PipelineRunAnalysis evidence.

It requires a WorkItem-backed non-production `dev` pipeline, declared
`gitops_repo` and `gitops_ref`, a safe relative `kustomization_path`, and an
image reference pinned with `@sha256:`. The resulting `gitops_update_plan`
artifact records the source ChangeSet, pipeline, declared DeploymentIntent,
target repository/ref, deterministic branch name, and a material hash.

The endpoint does not clone a repository, write a file, push a branch, create
a PR, or request an Argo sync. It is followed by a persisted `GitOpsChangeSet`,
which consumes the exact artifact and keeps source-code and GitOps diffs as
independent reviewable provenance. The dedicated GitOps writer is a separately
guarded dev-only capability; it can run only after the plan, gate, grant, and
writer configuration are all current.

Every WorkItem with a declared GitOps target now has a separate
`gitops_mutation` ApprovalGate. It scopes the exact GitOps repo/ref and is
independent of both `git_mutation` for application source and
`cluster_mutation` for the subsequent Argo sync.

Use the machine-facing CLI after the PipelineIntent has eligible terminal
evidence. When its Tekton analysis has recorded a verified
`pipeline_build_output`, omit `--image-ref` and Pharness derives the exact
digest-pinned reference. `--image-name` stays explicit because the Kustomize
entry is an operator-owned target, not something inferred from a build image.

```sh
pharness-cli pipeline-intents prepare-git-ops-update \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --kustomization-path 'apps/finance-api/overlays/dev/kustomization.yaml' \
  --image-name 'registry.example.test/team/finance-api' \
  --actor lucas \
  --reason 'prepare reviewed dev GitOps image update'
```

For legacy/manual evidence, provide an exact digest-pinned override. If a
verified build-output artifact exists, a different override is rejected rather
than allowing the GitOps target to drift from the build result.

```sh
pharness-cli pipeline-intents prepare-git-ops-update \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --image-ref 'registry.example.test/team/finance-api@sha256:replace_with_digest' \
  --kustomization-path 'apps/finance-api/overlays/dev/kustomization.yaml' \
  --image-name 'registry.example.test/team/finance-api' \
  --actor lucas \
  --reason 'prepare reviewed dev GitOps image update from legacy build evidence'
```

Create the independent review record from the returned artifact id. This is
still an API/state transition only, not a Git write:

```sh
pharness-cli gitops-change-sets create \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --gitops-update-plan-artifact-id "$GITOPS_UPDATE_PLAN_ARTIFACT_ID" \
  --actor lucas \
  --reason 'create reviewed dev GitOps change set'
```

Approve it only after reviewing its exact repository, base ref, deterministic
head branch, Kustomization path, image name, and digest-pinned image reference:

```sh
pharness-cli gitops-change-sets transition \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason 'GitOps image update reviewed'
```

Approval does not authorize a remote write by itself. The later writer must
also verify the still-current `gitops_mutation` gate and its exact repo/ref
scope before it can create a branch, commit, or pull request.

## Read-Only Base Revision Evidence

Before a future GitOps writer can create a branch, Pharness resolves the
declared GitOps base ref to an immutable commit SHA. It uses the separate
read-only GitHub observer identity, requires the exact GitOps repository to be
allowlisted for that identity, and records only repository/ref/SHA provenance.
It does not clone the repository, retrieve Kustomization contents, or mutate
GitHub.

```sh
pharness-cli gitops-change-sets resolve-base-revision \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --actor lucas \
  --reason 'resolve immutable GitOps base revision before writer planning'
```

The response returns a dispatched read-only Job. Its durable result artifact is
`gitops_base_revision`; only a `resolved` result with a 40-character SHA may
be used by GitOps delivery planning and a future writer preflight.

## Immutable Delivery Plan

After the GitOps ChangeSet is approved and its base revision has resolved,
prepare the immutable writer input:

```sh
pharness-cli gitops-change-sets prepare-delivery \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --actor lucas \
  --reason 'bind approved GitOps update to immutable base revision'
```

This creates a `gitops_delivery_plan` artifact. It binds the approved
GitOpsChangeSet revision and material hash to the exact observer-resolved base
commit, deterministic head branch, Kustomize image operation, and pinned image
digest. Repeating the command for unchanged evidence returns the same plan.

The plan remains deliberately non-executable. It does not read manifest
contents, create a branch, commit, push, open a PR, or sync Argo. The next
step is a separate GitOps writer preflight. It uses the same durable
`PermissionGrant` model as source delivery but narrows it to this GitOps
ChangeSet and this plan artifact:

```sh
pharness-cli gitops-change-sets authorize-delivery \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --created-by lucas \
  --reason 'authorize this exact reviewed GitOps delivery plan'

pharness-cli gitops-change-sets preflight-delivery \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --actor lucas \
  --reason 'record GitOps writer readiness'
```

The preflight remains `blocked` until the separate scoped `gitops_mutation`
gate is satisfied or waived. Once the gate and matching grant exist, it reports
`ready_for_writer`. `dispatch_ready` becomes true only when an independently
configured GitOps writer identity is enabled and allows the exact GitOps
repository. It has its own ServiceAccount, token Secret, repository allowlist,
and Git author identity; it never falls back to the source-code writer.

Once the exact GitOps writer identity is configured, the explicit execution
command dispatches a short-lived writer Job. It re-runs preflight immediately
before dispatch, clones only the resolved base commit, updates only the one
approved Kustomization image entry, creates the deterministic branch and
commit, and opens a GitHub PR. It never syncs Argo, patches Kubernetes, or
merges the PR:

```sh
pharness-cli gitops-change-sets execute-delivery \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --actor lucas \
  --reason 'create the reviewed dev GitOps pull request'
```

The command remains unavailable with the chart defaults. An operator must
separately enable the GitOps writer identity, provide its token Secret, and
allowlist the exact disposable dev GitOps repository. This is deliberate: the
new route is a controlled delivery primitive, not a default-on mutation path.

## Pull Request Observation And Merge Provenance

After the writer has reported a completed branch-and-PR result, a separate
read-only observer can record the exact current GitHub PR state. It reuses the
Git observer identity, never the GitOps writer token. The observer checks the
delivered head branch and commit SHA before accepting GitHub's response, and it
does not merge, update, label, comment on, or otherwise mutate the pull
request.

```sh
pharness-cli gitops-change-sets observe-delivery \
  --gitops-change-set-id "$GITOPS_CHANGE_SET_ID" \
  --actor lucas \
  --reason 'record current GitOps pull-request state'
```

An observation produces `gitops_delivery_pr_observation`. If and only if the
PR is closed as merged with a valid immutable merge SHA, Pharness also records
`gitops_delivery_merge`. That artifact is the future deploy preflight input:
Argo reconciliation must require the exact observed GitOps merge, rather than
treating PR creation as deployment provenance. This command is also disabled
until the existing read-only Git observer is configured and allowlisted for
the exact GitOps repository.

## WorkItem Reconciliation

`POST /api/work-items/:work_item_id/reconcile` reports this lifecycle as one
machine-facing next action after source merge, verified build output, and a
declared DeploymentIntent. It follows durable GitOps ChangeSet and artifact
evidence through review, base-revision observation, delivery-plan preparation,
authorization, writer availability, explicit execution, PR observation, and
merge provenance. A merged result, or an explicitly applied GitOps ChangeSet,
returns `awaiting_deployment_intent_review`.

Reconcile never invokes any command above. An API client or scheduler must
choose and invoke each mutation endpoint with its own scoped approval/grant.
That keeps controller polling safe and makes the GitOps provenance boundary
auditable even when future automation drives the full dev loop.

## Transformer Contract

The worker now has a tested, structured Kustomization image-update primitive
ready for the writer integration. It accepts only a YAML mapping with the
standard `images` sequence and exactly one entry whose `name` equals the
approved `image_name`. It replaces that entry's `newName` and `digest` from
the approved digest-pinned image reference and removes `newTag` so a mutable
tag cannot compete with immutable provenance. Missing, duplicate, malformed,
or non-digest-pinned targets are hard failures.

YAML comments and presentation may be normalized by the typed writer, so its
resulting Git diff remains a required GitOps ChangeSet/PR review surface.
