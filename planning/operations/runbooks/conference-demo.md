# Conference demo: PHarness trust boundaries

This runbook accompanies the [October Triangle InfoSeCon deck](https://docs.google.com/presentation/d/1Ml2NuZpXcP_y65QuLDyimpHQrz6g1NEM6988QtCHzfs/edit) and the [SecureCarolinas deck](https://docs.google.com/presentation/d/1NJS1UTo615kjGAvCNNmm9SFyhmm0V4AR4XWnr2zL_Ug/edit). The October deck adds architecture, October warnings, and design-response slides; the core demonstration is the same. Slide numbers below are October / SecureCarolinas.

**What is ready:** a repeatable, model-free demonstration of a typed allowed action, a secret-shaped request denied before execution, and durable audit evidence in the console. It creates audit records in the chosen API database. It does not read a Kubernetes Secret or mutate a cluster. Each run adds records; use a fresh disposable database for a clean rehearsal.

**What is not established:** the deck's free-form, multi-system release-promotion prompt (11 / 8) and its deterministic end-to-end release fixture are not implemented by these steps. The allowed image example parses a reference; it does not contact a registry or verify a digest. An Ask decision, an agent-generated action, sandbox network enforcement, a complete trace, an SBOM, signing, and full source-to-runtime provenance are not demonstrated here. Present these as architecture or separate evidence, not as results of this demo. The optional Release verifier below is a narrower, real read with prerequisites. Do not describe a green API health check or a parsed tag as release safety.

## Live cluster rehearsal and access

On 2026-10-28, the `lucas_engineering` context, namespace `pharness`, served API/UI revision `33dc3ea6cc02e2939411447c7464806370bed71e`. Preflight, allow, deny, and audit passed through the UI service proxy. The two actions added durable live audit records. At that time `/api/releases` returned no Releases, so the optional Release step could not be rehearsed.

The audit-list fix was deployed and verified on 2026-10-29 UTC: API/UI source revision `f2ea82930f332f76e829e66e27cf5806a4b1e24f`, GitOps revision `bd010e17b80a44f2d7115e5fb567291ee93820b7`. The deployed Insights page displayed the rehearsal's denied and executed events after a browser reload. Recheck the served revision and page on the day.

For live access, first verify the context and start this port-forward in a terminal:

```sh
kubectl config current-context
kubectl --context lucas_engineering -n pharness port-forward svc/pharness-ui 15174:80
```

In another terminal, configure the helper to use that same authenticated UI proxy and API instance:

```sh
export PHARNESS_DEMO_API=http://127.0.0.1:15174
export PHARNESS_DEMO_UI=http://127.0.0.1:15174
scripts/conference-demo/demo.py preflight
```

To preview the source fix against the live API before deployment, start `PHARNESS_API_PROXY=http://127.0.0.1:15174 npm run dev -- --port 15175 --strictPort` from `ui/`, then set `PHARNESS_DEMO_UI=http://127.0.0.1:15175` in the helper terminal. The browser and commands then use the same live database. The port-forward grants access through the UI's operator proxy; keep both listeners on loopback and stop them after the rehearsal. On the day, recheck deployment revision, connectivity, and target before relying on this setup.

## Prepare a local, isolated fallback

Use three terminals from the repository root. Dependencies: Rust toolchain, Python 3, Node/npm, and installed UI dependencies. Build before the conference, and rehearse on the same revision and laptop. The commands bind loopback only. Do not expose the development server to a network. Ports 4777 and 5173 must be free; check the startup output for binding failures. Keep both servers running while presenting.

Terminal 1 — compile and start the API with a new database (no deployment, no cluster credentials required for the two direct actions):

```sh
cargo build -p pharness-api
mkdir -p target/conference-demo
DEMO_DIR="$(mktemp -d "$PWD/target/conference-demo/demo-XXXXXXXX")"
DEMO_DB="$DEMO_DIR/pharness.sqlite"
printf 'Demo database: %s\n' "$DEMO_DB"
PHARNESS_BIND=127.0.0.1:4777 PHARNESS_DB_PATH="$DEMO_DB" \
  env -u FIREWORKS_API_KEY -u PHARNESS_OPERATOR_TOKENS target/debug/pharness-api
```

The database is created at the printed path and remains in `target/conference-demo`; save the path if you want to preserve evidence. The steps below never invoke a model. Existing environment configuration may affect policy; the preflight and expected outputs are mandatory.

Terminal 2 — start the console, after installing dependencies with `npm ci` in `ui/` if necessary:

```sh
cd ui
PHARNESS_API_PROXY=http://127.0.0.1:4777 npm run dev -- --port 5173 --strictPort
```

Terminal 3 — run the demo. Defaults are the local URLs above. Open the URL printed by each step. The helper detects the legacy versus newer UI; reload the audit page if the new row has not appeared yet.

```sh
scripts/conference-demo/demo.py preflight
```

Expect `health.ok: true`, `deny_secret_access: true`, and normal operational mode. Open the printed Status/Settings page and confirm it loads. If the API or console cannot load, stop and use previously captured, clearly labeled screenshots. Worker availability is not needed for the deterministic sequence.

## On stage

1. **Typed intent and allow (12 / 9).**

   ```sh
   scripts/conference-demo/demo.py allow
   ```

   Expect `decision: allow`, `status: ok`, `executed: true`. Explain that this direct API request is a typed, deterministic proposal with no model involved. It parses `team/checkout-api:v0.1.0-smoke`; `verification_status: unknown` and a null digest are expected. Open the printed audit page and show `direct_capability.executed`.

2. **Policy denial (13 / 10).**

   ```sh
   scripts/conference-demo/demo.py deny
   ```

   Expect `decision: deny`, `status: denied`, `executed: false`. This is a request for the resource type `secrets`, not a Secret name or value. Refresh the audit page and show `direct_capability.denied`. In the legacy audit view, expand the payload to show the decision and `executed`; the newer Insights list shows the event but not its full payload. Keep the terminal response visible for the full decision.

3. **Evidence (15 / 12).**

   ```sh
   scripts/conference-demo/demo.py audit
   ```

   The output contains the latest matching conference capability audit records, their unique action IDs, decisions, and execution flags. Open the audit page printed by the helper. These are direct capability events with actor `api` and no agent Run; they do not prove the slide's illustrative agent trace or end-user identity. The helper searches at most the latest 100 capability events; a busy database may require a more specific API query.

Stop the two local processes with Ctrl-C when finished. Retain the disposable SQLite database if needed; delete only the specific demo directory after deciding evidence is no longer needed. No automatic reset or cleanup touches existing databases.

## Optional: real post-sync development Release (11 / 8 and 16 / 13)

Only use this section if a suitable **existing** Release and target have been prepared and inspected in advance. Follow [release verification preconditions](release-verification-smoke-playbook.md): approved or completed, WorkItem-backed non-production `dev` Release, exact Deployment target, current completed Argo sync artifacts, read access, and any required Prometheus inventory. Confirm its source, target, actual sync, and runtime evidence separately. This operation performs typed reads and **writes observations/audit evidence to the PHarness database**; it does not complete the Release, sync Argo, or promote anything. It is not the broad model-driven assessment promised by the slide and does not establish Loki, traces, SBOM, signing, or full provenance.

Set the API and UI to the same intended instance. For a protected API, obtain the operator token using your normal private mechanism and set `PHARNESS_API_TOKEN` without putting it in the shell history or slides; the helper reads it from the environment. The console must separately be authenticated to the same instance. Use a secure tunnel or TLS for remote access. Check targets and access before the conference.

```sh
export PHARNESS_DEMO_API=http://127.0.0.1:4777
export PHARNESS_DEMO_UI=http://127.0.0.1:5173
scripts/conference-demo/demo.py preflight
scripts/conference-demo/demo.py release --release-id 'REPLACE_WITH_VERIFIED_DEV_RELEASE_ID'
```

Review the printed Release before the show, and expect `verified: true` only if the checks actually pass. Open the printed Release/WorkItems and Observations pages. If the API reports a conflict, timeout, or `attention_required`, show the actual result and stop; do not retry an uncertain POST blindly, force completion, or infer a production decision. The legacy console may require navigating from WorkItems to the owning delivery chain.

## Presentation corrections and rehearsal checklist

- Slide 7's PHarness-as-policy-broker architecture and slide 10's five boundaries are design statements; identify which controls are actually evidenced in the chosen environment. The `lea-agent-fireworks-egress` policy on 14 / 11 belongs to LEA, not proof of PHarness worker isolation; show the relevant reviewed policy and observed enforcement separately if available.
- Slide 11 / 8 promises a deterministic fallback for the full release prompt. This runbook supplies a narrower policy/audit fallback. Change the spoken claim or the slide before presenting; do not imply the full fixture ran.
- Slide 15 / 12 shows a conceptual trace sequence. Direct API audit events demonstrate proposal/decision outcome records, not an entire agent run. If presenting a real Run, select a pre-reviewed, sanitized Run and open its detail page; verify events and permissions in advance.
- Slide 16 / 13 includes “tests + SBOM”; repository release guidance says digest identity alone is not an SBOM, signature, or vulnerability attestation. Show only artifacts actually available and label signed provenance as future work.
- Rehearse API and browser against the **same** database, test projector scaling and network, remove sensitive tabs, verify displayed data is appropriate for a public audience, and capture sanitized fallback screenshots. Record the source revision and expected API/UI build serving the actual conference instance. A local rehearsal does not establish cluster readiness.
