# Finance metadata reliability campaign — 2026-08-29

This is the append-oriented evidence ledger for the active six-WorkItem
Finance campaign. It records controller evidence and observed provider state;
it does not infer deployment or runtime health from Repo Mode source delivery.

## Campaign state

- Product: `Finance` (`prod_01a043699d65721193e7e75d38654a2d`)
- Completed WorkItems: 1 of 6
- Current boundary: FRC-1 complete; FRC-2 may be created from the current
  finance-frontend source with yfinance merge `12ff05dab47778dd2344970001c4218c1825db96`
  pinned as read-only Product context.
- Raw-evidence hold: `rethold_1788004851369735270`, expiring after 90 days.

## FRC-1 — Validated Market summary endpoint

### Identity and scope

- WorkItem: `witem_01a04d3748ae7f409f95b8e1370c3249`
- Repository: `lward27/yfinance_wrapper`
- Source SHA: `56af994611396d886d1abefaa9745757981df834`
- Readiness assessment: `rready_01a04d30e0b5725084f6eef0c1ec273e`
- Repository contract: `rcontract_01a045a5863970519ea0c631464f7261`
- Approved WorkPlan: `wplan_01a04d388be072d1976d731ac457647a`, revision 2
- Workspace: `ws_01a04d3b9f087b329d3e75494d08fdae`
- Stage chain: `chain_01a04d3b9f237171aa10670baa3543d0`
- ChangeSet: `cset_01a04d417d337cb1b0d3ee15ee724ae3`, revision 1
- SourceDeliveryIntent: `srcintent_01a04d4277277680b0d4ea4deb2c87f3`

The first Planner submission referenced speculative paths outside the actual
RepositoryContract. Operator annotation
`annot_01a04d3a0b2072e38fcaa7897b172ac0` requested a replan. The replacement
Planner submission used only `src/**`, `tests/**`, and `readme.md` and became
the approved WorkPlan. No coding workspace had been authorized before that
correction.

### Runs and budgets

| Stage | Run | Turns | Tokens | Failures/retries | Evidence |
| --- | --- | ---: | ---: | --- | --- |
| Initial Plan | `run_01a04d37a55b7b329064a3cefb42cfbb` | 3 | 10,526 | 0 / 0 | Replanned before approval |
| Effective Plan | `run_01a04d3a29f27e31aeb47c752f81092c` | 4 | 18,781 | 0 / 0 | Controller-validated revision 2 |
| Implement | `run_01a04d3b9f2e77629c4bec1c0407ea9c` | 22 | 247,505 | 0 / 0 | 12 compactions; no truncated tool results |
| Test | `run_01a04d3f39057140890f7f8e7da45367` | 4 | 19,117 | 0 / 0 | Both declared commands passed |
| Verify | `run_01a04d4048bc7a1380f54f0d486e3880` | 11 | 81,284 | 0 / 0 | Approved; one bounded tool result truncated |

Preparation completed before Builder turn zero in 95,963 ms. Every Run
reported zero environment-discovery turns, zero tool failures, zero retries,
zero model approval waits, and zero budget extensions.

### Source and acceptance evidence

Changed paths, in first-change order:

1. `src/yfinance_wrapper/market_validator.py`
2. `src/yfinance_wrapper/api.py`
3. `tests/test_market_validator.py`
4. `tests/test_market_endpoint.py`
5. `readme.md`

- `python -m unittest discover -s tests -v`: passed, 37 tests.
- `python -m compileall -q src tests`: passed.
- Persisted source patch hash:
  `sha256:eb5337c73d8f680da4194c5087838692732b77a66af5c076247aaa4f5a122d94`.
- Implement outcome: `stageout_01a04d3f38fa7330a08c54236d0d99e3`.
- Test outcome: `stageout_01a04d4048a27ab094a29c42ea205897`.
- Verify outcome: `stageout_01a04d417d257a238a041e812750cf55`.

### Source delivery and merge provenance

- Pull request: `lward27/yfinance_wrapper#5`
- Approved head: `4fda61454f2d6f623856cd86a4d6c94104470c9d`
- Merge commit: `12ff05dab47778dd2344970001c4218c1825db96`
- Pre-merge observation: `providerchecks_01a04d6359bf74428bacc1ebd4276b76`
- Post-merge observation: `providerchecks_01a04d64479c7ff1b43f46a6a06fa6ad`
- Required-set hash:
  `sha256:4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945`
- Source Delivery outcome: `stageout_01a04d6447c672c08c4a9c604b4cc4b6`
- Closure: completed because the manual merge matched the approved head and
  fresh authoritative required-check observations.
- Release and Observe: controller-sealed `inapplicable`.

### PHarness defect and repair

The first writer dispatch failed with `git_push_permission_denied` before a PR
existed because the fine-grained PAT did not include yfinance_wrapper. The
sealed patch, base SHA, and branch were preserved. PHarness had no eligible
action for that recoverable pre-PR boundary.

PR `lward27/pharness#227` added a state-hashed `retry_source_delivery` action
limited to authentication, permission, and transport failures before PR
creation. It revalidates WorkItem, WorkPlan, ChangeSet, intent, exact patch
hash, repository allowlist, and an isolated source-writer check against the
exact Repository before dispatch. Policy failures and provenance changes remain
non-retryable.

Accepted PHarness release:

- Source: `10bd21113083336bcf284cac8551c280cb3c350e`
- Release commit: `426afddf2758dcba44e1ba93afe3a29132491c29`
- Runtime: `sha256:4d832d9079a418726386397a8e96c871af1b4ad575aa0710215baaf614a16014`
- UI: `sha256:eff254ecd3fadab9ae2427aaace5df3766a7e176c619e0f596c68d3a59d9be1d`
- Python runner: `sha256:cb65ee3070f26f8a6711de49e774b591a40377b0b40374a68571d264f2f9c83c`
- Node runner: `sha256:1e3d7df3d979a8b29900cf90fc37f31e95fcd5c3b9795cb9b266127a1acc6e46`

All four images were built through the dedicated `lucas-desktop` BuildKit
endpoint from the same SHA and verified as Linux/AMD64 with exact OCI source
and revision labels. Argo reconciled the exact release commit `Synced/Healthy`;
the live API and UI image IDs matched the declared digests, both had zero
restarts, API/UI revisions aligned, and both runner-profile checks passed.
This evidence does not assert an SBOM, signature, vulnerability report, or
cryptographically verified provenance.

## Next campaign step

Create FRC-2 as a new finance-frontend WorkItem only after refreshing its exact
current source readiness. Pin yfinance_wrapper at merge
`12ff05dab47778dd2344970001c4218c1825db96` as read-only context. FRC-2 must not
reuse FRC-1's workspace, source grant, or acceptance commands.
