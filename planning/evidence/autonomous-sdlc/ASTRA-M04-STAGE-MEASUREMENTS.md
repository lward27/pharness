# ASTRA M04C: Valid stage measurements

Date: 2026-09-06. Source: `dc0f235254267b06dd3ad4db80db486bea608c80`. Base main: `323278cb481d361072b150f43a2517fadf2b61ee`.
Status: implemented and locally validated; live qualification remains open.
Authority: [approved M04 program](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).

## Result

Planner, Test Diagnosis and Verifier now use source-backed V2 measurements. The
[scenario mapping](ASTRA-M04-STAGE-MEASUREMENT-MAPPING.md) accounts for all twelve
Planner, twelve Diagnosis and twenty-four Verifier cases. Their explicit fixture
revision is `stage-qualification-v2.3`. Onboarding, frozen coding/repair tasks,
registered models, profile limits and every numeric qualification gate are unchanged.
Historical V1 protocol fixtures and failed live reports retain their original meaning.

Planner uses concrete structured paths, declared acceptance names and coverage of
the requested files. The English negation parser is removed. Prose cannot authorize
a write or command. Actual baseline receipts, a genuine older source revision,
requirements and constraints replace answer markers and contradictory placeholder facts.

Diagnosis uses observed subprocess outcomes for assertions, compilation, a bounded
unused-import check, semantic failure, timeout and a missing executable. The lock
mismatch comes from the native RepositoryContract validator after an actual lock
change. The baseline case reproduces its failure before and after a documentation-only
change. No category or required summary phrase is supplied to the model.

Verifier receives requirements, a real candidate diff, source identity and actual
receipts. Expected verdicts, hidden oracle source and descriptive case identifiers
stay outside the agent workspace and inputs. The original 19 defective / 5 valid
balance is preserved. Defective candidates with green public tests require comparison
with the requirements. Private behavioral checks fail on the intended defects and
pass after correction; evidence-boundary cases have separate assertions.

## Integrity and diagnostic evidence

Native repository validation rejects invalid fixture locks and roots before model
execution. Every command has a deadline and bounded output. An isolated Python
bytecode cache prevents a same-size, same-second source edit from reusing an old
compiled candidate. Source and index fingerprints detect additional changes to an
already dirty working tree, which status text alone misses.

Reports retain the same bounded public inputs and observed receipts supplied to the
model, including when a typed submission is missing. Initial-envelope retention is
separate. Schema/reference failures, structured scope/coverage failures and judgment
failures receive distinct diagnostic classes. Missing or redacted material stays explicit.

## Objective validation

The [validation record](ASTRA-M04-STAGE-MEASUREMENTS-VALIDATION.json) covers **559 distinct
component tests**: a clean 558-test five-package run on `6c5330f`, followed by 22 passing
affected-stage checks after refining two controls, proving actual stale-source ancestry
and adding one regression. One existing live-only test remains ignored. The final
source passes all-target Clippy with warnings denied, formatting and five architecture
checks. Raw logs identify their respective source revisions; this is not presented
as one 559-test invocation.

Standalone replays on the final source pass [Planner 12/12](ASTRA-M04-STAGE-MEASUREMENTS-PLANNER-REPLAY.json),
[Diagnosis 12/12](ASTRA-M04-STAGE-MEASUREMENTS-DIAGNOSIS-REPLAY.json) and
[Verifier 24/24](ASTRA-M04-STAGE-MEASUREMENTS-VERIFIER-REPLAY.json). They retain real
source/command inputs, but use deterministic submissions. They prove fixture and
shared-runner operation; they are not evidence of model capability.

Development checks exposed and corrected invalid package-lock examples, stale Python
bytecode and an overbroad leakage assertion. Review also made the valid controls
preserve Unicode whitespace and missing-price behavior, and replaced a merely described
stale reference with a real ancestor commit. No historical model result was rescored.

## Compatibility, deployment and remaining proof

This slice changes the evaluator and compiled fixture-revision identifiers. There is
no database migration, new runtime dependency, model-policy change or Finance patch.
Deploy the matching immutable API/worker/evaluator set before running the revised
suite. Existing records remain readable. Active context-envelope Runs still require
the compatible worker established by M04A/B; do not roll them back to a worker that
cannot understand their saved contract.

No new provider evaluation or deployment was started for this slice. Hosted creation
and coding activation remain disabled. M04D must exercise small real-gateway cases;
M04E must prove actual implementation, failure, one repair and independent verification;
M04F still requires two full qualifying runs on the frozen deployed source.

## Design judgment

This removes avoidable measurement noise and makes failures inspectable. It does not
turn free-form engineering judgment into a solved scoring problem. Planner coverage
is structural; explanation quality and causal diagnosis still need review during
canaries and proof through the connected loop. The JavaScript cases exercise actual
text behavior, without claiming browser or DOM acceptance. The honest conclusion is
that the harness is now better prepared to measure the models, not that they are qualified.
