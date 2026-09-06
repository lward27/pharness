# ASTRA M07: Source merge protection decision

Status: **approved, applied and verified** on 2026-09-06 UTC (2026-09-05 local).
The owner approved the proposed protections in this task. See the
[execution evidence](ASTRA-M07-APPROVED-SOURCE-PROTECTION.md) for exact merge
revisions, GitHub readbacks and the runtime credential check.

## Approved policy

Require a pull request and the passing, up-to-date `Source integrity` check on
`main` in **yfinance_wrapper** and **finance-frontend**, including administrators.
Bind that check to GitHub Actions app 15368. Require zero human code reviews.
Block direct pushes, force pushes and main-branch deletion. Production GitOps
approval remains its separate human decision.

This repository-wide effect was raised because it changes the owner's daily
workflow. The owner has now authorized it; no second approval is required.

## Why this boundary is required

GitHub's PR merge endpoint supports an expected head SHA but no expected-base
argument. Strict required checks close the stale-base race at GitHub, while
PHarness also revalidates its approved head and source evidence.
[Merge API](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request) and
[strict check behavior](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches).

The CI-only [backend PR 7](https://github.com/lward27/yfinance_wrapper/pull/7) and
[frontend PR 8](https://github.com/lward27/finance-frontend/pull/8) are merged.
Their original [proposal validation](ASTRA-M07-SOURCE-CHECKS-PROPOSAL-VALIDATION.json)
is retained. The original [proposed request](ASTRA-M07-PROPOSED-MAIN-PROTECTION.json)
is historical: GitHub rejected its redundant empty `contexts` field. The
[applied request](ASTRA-M07-APPROVED-PROTECTION-API-REQUEST.json) omits that field
and preserves every approved setting. The actual PHarness source writer can read
the resulting protections in both repositories.

## Continuing execution

Keep the protections enabled. The automatic merge worker must stop if required
checks, strict mode or administrator enforcement disappear. It must merge only
the approved source head. Source merge still needs live autonomous acceptance;
these prerequisites do not close M07 or replace M04 qualification.
