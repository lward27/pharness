# Immutable Git Merge Provenance

## Decisions

- A WorkItem-backed `PipelineIntent` now has native lineage. The database
  migration makes its legacy remediation/incident fields nullable rather than
  fabricating an incident record. Existing incident-backed PipelineIntents are
  copied without change.
- A WorkItem build requires a current `git_delivery_merge` artifact. It is
  created only by the dedicated Git observer after GitHub reports the exact
  Pharness-created PR as merged and returns a valid 40-character merge SHA.
  A mutable branch or an unmerged PR cannot be a pipeline build source.
- GitHub PR observation has its own disabled-by-default ServiceAccount and
  token Secret. The observer Job receives neither Fireworks credentials, a
  writable Git token, nor a workspace volume; it only calls the internal API
  and GitHub's PR read endpoint.
- WorkItem PipelineIntent can now create non-executing DeploymentIntent,
  Release, and RegistryEvidence records. Their existing ChangeSet and WorkPlan
  form the durable source lineage, while legacy remediation/incident columns
  are nullable. WorkItem approval-gate lineage remains deliberately pending.
- A PipelineContract can declare `source_revision_param`. For WorkItem
  delivery it must name a required scalar parameter, and the PipelineIntent
  execution value must exactly equal the observed `merge_commit_sha`. The
  generated PipelineRun also carries that SHA in a provenance annotation.
  Existing incident-backed PipelineIntents are not forced onto this field.

## Backlog

- Define WorkItem-scoped ApprovalGate semantics before enabling WorkItem GitOps
  or Argo actions. This must model scope and invalidation directly rather than
  treating a null incident as authorization.
- Add GitHub API polling/backoff and webhook-triggered observation only after
  the observer smoke has proven the dedicated fine-grained read token.
- Add a UI status panel that distinguishes PR created, PR open, PR merged, and
  build source pinned. Do not infer merge state from the branch alone.
