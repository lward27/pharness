# ASTRA M09: Production contract refresh after measurement fixes

Status: local validation passed; PR 366 remains a draft stacked on PR 363. The integration takes staging draft `12d3a06d03cf73d83606d0e685cf9820a13bd47c`, including main `5a409453d28d68d22b12899a312df763c51e48dd`, into prior production draft `80e19feb148eadcc3e46e073ac3d2de1b02e88be` without conflicts. All four production-contract implementation files remain byte-identical to the earlier draft.

[Validation](ASTRA-M09-5A40945-INTEGRATION-VALIDATION.json) records 216 passing core/integration tests, including six production-contract checks, with six live checks unrun. Core Clippy with warnings denied, formatting and five architecture regressions pass. [M08 validation](ASTRA-M08-5A40945-INTEGRATION.md) separately covers the integrated staging branch.

The contract describes exact approval material and rollback constraints. A `HumanProductionApproval` value is still only data: it does not authenticate a human, persist an authorized decision, execute a production GitOps effect, or verify a deployment. Those gates remain open, as do bounded rollback execution and M11's actual human approvals. No production authority or live deployment changed. The separate source5a40945 PHarness release excludes both drafts.
