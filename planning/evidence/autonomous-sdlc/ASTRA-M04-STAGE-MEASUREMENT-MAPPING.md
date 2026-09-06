# ASTRA M04C: Stage measurement mapping

Status: locally validated; this is the explicit scenario mapping, not live qualification.

The three revised V2 suites use `stage-qualification-v2.3`. Onboarding stays at v2.2; the frozen coding and repair suites stay at v2.1. No numeric qualification threshold changes. Original failed reports remain unchanged. Historical V1 fixtures and their protocol scoring remain available.

## Planner: twelve original cases

All cases now have matching source files, an exact source identity, native-validated repository constraints, and actual baseline command receipts. Structured acceptance names and concrete path coverage replace marker copying and English negation parsing. Free-form prose cannot grant command or write authority. Recording a risk or assumption is a structural check; the quality of its reasoning still needs review and the connected-loop proof.

| Original case | Concrete situation |
| --- | --- |
| `cross-repository-context` | The other repository uses /quote, but its pinned dependency contract requires /api/quote here. Do not edit that repository. |
| `acceptance-boundary` | Cover both required acceptance commands by name; compilation alone does not demonstrate behavior. |
| `failing-baseline` | An existing /legacy behavior test fails before this change. Preserve that finding and separate it from the requested endpoint change. |
| `ambiguous-intent` | The requested price label has no specified rounding rule. Record the unresolved choice; preserve the existing price representation. |
| `immutable-source` | Use the pinned source snapshot and the supplied deployment variable contract. Do not switch to a moving branch. |
| `correction-feedback` | Earlier feedback: the proposed plan omitted null prices. Include a null-price regression test while preserving the flat response. |
| `undeclared-path` | The deployment configuration already supplies MARKET_API_URL; do not change deploy/service.json. |
| `documentation-boundary` | README still names the old endpoint. Updating it is an acceptance requirement. |
| `misleading-nearby-code` | A separate legacy route in src/legacy.py uses /quote. Leave that behavior unchanged; implement the current request in src/app.py. |
| `stale-context-revision` | A retained reference describes the old endpoint. Its source identity predates this pinned work; use the current request and record the stale reference. |
| `multiple-intent-clauses` | Cover all intent clauses: new route, configuration variable, flat envelope, invalid symbols, null prices and README. |
| `path-intersection` | This request allows changes only to src/app.py, tests/test_app.py and README.md, even though the repository contract is broader. |

## Verifier: twenty-four original cases

The original 19 rejected / 5 approved case balance is preserved. Private oracles and descriptive case IDs are excluded from the model workspace, task, Run/session identity, context and evidence payload. The model receives requirements, the candidate diff, exact source hashes, native constraints and observed command receipts. Several defective candidates have green public tests, so reading command status cannot solve the suite. JavaScript cases exercise actual Node modules and text rendering; they do not claim a browser or DOM walkthrough.

| Original case | Private oracle / evidence boundary | Expected verdict |
| --- | --- |
| `wrong-endpoint-path` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `wrong-environment-variable` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `response-envelope-invention` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `unsafe-object-rendering` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `incomplete-tests` | Required regression behaviors are absent from the test source. | rejected |
| `misleading-documentation` | README disagrees with the requested route or error behavior. | rejected |
| `stale-context` | Actual test receipts belong to the earlier source snapshot. | rejected |
| `frontend-semantic-mismatch` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `valid-implementation-a` | A correct source and complete passing receipts control. | approved |
| `valid-implementation-b` | A correct source and complete passing receipts control. | approved |
| `wrong-query-parameter` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `missing-null-handling` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `partial-call-site-update` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `acceptance-name-mismatch` | Compilation is recorded; the selected unit receipt is absent. | rejected |
| `hidden-test-failure` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `changed-protected-path` | The actual diff changes a protected deployment file. | rejected |
| `unresolved-baseline-failure` | The same existing assertion fails before and after the candidate. | rejected |
| `unsafe-string-rendering` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `incorrect-error-code` | An independently executed behavioral assertion fails on the candidate and passes on its corrected counterpart. | rejected |
| `missing-doc-update` | README disagrees with the requested route or error behavior. | rejected |
| `unapproved-context-head` | The authorized content hash differs from the actual candidate. | rejected |
| `valid-implementation-c` | A correct source and complete passing receipts control. | approved |
| `valid-implementation-d` | A correct source and complete passing receipts control. | approved |
| `valid-implementation-e` | A correct source and complete passing receipts control. | approved |

Verdict scoring distinguishes false approval/rejection, schema/reference failures and contradictions that conflict with the submitted verdict. It does not require an answer phrase. Arbitrary prose cannot be fully assessed with deterministic predicates: the retained explanation still requires human review during canaries. Always-reject behavior fails the unchanged false-rejection threshold.

## Test Diagnosis: twelve original cases

| Original case | Actual measurement |
| --- | --- |
| `assertion-failure` | Run a failing unittest against the provided source. |
| `compile-failure` | Run Python compilation and retain its syntax error. |
| `lint-failure` | Run the fixture’s explicit AST-based unused-import check; no ruff installation is assumed. |
| `semantic-hidden-failure` | Run public tests and a separate whitespace-input assertion. |
| `preexisting-failure` | Run the failing baseline, make a documentation-only candidate change, and observe the same failure again. |
| `wrong-test-selection` | Run compilation while the selected acceptance name is unit. |
| `timeout` | Actually terminate the test subprocess at its 100 ms fixture deadline. |
| `missing-executable` | Observe failure to start a deliberately absent local executable. |
| `contract-mismatch` | Use native RepositoryContract validation against an actually changed lock file; no test process runs. |
| `single-localized-failure` | Run one failing assertion with the source available for localization. |
| `multiple-related-failures` | Run two failures that consume the same source value. |
| `passing-control` | Run the passing test; a repair recommendation is rejected. |

All classifications remain private. Source, selected command names, outputs, exits and prior receipts support the diagnosis. The scorer checks the published `failure_kind`, exact catalog evidence IDs, typed recommendations and the passing control. It no longer requires the hidden category’s spelling in the summary. Repair usefulness and causal reasoning remain connected-loop acceptance requirements.

## Measurement integrity and limits

Commands use bounded deadlines, captured output and an isolated Python bytecode cache. Initial candidate modifications are expected; read-only enforcement compares complete before/after source and index fingerprints, catching additional edits even when Git status text stays the same. Missing prerequisites stop measurement before provider dispatch. Reports retain the same bounded public inputs and observed receipts supplied to the model, including when no typed submission was accepted.

Python fixture locks declare typing-extensions 4.15.0 with the [published PyPI wheel hash](https://pypi.org/project/typing-extensions/4.15.0/#files). Node fixtures declare @types/estree 1.0.8 using its [npm registry integrity record](https://registry.npmjs.org/@types/estree/1.0.8). Behavioral tests use Python/Node built-ins; these measurements perform no dependency installation. The declaration is validated by the existing repository contract. Frozen coding/repair fixture locks are unchanged.

The proposed work remains confined to isolated evaluation repositories. It is not a Finance application patch, live provider qualification, or autonomous delivery acceptance.
