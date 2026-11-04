# LUC-115 LiteLLM reservation and revocation qualification harness

This directory is a disposable, in-cluster harness that tests whether LiteLLM's native budget
reservation can be trusted as the PHarness Campus spend authority. It runs everything in one
namespace (`campus-litellm-spike`, Pod Security `restricted`, label
`pharness.dev/purpose=luc-115-litellm-spike`) against a deterministic fake upstream. No real
provider is reachable: the LiteLLM NetworkPolicy only allows egress to the fake upstream,
PostgreSQL, Redis and cluster DNS, and an egress check from the LiteLLM pod to `api.openai.com`,
`1.1.1.1` and the Kubernetes API failed while the three allowed edges connected.

Gateway accounting in this harness is not financial authorization. LiteLLM spend counters are an
admission heuristic that the results below show can undercount real provider liability. Identity
that a client can set (`user`, `metadata`, `litellm_metadata`, customer headers) is never
treated as authority (threats T08 and T09).

## Pinned identities

| Component | Image | Notes |
| --- | --- | --- |
| LiteLLM proxy | `ghcr.io/berriai/litellm-non_root:v1.108.0@sha256:08d313a00c46509c875d84f60c1448a260c478f399ade3c2abb940d358d5b724` | Tag `v1.108.0` (main-stable), commit `b48077ff9ef6b14dd3f188d020ee7f40e2c5ebca`, uid 65534. `litellm-database:v1.108.0` runs as root and was not used because it fails `restricted` |
| PostgreSQL | `docker.io/library/postgres:17.6-alpine@sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94` | emptyDir data |
| Redis | `docker.io/library/redis:7.4.6-alpine@sha256:3b73847e72874be07e6657b129a94761662b79bc0f679273757d4218573b2a98` | `coordination_redis`, emptyDir |
| Fake upstream and driver | `docker.io/library/python:3.13.9-alpine@sha256:e5fa639e49b85986c4481e28faa2564b45aa8021413f31026c3856e5911618b1` | stdlib only, code from `harness/` |

The running pod image IDs were checked against these digests. License files reviewed at the tag
are `LICENSE` (MIT for everything outside `enterprise/`, lines 1-4), `enterprise/LICENSE.md`
(BerriAI Enterprise license: production use needs a subscription, lines 7-13; development and
testing are allowed, lines 19-21) and `enterprise/pyproject.toml:7` (`LicenseRef-Proprietary`).
`docker/Dockerfile.non_root:153` copies `enterprise/` into the image, so the published image
contains proprietary code even when no license key is set.

## Layout and use

- `manifests/` namespace, NetworkPolicies, PostgreSQL, Redis, fake upstream, LiteLLM and the
  driver. Every container has requests and limits. With two LiteLLM replicas the limits add up to
  6.5 CPU and about 5.8 GiB, requests to 1.85 CPU and about 2.5 GiB.
- `manifests/litellm-config.yaml` three fake models (`campus-small`, `campus-large`,
  `campus-unpriced` with no pricing), retries and cooldowns off, 8 s timeout,
  `fail_closed_budget_enforcement`, `apply_user_budget_to_team_keys`, `coordination_redis`.
- `harness/fake_upstream.py` OpenAI-compatible fake that records every call and its ground-truth
  cost. Message markers select faults (`[[fault:429|500|timeout|cut]]`, `[[delay:S]]`,
  `[[ctok:N]]`).
- `harness/driver.py` setup, concurrent bursts, probes, revocation timing, identity probes and
  the report that joins driver results, upstream calls, gateway entity spend, Redis counters and
  `LiteLLM_SpendLogs`.
- `harness/reset_straddle.py` budget reset boundary probe.
- `scripts/create-secrets.sh` generates the master key and passwords in-cluster without echoing
  them. `scripts/spendlogs.sh` exports SpendLogs as CSV with prompt lengths only.
  `scripts/teardown.sh` removes every workload but keeps the namespace.

```sh
kubectl apply -f manifests/00-namespace.yaml && scripts/create-secrets.sh && kubectl apply -k .
D="kubectl -n campus-litellm-spike exec deploy/driver -- python3 /harness/driver.py"
$D setup s1 && $D burst s1 main --per-key 10
scripts/spendlogs.sh | kubectl -n campus-litellm-spike exec -i deploy/driver -- python3 /harness/driver.py report s1 --spendlogs -
```

Each scenario uses its own hierarchy: two course teams (cap $0.25 each), 20 students (user cap
$0.03, team members), 30 session keys (cap $0.02, `campus-small` only) with 10 students holding
two keys. One `campus-small` request costs about $0.00317. Organizations could not be created
because `/organization/new` returns 403 without an enterprise license, so the tenant level was not
tested natively.

## Results

Truth is the fake upstream's billed cost for every call it received. Gateway is LiteLLM key spend
(user and team totals were identical in every case). Unsettled is truth minus SpendLogs total.
Overspend is truth above the cap for that entity. Abandoned holds is the number of keys whose
Redis counter stayed above truth when the scenario ended. Counters expire 60 s after the last
write so none survived to the final report.

| Case | Sent | Admitted | Upstream calls | Gateway $ | Truth $ | Unsettled $ | Key over | User over | Team over | Holds | Duplicate calls |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| s1 burst, 1 replica | 300 | 138 | 138 | 0.43746 | 0.43746 | 0 | 0 | 0 | 0 | 0 | 0 |
| s2 burst, 2 replicas | 300 | 138 | 138 | 0.43746 | 0.43746 | 0 | 0 | 0 | 0 | 0 | 0 |
| s3 stream cut before usage (90 cut, 30 ok) | 120 | 120 | 120 | 0.2841 | 0.3813 | 0.0972 | 0 | 0 | course-a +0.0042 | 0 | 0 |
| s4 upstream 429, 500, timeout (20 each) | 60 | 0 | 60 | 0.0062 | 0.0636 | 0.0574 | 0 | 0 | 0 | 0 | 0 |
| s5 client overrides | 6 | 2 | 4 | 0.00634 | 0.00634 | 0 | 0 | 0 | 0 | 0 | 1 request, 3 calls |
| s6a pod force-delete at 0.7 s | 600 | 138 | 138 | 0.43884 | 0.43884 | 0 | 0 | 0 | 0 | 0 | 0 |
| s6b pod kill at 3 s | 600 | 138 | 138 | 0.43821 | 0.43821 | 0 | 0 | 0 | 0 | 0 | 0 |
| s6c kill after dispatch, 12 s upstream | 360 | 134 | 194 | 0.44338 | 0.61558 | 0.1722 | 17 keys, max +0.00538 | 10, max +0.01125 | a +0.11178, b +0.0038 | 0 | 0 |
| s6d kill after dispatch, 7 s upstream, 1 s grace | 360 | 138 | 198 | 0.43746 | 0.62826 | 0.1908 | 16 keys, max +0.00538 | 10, max +0.01125 | a +0.12446, b +0.0038 | 0 | 0 |
| s7 Redis down, then restored | 482 | 31 | 32 | 0.0955 | 0.0955 | 0 | 0 | 0 | 0 | 0 | 0 (one rid reused by the probe loop) |
| s7b Redis killed mid-burst | 420 | 137 | 137 | 0.43534 | 0.43534 | 0 | 0 | 0 | 0 | 0 | 0 |
| s8 stale Redis snapshot restored | 480 | 137 | 137 | 0.43429 | 0.43429 | 0 | 0 | 0 | 0 | 0 | 0 |
| s9 PostgreSQL down mid-run | 450 | 145 | 145 | 0.43746 | 0.45972 | 0 | 1 key +0.00856 | 1 +0.01445 | a +0.01952 | 0 | 0 |
| s11b caller identity probes | 17 | 14 | 14 | 0.04382 | 0.04382 | 0 | 0 | 0 | 0 | 0 | 0 |
| s12 unpriced model, fallbacks, retries, under-reserve | 56 | 48 | 56 | 0.08393 | 0.18764 | 0.10371 | 4 keys, max +0.0418 | 4, max +0.0318 | 0 | 0 | 4 requests, 9 calls |
| s13 budget reset, sequential | 32 | 12 | 12 | per period 0.00951 | per period 0.00951 | 0 | 0 per period | 0 | 0 | 0 | 0 |
| s13b budget reset straddle | 48 | 18 | 18 | 0.01905 in new period (keys 0, 1) | 0.01905 | 0 | 2 keys +0.00905 in one period | 0 | 0 | 0 | 0 |

Case notes:

- s3: every client got HTTP 200. Cut streams were logged as failure rows at $0.189 against
  $0.2862 of real cost, and course-a went over its cap while the gateway believed it was under.
- s4: 429 and 500 cost nothing, which is correct. Every timeout was billed upstream ($0.0636)
  while the gateway recorded $0.0062.
- s5: client `num_retries: 3` overrode gateway retries of 0 and produced three upstream calls for
  one request. Client `fallbacks` to a model outside the key allowlist were rejected (403), client
  `api_base` was rejected (401), `mock_response` was ignored and the call went upstream and was
  billed, and the allowlist rejected `campus-large` and `campus-unpriced` (403).
- s6a: Kubernetes sent SIGTERM and LiteLLM drained in-flight requests, so nothing was lost. s6b:
  150 network errors, holds were left in Redis and expired with the 60 s TTL, no overspend.
- s6c and s6d: requests already sent upstream when the pod died were billed by the provider but
  never settled. After the counters expired, follow-up traffic was admitted against the
  undercounted spend and team course-a overspent by about 50 percent of its cap.
- s7: fail-closed held. Requests got 503 while Redis was down and for about 60 s after it came
  back (Redis circuit breaker, `litellm/constants.py:561-566`), then recovered with no overspend.
- s8: a Redis restore with team-a counter at $0.1902 against a real $0.24092 did not cause
  overspend because fail-closed rechecks the DB floor. One pod returned 150 503s during recovery.
- s9: 112 requests got 503 "authentication database temporarily unavailable" and 7 were admitted
  during the outage. Entity spend still reads $0.43746 against a settled SpendLogs total of
  $0.45972 about 15 minutes later, and the follow-up burst overspent course-a. A second burst
  during the outage failed because both pods went NotReady.
- s12: the allowed unpriced model created no reservation and recorded $0 for $0.0951 of real
  cost. Five requests with fallbacks inside the allowlist made eight upstream calls. A retry plus
  timeout made three billed calls ($0.00954) against $0.00093 recorded. A request with
  `max_tokens: 20` where the provider returned 200 tokens pushed a key $0.0418 over its cap.
- s13b: three requests per key were in flight when the reset job ran. The job deletes the Redis
  counter and reseeds from the freshly reset DB row, so the in-flight holds vanished, a second
  wave was admitted at full cap, and both waves settled into the new period ($0.01905 against a
  $0.01 cap, 90 percent over).

### Caller identity (s11b)

Key-derived `user` and `team_id` in SpendLogs, user spend and team spend never changed, whatever
the client sent. `metadata.user_api_key_user_id`, `metadata.user_api_key_team_id`,
`metadata.team_id`, body `team_id` and `tags` were all ignored for attribution. `end_user` is
client-controlled: body `user`, `metadata.user_id`, `litellm_metadata.user`,
`x-litellm-end-user-id` and `x-litellm-customer-id` all set it to another student's id, and that
student's customer row accumulated $0.01565 of someone else's spend. A customer budget of $0.005
blocked the second request for that id, and rotating the id (header or body `user`) was admitted
immediately, so end-user budgets are not an enforcement boundary.

### Revocation (s10)

| Action | Topology | Handling pod | Other pod |
| --- | --- | --- | --- |
| `/key/delete` | 1 replica | rejects after 0.36 s | n/a |
| `/key/delete` | 2 replicas | rejects after 0.19-0.28 s | kept admitting for about 57 s (last success 56.6 s and 56.9 s, 156 and 197 upstream calls after the delete) |
| `/key/block` | 1 replica | rejects after 0.15 s | n/a |
| `/key/block` | 2 replicas | rejects after 0.13-0.32 s | rejects after 0.13-0.32 s, at most one in-flight success |

### Payload logging

Across 5,546 SpendLogs rows no row stored `messages`, `response` or `proxy_server_request`
content. Prompt storage is off unless `store_prompts_in_spend_logs` or
`STORE_PROMPTS_IN_SPEND_LOGS` is set (`spend_tracking/spend_tracking_utils.py:1905-1921`).
Callbacks and `turn_off_message_logging` were not exercised beyond the default.

## Source findings (v1.108.0, paths under `litellm/proxy/`)

- `spend_tracking/budget_reservation.py:254-353` creates the reservation. An unpriced model
  returns no reservation at all (305-309). Counters cover key, team, user (team keys only with
  `apply_user_budget_to_team_keys`, 546-563), end user, tag, model group, team member, org and
  project (487-614). Without fail-closed the policy resizes a reservation down to the remaining
  budget instead of rejecting (171-207). Cancellation reconciles to input cost only (402-432).
  The output estimate is clamped to the requested `max_tokens` or `max_output_tokens` (1648-1670),
  which is why a provider that returns more than `max_tokens` overspends.
- `proxy_server.py:3076-3154` reads spend from Redis, then memory, then a DB reseed, and returns
  503 when fail-closed cannot verify it (3151). The DB floor is cached for 5 s (3242-3275).
- `auth/user_api_key_auth.py:3934-3961` calls the reservation during auth, and line 2869 is the
  blocked-key check.
- `common_utils/reset_budget_job.py:601-615` resets a counter by deleting it from memory and
  Redis after the DB write, and 1171-1176 does that per key. In-flight holds are dropped with it.
- `management_endpoints/key_management_endpoints.py:5370-5374`: `/key/delete` evicts the token
  from the local in-memory cache only, and 5359 broadcasts only JWT mapping keys. `/key/block`
  (7716-7747) calls `auth/auth_checks.py:3327-3361 delete_cache_key_object`, which publishes an
  auth cache invalidation to every worker over `coordination_redis`. That difference explains the
  57 s cross-pod delete window, which is the 60 s in-memory TTL.
- `auth/auth_utils.py:1745-1817` takes the end-user id from `x-litellm-customer-id` or
  `x-litellm-end-user-id` (`litellm/constants.py:1652-1655`), configured header mappings, body
  `user`, `litellm_metadata.user` and `metadata.user_id`, all client-controlled.
- License gating: the whole organization router needs premium
  (`management_endpoints/organization_endpoints.py:99-114`), as do team admins
  (`team_endpoints.py:2864-2874`), key tags (`key_management_endpoints.py:1529-1532`), key
  regenerate (6181-6184), `model_max_budget` (8175-8179) and wildcard access groups (4782-4797).
  `auth/litellm_license.py:100-131` decides premium, and seat or team limits only apply with an
  air-gapped license (133-156).

## Verdict

LiteLLM's native reservation is not eligible as the Campus spend authority. It holds up under
plain concurrency, multiple replicas, Redis loss and stale Redis state when
`fail_closed_budget_enforcement` is on, but it undercounts or overspends in every case where the
provider bills for work the gateway does not settle: stream cuts, timeouts after dispatch, pod
death after dispatch, client retries and fallbacks, unpriced models, providers that exceed
`max_tokens`, a PostgreSQL outage and requests that straddle a budget reset. Organizations, the
natural tenant level, need an enterprise license.

LiteLLM can still be used as a routing and admission gateway in front of a PHarness ledger. The
minimum topology for that is PostgreSQL, a `coordination_redis` shared by every replica,
`fail_closed_budget_enforcement: true`, `apply_user_budget_to_team_keys: true`, gateway retries
and fallbacks off, explicit pricing on every model, and revocation through `/key/block` (or a
much lower auth cache TTL) rather than `/key/delete`.

The PHarness fallback ledger needs to:

- reserve and settle per upstream call, keyed by an idempotency id the ledger issues, so retries,
  fallbacks and duplicates are each counted
- book the full reservation as liability for timeouts, cuts, pod deaths and unknown outcomes until
  the provider usage or invoice proves otherwise
- reject models without pricing and cap `max_tokens` server-side, reserving for the provider's
  real maximum
- strip client `num_retries`, `fallbacks`, `mock_response`, `api_base`, `user`, `metadata` and
  customer headers before they reach the gateway
- own tenant, course, student and session identity from the authenticated session, never from
  client-supplied fields
- carry reservations across budget period boundaries instead of resetting them
- reconcile against provider usage and invoices on a schedule

## Residual risks

Real providers may bill timeouts, cuts and cancellations differently from the fake. The proxy ran
with one worker per pod, and multi-worker pods, more than two replicas and Redis Cluster were not
tested. `enable_redis_auth_cache` and lower `user_api_key_cache_ttl` were not tested as delete
mitigations. Organization budgets, tag budgets and model group budgets were not exercised.
Callback logging integrations were not tested for payload leaks. Version drift after v1.108.0
can change any of these results, so the harness should be rerun before relying on a later image.
