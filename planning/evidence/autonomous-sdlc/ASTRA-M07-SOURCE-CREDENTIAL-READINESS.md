# ASTRA M07: Source credential readiness

Status: the owner updated the writer permission; both protection reads were
[verified as authorized at 18:17 UTC](ASTRA-M07-SOURCE-CREDENTIAL-VERIFIED.json).
The [branch-protection decision](ASTRA-M07-SOURCE-MERGE-DECISION.md) remains pending.
The initial failure and remediation below are retained as history.
Observed 2026-09-05 against `lucas_engineering`, using the existing cluster
Secrets in memory. No credential values or HTTP authorization headers were retained.

| Credential | Repository | Source and PR reads | Main protection read |
| --- | --- | --- | --- |
| `pharness/pharness-git-writer-token` | `lward27/yfinance_wrapper` | HTTP 200 | HTTP 403 |
| `pharness/pharness-git-writer-token` | `lward27/finance-frontend` | HTTP 200 | HTTP 403 |
| `pharness/pharness-git-observer-token` | `lward27/yfinance_wrapper` | HTTP 200 | HTTP 404: Branch not protected |
| `pharness/pharness-git-observer-token` | `lward27/finance-frontend` | HTTP 200 | HTTP 404: Branch not protected |

Both main branches report `protected: false`. The backend head remains
`efa6294954b01a089a65419c85542b8fc2f95c83`; the frontend head remains
`c4d64f9242f2955064f99e659bf1648ce6bc4273`.

The guarded merge worker must read protection immediately before admission.
GitHub requires the fine-grained token's **Administration: read** repository
permission for this endpoint. The observed writer credential cannot perform that
read. The repository's returned `permissions` object does not prove a token has
the required endpoint permission; it reported administrative access despite the
403. These reads also do not prove source-write authority. [GitHub endpoint
documentation](https://docs.github.com/en/rest/branches/branch-protection#get-branch-protection).

The required repair was to add Administration **read-only** to the application-source
writer token for the two Finance repositories. It does not need permission to
edit branch protection. If the token is regenerated, replace the `token` key in
`pharness/pharness-git-writer-token` through the existing secret-management process.
The separately scoped GitOps writer credential is not this credential.

The repeated reads now return the explicit `Branch not protected` response rather
than HTTP 403; both main heads remain unchanged. After the branch-protection
decision and application CI merges, validate the actual
protection body and exact required check using the writer's own credential.
Keep automatic source merge blocked until both prerequisites are evidenced.

This check made no repository, branch, Secret or workload changes.
