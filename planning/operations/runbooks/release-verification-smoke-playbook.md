# Release Verification Smoke Playbook

This is the post-sync verification slice. It is intentionally separate from
the Argo executor smoke: it performs typed reads only and cannot request an
Argo sync, patch a Kubernetes resource, run a shell command, or read a secret.

Use it only after a disposable development WorkItem has completed a real Argo
sync through `deployment-intents execute --apply`. It does not work with the
older remediation-only Release smoke because the verifier requires durable
WorkItem ownership and an exact declared Deployment target.

## Preconditions

- The API is running and `PHARNESS_API_URL` targets it.
- The Release is `approved`, WorkItem-backed, and targets non-production
  `dev`.
- Its DeploymentIntent has one current `argo_sync_execution` artifact and a
  matching `argo_sync_result` artifact with `status = completed`.
- The DeploymentIntent declares `resource_kind = Deployment`, plus its exact
  namespace and name.
- The API identity can read that Argo Application and Deployment. It needs no
  Argo mutation privilege for this step.
- If the active DeploymentContract sets
  `post_sync_verification.prometheus_inventory = "required"`, the API identity
  must also reach `PHARNESS_PROMETHEUS_URL`. The verifier performs only its
  bounded inventory read; it does not accept a query from this command.

## Environment

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Select An Approved Development Release

```sh
cargo run -p pharness-cli -- releases list \
  --status approved \
  --target-environment dev \
  --limit 20 \
  | tee target/pharness-release-verification-candidates.json
```

Choose a WorkItem-backed Release from the output and set its id exactly:

```sh
export RELEASE_ID=rel_replace_with_the_approved_dev_release_id
test "$RELEASE_ID" != rel_replace_with_the_approved_dev_release_id
```

Inspect the immutable delivery target before reading the cluster:

```sh
cargo run -p pharness-cli -- releases get \
  --release-id "$RELEASE_ID" \
  | tee target/pharness-release-verification-release.json
```

```sh
jq '{id, status, deployment_intent_id, target_environment, target_namespace, argo_application, work_plan_id}' \
  target/pharness-release-verification-release.json
```

Expected signal:

- `status` is `approved`.
- `target_environment` is `dev`.
- The target namespace and Argo Application are the disposable target you
  intend to observe.

## Run Read-Only Verification

```sh
cargo run -p pharness-cli -- releases verify \
  --release-id "$RELEASE_ID" \
  --actor lucas \
  --timeout-ms 30000 \
  | tee target/pharness-release-verification.json
```

```sh
jq '{status, verified, completed, checks, argo: .argo_observation.data_json.analysis, workload: .workload_observation.data_json.analysis}' \
  target/pharness-release-verification.json
```

Expected signal for a healthy disposable release:

- `status` is `verified`.
- `verified` is `true`.
- `completed` is `false`; this first command only records read-only evidence.
- Argo analysis reports `sync_status = Synced` and `health_status = Healthy`.
- Workload analysis reports `status = healthy`.
- When the contract requires Prometheus inventory, `checks` includes
  `prometheus_inventory` with `passed = true`, and
  `.observability_observation.data_json.inventory` is present.

An `attention_required` result is an expected safety outcome for an unhealthy
or still-progressing target. Do not force completion; inspect the two returned
observation ids and the Release audit trail instead.

## Complete Only After Healthy Evidence

The following command updates only the durable Release lifecycle. It does not
modify Argo or Kubernetes resources, but it requires the fresh verification
reads above to pass.

```sh
cargo run -p pharness-cli -- releases verify \
  --release-id "$RELEASE_ID" \
  --complete \
  --actor lucas \
  --reason "disposable dev release verified after completed Argo sync" \
  --timeout-ms 30000 \
  | tee target/pharness-release-completed.json
```

```sh
jq -e '.status == "verified" and .verified == true and .completed == true and .release.status == "completed"' \
  target/pharness-release-completed.json
```

## Verify Durable Evidence And Audit

```sh
cargo run -p pharness-cli -- releases get \
  --release-id "$RELEASE_ID" \
  | tee target/pharness-release-completed-detail.json
```

```sh
jq -e '.status == "completed" and .release_json.post_sync_verification.status == "verified" and .release_json.post_sync_verification.runtime_ready == true' \
  target/pharness-release-completed-detail.json
```

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind release \
  --resource-id "$RELEASE_ID" \
  | tee target/pharness-release-verification-audit.json
```

```sh
jq -e '[.events[].kind] | index("release.post_sync_verified") != null and index("release.completed") != null' \
  target/pharness-release-verification-audit.json
```

## Current Boundary

This proves a completed Argo operation plus Application health and one exact
Deployment rollout. An active contract can additionally require the bounded
Prometheus inventory criterion; Loki and traces are not yet contract criteria.
It does not verify image provenance and is blocked for production-targeted
WorkItems.
