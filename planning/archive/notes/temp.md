Yes. Treat the labels as dev, but keep the capability identities narrow. Start with a real **source branch/PR smoke**; leave GitOps, Argo, and API ingress off until that works.

The live `pharness` Argo Application deploys `https://github.com/lward27/pharness.git`, path `deploy/helm/pharness`, from `HEAD`. The disposable source repo is already allowlisted:

```sh
https://github.com/lward27/yfinance_wrapper.git
```

**1. Create two GitHub tokens**

Create two fine-grained GitHub tokens, both scoped only to `lward27/yfinance_wrapper` and expiring soon:

- Writer: `Contents: Read and write`, `Pull requests: Read and write`
- Observer: `Contents: Read-only`, `Pull requests: Read-only`

**2. Create the writer and observer Secrets**

Run these exactly; the token text is not echoed or retained in shell history:

```sh
kubectl config use-context lucas_engineering
export PHARNESS_NAMESPACE=pharness

read -rs "PHARNESS_GIT_WRITER_TOKEN?Paste the writer GitHub token: "
printf '\n'

kubectl -n "$PHARNESS_NAMESPACE" create secret generic pharness-git-writer-token \
  --from-literal=token="$PHARNESS_GIT_WRITER_TOKEN" \
  --dry-run=client -o yaml | kubectl apply -f -

unset PHARNESS_GIT_WRITER_TOKEN

read -rs "PHARNESS_GIT_OBSERVER_TOKEN?Paste the observer GitHub token: "
printf '\n'

kubectl -n "$PHARNESS_NAMESPACE" create secret generic pharness-git-observer-token \
  --from-literal=token="$PHARNESS_GIT_OBSERVER_TOKEN" \
  --dry-run=client -o yaml | kubectl apply -f -

unset PHARNESS_GIT_OBSERVER_TOKEN

kubectl -n "$PHARNESS_NAMESPACE" get secret \
  pharness-git-writer-token \
  pharness-git-observer-token \
  -o json | jq '.items[] | {name: .metadata.name, keys: (.data | keys)}'
```

Expected: each Secret exposes only a `token` key. Do not print or decode either Secret.

**3. Update `deploy/helm/pharness/values.yaml`**

Change only these blocks for the first live smoke:

```yaml
worker:
  gitWriter:
    enabled: true
    tokenSecretName: pharness-git-writer-token
    allowedRepos:
      - https://github.com/lward27/yfinance_wrapper.git
    githubApiUrl: https://api.github.com
    authorName: Pharness
    authorEmail: pharness@lucas.engineering

  gitObserver:
    enabled: true
    tokenSecretName: pharness-git-observer-token
    allowedRepos:
      - https://github.com/lward27/yfinance_wrapper.git
    githubApiUrl: https://api.github.com

  gitOpsWriter:
    enabled: false

  argoExecutor:
    enabled: false

ingress:
  apiEnabled: false
```

Keep `workspaceAllowedRemoteRepos` as it is. Do not add broader source or GitHub allowlists.

**4. Render and publish the chart change**

```sh
cd /path/to/pharness

helm template pharness deploy/helm/pharness \
  --namespace pharness > /tmp/pharness-rendered.yaml

kubectl apply --dry-run=server -f /tmp/pharness-rendered.yaml

git diff -- deploy/helm/pharness/values.yaml
git add deploy/helm/pharness/values.yaml
git commit -m "Enable disposable Git writer and observer smoke"
git push
```

Merge that change through the normal GitHub path. Argo CD will reconcile because its target revision is `HEAD`.

**5. Verify configuration without exposing secrets**

```sh
kubectl -n pharness rollout status deploy/pharness-api --timeout=180s

kubectl -n pharness exec deploy/pharness-api -- sh -c \
  'printf "writer=%s\nobserver=%s\nwriter_repos=%s\nobserver_repos=%s\n" \
  "$PHARNESS_GIT_WRITER_ENABLED" \
  "$PHARNESS_GIT_OBSERVER_ENABLED" \
  "$PHARNESS_GIT_WRITER_ALLOWED_REPOS" \
  "$PHARNESS_GIT_OBSERVER_ALLOWED_REPOS"'
```

Expected: both enabled flags are `true`, and both allowlists contain only `yfinance_wrapper`.

Then use the retained source smoke path in
[git-writer-pr-executor-smoke.md](../runbooks/git-writer-pr-executor-smoke.md).
It will create one bounded branch and PR, then stop for review/merge
provenance. It will not auto-merge.

**What to enable later**

Do **not** enable these yet:

- `gitOpsWriter`: it needs a third, separate token scoped only to `https://github.com/lward27/lucas_engineering.git` and a dedicated disposable Kustomize target. The current writer intentionally updates an exact `kustomization.yaml`; `yfinance-wrapper` currently points at a Helm chart path.
- `argoExecutor`: it needs a dedicated disposable Argo Application. There is no `finance-api` Application, and current finance applications target `apps-prod`. Create a dedicated app such as `pharness-yfinance-dev-smoke` first, even if “prod” is only a label.
- `ingress.apiEnabled`: the API template is ready, but it is unnecessary for the first smoke. Enable it only after verifying unauthenticated `/api/runs` returns `401` and an operator bearer token returns `200`.

The next implementation slice should create the disposable Kustomize/Argo target in `lucas_engineering`, then we can enable GitOps writer and the exact-Application Argo runner for a full live loop.



cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "declare Kubernetes coding alpha review boundary" \
  | tee target/kubernetes-coding-alpha-plan.json

export WORK_PLAN_ID="$(jq -r '.work_plan.id' target/kubernetes-coding-alpha-plan.json)"

cargo run -q -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "review source-only Kubernetes attempt"

cargo run -q -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status approved \
  --actor lucas \
  --reason "approve bounded development coding attempt"
