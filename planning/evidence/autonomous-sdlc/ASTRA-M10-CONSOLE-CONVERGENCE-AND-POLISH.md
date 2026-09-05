# ASTRA M10: Console convergence preparation

Status: initial implementation validated locally; **M10 acceptance remains open**.
Source base: `2249950d225a4632b24235c2b6f2d8469a774243`; isolated branch
`codex/astra-console-convergence`. The deployed Lamina console and the owner's
selected `Pharness Console.dc.html` remain the visual authority. This work does
not change the eight primary navigation destinations, colors, typography or
recorded interval behavior. It has not been released.

## Objective results

- Configuration-load failure now shows an unavailable state with a keyboard-
  operable retry. It cannot fall into the older operational console because an
  authenticated API is unavailable. Explicitly disabled historical flags retain
  their existing compatibility behavior.
- WorkItem intent/current condition precedes the lifecycle timing diagram.
  Missing reasons and missing next actions are named as missing; neither becomes
  “no active blocker” or an invented instruction to wait. Existing blocked
  action summaries and the controller's recorded condition take precedence.
- Hosted WorkItems use all eight lifecycle stages. Release and Observe remain
  pending when no outcomes exist; the console no longer inherits source-only
  “inapplicable” copy. The saved delivery projection supplies finite build,
  staging and production contracts and required evidence. Source merge never
  becomes an invented deployment or completed hosted result.
- Source-only history retains its six engineering/source lanes and recorded
  inapplicable tail. Its merge evidence stays readable. “Manual merge” is removed
  from the shared delivery heading so the heading also fits authorized automatic
  delivery without relabeling historical facts.
- The M06 `workflow_control` projection is consumed when present. Pause/resume/
  cancel controls remain separate from lifecycle decisions; all returned controls
  are available. A `workflow` control cannot replace the actual lifecycle stage.
  These fields are additive and absent in the current schema-52 reader.
- A failed refresh retains only the same record, displays a stale warning and
  disables its action/control buttons. Existing request cancellation and
  cross-scope isolation remain covered by the inherited resource tests.
- Submission and repository registration use “Check readiness.” The product
  summary describes pinned source and recorded readiness. The older capability
  checker explicitly explains temporary validation Jobs and persisted evidence;
  it no longer calls those effects read-only.

## Validation and scope of proof

The production UI build and **79 unit checks** passed. **109 distinct browser
checks** passed: 108 presentation/regression checks across desktop and phone
projects, plus the existing real API/controller source-only journey. That journey
was also repeated with both Lamina and the preceding flagged shell and verified
against the intentionally updated visual baselines. Its mobile duplicate is an
existing explicit skip, while the journey itself captures both widths. Repeated
runs are not added to the distinct count.

The hosted presentation checks cover eight stages, intent before timing, pending
release/observation, pre-merge production authority wording, configuration failure
and keyboard retry, retained stale records, both workflow controls disabled on
failed refresh, both themes, and no API mutations from navigation. Accessibility
checks cover WCAG 2 A/AA rules and page overflow at 390-pixel and desktop widths.
The source-only real-server test exercises actual controller/store behavior with
local fake GitHub/Kubernetes boundaries; it is not live provider or deployment
acceptance. None of these fixtures counts as M11 autonomous Finance work.

The first new fixture intercepted a JavaScript source path containing `/api/`;
it was restricted to paths beginning `/api/`. One unit assertion had an incorrect
heading case. The real journey initially expected the new registration label
before that second screen had been updated; the registration copy was aligned.
These were repaired without raising test timeouts, loosening assertions or
changing workflow authorization. Initial and final logs remain locally identified
by the [validation manifest](ASTRA-M10-CONSOLE-VALIDATION.json).

## Subjective review

The condition is easier to find, and the main question is now whether the change
is blocked, waiting or complete. Keeping the existing navigation and Lamina
identity avoids another product reset. Required-but-unevidenced release stages are
much more honest than labeling them inapplicable.

This is still too wordy. The title/intent, scope strip and detail grid repeat
information, the execution-envelope panels occupy substantial space, and phone
pages remain long. Moving the diagram down improves hierarchy but does not finish
polishing it. The next pass should reduce repeated fields and subordinate the
existing technical panels, using actual M09 production/recovery projections.
It should not add layout customization or invent progress summaries.

## Findings coverage and remaining work

| Finding | Current disposition |
| --- | --- |
| F02 | Existing separate facts/claims/caveats rendering retained; final contradictory and recovered-state walkthrough remains. |
| F03 | Configuration failure and same-record stale actions corrected; existing unavailable route checks revalidated. Remaining reachable legacy surfaces need final retirement audit. |
| F04 | Existing cancellation/isolation tests pass; complete scope/filter/pagination consistency remains. |
| F05–F07 | Condition-first hierarchy, coherent hosted/legacy semantics, keyboard and phone checks improved; final visual judgment remains open. |
| F09 | Eight-stage hosted presentation added; historical semantics preserved. Final delivery integration remains. |
| F10 | Product-scoped creation already starts with blank user intent and validated repository selection. Old wizard demo defaults remain on the retiring surface and must be removed with its route retirement. |
| F14 | Readiness wording and actual effects corrected on the touched creation/registration paths. Complete remaining terminology audit at closeout. |
| F15–F16 | Bootstrap fallback and stale-action paths improved. Competing operational routes, full filter/state matrix and real production/recovery projections remain open. |

[Program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) and
[M10 gates](../../programs/autonomous-sdlc/ASTRA-10-CONSOLE-CONVERGENCE-AND-POLISH.md)
remain authoritative. The current screenshots are explicitly presentation fixtures,
not proof of production approval, a real release or runtime verification:

- [Hosted overview, dark desktop](ASTRA-M10-SCREENSHOTS/ASTRA-hosted-overview-dark-desktop.png)
- [Hosted overview, light phone](ASTRA-M10-SCREENSHOTS/ASTRA-hosted-overview-light-mobile.png)
- [Required delivery evidence, phone](ASTRA-M10-SCREENSHOTS/ASTRA-hosted-delivery-light-mobile.png)
- [Stale workflow controls, phone](ASTRA-M10-SCREENSHOTS/ASTRA-hosted-stale-controls-mobile.png)

## Combined-source revalidation

The M06 controller and M04 prompt clarification were merged into this branch at
`047f13eb8b4627745ebe507810e46aec297c430e`. The 79 unit checks, production UI build, and real
API/controller journey with both console flags passed again without updating
visual baselines. The [combined validation manifest](ASTRA-M10-COMBINED-CONTROLLER-VALIDATION.json)
binds the logs to that source. This verifies source-only compatibility; hosted
release acceptance remains open.

PR 334 merged at `ca98fa7c7474902d206e130ca14eddddec8d82a7` on 2026-09-05 at
14:36 UTC. This source merge has not yet changed the live image. Final production
and recovery projections, route retirement and milestone acceptance remain open.
