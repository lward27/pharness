# Campus session network isolation and approved egress

Status: in force on the M1 lab slots (`campus-lab-slot-1..5`) since GitOps
`lucas_engineering` merges `c56527b` (PR #127) and `18cbf85` (PR #128), Argo
app `campus-lab`. Qualified in the cluster with
`scripts/campus-egress-cluster-test.sh` (LUC-130). The proxy image is built
from the LUC-130 branch and pinned by digest; the provider wiring ships with
the next PHarness release.

## Design

Lab slots deny all ingress and egress by default. A session Pod can open TCP
connections to exactly two things outside its own Pod:

| Destination | Who | Opened by |
| --- | --- | --- |
| Campus broker, `campus-poc/app=pharness-campus-api` port 4790 (Service `pharness-campus-broker`, pinned ClusterIP `10.43.130.31` in the provider) | every session | slot NetworkPolicy `campus-lab-session-broker` |
| Campus egress proxy, `campus-egress/campus-egress-proxy` port 3128 (ClusterIP `10.43.130.30`) | Pods labelled `pharness.dev/campus-egress=proxy` | slot NetworkPolicy `campus-lab-session-egress` |

There is no DNS in a slot. Session tools use `HTTPS_PROXY`, and the proxy
does the name resolution.

The proxy (`pharness-worker`, execution kind `campus_egress_proxy`;
rules in `crates/pharness-workspaces/src/campus_egress.rs`) accepts only
`CONNECT <approved host>:443`. It then:

1. finds the session Pod that owns the source address by listing Pods in the
   slot namespaces on every tunnel (Role: `list pods` only). That Pod must be
   running, carry the controller and egress labels, and approve the host in
   `pharness.dev/campus-egress-hosts`. If the lookup fails, the tunnel is
   refused;
2. resolves the host itself and refuses the tunnel when any answer is not a
   public unicast address. It dials an address it has already checked, so a
   DNS-rebinding answer is never used. The proxy's own NetworkPolicy also
   denies private and special ranges;
3. completes verified TLS to the upstream (web PKI roots, the approved name),
   then terminates the session's TLS with a certificate from the Campus
   egress CA (cert-manager, `campus-egress-proxy-tls`), offering only
   HTTP/1.1;
4. checks every request inside the tunnel:

| Route | Hosts | Allowed |
| --- | --- | --- |
| package | `pypi.org`, `files.pythonhosted.org`, `registry.npmjs.org` | `GET`, `HEAD` |
| git read | `github.com` | `GET`, `HEAD`, and `POST /<owner>/<repo>/git-upload-pack` with `application/x-git-upload-pack-request` |
| retrieval | `codeload.github.com`, `raw.githubusercontent.com` | `GET`, `HEAD` |

On every route the proxy refuses `receive-pack` anywhere in the decoded
target (git push), protocol upgrades (WebSocket, h2c), absolute-form
targets, a `Host` header that differs from the tunnel's host, and request
bodies over 8 MiB. Redirects go back to the client unchanged. Following a
redirect opens a new tunnel, which is checked again from step 1. Each
decision is logged with the session and a refusal code, and without
credentials.

A session's approved hosts are the destinations of its effective policy.
The provider refuses any destination that is not one of these routes on
port 443.

## Provider enforcement

`KubernetesCampusProvider` refuses a launch before it creates anything when:

- a required slot NetworkPolicy is missing (`campus-lab-default-deny`,
  `campus-lab-session-broker`), or the default deny no longer selects every
  Pod for both ingress and egress;
- the session approves egress, but the provider has no egress proxy
  configured, or the slot has no `campus-lab-session-egress` policy;
- a destination is not an approved route.

Each session Pod starts with a `network-gate` init container. It probes
canaries that the slot policy denies (the API ClusterIP `10.43.0.1:443` and
`1.1.1.1:443`). The workspace starts only after two consecutive rounds find
every canary closed. If the canaries stay open for 60 s, the Pod fails. This
gate covers the window after Pod start before the network-policy controller
has programmed the Pod's rules: in the cluster, an ungated Pod under
default deny reached the API for up to 0.9 s after it started.

For egress sessions, the provider adds the egress label, the hosts
annotation, `HTTPS_PROXY`, `NO_PROXY=<broker IP>`, and the CA bundle
variables (`SSL_CERT_FILE`, `GIT_SSL_CAINFO`, `REQUESTS_CA_BUNDLE`,
`PIP_CERT`, `NODE_EXTRA_CA_CERTS`). These variables point at `egress-ca.pem`
in the read-only broker token volume. The CA certificate is public. Its key
never leaves `campus-egress`.

## Cluster objects (`charts/campus-lab`)

| Object | Purpose |
| --- | --- |
| Namespace `campus-egress`, ServiceAccount, Deployment and Service `campus-egress-proxy` | the proxy, `runAsNonRoot`, read-only root, `fsGroup 65532` for the TLS Secret |
| cert-manager self-signed issuer, CA Certificate, CA issuer, `campus-egress-proxy-tls` | session-facing certificate for the six names |
| NetworkPolicy `campus-egress-proxy` | ingress only from slot Pods with the egress label; egress only to public addresses on 443, DNS, and the API server endpoint `192.168.20.192/32:6443` |
| Role/RoleBinding in each slot | the proxy may `list pods`, and nothing else |
| NetworkPolicies `campus-lab-session-broker`, `campus-lab-session-egress` in each slot | the two allowed destinations |
| controller Role additions | Secrets create/get/list/update/delete (the session token Secret); NetworkPolicies get/list. No create, update, patch or delete on policies, no namespace patch |
| ResourceQuota | two Secrets per slot |
| VAP `pharness-campus-lab-session-guard` | additionally requires the `network-gate` init container (exactly one, not a sidecar), a consistent egress label and hosts annotation with approved hosts only, and the token volume (one non-optional Secret volume of the session's broker token Secret, mounted read-only without `subPath`) |
| VAP `pharness-campus-lab-secret-guard` | slot Secrets must be `Opaque` broker token Secrets: the derived name, controller/tenant/session/slot/generation/`broker-token` labels with the slot matching the namespace label, and only the `token` key plus optional `egress-ca.pem` |

## Qualifying a cluster

```sh
scripts/campus-egress-cluster-test.sh \
  --chart <lucas_engineering>/charts/campus-lab --out /tmp/campus-egress
```

The script needs cluster-admin, helm, jq, openssl and cargo. It launches
real sessions through the controller identity, using the ignored tests
`crates/pharness-workspaces/tests/campus_session_jobs_live.rs` (phases
`egress-provision`, `egress-delete`, `enforcement`) and
`crates/pharness-api/src/app/campus/tests/broker_live.rs`. The probes
(`scripts/campus-egress-probe.py`) run inside the sessions. It deletes
everything it creates and prints what remains.

The broker phase uses the API journal in process. The broker runs in the
test, not in the production API, so the test proves token delivery,
scraping, rotation and revocation. It does not prove HTTP reachability of
the broker from the Pod. The phase uses a probe namespace labelled with the
journal's slot id (`tenant-a-lab-slot-01`).

## Results (2026-11-05, run `035824`)

167 checks passed, 0 failed, and nothing was left behind.

- Functional, from real sessions under gVisor in Argo-managed slots:
  `pip install requests==2.32.3` (12 s), `npm install is-number@7.0.0` (5 s),
  `git clone` of `octocat/Hello-World` on both the Python and Node runners,
  codeload tarball, raw file, and PyPI simple index. `git push` was refused
  (`campus egress refused: git_push`).
- Unreachable from egress and non-egress sessions: the API (ClusterIP and
  endpoint), the kubelet, node SSH, cloud metadata, the pharness-api Service
  and Pod (4777), the model gateway, kube-dns (TCP and UDP), public DNS, name
  resolution, raw public IPs on 443, 80 and 22, and a peer session's
  listener. Sessions without egress cannot reach the proxy at all.
- Refused by the proxy: CONNECT to raw IPv4 and IPv6, metadata, the cluster
  API by IP and by name, unapproved hosts, approved-host suffixes, trailing
  dots, user info, ports 80 and 22, and a host approved only for another
  session; plain HTTP forwarding; HTTP/2 prior knowledge; data before the
  tunnel; ALPN `h2` only; POST and PUT to package indexes; a mismatched Host;
  absolute-form targets; WebSocket and h2c upgrades; receive-pack discovery,
  POST and its percent-encoded form; upload-pack with another content type;
  POST to other GitHub paths; redirects off the approved hosts.
- Admission: 16 session-Job and 10 Secret cases, and 27 RBAC checks (the
  controller cannot delete, widen or add policies, or unlabel a slot; the
  proxy can only list Pods).
- Enforcement: no proxy configured, a missing egress policy, a missing
  isolation policy, a deleted broker policy, and a default deny weakened to
  ingress only each refused the launch and created nothing. With no policy
  at all, the gate failed the Pod and the workspace never started.

## Residual risks

- Standard NetworkPolicy has no FQDN rules. The proxy's own egress is
  "public addresses on 443", and the host approval happens in the proxy.
  A proxy bug would widen egress to any public HTTPS host, but not to private
  ranges.
- The proxy decrypts session traffic. The CA key lives only in
  `campus-egress`, and the CA is trusted only inside sessions.
- One proxy replica on the shared lab node. When it is down, egress fails
  closed.
- The network gate depends on canaries that are always reachable when they
  are not denied. If `1.1.1.1` is unreachable from the node and the API
  ClusterIP changes, the gate could pass early. Both canaries are
  configurable.
- `KUBERNETES_SERVICE_*` variables are still injected by the kubelet. They
  carry no credential, and the API is unreachable.
- The proxy logs use ANSI colours. The qualification script strips them.
