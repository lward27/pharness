# ASTRA M08: Integration with the approved M04 process changes

Date: 2026-09-06. Source: `718d412745b2afe63f8fabc399f7ad242fa73e3e`.
Status: integrated and locally validated in the existing staging draft; not deployed or accepted as autonomous staging.

The M08 draft now includes main `92f8f1b` and its trusted submissions, versioned context delivery, source-backed stage measurements, and bounded diagnostic scope. The existing native readers, baseline admission and durable candidate controller merged without a code conflict. The only conflict was in the program document; its current M04 and M09 decisions were retained and M08 progress reconciled.

[Validation](ASTRA-M08-M04-PROCESS-INTEGRATION-VALIDATION.json) records 816 passing workspace tests, six existing live-only tests excluded, all-target Clippy across eight affected packages, formatting, and the established architecture checks. Counts exclude nested benchmark child tests. The first architecture invocation used an incorrect script name after the other checks had passed; its error is retained alongside the successful existing guardrail invocation.

The actual Argo Finance overlay renders 59 resources. Its three finite observer roles exactly match the previously validated roles; the six Role/RoleBinding templates remain unchanged. Hosted creation and Coding Reliability V2 remain disabled. Local Node 26/Python 3.14 checks do not qualify the Linux AMD64 execution profiles.

The M04 diagnostic image set remains frozen on `92f8f1b`; this staging draft is not part of that release. Merge and deploy M08 only after the current diagnostic operation is reconciled and its own release review is complete. The real staging baseline observations remain historical evidence. Automatic source-to-staging progression, fresh runtime verification on that progression, and the Finance acceptance requests are still required.
