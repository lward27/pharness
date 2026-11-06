# Campus M1 demonstration: one governed lab session, login to verified teardown

`ui/scripts/campus-m1-demo.mjs` runs the M1 demonstration (LUC-137) against the
deployed Campus PoC in namespace `campus-poc`. A real browser (Playwright
Chromium) signs the synthetic users in through Keycloak, launches labs, drives
the browser terminal and presses **Stop lab**. `kubectl` on the operator's
context records the deployed inputs and checks, independently of the browser,
that the session's authority and resources are gone afterwards. Every check
lands in one evidence bundle.

The demo uses only the synthetic realm users (`campus-admin`, `instructor1`,
`student1`, `student2`) and the broker's fake inference model `campus-fake`.
No paid provider is called.

## Before running

- The Campus PoC is deployed from the same revision as the checkout that runs
  the harness. The harness compares the deployed `campus-labs.json` with
  `deploy/helm/pharness-campus/files/campus-labs.json` in the checkout and
  fails preflight when they differ.
- The catalog has assignment `cs101/research-timeout-demo` (research profile,
  300 s absolute lifetime). It ships in the chart; there is no GitOps change.
- The `kubectl` context (default `lucas_engineering`) can read
  `campus-idp/campus-realm-secrets`, Argo Applications, the admission policies
  and the lab slots, and can `exec` into lab Pods. The harness reads the four
  `<user>-password` keys into memory and types them into the Keycloak form;
  it never prints or saves them.
- No lab session of `student1` or `student2` needs to be stopped by hand: the
  harness stops any live one first and waits for it to end.
- Node and the UI dependencies, once per checkout:

  ```bash
  cd ui && npm ci && npx playwright install chromium
  ```

## Running

```bash
cd ui
# Before the public switch-on: reach both hostnames through a port-forward to
# Traefik. TLS, hostnames and the Origin are still the public ones.
node scripts/campus-m1-demo.mjs --port-forward
# After the switch-on, through DNS and the Cloudflare tunnel:
node scripts/campus-m1-demo.mjs
```

A run takes about 15 to 20 minutes. It prints `PASS`/`FAIL` per check and
`TIME` per measured interval, and exits 0 only when every check passed, no
step raised an error and the evidence secret scan found nothing. Useful
options: `--out <dir>` (outside the repository, empty), `--headed`,
`--skip-coding`, `--skip-timeout`, `--context <ctx>`; `--help` lists all.
`--self-test` checks the parsers and the secret scan offline.

`python3 scripts/tests/campus-m1-demo-test.py` runs both in-sandbox probes
against a fake broker on loopback and the harness self-test; CI runs it.

## What each step checks

**Pinned inputs and preflight.** Records the harness revision and file hashes,
the Kubernetes version, the Argo Applications `pharness-campus`, `campus-lab`
and `campus-idp` (sync, health, revision), the API and UI images with each
Pod's running image ID, hashes of the deployed `campus-labs.json`,
`lab-provider.json` and `terminal.json`, the catalog's profiles and policy
layers, the three ValidatingAdmissionPolicies, each slot's NetworkPolicies,
the `gvisor` RuntimeClass and the egress proxy and IdP images. Fails unless
`pharness-campus` is Synced and Healthy, one API Pod runs the pinned digest
with the Kubernetes provider and the kubectl terminal, the public origin is
the base URL, the deployed catalog equals the checkout, and every slot has its
label and its default-deny and broker NetworkPolicies.

**Sign-in and membership.** Each user signs in through the Keycloak form and
`/api/campus/me` returns their subject. `campus-admin` enrolls `instructor1`
as instructor and both students in `cs101`. The assignments list shows the
research, coding and timeout assignments as launchable, the last with a 300 s
lifetime.

**Launch envelope.** Launch bodies naming `image`, `namespace`, `resources`,
`expires_at` or `profile_id` are refused with 400 `invalid_request`; a launch
in course `cs999` is refused with 403 or 404.

**Research lab (`student1`, Python runner, network denied).**

- Launch is created and reaches `ready`; a second launch is refused with 409
  `active_session_exists`.
- `kubectl` finds the session Pod in exactly one slot. The Pod runs under
  gVisor, without a service-account token, with literal environment only,
  without host network or a shared process namespace and with a read-only
  root; every image is pinned by digest, the session image is the catalog
  profile's digest, and the Job's `activeDeadlineSeconds` does not exceed the
  session lifetime.
- `instructor1` sees the session and its owner in the course view.
- The terminal connects and `codex --version` answers.
- The research probe (`scripts/campus-m1-demo-probe.py`) is typed into the
  terminal and runs inside the sandbox: the broker token is bound to this
  session, fake inference answers, another model is refused, absent broker
  routes are 404, missing and forged tokens are refused, there is no
  service-account token or credential-shaped environment variable, the token
  is not in any process's environment or arguments, the token mount is
  read-only, processes belong to the student, the process limit applies,
  there is no DNS, and HTTPS to an outside host, raw IPs, the egress proxy,
  the Kubernetes API, the Campus operator API and cloud metadata are all
  unreachable.
- Separately, a poller runs through `kubectl exec` and confirms the broker
  accepts the session's token.
- `student2` gets 404 `session_not_found` for the session, its terminal
  ticket, stop and reset, and 403 for the course's session list;
  `instructor1` cannot get a terminal ticket for a student's session; neither
  `student2` nor an unauthenticated browser can open the terminal WebSocket;
  the owner's cookie is upgraded with the Campus Origin (control) and refused
  from a foreign Origin; a terminal ticket works once and is refused on reuse
  with 4001 `ticket_rejected`; `student2`'s Campus page shows "not found".
- **Stop lab** is pressed on the Campus page. Then: the open terminal closes
  with `session_stopped`; the poller's next request is refused within 60 s;
  the session reaches `stopped`; the Job, Pod, token Secret and anchor
  ConfigMap are gone from the slot within 300 s (the tombstone ConfigMap stays
  as the slot's fence and is not a live resource); a new terminal ticket is
  refused with 409 `session_stopped`.

**Absolute expiry (`student2`, `research-timeout-demo`).** Starts after the
research lab and runs alongside the coding lab. The expiry is fixed at
admission (300 s), the Job's deadline is at most the remaining lifetime, the
terminal connects and `codex --version` answers, and the poller sees the token
accepted. The terminal is still open 10 s before expiry. At expiry nobody
presses anything: the terminal closes with `session_expired`, the broker
refuses the token within 60 s, the session reaches `stopped`, its resources
are gone within 300 s and a new terminal ticket is refused.

**Coding lab (`student1`, Node runner, approved egress through the proxy).**
The same launch, Pod, course-view, terminal and stop checks as the research
lab, with the coding probe (`scripts/campus-m1-demo-probe.mjs`): the shared
broker and sandbox checks, plus `HTTPS_PROXY` points at the Campus proxy, the
proxy admits an approved host (pypi) and refuses an unapproved one with 403,
`git ls-remote` works against github.com and fails against gitlab.com, and
`git push` fails, with the proxy refusing the receive-pack discovery request
(403, `git_push`).

**Sign-out and final state.** After **Sign out**, `/api/campus/me` is 401.
No live object of any demo session remains in any slot.

**Cleanup and evidence.** Whatever the outcome, the harness stops any demo
session still live, then writes the bundle and scans it for every password,
cookie value and broker-token-shaped string it held. A hit fails the run and
names the file to delete before sharing.

## Evidence bundle

Default `/tmp/luc137-demo-<UTC timestamp>/`:

- `evidence.json`: options, pinned inputs, identities (issuer, subject,
  email), assignments, per-session status history, workload description,
  poller lines, timings, every check and the summary.
- `pinned-inputs.json`: the pinned inputs alone.
- `screenshots/`: signed in, each lab ready, agent CLI, probe output, peer
  refusal, after stop or expiry, signed out.
- `transcripts/<phase>.txt`: the terminal output received by the browser.
  Only received frames are captured, so the ticket the browser sends is not.
- `api-log.txt`: Campus API log lines naming a demo session, the audit
  record for launch, ticket, connect, stop, expiry and cleanup.
- `kubectl-commands.log`: every kubectl command the harness ran (no output).

Tracing, video and HAR recording are off.

## Limits

- The harness checks the browser's view and the cluster's view; it does not
  read the SQLite journal directly. The API log and the course view stand
  in for the audit trail.
- Revocation latency is measured on the harness clock when the poller's line
  arrives through `kubectl exec`, and also recorded on the Pod's clock.
- A rerun creates new sessions; earlier evidence directories are never
  overwritten.
