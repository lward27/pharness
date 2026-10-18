# ASTRA M09: Production decision types on the updated process contracts

Date: 2026-09-06. Source: `b1e878d6e629728e335a5ec3542fb0ac4604104d`.
Status: locally validated draft; no production workflow enabled.

The existing production-decision material now integrates with M04A–D and the updated M08 staging draft. Its production type files are byte-identical to `b71e267`; only the surrounding source and milestone wording changed. [Validation](ASTRA-M09-M04-PROCESS-INTEGRATION-VALIDATION.json) records 213 passing core-package tests, six existing live-only tests excluded, all-target Clippy with warnings denied, and formatting. The preceding M08 integration separately records 816 workspace checks; those were not all rerun for this four-file pure core addition.

This checks compatibility of the exact-digest proposal, GitOps diff and timed approval material. A supplied actor string still does not prove human approval. The authenticated decision endpoint, durable admission, native evidence revalidation, production writer and bounded rollback are still required. No production approval, write, deployment or acceptance is claimed.

The diagnostic release remains frozen on `92f8f1b`. This draft is retained in PR 366 above the staging draft, ready for the next production integration work after its dependencies are satisfied.
