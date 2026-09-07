# ASTRA M04: Explicit writable scope at submission

2026-09-07. Source implementation follows the [source-81 diagnostic](ASTRA-M04-WRITABLE-SCOPE-CONTRACT-GAP.md). Release and live diagnostic acceptance are pending; M04 remains unqualified.

## Implementation boundary

The shared Onboarding prompt is now `repo-onboarding-v2@2026-09-07.2`. Its typed tool describes exact-file permissions and explicit recursive subtrees using generic examples. It keeps the distinction between current onboarding configuration permissions and proposed future development scope.

The production RepositoryContract candidate validator rejects trailing/repeated separators and `.` components in writable expressions. Existing traversal, absolute-path, wildcard and secret-path checks remain in force. Validation never rewrites a submitted path: `src/` is rejected with guidance to choose an exact file or an explicit subtree. A bare extensionless filename remains a legitimate exact path; the validator does not infer recursive authority from directory-looking names. Discovery roots retain their existing separate validation.

The native submission tool returns an actionable InvalidArguments result before accepting the proposal. A revised explicit proposal may then be submitted under the existing bounded runtime and approval rules. This does not authorize source mutation during onboarding. Historical proposal JSON remains readable, with its original content and verdict retained; approval and materialization revalidate it before permitting new work.

No scorer allowlist, fixture answer, model selection, budget, repair limit, database migration or Finance approval changes. The two earlier failed Onboarding results remain failed. [Prompt and tool hashes](ASTRA-M04-WRITABLE-SCOPE-CONTRACT.json) are captured from the production constructors and must match the later native evaluation record.

## Verification

The deterministic regression exercises the actual submission tool with the failed directory-form proposal and checks early rejection, unchanged input, retained historical readability and acceptance of an explicit revision. The actual write executor exercises nested descendants, exact-file grants, sibling-prefix exclusion and traversal rejection. Candidate checks cover malformed expressions while preserving extensionless files and filenames containing spaces. The existing evaluator regression still rejects onboarding-only configuration paths as future development scope.

Local checks passed: core 163 (one existing opt-in test ignored), runhost 45, evaluator 48, and onboarding API 11. The evaluator suite includes intentionally failing fixture programs; its overall result passed. The first write regression needed its nested parent directory created in the fixture, since the existing file-write tool does not create parents; product behavior was unchanged. Architecture boundary checks passed. Clippy with warnings denied, formatting, diff checks and changed-document local links passed. A first Clippy invocation overlapped deletion of the temporary contract inspector and failed to find that inspector; the serial rerun against the final source passed. Deterministic fixtures establish these contract boundaries; they do not qualify a model.

## Deployment and recovery

Build all seven images and the native bundle from one merged source using the explicit `pharness-mac` builder in Rancher Desktop while lucas-desktop is off. Keep outputs outside the source worktree. Verify registry identities, the actual production Helm overlay and server-side dry-run, then merge the authorized PHarness release pins and observe Argo automatically reconciling them.

Collect the five-minute baseline and ten-minute released service window with exact Pod/image identities, readiness, fresh scoped telemetry and bounded logs. Check there are no active Onboarding runs before the tool-interface rollout: stored tool hashes are not rewritten, and incompatible attempts must fail closed rather than inherit a new interface. Source `81edd9e81d280b0eda866b719c86562b3ff2f02c` is the preceding compatible release; schema 55 and the existing compatible-reader floor remain unchanged. Any recovery uses compatible immutable GitOps pins and preserves data and evaluation history.

After release acceptance, renew the existing 30-case protocol verification if expired and submit one unchanged-policy MiniMax `python-contract` diagnostic. Record the evaluation, prompt/tool hashes, source, image, actual submission and native verdict. Do not start broad comparison or frozen qualification to bypass a failed diagnostic. Local Windows inference remains pending the owner's endpoint/model setup and has no active target.
