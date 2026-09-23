# Pharness & Lucas Engineering — Security Review

**Scope:** `/home/wardl/Personal/pharness` (Rust control plane: `pharness-api`, `pharness-worker`, `pharness-runhost`, `pharness-model-gateway`, `pharness-codex-host`, `pharness-core`) and `/home/wardl/Personal/lucas_engineering` (Helm/K8s cluster-ops repo, incl. the `openclaw` agent deployment).
**Method:** Original review (2026-09-18): static source/manifests only. Reconciliation (2026-09-23): current PHarness source/config, sanitized live PHarness readiness, and bounded read-only Lucas-cluster object-name checks; no exploit attempts or credential values emitted.
**Original review date:** 2026-09-18. **Reconciled:** 2026-09-23.

> All secret values below are redacted. Where a finding names a secret, only its *location and nature* are given, never its value.

## Reconciliation update — 2026-09-23

The original findings below are a static snapshot from 2026-09-18. They were checked against PHarness `main` at `cffa63f2cf41168883e798ded07d2bb9c2e65639`; this update records disposition without exposing credential material or rewriting the original analysis.

| Finding | Current disposition |
| --- | --- |
| C1 — `.claude/launch.json` operator token | The token variable/value is absent from the current file; commit `cbe7ecc` removed it. Earlier reachable Git history still contains the pre-removal version unless repository history is rewritten, and no rotation evidence was found. Continue to treat the credential as compromised. History rewriting and rotation were not attempted here. |
| H1 — public PHarness API ingress | Still open: the live `pharness-api.lucas.engineering` ingress exists and has no Basic Auth annotation; the live UI ingress does. Do not simply copy UI Basic Auth: a single `Authorization` header cannot carry both Basic and bearer credentials. Use an internal/private route or a separately designed mTLS/OIDC layer. |
| H2 — worker checkout environment/hooks | Fixed by `111fb9b`: checkout clears inherited environment, permits only the needed preparation proxy variables plus the hardened Git variables, disables hooks, and keeps global/system Git config off. The worker tests and release evidence are recorded in [Slice 2](planning/evidence/autonomous-sdlc/ASTRA-M04E-111FB9B-CONNECTED-LOOP-READINESS-RESTORE.md). |
| C2/C3/H3 — OpenClaw | No OpenClaw paths were found in current Lucas `main` (`7eb5784`), and bounded cluster listing found no matching Deployment/StatefulSet, ConfigMap, ServiceAccount, ClusterRoleBinding, Pod, or Ingress. The 2026-09-18 findings are historical for the then-reviewed configuration; no OpenClaw change was needed or made here. Re-check before any redeployment. |
| M1 — constant-time comparison | Already fixed in the current tree: `auth.rs::token_matches` compares SHA-256 digests with `subtle::ConstantTimeEq`. |
| M2 — empty operator-token fail-open | Already fixed at startup in `pharness-api/src/main.rs`: a non-loopback bind is rejected unless at least one operator token is configured. Token-free loopback development remains supported. |
| L1 — workspace command argument policy | Remains defense-in-depth only; arguments are executed directly, not through a shell. An executable-specific allowlist remains the stronger future improvement. |

The low-effort source fixes H2, M1, and M2 are already present in remote history, so this continuation does not duplicate them. The current launch config removal is also upstream. No credential value was read into terminal output or evidence. A full Git-history purge, credential rotation, API-ingress redesign, and OpenClaw privilege reduction have not been performed: they require coordinated secret/GitOps or separate-repository handling.

---

## Summary

| # | Severity | Repo | Finding |
|---|----------|------|---------|
| 1 | **CRITICAL** | pharness | Operator bearer token remains in reachable Git history (absent from current `.claude/launch.json`; rotation unverified) |
| 2 | **CRITICAL — historical/absent** | lucas | Old `openclaw` exec-approval socket token finding; no matching current ConfigMap/workload found |
| 3 | **CRITICAL — historical/absent** | lucas | Old `openclaw` `cluster-admin`/unrestricted-tools finding; no matching current binding/workload found |
| 4 | **HIGH** | pharness | Public `pharness-api` ingress exposes the entire API with no Basic Auth (bearer-only) |
| 5 | **HIGH — fixed** | pharness | Worker git-checkout environment/hooks hardened in `111fb9b` |
| 6 | **HIGH — historical/absent** | lucas | Old `openclaw` gateway ingress finding; no matching current ingress found |
| 7 | **MEDIUM — fixed** | pharness | Bearer-token comparison uses constant-time digest equality |
| 8 | **MEDIUM — fixed** | pharness | Startup rejects non-loopback binds with no operator tokens |
| 9 | **LOW** | pharness | `run_workspace_command` shell-token blocklist is incomplete (mitigated: not shell-run) |
| 10 | **LOW** | lucas | Committed `.DS_Store` files (local filename disclosure) |
| 11 | **INFO** | both | Several controls are well-built (documented below as "verified mitigations") |

**Current headline:** the remaining PHarness items are the still-reachable historical token blob (rotation unverified) and the live public bearer-only API ingress. OpenClaw's previously documented blast radius is absent from the current source/cluster objects inspected, but should be re-audited before redeployment. Several PHarness code findings from the original snapshot are already fixed.

---

## Pharness — detailed findings

### C1. Historical committed operator token in `.claude/launch.json` (current file cleaned)
- **File:** `.claude/launch.json:7` (git-tracked)
- **Historical finding:** a 64-hex bearer token was baked into the committed VS Code/Claude launch config. The current file no longer contains that variable/value (`cbe7ecc`), but the prior commit remains reachable in history:
  ```json
  "PHARNESS_API_PROXY=http://127.0.0.1:24777",
  "PHARNESS_API_PROXY_TOKEN=[REDACTED]"
  ```
- **Impact:** until rotated, anyone with access to the reachable historical blob could obtain the old credential. Its validity/rotation state was not verified. H1 remains an independent bearer-only public API exposure.
- **Remediation:**
  1. Rotate/revoke the token (treat as compromised); this has not been verified.
  2. Coordinate a history rewrite and downstream clone cleanup if the owner wants the blob purged. It was not attempted because this rewrites shared Git history.
  3. Add a CI secret scan after deciding how to handle the known historical finding; source dev tokens from a gitignored local secrets store, not the shared launch config.

### H1. Public API ingress has no Basic Auth (bearer-only)
- **File:** `deploy/helm/pharness/templates/ingress.yaml` — the `pharness-api` ingress block (third resource in the file, `path: /`, `pathType: Prefix` → `pharness-api:4777`, no `auth-type: basic` annotation).
- **Contrast:** the `pharness-ui` ingress in the *same file* **does** set `nginx.ingress.kubernetes.io/auth-type: basic` + `auth-secret`. The `pharness-api` and `pharness-agent-host-api` ingresses do not.
- **Config:** `deploy/helm/pharness/values.yaml` has `ingress.apiEnabled: true` (default) and `api.host: pharness-api.lucas.engineering`.
- **Impact:** the entire operator API (runs, work-items, repo-mode, deployment, inference, agent-hosts admin routes) is a bearer-only surface on a public hostname. If the operator token is ever leaked (see C1), remote full control is possible. Bearer-only on the public internet also means the token is the *only* gate — no second factor, no IP allowlist.
- **Remediation:** prefer an internal/private API route or design a separate mTLS/OIDC access layer. Do not merely copy UI Basic Auth: clients already use the `Authorization` header for the operator bearer token, and a single request cannot send Basic and bearer credentials in that header simultaneously. Startup now rejects non-loopback binds without operator tokens, but that does not add a second gate to the public ingress.

### H2. Worker git checkout secret-environment gap (fixed in `111fb9b`)
- **File:** `crates/pharness-worker/src/main.rs` — `repository_git_output` (~lines 1288–1299)
- **Historical finding:** `repository_git_output` once cleared inherited environment without restoring the preparation proxy variables. Commit `111fb9b` now passes only the required proxy allowlist plus the hardened Git variables, keeps hooks disabled and global/system Git config off, and retains the token-via-askpass boundary. See the [Slice 2 release evidence](planning/evidence/autonomous-sdlc/ASTRA-M04E-111FB9B-CONNECTED-LOOP-READINESS-RESTORE.md).
- **Current disposition:** fixed and tested (`pharness-worker` tests plus workspace check recorded in the release evidence). No remaining checkout finding identified in this code path.

### M1. Non-constant-time bearer-token comparison (already fixed)
- **File:** `crates/pharness-api/src/app/auth.rs:34-38` (`token_matches`), used by both `require_worker_token` and `require_operator_token`.
- **Historical finding:** the comparison used ordinary equality. Current `token_matches` compares SHA-256 digests with `subtle::ConstantTimeEq` for both worker and operator tokens.
- **Current disposition:** fixed in the current tree; no code change required.

### M2. Operator auth fails open when no tokens are configured (startup guard added)
- **File:** `crates/pharness-api/src/app/auth.rs:53`
  ```rust
  if state.operator_tokens.is_empty() || request.uri().path() == "/health" {
      return next.run(request).await;   // ← no auth at all when tokens are empty
  }
  ```
- **Historical finding:** middleware still preserves token-free loopback development, but startup now calls `validate_operator_auth_configuration` and refuses any non-loopback bind when `PHARNESS_OPERATOR_TOKENS` is empty. A unit test covers public-bind rejection and the loopback exception.
- **Current disposition:** the dangerous public-bind configuration fails closed. The loopback-only behavior is intentional and documented.

### L1. `run_workspace_command` shell-token blocklist is incomplete (mitigated)
- **File:** `crates/pharness-runhost/src/lib.rs:1920-1987` (`validate_workspace_command`)
- **What:** blocks `;`, `&&`, `||`, backtick, `$(`, `>`, `<` in args but **not** standalone `&` or `|`.
- **Why it's low:** these commands are launched via `Command::new(executable).args(args)` — **not** through `/bin/sh -c` — so shell metacharacters in args are passed literally to the target binary and are *not* interpreted as shell operators. The residual risk is only if a target binary treats an argument as a program/option (e.g. `-c`/`--eval`, which `contains_inline_program` already blocks for `python`/`node`).
- **Remediation (defense-in-depth):** prefer an explicit allowlist of executables + a per-executable argument policy rather than a global denylist; consider blocking `&`/`|`/`$`/`{`/`}` uniformly.

### L2 (INFO). Committed `.DS_Store`
- **Files:** `lucas_engineering/.DS_Store`, `lucas_engineering/charts/.DS_Store`
- **Impact:** local directory/file-name disclosure only. **Remediation:** add to `.gitignore`, `git rm --cached`.

> **Not a finding (false-positive check):** `planning/evidence/autonomous-sdlc/ASTRA-M02-REGISTRY-STABILITY.json` contains a field named `checksum/secret` whose value is a **SHA-256 content hash**, not a credential. No action needed.

---

## Lucas Engineering (`lucas_engineering`) — detailed findings

The three OpenClaw findings below describe the 2026-09-18 snapshot. They were not found in current Lucas `main` (`7eb5784`), and bounded read-only cluster listings found no matching workload, ConfigMap, ServiceAccount, ClusterRoleBinding, Pod or Ingress. Treat them as historical configuration evidence, not proof of a current deployment; repeat these checks before OpenClaw is restored.

### C2. `openclaw` exec-approval socket token hardcoded in a ConfigMap
- **File:** `charts/openclaw/templates/configmap.yaml:72`
  ```yaml
  exec-approvals.json: |
    { "version": 1,
      "socket": { "path": "/home/openclaw/.openclaw/exec-approvals.sock",
                  "token": "[REDACTED]" } ,   # 26-char static token, committed
  ```
- **Impact:** a static, committed token that authenticates against the exec-approval socket. Because it lives in a ConfigMap (not a Secret) and is committed, it's readable by anyone with `get configmap` on the namespace, and it never rotates.
- **Remediation:** move the token to a Secret; rotate it; make the socket auth short-lived/mTLS rather than a static bearer; do not template the value from the repo.

### C3. `openclaw` agent: `cluster-admin` + unrestricted shell/FS tooling
- **Files:**
  - `charts/openclaw/templates/rbac.yaml` — `ClusterRoleBinding openclaw-cluster-admin` binds the `openclaw` ServiceAccount to the **`cluster-admin`** ClusterRole (cluster-wide, all namespaces, all resources).
  - `charts/openclaw/templates/configmap.yaml:74-95` — `exec-approvals.json` defaults `allow` includes **`Bash(*)`, `Read(*)`, `Write(*)`, `Edit(*)`** plus explicit `kubectl *`, `helm *`, `git *`, `ssh *`, `curl *`, `python3 *`, `pip *`, `npm *`.
  - `charts/openclaw/templates/statefulset.yaml` — `KUBECONFIG` mounted from `openclaw-kubeconfig` Secret; `GH_TOKEN` written to `.git-credentials` (`:246-247`); init containers install `kubectl` + `gh`.
  - `charts/openclaw/templates/networkpolicy.yaml` — egress to `443` (external), `22` (homelab VMs `192.168.20.0/24`), and the K8s API (`6443`).
- **Impact (the single biggest blast radius in either repo):** this is an LLM-driven ops agent that has (a) unrestricted shell and filesystem write, (b) `cluster-admin` on the whole cluster via a mounted kubeconfig, (c) `GH_TOKEN`, (d) outbound SSH + 443. A **prompt injection** (e.g. via a repo README, a GitHub issue, a log line it reads) or a **model jailbreak** can therefore: exfiltrate the GH token and any mounted secret, `kubectl` into any namespace (read/update/delete/secrets), pivot to the homelab VMs over SSH, and install backdoors. There is no least-privilege boundary between "agent" and "cluster admin."
- **Remediation (defense in depth):**
  1. **Drop `cluster-admin`.** Give the agent a narrowly scoped Role/RoleBinding (only the namespaces/resources it must manage, verbs it needs). This is the single highest-value fix.
  2. Replace `Bash(*)`/`Read(*)`/`Write(*)` with a tight allowlist of specific commands; keep `Read`/`Write` scoped to the workspace PVC, not `*`.
  3. Require human approval (the exec-approval socket exists for this — wire it to actually gate, don't `allow` everything by default) for `kubectl`, `ssh`, `helm`, and any `write` outside the workspace.
  4. Tighten egress: drop blanket `22`/`443` to an explicit host allowlist; consider a dedicated egress proxy like Pharness uses.
  5. Store `GH_TOKEN` and the kubeconfig as Secrets (already the case) but scope the GitHub token to the minimum (repo, not org/`admin:repo` write if not needed) and the kubeconfig to a scoped SA.

### H3. `openclaw` gateway ingress has no auth annotation
- **File:** `charts/openclaw/templates/ingress.yaml` — exposes `path: /` → the openclaw service with **no** `auth-type`/`auth-secret` annotation.
- **Impact:** the agent gateway's public host is reachable with no ingress-level authentication (relies on app-level token, `OPENCLAW_GATEWAY_TOKEN`).
- **Remediation:** add Basic Auth / mTLS at the ingress (mirroring Pharness' UI ingress) or keep it internal-only.

---

## Verified mitigations (things done well — keep)

These were checked and are **correct**; they reduce the severity of the code-injection surface and are worth preserving:

1. **Egress proxy with exact-host allowlists** — `deploy/helm/pharness/templates/egress-proxy.yaml` + worker egress-proxy code (`crates/pharness-worker/src/main.rs`, `PHARNESS_EGRESS_PROXY_ALLOWED_HOSTS_JSON`). Runner pods have **no direct internet egress**; they go through phase-split CONNECT proxies (`preparation` vs `coding`) with server-owned exact-host lists (`github.com`, `pypi.org`, `files.pythonhosted.org`, `registry.npmjs.org` / `api.fireworks.ai`, `api.openai.com`). Port is pinned to 443 and non-matching hosts are rejected.
2. **Model-gateway inference grants** — HMAC-signed grants with **constant-time** `verify_slice`, expiry enforcement, nonce replay protection, and per-run tool-schema hash pinning (`crates/pharness-core/src/inference.rs`). A well-built anti-forgery/anti-replay boundary for model calls.
3. **Agent-hosts internal auth** — per-host and per-lease bearer credentials, stored and compared as **SHA-256 hashes** (`crates/pharness-api/src/app/agent_hosts.rs` `authorize_host`/`authorize_lease`), so the raw credentials are not persisted.
4. **Codex-host secret handling** — `write_secret_json` uses `0o600` + atomic temp-then-rename (`crates/pharness-codex-host/src/service.rs:673`), and an explicit auth-boundary probe before executing a lease.
5. **Run-phase environment isolation** — `run_acceptance` and `run_workspace_command` (`crates/pharness-runhost/src/lib.rs`) call `env_clear()` and set a minimal env, and patch application binds **preimage SHA-256** hashes so a patch can't clobber an unexpected file.
6. **Secret-shaped path guards** — `secret_shaped_path`/`secret_shaped_project_path` reject `.env*`, `*.pem`, `*.key`, `*kubeconfig*`, `*credential*`, `*secret*`, `*token*` from writable paths and workspace evidence.
7. **Repository URL validation** — `validate_https_git_url`/`validate_git_ref`/`validate_commit_id` (runhost) and the GitHub-only repo parser restrict clone URLs to `https://github.com/...`, reject embedded credentials (`@`), queries, fragments, and `..` — mitigating repo-URL SSRF/credential-injection.
8. **Deployment defaults** — API binds loopback by default; all K8s Services are `ClusterIP` (no `NodePort`/`LoadBalancer`); tokens are injected via `secretKeyRef` (not literals); UI ingress has Basic Auth; pods use `runAsNonRoot`, `drop: ["ALL"]` capabilities, `seccompProfile: RuntimeDefault`, `readOnlyRootFilesystem` where applicable.

---

## Recommended next steps (priority order)

1. **Credential response (C1):** confirm revocation/rotation of the exposed operator token. Coordinate any Git history rewrite and downstream clone cleanup; do not force-push shared history as an incidental cleanup step.
2. **Design the API ingress boundary (H1):** prefer an internal/private route or a distinct mTLS/OIDC layer. The current public `pharness-api.lucas.engineering` ingress has no Basic Auth while the UI ingress does; revalidate CLI, console proxy, agent-host and SSE behavior before rollout.
3. **Secret-scanning control:** no gitleaks/trufflehog workflow was found in this PHarness tree. Add a CI scanner after choosing a safe baseline for the known historical blob, and verify it rejects new secret-bearing commits.
4. **Keep OpenClaw off the current-state issue list** while its source/deployment objects remain absent. Before any redeployment, require scoped RBAC, approval-gated tools, explicit egress, and ingress authentication.
5. **Defense in depth:** replace the run-workspace shell-token denylist with executable-specific argument policies when that contract can be tested without breaking native commands.
6. **Tooling:** when the Rust toolchain is available, install it and run `cargo audit`; the original review was static and did not perform dependency auditing.

*All token/credential values have been redacted (`[REDACTED]`).*
