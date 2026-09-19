# Pharness & Lucas Engineering — Security Review

**Scope:** `/home/wardl/Personal/pharness` (Rust control plane: `pharness-api`, `pharness-worker`, `pharness-runhost`, `pharness-model-gateway`, `pharness-codex-host`, `pharness-core`) and `/home/wardl/Personal/lucas_engineering` (Helm/K8s cluster-ops repo, incl. the `openclaw` agent deployment).
**Method:** static, local-only review of source + deployment manifests. No builds, no network, no secrets emitted (all token/credential values are `[REDACTED]`).
**Date:** 2026-09-18.

> All secret values below are redacted. Where a finding names a secret, only its *location and nature* are given, never its value.

---

## Summary

| # | Severity | Repo | Finding |
|---|----------|------|---------|
| 1 | **CRITICAL** | pharness | Live operator bearer token committed to git (`.claude/launch.json`) |
| 2 | **CRITICAL** | lucas | `openclaw` exec-approval socket token hardcoded in a ConfigMap |
| 3 | **CRITICAL** | lucas | `openclaw` agent is `cluster-admin` + unrestricted `Bash(*)`/`Read(*)`/`Write(*)` |
| 4 | **HIGH** | pharness | Public `pharness-api` ingress exposes the entire API with no Basic Auth (bearer-only) |
| 5 | **HIGH** | pharness | Worker git-checkout inherits the full secret environment and does not disable repo hooks |
| 6 | **HIGH** | lucas | `openclaw` gateway ingress exposes `/` with no auth annotation |
| 7 | **MEDIUM** | pharness | Bearer-token comparison is not constant-time (`token_matches`) |
| 8 | **MEDIUM** | pharness | Operator auth fails open when no tokens are configured |
| 9 | **LOW** | pharness | `run_workspace_command` shell-token blocklist is incomplete (mitigated: not shell-run) |
| 10 | **LOW** | lucas | Committed `.DS_Store` files (local filename disclosure) |
| 11 | **INFO** | both | Several controls are well-built (documented below as "verified mitigations") |

**Headline:** the highest-severity items are *secret management + blast radius*, not exotic code bugs. Two real tokens are committed to git, and the `openclaw` agent combines unrestricted shell/FS tooling with `cluster-admin` and outbound network (SSH + 443) — a single prompt-injection or model jailbreak there yields full cluster compromise.

---

## Pharness — detailed findings

### C1. Committed live operator token in `.claude/launch.json`
- **File:** `.claude/launch.json:7` (git-tracked)
- **What:** a 64-hex bearer token is baked into the committed VS Code/Claude launch config:
  ```json
  "PHARNESS_API_PROXY=http://127.0.0.1:24777",
  "PHARNESS_API_PROXY_TOKEN=[REDACTED]"
  ```
- **Impact:** anyone with read access to the repo (clones, forks, CI logs, `.bundle`/archive exports) obtains a working operator credential. Combined with H1 (API reachable on a public hostname with bearer-only auth), this is a direct path to full operator control of the API from the internet.
- **Remediation:**
  1. Rotate the token immediately (treat as compromised).
  2. Remove the value from the file and from git history (e.g. `git filter-repo` / BFG), or stop tracking the file.
  3. Add a pre-commit/CI secret scan (gitleaks/trufflehog) so this cannot recur.
  4. Source dev tokens from a gitignored `.env` / secrets store instead of a tracked config.

### H1. Public API ingress has no Basic Auth (bearer-only)
- **File:** `deploy/helm/pharness/templates/ingress.yaml` — the `pharness-api` ingress block (third resource in the file, `path: /`, `pathType: Prefix` → `pharness-api:4777`, no `auth-type: basic` annotation).
- **Contrast:** the `pharness-ui` ingress in the *same file* **does** set `nginx.ingress.kubernetes.io/auth-type: basic` + `auth-secret`. The `pharness-api` and `pharness-agent-host-api` ingresses do not.
- **Config:** `deploy/helm/pharness/values.yaml` has `ingress.apiEnabled: true` (default) and `api.host: pharness-api.lucas.engineering`.
- **Impact:** the entire operator API (runs, work-items, repo-mode, deployment, inference, agent-hosts admin routes) is a bearer-only surface on a public hostname. If the operator token is ever leaked (see C1), remote full control is possible. Bearer-only on the public internet also means the token is the *only* gate — no second factor, no IP allowlist.
- **Remediation:** add the same Basic Auth (or a mTLS / OIDC proxy) annotation to the `pharness-api` ingress, or restrict it to an internal/private ingress; at minimum, require that `ingress.apiEnabled` and a token be set together (see M2).

### H2. Worker git checkout leaks the secret environment to untrusted repo hooks
- **File:** `crates/pharness-worker/src/main.rs` — `repository_git_output` (~lines 1288–1299)
- **What:** the git commands used to clone/checkout the *target repository* set `GIT_TERMINAL_PROMPT`, `GIT_ASKPASS`, `GIT_CONFIG_NOSYSTEM`, but do **not** call `.env_clear()` and do **not** pin `core.hooksPath`. They therefore inherit the worker's **full** environment, which includes `PHARNESS_WORKER_TOKEN`, `PHARNESS_SOURCE_READER_TOKEN`, `PHARNESS_GIT_WRITER_TOKEN`, etc.
- **Why it matters:** when git fetches/checks out a repo, any **git hook** shipped in that repo (`.git/hooks`, or `post-checkout`/`prepare` invoked by git) runs as a child of git and inherits that environment. A malicious or compromised repository could therefore read the worker/source/writer tokens and exfiltrate them. The **preparation** phase has network egress (see "verified mitigations" — egress proxy allows `github.com`/`pypi.org`/`files.pythonhosted.org`/`registry.npmjs.org`), so exfiltration is feasible.
- **Contrast (the good news):** the *run* phase is hardened — `ProjectTools::run_acceptance` and `run_workspace_command` in `crates/pharness-runhost/src/lib.rs` both call `.env_clear()` and set a minimal env (PATH/HOME/LANG only). The gap is specifically the **preparation/checkout** phase.
- **Remediation:**
  1. In `repository_git_output`, call `.env_clear()` and re-add only `PATH`, `HOME`, `LANG`, `GIT_TERMINAL_PROMPT`, `GIT_ASKPASS`, `GIT_CONFIG_NOSYSTEM`.
  2. Pin `core.hooksPath` to an empty/controlled dir (or pass `-c core.hooksPath=/dev/null`-style config) for every checkout so repo-bundled hooks never run.
  3. Optionally set `GIT_OPTIONAL_LOCKS=0` and run fetch in a directory with no hooks.

### M1. Non-constant-time bearer-token comparison
- **File:** `crates/pharness-api/src/app/auth.rs:34-38` (`token_matches`), used by both `require_worker_token` and `require_operator_token`.
- **What:** compares `Sha256(provided) == Sha256(expected)` with a normal `==`. Not constant-time.
- **Impact:** theoretical timing side-channel on a 256-bit digest. Practical exploitability over a network is low (noise, and the digest, not the raw secret, is compared), so this is **medium/low** in practice — but it is the standard thing to fix.
- **Remediation:** use a constant-time comparison (e.g. `subtle::ConstantTimeEq` or `ct_eq` on the digest bytes).

### M2. Operator auth fails open when no tokens are configured
- **File:** `crates/pharness-api/src/app/auth.rs:53`
  ```rust
  if state.operator_tokens.is_empty() || request.uri().path() == "/health" {
      return next.run(request).await;   // ← no auth at all when tokens are empty
  }
  ```
- **Impact:** if `PHARNESS_OPERATOR_TOKENS` is unset/empty *and* the API is bound to a non-loopback address, **every** operator route is unauthenticated. The default bind is loopback (`127.0.0.1:4777`, `crates/pharness-config/src/lib.rs:18`), so the default local posture is safe — but a `PHARNESS_BIND=0.0.0.0` deployment that forgets the tokens is silently wide open.
- **Remediation:** fail *closed*: if the bind address is non-loopback, refuse to start (or require at least one operator token) when `operator_tokens` is empty. At minimum, log a prominent warning.

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

1. **Rotate + purge the committed tokens** (Pharness C1, Lucas C2) and add a CI secret scanner. *Do this first — these are live.*
2. **De-privilege `openclaw`** (C3): replace `cluster-admin` with a scoped Role, replace `Bash(*)`/`Read(*)`/`Write(*)` with a tight allowlist, and actually gate high-risk ops through the exec-approval socket.
3. **Close the API ingress auth gap** (H1) — add Basic Auth/mTLS to the `pharness-api` ingress or make it internal-only; fail-closed on missing operator tokens (M2).
4. **Harden worker checkout** (H2): `env_clear()` + pin `core.hooksPath` for all repository git commands.
5. **Constant-time token compare** (M1) — quick `subtle::ConstantTimeEq` swap.
6. **Housekeeping:** auth on the openclaw ingress (H3), remove committed `.DS_Store` (L2), tighten the `run_workspace_command` arg policy (L1).
7. **Tooling:** install the Rust toolchain and run `cargo audit` (blocked earlier — toolchain absent) to surface vulnerable dependencies; run `gitleaks`/`trufflehog` across both repos.

*All token/credential values have been redacted (`[REDACTED]`).*
