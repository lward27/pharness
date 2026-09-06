# ASTRA M07: Approved source protection applied

Status: the owner's branch-protection decision is implemented and verified.
Observed: 2026-09-06T00:57:26.051365+00:00. PHarness baseline: `bdf979a9b8ea375533155803010489083242545e`.
M07 autonomous source-delivery and complete program acceptance remain open.

## Result

The owner approved the documented policy in this task. Both tested CI pull requests
were revalidated against their exact heads and unchanged bases before merge.
Only `.github/workflows/source-integrity.yml` changed in either application.

| Repository | CI pull request | Resulting main revision |
| --- | --- | --- |
| yfinance_wrapper | [PR 7](https://github.com/lward27/yfinance_wrapper/pull/7) | `97110919da6c83b84279dab02fa34426de9adc53` |
| finance-frontend | [PR 8](https://github.com/lward27/finance-frontend/pull/8) | `91b558003e1a9cbb64f229b0c0f97b827b328df9` |

Both `main` branches now require a pull request and the passing **Source integrity**
check from GitHub Actions app **15368**, with strict up-to-date checks and administrator
enforcement. They require zero human reviews; code-owner and last-push approval are
disabled. Force pushes and branch deletion are disabled. Production approval remains
a separate human decision before the production GitOps merge.

## Evidence and limits

- Original CI: backend 37 tests and release build; frontend 49 tests, lint with its
  existing warning, and release build. The fresh preflight verified successful check
  IDs, exact PR heads, clean merge state, unchanged bases and the one-file diffs.
- GitHub responses and readbacks verify both merges and every approved policy field.
  A separate readback using `pharness/pharness-git-writer-token`, key `token`, returned
  HTTP 200 and the exact required settings for both repositories. No credential is
  retained in these artifacts.
- The first backend protection request returned HTTP 422: GitHub treated the empty
  legacy `contexts` array and explicit `checks` array as ambiguous alternatives.
  The corrected request omits only that redundant legacy field and retains the
  app-bound check and all approved behavior. The rejection and partial completion
  are preserved; the backend merge was reconciled rather than repeated.
- No application code, deployment manifest, running image, production approval,
  force push or deletion changed. These operator-applied prerequisites are not an
  autonomous WorkItem. Current Finance images remain pinned to their earlier,
  actually built source revisions; later work must start from these newer CI-bearing
  main revisions and obtain new source/build evidence.

[Validation and hashes](ASTRA-M07-APPROVED-SOURCE-PROTECTION-VALIDATION.json) cover
the preflight, exact requests, mutation receipts, readbacks and retained logs.
[Actual writer readback](ASTRA-M07-APPROVED-PROTECTION-WRITER-VERIFIED.json) proves
the runtime credential can inspect the required protection.
[GitHub's API contract](https://docs.github.com/en/rest/branches/branch-protection#update-branch-protection)
documents strict checks, app IDs and administrator enforcement.

## Operational boundary

New source work goes through PRs and passing checks, including administrator work.
No extra human source approval was introduced. Removing or weakening these controls
must make PHarness stop automatic source merge. Do not silently bypass them to finish
the program. M04 qualification and a real automatic exact-source merge/build chain
remain required before M07 can close. No additional owner decision is pending for
this policy.
