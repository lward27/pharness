# ASTRA M08: Durable staging GitOps handoff

Historical implementation record. This foundation subsequently deployed in source
`19b0c55`; see [live release preservation](ASTRA-19B0C55-LIVE-PRESERVATION.md).
The newer [baseline admission gate](ASTRA-M08-DURABLE-BASELINE-ADMISSION.md)
supersedes this record's pre-write checks and requires another compatible release.
Neither deployment closes autonomous staging or M08 acceptance. The observations
and deployment instructions below describe the original, pre-release checkpoint.

Source base: `5c2c2b5622d70e795f24c5a1eb6e92ec533e1185`, after refreshing from initial base `6f22bc832c07037b69c8a991186d96e2297b3ced`. Branch: `codex/astra-staging-controller`. The validation receipt records the final source-file hashes and test results.

## Implemented behavior

The existing durable controller can now move a verified autonomous Finance build into a staging GitOps operation. It revalidates the sealed source evidence, successful native build operation, original terminal build receipt, declared source commit and image digest, saved workflow policy, and current staging DeploymentContract. A green PipelineIntent alone is insufficient.

The operation owns the application repository, its staging environment, and the Lucas Engineering GitOps main-branch writer lock. It records the original authority and complete writer/reader Job manifests before dispatch. Retries retain the same operation IDs, image, credentials, time limits and Kubernetes identities. Original staging authority permits observation for at most one hour; new write admission is additionally limited to the existing 30-minute authorization ceiling. Configured worker deadlines, resource limits and retention remain unchanged.

The native worker reads the current GitOps main revision and the selected Kustomization at that exact commit. Its saved proposal includes the original file, blob ID, base commit and updated file. The only permitted edit replaces one existing immutable image digest. Mutable baselines, missing or ambiguous image entries, no-ops, extra file changes, different namespaces and production targets fail validation. The existing byte-preserving Kustomization patcher is shared with the legacy worker instead of maintaining two implementations.

Only these targets are expressible:

| Application | GitOps path | Image repository |
| --- | --- | --- |
| yfinance | `charts/finance-staging/yfinance/kustomization.yaml` | `registry.lucas.engineering/yfinance_wrapper` |
| Finance frontend | `charts/finance-staging/frontend/kustomization.yaml` | `registry.lucas.engineering/finance-frontend` |

Both use `lward27/lucas_engineering`, `main`, and explicit `apps-staging` Kustomizations. The schema-54 DeploymentIntent and GitOpsChangeSet preserve the original PipelineIntent and optional Run lineage. This path creates no synthetic agent Run, new build, production deployment, release success or runtime evidence.

## The GitOps write boundary

Application source delivery still requires its protected PR and exact-head merge. This finite **staging GitOps** path uses one atomic `createCommitOnBranch` GraphQL mutation with the saved `expectedHeadOid`, rather than a GitOps PR. It changes one file and advances main only if its base remains current. The saved GitOpsChangeSet and plan artifact provide the exact reviewable diff and authority. GitHub documents this expected-head concurrency precondition in its [commit API](https://docs.github.com/en/graphql/reference/commits).

The worker refuses to write if GitOps main is protected and does not change branch policy. GitHub also enforces its current permissions at the mutation. A future protected GitOps branch requires a separately reviewed delivery path; this implementation does not bypass protections. The proposed protections on the two **application** repositories remain a separate pending owner decision.

Immediately before admission, the worker rereads the GitOps base, and the API revalidates current source, build, policy, target, active workflow control and original time window. A serialized, persisted admission allows exactly one mutation attempt. An acknowledgment timeout does not grant another attempt. `clientMutationId` is only correlation, not an idempotency guarantee.

After that attempt, the worker reads GitHub to establish the actual result: exact operation message and hashes, one parent matching the saved base, one modified allowlisted file, exact proposed file content, a changed blob, and inclusion on current main. An unrelated later commit is acceptable only when the admitted commit remains an ancestor and the selected file is unchanged. A changed base, unavailable/truncated history, contradictory file content or uncertain outcome stops progression. No automatic new plan or write is issued to make the operation appear successful.

## Recovery and evidence semantics

- Before GitOps admission, an interrupted controller can recover only the original writer Job identity and unchanged authority. Repeated or stale admission requests cannot start a second write.
- After admission, an unavailable or terminal writer may receive one separately recorded observer Job. That observer uses the existing **GitOps reader Secret and service account**. It carries no GitOps writer credential.
- The observer is recorded before dispatch. An uncertain create or later deletion never allocates a replacement observer or extends the original window.
- Pause and cancellation stop new writes. Observation of an already admitted effect can continue. New development and promotion remain stopped.
- GET context requests are read-only, including after an interruption between saving a plan artifact and linking its GitOpsChangeSet. Serialized admission can finish that linkage before permitting the write.
- Callback receipts are durable and repeat-safe. Contradictory identities are rejected. A later unconfirmed observation cannot erase a recorded commit. An authentic effect remains auditable if source or configuration changes afterward, but changed authority blocks progression.
- A recorded GitOps commit leaves the staging operation running and retains its delivery locks. Deployment and runtime verification remain explicitly pending. Neither Release nor Observe closes, and no WorkItem succeeds at this boundary.

Four internal worker-authenticated routes serve context, plan persistence, single admission and outcome recording under `/api/internal/deployment-intents/:deployment_intent_id/staging/`. Operator controls continue through the existing hosted controller. The checked-in route inventory contains all four routes and their authentication class.

GitHub requests use fixed HTTPS endpoints, no redirects, no HTTP mutation retry policy, bounded response bodies and existing trusted TLS verification. Observation is limited to 12 attempts within the original time window and at most 20 history entries per attempt. Provider error bodies, author details and credentials are excluded from recorded error text. These bounds can yield an inconclusive result; they cannot yield a false success.

## Validation and practical limits

The complete API/core/store/worker run passed **560 tests**, with one existing gated live Tempo test ignored. Clippy with warnings denied, formatting, architecture boundaries and five dependency-parser tests passed. See [the machine-readable validation receipt](ASTRA-M08-DURABLE-STAGING-GITOPS-VALIDATION.json). Local tests exercise exact one-file plans for both real staging manifests, admission loss, GitHub response loss, conflicting heads, duplicate callbacks, expired ownership, stale owners, partial persistence, pause/cancellation, changed build evidence, and writer/reader isolation. These use deterministic local HTTP and Kubernetes fixtures. They are engineering evidence, **not a real GitHub mutation, deployment or autonomous Finance acceptance run**.

The first broader run found the intentional route-inventory change; the inventory and reviewed count were updated. Initial lint also caught a convenience method newer than the declared Rust minimum and two unnecessary returns in the extracted patcher. Those were corrected without raising the Rust version. Failed-run logs remain in the validation receipt.

The existing schema-54 [migration and rollback requirements](ASTRA-M08-DELIVERY-RECORD-COMPATIBILITY.md) still apply. This slice adds no migration and has not touched the live schema-53 database, deployed images, Finance manifests, branch protections or running qualification Jobs.

## Deployment, recovery and next gate

After the active qualification Job finishes, validate a current live database copy with the schema-54 reader, build the complete immutable PHarness release from one merged source revision, and record the minimum compatible rollback release. Deploy compatible API and worker readers before allowing hosted workflow writes. Keep hosted creation disabled until the matching runtime and all required profiles qualify. Prior qualifications do not automatically qualify a new binary.

Before live staging execution, verify the GitOps writer and reader bindings and scope, actual base manifest, saved Finance contract, and immutable build evidence. Run the normal controller; do not inject synthetic build receipts or manually advance a WorkItem to satisfy this gate. An unavailable credential or changed branch policy remains an explicit blocker.

The next M08 slice must correlate this admitted GitOps commit with Argo reconciliation, actual Deployment/Pod image identity, functional probes and fresh application-scoped Mimir, Loki and instrumented Tempo evidence. It must persist the five-minute baseline and staging windows and distinguish pass, confirmed failure and inconclusive observations. Frontend runtime configuration remains pending its M11 change. Production approval, production writes, bounded rollback and the final runtime outcome remain M09–M12 gates.

If a write response is lost, observe the recorded operation; do not rerun it manually. If the bounded observer cannot prove the original effect, preserve the WorkItem and require intervention. Once schema 54 is deployed, roll back only to a release containing the compatible reader. Do not return to the currently deployed schema-53 binary after the migration.
