# ASTRA M08: Staging and runtime verification

Status: basic Tempo reader, schema-54 delivery records and durable staging foundation deployed in source `19b0c55`. New native Finance identity, probe, signal and trace integration remains in development; autonomous staging acceptance is open.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M07 and usable M02 staging bindings.

The [native Tempo reader evidence](../../evidence/autonomous-sdlc/ASTRA-M08-BOUNDED-TEMPO-READER.md)
records the tested finite query, a real staging trace sample, explicit collection
limits and inconclusive behavior. The basic reader is deployed but its live Tempo endpoint is not yet configured. It does not establish release identity and cannot satisfy staging or promotion gates alone.

The [delivery-record compatibility slice](../../evidence/autonomous-sdlc/ASTRA-M08-DELIVERY-RECORD-COMPATIBILITY.md)
extends the existing build/deployment/GitOps graph for separate staging and
production records. Schema 54 preserves historical values and rejects legacy
advancement of hosted deployments. The SQLite uniqueness change requires a
transactional table replacement; the isolated migration and [live preservation checks](../../evidence/autonomous-sdlc/ASTRA-19B0C55-LIVE-PRESERVATION.md) passed before hosted writes. Hosted creation remains disabled. This does not
establish a deployed candidate, production approval or release verification.

The [durable staging GitOps slice](../../evidence/autonomous-sdlc/ASTRA-M08-DURABLE-STAGING-GITOPS.md)
connects verified autonomous builds to one admitted, finite staging digest update.
It preserves original operation and Job identities, uses an atomic expected-base
GitHub commit, and recovers uncertain outcomes with one separate GitOps reader.
Its local tests do not establish a real GitOps mutation or running deployment.
The operation retains its delivery locks after a committed digest while actual
Argo and runtime verification remain pending. Production cannot be expressed by
this authority; the existing production approval boundary is unchanged.

The [native deployment identity reader](../../evidence/autonomous-sdlc/ASTRA-M08-DEPLOYMENT-IDENTITY.md)
checks the exact Argo revision, Deployment/ReplicaSet/Pod ownership, running digest
and ready service endpoints. Both existing staging baselines passed live read-only
checks. It is not deployed or connected to the controller and does not establish
a runtime observation window or application acceptance.

The [deterministic application probes](../../evidence/autonomous-sdlc/ASTRA-M08-FUNCTIONAL-PROBES.md)
are implemented and tested but not deployed. Frontend staging HTTP checks passed;
the existing yfinance staging image failed current-source validation and market-route
checks. [Baseline drift](../../evidence/autonomous-sdlc/ASTRA-M08-FINANCE-BASELINE-DRIFT.md)
records the discrepancy and the unproven source commit behind that image. Preserve
these failures when defining baseline expectations and candidate acceptance.

The [native window signal reader](../../evidence/autonomous-sdlc/ASTRA-M08-WINDOW-SIGNALS.md)
passed two real five-minute staging observations at source `8173a4e83af22f1c2e178936e190644396f81ddc`.
It binds readiness/restart metrics and Pod-UID log streams to bracketed deployment identities,
uses all-address application HTTP counters, and rejects missing/stale evidence. Live testing
caught Loki sampling alignment and a real Promtail out-of-memory delivery gap; both failed
attempts remain recorded. The finite [collector recovery](../../evidence/autonomous-sdlc/ASTRA-M08-PROMTAIL-MEMORY-RECOVERY.md)
is deployed through GitOps and fresh log coverage passed. The PHarness reader itself is not
deployed. These results do not override backend functional failures or establish trace
correlation, durable baseline admission, automatic staging or production readiness.

The [health-request trace correlation](../../evidence/autonomous-sdlc/ASTRA-M08-HEALTH-TRACE-CORRELATION.md)
passed a real five-minute staging check with matching request/parent IDs and a complete
Tempo server span. Other backend probes still failed. The reader is not deployed;
[API access preflight](../../evidence/autonomous-sdlc/ASTRA-M08-API-RUNTIME-READER-PREFLIGHT.json)
identified missing finite Kubernetes read permissions and telemetry configuration.
The local operator checks cannot substitute for deployed controller verification.

The [finite API observer preparation](../../evidence/autonomous-sdlc/ASTRA-M08-FINANCE-OBSERVER-ACCESS.md)
adds namespace-scoped Finance reads and a separate Mimir endpoint while preserving legacy
Prometheus inventory. Local code/chart validation and six in-cluster HTTP routing checks
passed. The change is not deployed; effective API permissions and native controller
execution still require observed evidence. The earlier empty-query failure is retained.

The [current-source staging baseline](../../evidence/autonomous-sdlc/ASTRA-M08-YFINANCE-CURRENT-STAGING-BASELINE.md)
now runs the existing tested `efa6294` image after GitOps PR 56. The unchanged functional
probes and one shared five-minute metrics/log/health-trace window passed. Production is
unchanged. This program-operated platform correction provides a usable staging starting
point; it is not the autonomous delivery demonstration or an M11 maintenance WorkItem.

The [bound runtime assessment](../../evidence/autonomous-sdlc/ASTRA-M08-RUNTIME-ASSESSMENT.md)
passed a single native five-minute staging window with one shared identity pair. The
[durable baseline gate](../../evidence/autonomous-sdlc/ASTRA-M08-DURABLE-BASELINE-ADMISSION.md)
connects those readers to persisted controller windows and a fresh staging admission.
These newer slices require a future compatible API/worker release. Neither the local
native proof nor the controller fixtures constitute an autonomous staging delivery.


## Objective and scope

Verify a running candidate using actual deployment identity and application behavior.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Update the staging GitOps digest automatically and observe Argo auto-sync plus the running imageID. Reuse existing deployment/effect records.

2. Replace inventory-only success with bounded functional probes, Prometheus/Mimir service metrics, Loki logs, and Tempo traces for instrumented applications.

3. Add only the bounded Tempo query capability this native path needs. Store query, window, threshold, freshness, target, result, and correlation evidence.

4. Use five-minute baseline/staging and ten-minute production defaults, retaining stricter existing service requirements. Missing/no-data signals are inconclusive.

5. Demonstrate the staging mechanism on yfinance. Keep frontend runtime-configuration behavior explicitly pending M11.

## Interfaces and compatibility

Existing release verification evidence enriched with native target/query contracts; bounded Tempo read action only.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] Real yfinance staging GitOps revision, Argo revision, pod imageID, functional probes, and fresh LGTM signals agree.
- [ ] Wrong digest, unrelated telemetry, stale observations, empty required query results, unhealthy rollout, and failed acceptance block promotion.
- [ ] Queries and probes are read-only, application-scoped, bounded, and do not fabricate frontend traces.
- [ ] Record measured latency without inventing an SLO. No cluster-inventory green status substitutes for application evidence.

Adapter/decision tests for pass/fail/inconclusive, live yfinance staging observation, and preserved raw bounded evidence.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Leave production unchanged. Restore the recorded staging baseline through GitOps when safe; preserve failed candidate and verification evidence.

## Evidence and closeout

Write ASTRA-M08-STAGING-AND-RUNTIME-VERIFICATION.md with exact digest and query/window result map.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F13.
Update the master ledger and this document only after its checks are evidenced.
Unmet criteria remain unchecked with a concrete reason and next action.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md and this milestone. Verify dependencies against current
evidence, inspect the affected implementation, execute the bounded changes above,
and run the specified meaningful checks. Preserve user work and all safety/identity
boundaries. Record results, commit the implementation and evidence, update the master
and finding ledger, then continue the next eligible milestone. If an external input is
missing, explain the exact blocker and continue independent work. Do not weaken a gate,
silently switch provider/budget, or claim unexecuted deployment or autonomous acceptance.
