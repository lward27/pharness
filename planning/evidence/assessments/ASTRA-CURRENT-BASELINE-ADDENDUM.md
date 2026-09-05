# ASTRA current-baseline addendum

Observed: 2026-09-05T00:51:25.906783+00:00. This updates the interpretation of the
[original review](ASTRA-REVIEW-OVERVIEW.md), which remains evidence for source
`12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d`. It does not rewrite that review.
Implementation authority: [approved ASTRA program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md).

## Verified baseline

| Concern | Current evidence | Limit |
| --- | --- | --- |
| PHarness main | `c36b46aceb72f3d7097bc0bdee74810c745f7c0c` from remote and local object | Refresh before each milestone |
| Live PHarness Argo | Same revision, Synced/Healthy | Health is not autonomous SDLC acceptance |
| Compiled release source | `83a2689c877a3f48688d1d457c34e83474698c46`, recorded by accepted Lamina release | Configuration/evidence commits differ from compiled source |
| Runtime image | `sha256:59530854356d2c8888f0e1715a2d5fab4d0a708bf1212b92a4fc50ed64d5c64d` | Deployment metadata observed; not a new build |
| UI image | `sha256:8945c2e59469fcbd3973e66e282e5c82c607bc9c5656fe154285962f9744c68e` | Lamina already shipped |
| Gateway image | `sha256:e696f3c1f573a5edfb3ffbb2dce799721d3bd20d3db9858a813e2caf56403672` | Gateway availability is not qualification |
| GitOps main and Finance Argo | `fa27225c4c33b710ce24708e17fd39ac05ab6aeb`, Synced/Healthy | Production Applications auto-sync |
| Frontend running artifact | `sha256:248437be58bdeed738d614d13dd2c09232c18119ea0f10309e2a14ffed6d3f3d` from Pod imageID | Deployment still specifies `latest`; original build source not established by this read |
| Finance TLS | frontend, finance-db, and yfinance Certificates Ready=False/Expired; expiry 2026-08-20 | M02 must repair and verify normal HTTPS |
| Database source | 51 migration files through `0051_portable_agent_hosts.sql`; all applied to fresh in-memory SQLite; 77 tables | This is fresh-schema validation, not a read of the live database |
| Data lifecycle | Existing accepted Finance generation/retention machinery is preserved | Live generation must be revalidated before M05 writes; do not reset it |

The reviewed branch is substantially behind main. The implementation uses an isolated
worktree from main and preserves the original saved checkout and its untracked files.
The earlier plan's `0049` schema reference was the data-lifecycle milestone, not the
current maximum migration. The numbered program now uses verified current schema 0051.

## Changed conclusions

**F01 is fixed upstream, not a defect to reimplement.** Source-merge closure reuses the
idempotent inapplicable-tail sealing helper. M03 retains/revalidates meaningful regression
coverage and M06 tests retry-safe hosted completion.

**The Lamina redesign is already accepted.** Its current API-backed loading/error/stale
states, history, real interval timing, and interaction work supersede several prototype
and old-console concerns. M10 verifies remaining reachable behavior and simplifies
hierarchy; it does not rebuild Lamina or treat the old mock as the visual reference.

**Both Finance market features exist.** Backend Market support is on main
`12ff05dab47778dd2344970001c4218c1825db96`; frontend main
`068510d3442efc75c036b327d3820b17c87c8c0a` includes MarketOverview, adapter tests,
and the US market API helper. The old campaign's FRC-2-next statement is dated history.
Source presence does not prove a current production deployment of those features.

**The primary autonomy gap remains.** Current source-only closure and separate delivery
capabilities do not demonstrate a durable continuous request-to-runtime journey. Coding
Reliability V2 exists but is not qualified/enabled for this program at baseline. Historical
provider-account failure is not asserted to be a current outage until M04 tests it.

## Findings that still require work

- F02: verifier normalization still drops submitted risks from top-level outcome fields
  and can normalize an approved submission without faithfully reflecting contradictions.
- F11: executed boundary check reports products.rs at 5,502 lines and repo_mode.rs at
  8,152 lines (limit 3,500); five app test modules use wildcard imports. The dependency
  parser aborts after treating text inside a Rust string as a `use` declaration.
- F12: 40 tracked paths remain below `crates/pharness-eval/target/`.
- F03/F04/F10/F15: current Lamina improved the primary path, but legacy fallback/query
  surfaces still require explicit convergence or tested retirement.
- F05–F07/F09/F16: original prototype concerns become implementation acceptance scenarios,
  not blanket claims that the deployed Lamina console is broken.
- F08: README and planning entry points contain stale product/campaign claims; M01 corrects
  them, and M12 must reconcile final acceptance again.
- F13/F14: complete-journey proof and truthful readiness effects remain required.

See the [master findings ledger](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md#review-coverage-ledger)
for individual owners and closure criteria. Nothing is closed merely by assigning it.

## Visual authority and evidence limits

The user selected [Pharness Console.dc.html](../../pharness-ui-design-overhaul/Pharness%20Console.dc.html).
Its SHA-256 is `a24a8a442d08034d36165568f80cb141f6112d2b1f2fd2632a410e6664e9869c`. Preserve its visual identity and the deployed Lamina
implementation while correcting fictional/unsupported state and decision hierarchy.

This refresh executed remote/source metadata reads, bounded cluster health metadata,
fresh SQLite migration application, architecture checks, and generated-file inventory.
It did not run a live model evaluation, complete hosted WorkItem, production promotion,
or representative-user usability study. Prior release test counts remain historical
reported evidence, not checks rerun here. M01 made no cluster or application-code change.
