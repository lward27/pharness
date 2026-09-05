# ASTRA M07: Source merge protection decision

Status: ready for owner decision. Observed 2026-09-05 at 15:26 UTC.

## Recommendation

Require a pull request and the passing, up-to-date `Source integrity` check on
`main` in **yfinance_wrapper** and **finance-frontend**, including administrators.
The source workflow needs no human review: the required reviewer count is zero.
Production GitOps approval remains its separate human decision.

This changes the owner's daily workflow: direct pushes, force pushes and deleting
either `main` branch become blocked. Work goes through a PR; changed base code
requires another check before merge. That repository-wide effect is why this is
being brought to the owner under the request to ask about large downstream impacts.
No cluster workload or GitOps branch protection changes are proposed here.

## Why this is necessary

Both branches currently report `protected: false` and no required checks. GitHub's
PR merge endpoint supports an expected head SHA but has no expected-base argument.
A separate base check can race with another writer. The proposed strict required
check closes that stale-base merge boundary at GitHub, while PHarness still checks
the approved head, source tree and bound evidence before and after the operation.
[Merge API](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request) and
[strict check behavior](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches).

## Concrete changes, already tested

- [Backend PR 7](https://github.com/lward27/yfinance_wrapper/pull/7) adds the single
  CI workflow. Its GitHub run passed all 37 existing tests and the release build.
- [Frontend PR 8](https://github.com/lward27/finance-frontend/pull/8) adds the same
  workflow. Its GitHub run passed 49 existing tests, lint with the existing one
  warning, and the release build.
- Both workflows use a pinned Checkout v7.0.1 action, read-only repository access,
  no retained checkout credential, no service secrets, the actual proposed merge
  tree, Linux AMD64, and the existing 60-minute ceiling. They publish no image and
  deploy nothing. Application source and tests are unchanged.
- The exact [branch-protection request](ASTRA-M07-PROPOSED-MAIN-PROTECTION.json)
  binds `Source integrity` to the observed GitHub Actions app ID 15368, enables
  strict checks and administrator enforcement, requires a PR with zero human
  approvals, and prevents force pushes/deletion.

[Validation](ASTRA-M07-SOURCE-CHECKS-PROPOSAL-VALIDATION.json) records both source
heads, current bases, check IDs, job links, test excerpts and successful steps.
The PRs are open and the protection request has **not** been applied.

## Execution after the decision

Revalidate the two PR heads, current bases and successful checks, merge their
tested CI changes, then apply the exact request to each application's `main`.
Read back and verify every protection field. PHarness's merge worker must fail
closed if the required check, strict setting or administrator enforcement later
disappears, and it must use the exact approved head for the merge operation.

If the owner keeps the current unprotected workflow, automatic source merge stays
blocked; M07 cannot be accepted by substituting a weaker race-prone check. Reversing
the protection later also disables autonomous merge rather than silently weakening
the program gate. No branch deletion or force push is part of validation.
