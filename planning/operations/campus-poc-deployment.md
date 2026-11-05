# Campus PoC deployment (M1)

The Campus proof of concept runs the Campus API, its lab reconciler, broker
and browser terminals, and the Campus page in namespace `campus-poc`. It is
a separate PHarness instance from the operator deployment in `pharness`:
own database, own OIDC client, own identities. The two share only the
release images. Users are synthetic; no AWS or paid model provider is used.

Related: [campus-oidc-auth.md](campus-oidc-auth.md),
[campus-lab-admission.md](campus-lab-admission.md),
[campus-session-jobs.md](campus-session-jobs.md),
[campus-egress.md](campus-egress.md),
[campus-browser-terminal.md](campus-browser-terminal.md).

## Layout

| What | Where | Owned by |
| --- | --- | --- |
| Campus page and `/api/campus/` | `https://campus.lucas.engineering` | `deploy/helm/pharness-campus` (this repository) |
| Test IdP (Keycloak, realm `campus`) | `https://campus-idp.lucas.engineering/realms/campus` | `charts/campus-idp` (lucas_engineering) |
| Lab slots `campus-lab-slot-1..5`, egress proxy, `campus-poc` Namespace, slot RBAC | cluster | `charts/campus-lab` (lucas_engineering) |

Public routing goes Cloudflare tunnel (remotely managed, dashboard only)
to `http://traefik.traefik.svc.cluster.local:80`, then by Ingress. Only two
things are public:

- `campus.lucas.engineering`: `/api/campus/` to Service
  `campus-poc/pharness-campus-api:4777`, `/` to the static Campus page
  (`campus-poc/pharness-campus-ui:80`, which serves `/campus.html` and
  `/assets/` and redirects `/` to `/campus.html`). The operator API of
  this instance, `/health` and the broker are not routed.
- `campus-idp.lucas.engineering`: the `campus` realm and Keycloak static
  resources only; never `/admin` or the master realm.

Never public: the broker (`campus-poc/pharness-campus-broker`, pinned
ClusterIP `10.43.130.31:4790`), lab slots, the egress proxy and the
Keycloak admin console.

The Campus API reaches the IdP by its public name: a hostAlias maps
`campus-idp.lucas.engineering` to the pinned Keycloak ClusterIP
`10.43.130.32`, which serves the same Let's Encrypt certificate
(`campus-idp/campus-idp-tls`, DNS-01). The issuer, endpoints and
certificate are therefore identical for the browser and the back channel,
and the API needs no DNS and no internet.

## Identities and RBAC

| Identity | Can | Cannot |
| --- | --- | --- |
| `campus-poc/pharness-campus-controller` (the API Pod's ServiceAccount; token projected only to `/var/run/pharness-campus/controller`) | In each slot: create/get/list/watch/delete Jobs; get/list/watch/delete Pods; create/get/list/delete ConfigMaps; create/get/list/update/delete Secrets (admission admits only session token Secrets); get/list NetworkPolicies. Cluster-wide: get RuntimeClass `gvisor` | `pods/exec`, Pod creation, anything in `campus-poc` or `pharness`, patching namespaces, quotas, policies or RBAC |
| `campus-poc/pharness-campus-terminal` (token Secret `pharness-campus-terminal-token`, mounted read-only at `/var/run/pharness-campus/terminal`) | In each slot: get/list Pods, create/get `pods/exec` | Everything else, and anything in `pharness` |
| Campus tenant admin | Bootstrap admin `issuer\|3a039679-77a7-4e38-a134-55da25580176` (`campus-admin`); manages course membership | Nothing outside tenant `campus-poc` |

Each identity has its own kubeconfig in ConfigMap `pharness-campus-config`
(`controller-kubeconfig`, `terminal-kubeconfig`), naming the API server by
ClusterIP `https://10.43.0.1:443`. The slot Role and admission policy are
in `charts/campus-lab`; verify them with
`scripts/campus-session-jobs-cluster-test.sh` and the `can-i` checks in
[campus-browser-terminal.md](campus-browser-terminal.md).

Admission into the API is by course membership only; Keycloak supplies no
roles. Instructors and students are members the tenant admin adds.

## Synthetic users

Defined in `charts/campus-idp/values.yaml` with fixed ids (the OIDC
`sub`). Emails are `<username>@campus-poc.example`.

| Username | Subject (`sub`) | Campus role |
| --- | --- | --- |
| `campus-admin` | `3a039679-77a7-4e38-a134-55da25580176` | tenant admin (bootstrap) |
| `instructor1` | `7478093a-d871-408b-95ea-17c3e4456225` | instructor of `cs101` (add via API) |
| `student1` | `309722b2-84a5-42e9-b674-cc4682e676cb` | student of `cs101` (add via API) |
| `student2` | `beb261e7-b3a8-4921-914c-af207433476b` | student of `cs101` (add via API) |

Passwords live in Secret `campus-idp/campus-realm-secrets`, key
`<username>-password`. Read one only on your own terminal and never paste it
into a record:

```sh
kubectl -n campus-idp get secret campus-realm-secrets \
  -o jsonpath='{.data.student1-password}' | base64 -d; echo
```

### Adding a synthetic user

1. Add the password first, or Keycloak cannot start:
   ```sh
   pw="$(openssl rand -base64 18)"
   kubectl -n campus-idp patch secret campus-realm-secrets --type merge \
     -p "{\"stringData\":{\"student3-password\":\"${pw}\"}}"; unset pw
   ```
2. Add the user to `users` in `charts/campus-idp/values.yaml` with a new
   fixed id (`uuidgen`) and an `@campus-poc.example` email; open and merge
   the GitOps PR. The realm checksum rolls Keycloak, which re-imports the
   realm (its database is an emptyDir, so the realm in Git is the whole
   truth; existing Keycloak sessions end).
3. Signed in as `campus-admin`, add the membership. In the browser console
   on `https://campus.lucas.engineering/campus.html`:
   ```js
   const me = await (await fetch('/api/campus/me')).json();
   await fetch('/api/campus/courses/cs101/members', {
     method: 'PUT',
     headers: {'content-type': 'application/json', 'x-campus-csrf': me.csrf_token},
     body: JSON.stringify({
       issuer: 'https://campus-idp.lucas.engineering/realms/campus',
       subject: '<the new user id>',
       role: 'student',
     }),
   });
   ```
   `DELETE` with `{issuer, subject}` removes a member.

## Secrets (out of band, never in Git)

Created once with random values; never print them.

| Secret | Keys |
| --- | --- |
| `campus-idp/keycloak-bootstrap-admin` | `username`, `password` (Keycloak master admin; console reachable only in-cluster) |
| `campus-idp/campus-realm-secrets` | `client-secret` (the `pharness-campus` client), `<username>-password` per user |
| `campus-poc/pharness-campus-secrets` | `oidc-client-secret` (equal to `client-secret` above), `login-cookie-key` (32 random bytes, base64), `operator-tokens` (`campus-operator=<random>`; the operator API is not routed, but PHarness requires a token on a non-loopback bind) |
| `campus-poc/pharness-campus-terminal-token` | ServiceAccount token, populated by Kubernetes (`charts/campus-lab`) |

```sh
kubectl -n campus-poc create secret generic pharness-campus-secrets \
  --from-file=oidc-client-secret=<(kubectl -n campus-idp get secret campus-realm-secrets \
      -o jsonpath='{.data.client-secret}' | base64 -d) \
  --from-file=login-cookie-key=<(openssl rand -base64 32) \
  --from-file=operator-tokens=<(printf 'campus-operator=%s' "$(openssl rand -hex 32)")
```

Rotating the client secret means updating both Secrets, then restarting
Keycloak and `deploy/pharness-campus-api`.

## Campus API configuration

`deploy/helm/pharness-campus/values.yaml` and `files/`:

- Tenant `campus-poc`, organization `org_campus_poc`, database generation
  `dbgen_campus_poc_20261105`; SQLite on PVC `pharness-campus-data`
  (local-path, 1 Gi; kept by Helm and Argo on uninstall, sync prune and
  Application deletion). One replica,
  `Recreate`.
- OIDC: issuer above, client `pharness-campus` (confidential, PKCE S256),
  redirect `https://campus.lucas.engineering/api/campus/auth/callback`,
  post-login path `/campus.html`.
- Labs: `files/campus-labs.json`, slots `campus-lab-slot-1..5`
  (`lab_slot_ids`), course `cs101` with assignments `research-lab` and
  `coding-lab` (1.5 CPU, 1.5 GiB memory, 1 GiB storage each).
- Provider: `PHARNESS_CAMPUS_LAB_PROVIDER=kubernetes` with
  `files/lab-provider.json` (M1 slot map, broker `10.43.130.31:4790`,
  egress proxy `10.43.130.30:3128` and its CA, controller kubeconfig).
  Startup fails if the slot map and the catalog differ or an assignment
  does not fit a slot.
- Terminal: `PHARNESS_CAMPUS_TERMINAL=kubectl` with `files/terminal.json`
  (terminal kubeconfig, `/bin/bash -l`).
- Disabled: inference gateway, direct Fireworks, model-grant signer, Codex
  agent backend, scheduled retention. Worker mode `local`; nothing runs
  agents or calls a model provider.
- Images: the same `pharness-runtime` and `pharness-ui` digests as
  `deploy/helm/pharness/values.yaml`, pinned together by
  `scripts/pharness-release-pin.sh`.

## NetworkPolicies

`campus-poc` (this chart): default deny in and out. The API accepts
Traefik on 4777 and Pods in namespaces labelled
`pharness.dev/campus-lab=true` on 4790; it may reach only the API server
endpoint (`192.168.20.192:6443`) and Keycloak Pods on 8443. No DNS, no
internet. The UI accepts Traefik on 8080 and has no egress.

`campus-idp` (`charts/campus-idp`): Keycloak accepts Traefik (HTTP) and
the Campus API Pods (HTTPS) only.

Lab slots (`charts/campus-lab`): sessions reach only the broker
(`campus-poc/app=pharness-campus-api:4790`) and the egress proxy.

## Rollout order

1. GitOps (merged): `campus-idp` chart; `campus-poc` Namespace, the two
   ServiceAccounts, terminal token Secret and slot RBAC in `campus-lab`.
2. Out of band: the Secrets above.
3. Merge the PHarness change that adds this chart and the Kubernetes
   provider wiring, then cut a release
   (`scripts/pharness-release.sh`). The pin updates both charts' digests.
   Images older than that release do not have the Kubernetes provider or
   the Campus page.
4. Repin the egress proxy to the release runtime digest:
   `scripts/campus-egress-release-pin.sh <gitops-checkout> <runtime-digest>`,
   then open and merge that GitOps PR.
5. Merge the GitOps PR that adds Argo Application `pharness-campus`
   (path `deploy/helm/pharness-campus`, namespace `campus-poc`, ingress
   off). Check Argo Synced/Healthy, the Pod's image ID equal to the pin,
   and the smoke checks below through a port-forward.
6. Public switch-on (owner): in the Cloudflare dashboard add public
   hostnames `campus.lucas.engineering` and `campus-idp.lucas.engineering`
   to the tunnel, service `http://traefik.traefik.svc.cluster.local:80`.
   Then merge the GitOps PR that enables both ingresses. Both TLS
   certificates are DNS-01, so they exist before the routes do.

## Smoke checks

Through `kubectl -n campus-poc port-forward svc/pharness-campus-api 4777`
(the operator token stays in the Secret):

- `GET /health` is 200, and the Pod's image ID equals the pinned digest.
- `GET /api/campus/auth/login` is 302 to
  `https://campus-idp.lucas.engineering/realms/campus/protocol/openid-connect/auth`
  with `code_challenge_method=S256`, and sets `__Host-pharness-campus-login`.
- `GET /api/campus/me` without a session is 401.
- The Pod log shows the reconciler registered `campus-lab-slot-1..5` and no
  provider or kubeconfig error.

After switch-on, sign in as each synthetic user from a browser, launch and
stop a lab, and open its terminal. That scripted demo is a separate step.

## Teardown

1. Delete Argo Application `pharness-campus` (remove its root-app template
   and merge). Its finalizer removes the Deployment, Services, ConfigMaps,
   NetworkPolicies and Ingress. Then delete what it keeps:
   `kubectl -n campus-poc delete pvc pharness-campus-data` and
   `kubectl -n campus-poc delete secret pharness-campus-secrets`.
2. Live sessions: the reconciler is gone, so delete leftover session Jobs,
   Pods, ConfigMaps and Secrets in `campus-lab-slot-1..5` by their
   `pharness.dev/campus-session` label.
3. Disable `campus-idp` in root-app; then
   `kubectl -n campus-idp delete secret keycloak-bootstrap-admin campus-realm-secrets`
   and delete the namespace.
4. Remove the two public hostnames from the Cloudflare tunnel and their DNS
   records.
5. The `campus-poc` Namespace, ServiceAccounts and slot RBAC go with
   `charts/campus-lab` changes; remove them there if the slots stay.

## Residual risks

- The terminal credential is a long-lived ServiceAccount token Secret
  (the API Pod can project only its own ServiceAccount's token). It is
  scoped to `pods/exec` in the slots; rotate by deleting the Secret's data
  (Kubernetes repopulates it) and restarting the API.
- SQLite on a local-path PVC ties the API to one node and has no backup.
- Keycloak uses the `dev-file` database on an emptyDir: it is a test
  IdP, and restarts end every IdP session.
- Pinned ClusterIPs (`10.43.130.30/31/32`) and the API server endpoint in
  NetworkPolicy are cluster-specific.
