# ASTRA M08: Durable baseline admission

Status: implementation and local validation passed. Not deployed and not M08 or M11 acceptance. The [validation receipt](ASTRA-M08-DURABLE-BASELINE-ADMISSION-VALIDATION.json) records the source revision, checks and retained failed attempts.

## Behavior and authority

A hosted staging operation now requires a controller-owned five-minute baseline before its one GitOps write. The immutable GitOps plan determines the current image digest and GitOps revision; the worker cannot submit a replacement runtime result. Native deployment, probe, Mimir, Loki and health-trace reads provide the evidence. The existing bounded runtime assessment requires matching identities, windows and receipt hashes and refuses missing or contradictory evidence.

The API saves the window before reading the application. It starts on the next 30-second query grid after a 30-second preparation allowance. Its end and 60-second evidence expiry are immutable. The existing 15-second reconciliation schedule and persisted due times advance the observation without a browser or sleeping through a five-minute request. Read-only observation continues during a pause; creation of a new baseline and all staging writes require active authorization. Neither pause nor restart renews the original one-hour operation window or 30-minute write ceiling.

Generic agent tool artifacts use an `art_` identity and a fixed tool-result kind; they cannot create the controller's deterministic `staging_baseline_` records. A regression test also rejects a matching-hash claim with the wrong native artifact kind.

Window, initial identity, functional probes, final identity and completed assessment are separately retained as artifacts bound to the exact staging authority, plan, session and optional Run. The operation links the original window and result IDs. An interruption between writing the window and linking it is recoverable. Each native collection is marked before it starts. If interrupted before a result is saved, that collection remains inconclusive instead of silently receiving another request budget. A waiting window resumes normally after a restart.

Context GET remains read-only. Its `may_advance` is false without fresh, passed evidence, and after a consumed admission identity attempt. Admission reevaluates the original evidence and reads the current deployment again. Changes to its pod identity, restart count, template, service, image or GitOps revision block the write. Current source, build, policy and workflow control are rechecked after that read. The immutable admission binds the baseline and current-identity artifacts and their hashes.

The writer checks the returned expiry immediately before its sole expected-head GitHub commit. The expiry can only be the existing baseline window's end plus 60 seconds, within the original write and operation ceilings. Missing, stale, rebound or expanded admission cannot dispatch. The existing rule against retrying an uncertain GitHub mutation remains in force.

## Compatibility and deployment

New staging authority is `pharness.dev/hosted-staging-gitops/v1alpha2`. The previous v1alpha1 contract remains readable by the new code, but cannot authorize a new dispatch. Older writers reject v1alpha2 instead of silently ignoring baseline admission. No database migration, new runner, production target, external scheduler or execution-budget increase was added.

The live PHarness release is still source `19b0c55c48e3614d0b4507d56df3029a52475618`, with schema 54 and zero hosted records verified before qualification. Its schema-compatible rollback floor remains that release **while no v1alpha2 authority is written**. Before enabling new hosted writes, deploy the API and worker from the same merged revision containing this contract, observe their immutable identities, and record that release as the new minimum compatible recovery reader/writer. Do not roll back a stored v1alpha2 operation to the older source-19 staging executor.

The native observer changes and finite API permissions in the preceding M08 slices must deploy together with this gate. Keep hosted creation disabled until required coding profiles qualify. Qualification evidence belongs to the exact tested runtime; a healthy deployment does not transfer a qualification to another binary.

## Validation and evidence boundaries

Across the final checks, 291 API/admin tests, 206 core tests and 49 worker tests passed. Fourteen focused staging checks cover the final controller fixtures. Clippy with warnings denied, formatting, architecture boundaries and five dependency-parser tests passed. The first broad run exposed an old worker recovery fixture without baseline admission; it was updated to provide the required bound admission, and the complete worker suite then passed. A new transport test confirms that missing or expired admission sends no GitHub mutation. Initial compiler and architecture failures are retained as implementation history.

The focused controller tests exercise absent, stale and contradictory baseline evidence; read-only context; a changed or restarted pod; immutable windows across pause and replacement ownership; partial persistence; consumed admission attempts; duplicate callbacks; and the existing uncertain-commit reader recovery. Core checks cover the finite original image extraction, historical contract reads, new dispatch requirements and unchanged/fresh admission identity. Worker checks cover missing, rebound, expired and expanded admission.

Test fixtures use an isolated executable and files. Their evidence shape derives from the recorded native reader exercise, with application identity, time and correlation rebound for deterministic tests. They are explicitly synthetic and never count as live or autonomous evidence. Tests contact no cluster, provider, registry or Finance service through this fixture.

The separately recorded [live native assessment](ASTRA-M08-RUNTIME-ASSESSMENT.md) passed against current yfinance staging. That proof establishes the native reader behavior in the program's operator environment. It does not prove that the deployed API collected a baseline or that PHarness performed an autonomous staging delivery.

## Recovery and remaining gate

A failed or incomplete baseline stops the write and preserves its original artifacts. A lost admission response cannot produce another admission or GitHub mutation. If a GitOps effect was already admitted, retain the existing writer/observer identities and recover the observed commit under the existing bounded protocol. Never manually create a fresh evidence window to make the same operation green.

This slice still leaves the committed staging operation open. M08 next requires the controller to observe Argo's candidate revision and exact running digest, persist its candidate observation window and accept the full runtime evidence. Production approval and rollback remain M09, and the two new Finance changes and their human production approvals remain M11. Production manifests, application code, credentials, branch policy and live qualification Jobs were not changed by this slice.
