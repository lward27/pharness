# SQLite restore rehearsal — 2026-10-18

First rehearsal of
[`sqlite-backup-and-restore.md`](../runbooks/sqlite-backup-and-restore.md)
against the production claim, run the same day `backup.enabled` went live
(#442). It exercised sections 1–3 of the runbook with a scratch destination
claim; no release was pointed at the restored copy and the live API was not
put into `read_only`, so section 3 step 4 and the readiness checks in step 5
remain unrehearsed.

| Item | Value |
| --- | --- |
| Context / namespace | `lucas_engineering` / `pharness` |
| Runtime digest | `sha256:2617628d1b955daa28671b2074c4674f03f309b3fb94c9915cb7e147d0a828a1` (source `8f8ab08`) |
| Backup Job | `pharness-backup-manual-202610181952`, created from `cronjob/pharness-backup`, 19 s |
| Archive | `/archive/nightly/20261018T195212Z/` on claim `pharness-api-backups-finance-20260827` |
| `database_sha256` | `sha256:3e46e0e11e78084a0689f1220276d25f324474664720ce132749fb679e5910a3` |
| `manifest_sha256` | `sha256:a1ce423d69b7028320f97966867ed58a39cbe7db3e80a8bba7824d23b96fd1cf` |
| Restore claim | `pharness-api-data-restore-rehearsal-20261018` (2Gi, `local-path`, retained) |
| Restore Job | `pharness-restore-rehearsal-20261018`, 12 s from create to complete |
| Checksum | `sha256sum -c` against the manifest: `OK` |
| Restored copy | `pharness-admin data archive` re-read it with integrity checks on both sides; manifest counts `work_items: 14`, `stage_outcomes: 102`, `hosted_operations: 0`, `approval_gates: 0` |

Observations:

- The backup claim stays `Pending` (and the Argo app `Progressing`) until the
  first Job binds it; `local-path` binds on first consumer. The nightly run or
  a manual Job clears that within a minute.
- The restore Job's `test ! -e /data/pharness.db` guard and the checksum step
  behaved as documented; the Job would have failed on a reused claim.
- Both rehearsal objects are left in place for inspection and are labelled
  `pharness.dev/archive-role: restore-rehearsal`. Deleting them is a separate,
  explicitly authorised cleanup.

Next rehearsal should include section 3 step 4 in a disposable namespace, as
section 5 of the runbook describes, so that `adoptExisting` and the
`pharness_build_info` check are exercised too.
