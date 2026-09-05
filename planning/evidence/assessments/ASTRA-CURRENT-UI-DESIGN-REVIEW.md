# Review of the selected PHarness Console design

Date: 2026-09-04. Primary design: [Pharness Console.dc.html](<../../pharness-ui-design-overhaul/Pharness Console.dc.html>), explicitly selected by the owner. [Overview](ASTRA-REVIEW-OVERVIEW.md) · [Separate implemented UI review](ASTRA-IMPLEMENTED-UI-REVIEW.md)

## Authority and method

This report evaluates the selected HTML design, not the older React console. The two August 23 PNG concepts are background references only. The HTML and adjacent `support.js` were served locally and inspected as an interactive prototype. Navigation, theme, lifecycle variants, cockpit tab behavior, the accessibility tree, and narrow-window layout were examined. Default desktop inspection was at 883 × 1019 CSS pixels; a narrow inspection measured 433 × 938 CSS pixels. Wider layout was also inspected.

The prototype uses fixture data. Its counters, “Live” status, execution progress, and effects are not observations from a running PHarness backend. Several controls are decorative or only change selected styling. That is normal for design exploration, but it limits what can be claimed as validated interaction.

## My visual judgment

**This is a stronger visual direction, but it is not yet an intuitive operating experience.** The spacing, repeated card treatment, purposeful work titles, and grouping are coherent. It has an identifiable aesthetic and feels deliberately designed.

The visual language is also doing too much. Translucent cards sit over grain, dots, curved grid lines, gradients, glows, and moving effects. In dark theme, the background competes with small secondary text. The product looks busy even when the underlying information is fairly simple.

I would retain the typography family, restrained status colors, work-centered cards, and clear section spacing. I would reduce background texture, blur, glow, and tiny uppercase labels. This is a readability preference grounded in the inspected layouts, not a measured contrast-compliance claim.

## What works and should survive refinement

| Design choice | Why it helps |
| --- | --- |
| One exception thread per WorkItem | Reduces duplicate destinations for the same underlying change while retaining multiple signals |
| Plain work titles such as “Reduce checkout timeout regression” | Gives the operator a reason to care before introducing identifiers |
| A visible reason for blockage | Makes stopping understandable and supports a useful next decision |
| Tool approvals separated from lifecycle gates | Preserves a meaningful difference in authority |
| Gates grouped under their owning plan | Gives repeated review conditions context instead of displaying a flat wall of cards |
| Future gates disabled with a reason | Communicates sequence without pretending future decisions are currently actionable |
| Target and immutable source visible in the cockpit | Provides the context needed to review an action safely |
| Attempt limits and wait deadlines available | Helps explain why a workflow is paused or will stop |

These are worth preserving without introducing more controls.

## F05 — Make the example workflow tell one consistent story

**Observed design facts:** the cockpit says the WorkItem is blocked at ChangeSet capture and Delivery is “not started.” The same cockpit displays an active Tekton pipeline observation, check 4 of 7. A staging validation gate at order 3 of 4 is listed as a blocker for capturing the ChangeSet. The capture button is disabled, but an “Advance internal steps” button looks available beside it.

Relevant design locations: [cockpit and actions](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L434>), [active wait](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L603>), and [blocker fixtures](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L1366>).

**Judgment:** a new operator cannot tell whether they should wait, approve a gate, review acceptance, or force advancement. The interface is attractive, but the story is contradictory. These fixture inconsistencies matter because implementers commonly translate polished examples directly into behavior.

**Simplification:** use one existing, internally consistent backend state per example. If capture is blocked by missing acceptance evidence, make the relevant evidence/recovery action primary. If the WorkItem is waiting for Tekton, show that as the current condition and place earlier capture details in history. Remove the generic advance button unless it represents a specific permitted existing transition, in which case name that transition.

Do not solve this by adding another status model or allowing out-of-order actions.

## F06 — Put the decision above the lifecycle illustration

The default `laminae` variant occupies a large card between the WorkItem identity and the next required step. At the inspected desktop size, the next-step card begins near the bottom of the initial viewport. Switching to `beads` substantially reduces this height and makes the current blocker easier to reach.

**Judgment:** the next useful decision deserves more visual priority than a diagram explaining the controller. The current hierarchy is backwards for an exception-driven operator console.

Suggested reading order, using existing content:

1. WorkItem title, target, current condition.
2. Why it needs attention, or what it is waiting for.
3. The one useful action and its effect.
4. A compact lifecycle summary.
5. Evidence, execution envelope, and history.

Among the three alternatives, I favor the compact linear treatment for ordinary operation. The lane chart could serve detailed history if it is already needed. The orbit adds visual interpretation work without helping the current decision.

The three variants are legitimate design exploration; they are not evidence that three production modes have already been built. Choose one before implementation. Do not ship “beads / laminae / orbit” as another preference the operator must understand.

There are also semantic problems with the chart. Its column says “ELAPSED” but contains “commit pinned,” “WorkPlan v2,” and “current boundary,” not durations. The fill position is hard-coded to `44%`; there is no demonstrated calculation that would make that a meaningful fraction of work completed. Remove the implied precision or use an existing discrete state indicator. See [rail markup](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L481>) and [rail data](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L1338>).

## F09 — Resolve lifecycle language before styling it further

The prototype uses Source → Plan → ChangeSet → Delivery → Verify → Terminal. The [product vision](../../design/product-vision-and-boundaries.md#L91) uses Discover → Plan → Implement → Test → Verify → Release → Observe. Repo Mode adds source delivery and explicitly excludes deployed Release/Observe work.

These are not merely different labels for the same sequence. “ChangeSet” is an artifact/boundary, “Terminal” is a state category, and the placement of Verify changes what the user might think has already been proven.

**Recommendation:** choose the existing applicable workflow as the authority for each screen and label its outcomes consistently. Do not invent a third canonical lifecycle to bridge the two. For source-only work, clearly indicate that completion means reviewed source merge. A Product summary should aggregate WorkItems rather than pretend the whole Product occupies one stage.

The selected HTML also has seven navigation destinations, while the written Repo Mode screen contract proposes Overview, Products, Repositories, WorkItems, Agents, and other destinations. The owner has selected this HTML as the current visual reference. This review does not recommend mechanically adding every screen in the older contract. The conflict needs to be resolved in the artifacts so the next implementer has one direction.

## F07 — Responsive and keyboard behavior need correction

**Observed:** at 433 CSS pixels wide, Triage retains fixed-width status and action columns. The primary title column collapsed to roughly 2 CSS pixels in the DOM measurement, producing severe wrapping and overflow. The page's overall scroll width still equaled its viewport width, so checking only for horizontal page overflow would incorrectly suggest success. The horizontal navigation hides its scrollbar, making offscreen destinations harder to discover.

The WorkItems list similarly specifies fixed columns totaling 564 pixels before its flexible title, gaps, and padding. The cockpit retains side-by-side cards and a four-column identity grid. At the ordinary 883-pixel desktop width, the Runs example's long “Budget extension requested” status crowds the turn count. This is not solely a phone problem.

**Source facts:** Triage, WorkItem, and expandable audit rows use clickable `div` elements without button/link semantics or keyboard focus handling. These are absent from the accessibility tree as actionable controls, even though they are primary pointer interactions. The old React Triage rows are actual buttons; do not regress that behavior while adopting this design.

Evidence: [Triage rows](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L165>), [WorkItem rows](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L411>), [audit rows](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L863>).

**Simplification:** stack the existing row fields at narrow widths, preserve the title and action, and move secondary metadata below. Use native links/buttons for navigation and disclosure. Retain full labels rather than abbreviating “Incidents” to “Inc.” These are corrections to existing surfaces, not new features.

Animations include recurring pulse, sheen, and ring effects. No reduced-motion treatment was found in the design source. A quieter default and reduced-motion support would improve approachability. A full keyboard, screen-reader, and contrast audit remains outstanding.

## The approval example teaches the wrong boundary

The Tools screen presents an approvable request to write `/etc/pharness/policy.toml`, explicitly outside the pinned workspace, and shows a patch relaxing review rules. The example says no rule can grant the action “without review.”

The filesystem tool independently rejects writes whose canonical parent is outside its workspace root. Human approval does not remove that filesystem boundary. See [prototype approval fixture](<../../pharness-ui-design-overhaul/Pharness Console.dc.html#L1001>) and [filesystem boundary](../../../crates/pharness-core/src/tools/fs.rs#L54).

**Recommendation:** use an approval example that is valid within the declared tool boundary. Present an outside-workspace action as denied/unavailable with an explanation, not as something an operator can make safe by pressing Approve. This is a fixture/copy correction. It is not a recommendation to relax the sandbox.

## Reduce the explanation the operator must parse

| Current wording | Clearer wording or treatment | Meaning to preserve |
| --- | --- | --- |
| “Durable autonomous delivery intents…” | “Changes PHarness is working on” | WorkItems retain durable history |
| “Review execute_attempt” | “Review missing test evidence” | Link to the actual missing acceptance evidence |
| “Advance internal steps” | Remove, or name the specific permitted next action | Never suggest a way around a blocker |
| “Tools” | “Tool approvals” | This page is a decision queue, not a tool catalog |
| `staging_validation` | “Staging review,” with the exact key in details | The specific lifecycle condition still matters |
| “Terminal” | The actual outcome: merged, failed, cancelled, etc. | Terminal does not mean successful |
| “Execution envelope” | “Run limits” | Keep concrete attempts, budget, and deadline information |

“Capture Change Set” may legitimately describe a state-changing action. Do not rename it “Review changes” if clicking it creates a new immutable record. Better copy can say “Record changes for review” while clearly explaining the effect.

Triage's five summary cards repeat related counts before showing actionable work. I would reduce their prominence, especially on smaller screens. Keep the queue and its reasons central. “16 signals” is supporting context, not necessarily 16 things the user needs to do.

The fixture says “Oldest exceptions first,” but the displayed ages begin 41m, 34m, 1h12m, then continue toward 5h22m. Match the example order to the stated rule or describe the actual prioritization. This is a small inconsistency that is cheap to remove.

## F16 — Finish the state design before calling the interaction validated

The cockpit's Overview, Attempt, Delivery, and Evidence controls change selected styling, but the cockpit content remains the same. Clicking Attempt did not show the separate execution stream. Search, New WorkItem, and multiple action controls have no corresponding workflow handler in the prototype. This report treats those as unfinished design coverage, not production defects or a request to build new features.

The selected artifact does not demonstrate loading, failed load, stale data, empty scope, or unavailable capability states. Its “Live” badge remains an unconditional fixture. Those states are especially important because the implemented UI already has a confirmed failure-to-empty-state problem.

The smallest useful design refinement is to specify how these existing screens behave under ordinary states: fresh empty queue, unavailable queue, stale retained data, blocked work, external wait, and successful source-only completion. Use known data and honest wording. Do not add more successful-looking widgets before these meanings are settled.

**My conclusion:** keep this visual direction, but simplify it aggressively. The next iteration should make the current condition and next decision obvious, use a single coherent workflow example, and remain legible when space or data is missing. It does not need to look more sophisticated. It needs to be easier to trust.
