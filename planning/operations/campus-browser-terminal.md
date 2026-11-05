# Campus browser terminals

Status: implemented and tested against the in-process fake terminal and a
real `pharness-api` process in the browser. The Kubernetes attacher
(`kubectl exec`) is implemented and unit-tested but has not run against the
real lab slots: the terminal identity's RBAC lives on the unmerged
`lucas_engineering` branch `lucas/luc-134-campus-terminal-rbac`, and no
Campus API is deployed with a terminal configuration yet.

A student opens the terminal of their own ready lab session in the browser
and gets an interactive shell in the session's container, where the runner
images ship the agent CLI at `/usr/local/bin/codex`. Inference goes through
the Campus broker ([campus-broker-tokens.md](campus-broker-tokens.md)),
whose only inference endpoint is still the LUC-131 fake. "Runs the agent
CLI" therefore means the CLI starts and answers in the terminal
(`codex --version`); a real model conversation needs a real inference
upstream.

| Piece | Location |
| --- | --- |
| Terminal contract, wrapper command, fake terminal | `crates/pharness-core/src/campus_terminal.rs`, `campus_terminal/fake.rs` |
| Kubernetes attacher (`kubectl exec` under a pty) | `crates/pharness-core/src/campus_terminal/kubectl.rs` |
| Configuration, ticket issuance, admission checks | `crates/pharness-api/src/app/campus/terminal.rs` |
| Tickets, limits, connection count, revalidation signal | `crates/pharness-api/src/app/campus/terminal_registry.rs` |
| WebSocket protocol, rechecks, rate limits, audit | `crates/pharness-api/src/app/campus/terminal_socket.rs` |
| Audit events | `crates/pharness-store/src/sqlite/campus_terminal.rs` |
| Browser page | `ui/campus.html`, `ui/src/campus/` |
| Tests | `crates/pharness-api/src/app/campus/tests/terminal.rs`, `ui/src/campus/*.test.ts*`, `ui/tests/campus-terminal-real-server.spec.mjs` |

## Routes and protocol

Both routes sit behind the Campus session middleware
([campus-oidc-auth.md](campus-oidc-auth.md)): only the
`__Host-pharness-campus` cookie authenticates them, and worker, operator
and broker credentials are refused.

`POST /api/campus/lab-sessions/:session_id/terminal-ticket` (body `{}`,
`X-Campus-CSRF` required, like every Campus mutation). The caller must own
the lab session and still be allowed to launch labs in its course; course
staff, other students and everyone else get `404 session_not_found`. The
session must be ready, current (not reset or replaced), not stopping and
unexpired, and the catalog must still hold the exact policy it was admitted
under.

| Status | Code | Meaning |
| --- | --- | --- |
| 201 | | `{"ticket", "expires_at", "socket_path", "protocol": "pharness.campus.terminal.v1"}` |
| 404 | `terminal_unavailable` | terminals are not configured |
| 404 | `session_not_found` | unknown session or not the owner |
| 409 | `session_not_ready`, `session_stopped`, `session_expired`, `session_superseded`, `policy_changed` | the session does not admit a terminal now |
| 429 | `too_many_tickets` | four outstanding tickets for this browser session (or 1024 overall) |

`GET /api/campus/lab-sessions/:session_id/terminal` upgrades to a WebSocket
with subprotocol `pharness.campus.terminal.v1`. The handshake needs the
Campus cookie and an `Origin` equal to `PHARNESS_CAMPUS_PUBLIC_ORIGIN`; a
missing or foreign Origin, a cross-site fetch, a non-owner or a plain HTTP
request is refused before the upgrade. After the upgrade:

1. The browser sends, within 10 s, exactly one text message
   `{"type":"auth","ticket":"...","cols":N,"rows":N}`. The ticket travels
   inside the socket, never in the URL, so it cannot reach proxy or access
   logs.
2. The server redeems the ticket, re-checks the session, takes one of the
   session's two terminal places, attaches, records `campus_terminal.opened`
   and sends `{"type":"ready","connection_id","cols","rows"}`.
3. Binary frames carry terminal bytes both ways. The browser may send
   `{"type":"resize","cols":N,"rows":N}` (1 to 500 columns, 1 to 200 rows; the same bounds apply to `auth`). Any other or
   unknown message closes the connection; frames over 64 KiB close it too.
4. The server ends with `{"type":"closed","reason":"..."}` and a close frame.

| Close code | Reason | Browser offers reconnect |
| --- | --- | --- |
| 1000 | `exited` (the shell exited) | yes |
| 4001 | `ticket_rejected` (unknown, used, expired or for another session) | yes, with a new ticket |
| 4002 | `auth_timeout` | yes |
| 4003 | `invalid_message` | yes |
| 4004 | `too_many_terminals` (two per lab session) | yes |
| 4005 | `terminal_unavailable` (attacher refused or unavailable) | yes |
| 4006 | `workload_not_running` | yes |
| 4010 | `session_stopped`, `session_expired`, `session_superseded`, `generation_changed`, `policy_changed`, `session_not_ready` | no for all but `session_not_ready` |
| 4011 | `logged_out` | no; sign in again |
| 4012 | `access_revoked` (owner lost launch permission in the course) | no |
| 4013 | `idle_timeout` (30 min without input) | yes |
| 4014 | `client_too_slow` (a send blocked for 10 s) | yes |
| 1011 | `internal_error` | yes |

## Ticket design

- 256 random bits from the system CSPRNG, the same token format as Campus
  sessions. The API keeps only the SHA-256 of the ticket, in memory.
- A ticket is bound to the identity, the browser session (hash of the
  Campus session id), the lab session, its lab generation, the fenced
  workload (slot and slot generation) and the effective policy hash, all
  read server-side at issue time.
- Lifetime 30 s. Redeeming removes the ticket whatever the outcome, so a
  ticket works at most once, even after a refusal. A ticket from another
  browser session, for another lab session or after expiry is refused with
  the same `ticket_rejected` close.
- Tickets are not persisted: restarting the API invalidates all of them and
  ends every open terminal. The browser session survives a restart, so the
  page reconnects with a new ticket. No migration is needed.

## Revocation and close latency

An open terminal re-checks, every 5 s and immediately when a handler on the
same process signals a revalidation: the browser session is still active
(not logged out, revoked or expired), the lab session still admits a
terminal and is the exact generation, workload and policy the ticket was
for, and the owner is still allowed to launch labs in the course. Logout,
stop, reset and member removal signal a revalidation. A failed check closes
the socket and drops the attacher stream, which ends the terminal process
(the fake ends its shell; `kubectl exec` is killed, which ends the exec
session). Reconnecting needs a new ticket, and the ticket route applies the
same checks, so a stopped, expired or signed-out terminal cannot be revived.

Bounds: same-process events close at once; expiry and changes made by
another API replica are seen within the 5 s recheck interval. Both are well
inside the 60 s requirement.

Measured (tests, recheck shortened to 200 ms):

| Event | API test (`stop_logout_expiry_and_lost_membership_close_open_terminals_quickly`) | Browser test (Playwright) |
| --- | --- | --- |
| Stop | 2 ms | 70 ms |
| Sign out | 0 ms | 50 ms |
| Member removal | 201 ms (recheck) | |
| Expiry | 159 ms (recheck) | |
| API process killed | socket ends in under 2 s; reconnect needs a new ticket | |

## Limits

| Limit | Value |
| --- | --- |
| Ticket lifetime | 30 s |
| Outstanding tickets | 4 per browser session, 1024 per process |
| Open terminals | 2 per lab session |
| First message | 10 s |
| Idle | 30 min without input |
| Message size | 64 KiB |
| Output | 1 MiB/s per terminal with a one-second burst |
| Keepalive | WebSocket ping every 30 s |
| Send timeout | 10 s, then `client_too_slow` |

Every shell starts through `terminal_command`: `/bin/sh -c` sets
`ulimit -u` (or `-p`) to the session policy's process limit, `ulimit -c 0`
and `TERM=xterm-256color`, then execs the configured shell. If neither
`ulimit` spelling works the chain stops before the shell, so no terminal
runs without the limit. This applies the policy's process limit to
terminal processes, which `kubectl exec` would otherwise start outside the
limit PID 1 set (see [campus-session-jobs.md](campus-session-jobs.md)).

## Audit

`campus_session_events` records `campus_terminal.opened`
(`connection_id`, `lab_generation`, `cols`, `rows`),
`campus_terminal.closed` (`connection_id`, `reason`, `bytes_in`,
`bytes_out`, `duration_ms`) and `campus_terminal.refused`
(`connection_id`, `reason`, and for tickets `unknown_or_used`, `expired`
or `wrong_session`). Tickets, terminal content and attacher error text are
never recorded. If the `opened` event cannot be written the terminal is
closed with `internal_error`.

## Configuration

| Variable | Values |
| --- | --- |
| `PHARNESS_CAMPUS_TERMINAL` | unset or `none` (default, routes answer `terminal_unavailable`); `fake` (only with `PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP=true`); `kubectl` |
| `PHARNESS_CAMPUS_TERMINAL_CONFIG_FILE` | required for `kubectl`: JSON `KubectlCampusTerminalConfig` |

Terminals need the lab catalog (`PHARNESS_CAMPUS_LABS_FILE`); startup fails
otherwise, and on any invalid value or configuration.

```json
{
  "kubectl": "kubectl",
  "global_args": ["--kubeconfig=/var/run/pharness/campus-terminal/kubeconfig"],
  "controller": "pharness-campus-controller",
  "slots": {
    "campus-lab-slot-1": "campus-lab-slot-1",
    "campus-lab-slot-2": "campus-lab-slot-2",
    "campus-lab-slot-3": "campus-lab-slot-3",
    "campus-lab-slot-4": "campus-lab-slot-4",
    "campus-lab-slot-5": "campus-lab-slot-5"
  },
  "shell": ["/bin/bash", "-l"]
}
```

The attacher lists Pods in the slot's namespace by the occupancy's labels
(`pharness.dev/campus-controller`, `-session`, `-slot`, `-generation`,
`-record=workload`) and attaches only when exactly one matches, is
`Running` and `Ready`, is not being deleted and has the session container.
It then runs
`kubectl <global_args> exec --stdin --tty --namespace=<slot> --container=session <pod> -- <terminal_command>`
under a local pty. Namespace and Pod come from the journal and the label
match, never from the browser.

### Kubernetes identity

The terminal identity is separate from the controller, so the controller
keeps no `pods/exec`. In `charts/campus-lab` (GitOps):

- ServiceAccount `campus-poc/pharness-campus-terminal`
  (`automountServiceAccountToken: false`), no cluster-wide permissions.
- In each slot (and each probe namespace) Role and RoleBinding
  `pharness-campus-terminal`: `pods` get, list; `pods/exec` get, create.
  Exec over WebSocket needs `create` from Kubernetes 1.31, `get` before.

The Campus API Pod runs as the controller and can only project its own
ServiceAccount's token, so the terminal credential is the ServiceAccount
token Secret `campus-poc/pharness-campus-terminal-token`, mounted
read-only into the API container and named by a separate kubeconfig
(`deploy/helm/pharness-campus`, [campus-poc-deployment.md](campus-poc-deployment.md)).

### Ingress and the browser page

- The Campus host must route `/api/campus/` straight to the API with
  WebSocket upgrades allowed and a read timeout above the 30 s ping. It must
  not go through the operator UI proxy (`deploy/docker/nginx-ui.conf.template`),
  which injects the operator token into every `/api/` request.
- Serve `campus.html` and its assets (built by `npm run build` into
  `ui/dist`) from the Campus host, and set
  `PHARNESS_CAMPUS_POST_LOGIN_PATH=/campus.html`. The page reads the lab
  session from `#/lab-sessions/<id>` or `?session=<id>`.
- xterm.js (`@xterm/xterm` 6.0.0 and `@xterm/addon-fit` 0.11.0, MIT) is
  bundled; its licence is shipped at `/licenses/xterm-MIT.txt`.

## Browser page and accessibility

- Status messages are in one `role="status"` `aria-live="polite"` region;
  when a terminal closes, focus moves to the status message, or to
  Reconnect when reconnecting can help.
- After connecting, focus is in the terminal and Tab is sent to the shell.
  Ctrl+] (Control and right bracket) moves focus to the Disconnect button;
  the page says so on connect and in the help text under the terminal.
- A "Screen reader mode" switch turns on xterm.js's screen reader support.
- The page reconnects on its own at most three times, with new tickets and
  1 s, 2 s and 4 s back-off, only when the connection dropped without a
  server reason (for example an API restart). It never reconnects after
  stop, expiry, replacement, policy change, lost access or sign-out.
- Stop lab and Sign out are on the page; the server's close reason, not
  the HTTP response, decides the final status message.

## Tests

- `cargo test -p pharness-api --bin pharness-api campus::tests::terminal`:
  ten tests over real sockets with the fake terminal (owner runs the agent
  CLI; CSRF, origin, owner and readiness checks on tickets; handshake
  refusals; single-use, expiry and cross-session tickets; replaced
  generations; stop, logout, expiry and membership-removal latency;
  reconnect after the API process is killed; limits, rate and idleness;
  attach failures and their audit; configuration).
- `cargo test -p pharness-core campus_terminal`: wrapper command, fake
  terminal, Pod selection and kubectl arguments.
- `npm test` in `ui/`: protocol and page tests (`src/campus/`).
- `npx playwright test tests/campus-terminal-real-server.spec.mjs --project=desktop`:
  builds and starts `pharness-api` with the fake labs and terminal and a
  local OIDC IdP, signs in, launches a lab, runs `codex --version`, checks
  keyboard focus and the Ctrl+] escape, runs axe checks, then stops the lab
  and signs out and measures the close latency.

## Live verification still needed

1. Done (LUC-137, RBAC merged as lucas_engineering PR #130):
   `kubectl auth can-i create pods --subresource=exec -n campus-lab-slot-1 --as system:serviceaccount:campus-poc:pharness-campus-terminal`
   answers yes; the same in `pharness`, and Pod creation and Secrets in
   the slot, answer no.
2. With an approved runner image (LUC-121), attach to a real session through the
   kubectl attacher: `ulimit -u` shows the policy limit, `codex --version`
   answers, and stop closes the terminal and ends the exec process in the
   Pod (check with `ps` from a second terminal before stopping).
3. Confirm the Campus ingress passes WebSocket upgrades and the `Origin`
   header unchanged.

## Residual risks

- Tickets and the revalidation signal are per process. With several API
  replicas a change on one replica reaches terminals on another within the
  5 s recheck, not at once.
- Killing `kubectl exec` relies on the container runtime ending the exec
  session when its stream closes. Under gVisor this is unverified (live
  step 2). A shell that ignores SIGHUP could outlive the stream until the
  Pod ends; it stays inside the session's limits and slot.
- The process limit is an rlimit counted per user in the sandbox; terminal
  and session processes share it.
- Terminal content is not recorded. Abuse inside a terminal is bounded by
  the session's sandbox, not observed.
