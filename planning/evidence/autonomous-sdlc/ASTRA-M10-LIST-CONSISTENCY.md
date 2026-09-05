# ASTRA M10: List consistency and readable failures

Status: implemented and validated locally; not deployed. M10 acceptance remains open.
Date: 2026-09-05. Source base: `48c77b7b4438d621ff9563b913857bcf771f1800`.
Branch: `codex/astra-console-list-consistency`.

## Objective results

- WorkItem and Repository search inputs remain mounted and focused during loading
  and failed requests. Lifecycle controls remain usable when their list is unavailable.
- WorkItems, Repositories and AgentRuns share pagination that uses the server's
  complete filtered count and the actual visible row count. An emptied later page
  retains Previous even when the total shrinks below one page; Previous returns to
  an available page. Single-page results show their count without disabled navigation.
- Filtered emptiness, an empty registry/history, and an empty later page have
  different explanations. An absent count is unavailable rather than zero.
- Legacy WorkItems have independent pagination using the same search and lifecycle
  partition. Changing either resets both offsets. Their loading or failure state
  remains visible independently of the primary list, and their heading describes
  their original workflow rather than claiming the new hosted SDLC is complete.
- Compiled-profile failures are visible. JSON error responses display the server's
  explanation, while unknown JSON/HTML failures use the HTTP status. The error's
  status remains available to stale-action and conflict recovery.
- Repository inventory has a valid named, keyboard-focusable region; the empty
  state no longer violates the ARIA labeling rule caught by the browser checks.

## Validation and limitations

The production UI build and **94 unit checks** pass, including 15 new checks for
pagination recovery, preserved search focus, independently paged legacy records,
failure isolation, missing totals, and readable errors with retained conflict status.
**116 distinct presentation/browser checks** pass across desktop and 390-pixel
phone projects: 115 in the final full run, then the remaining mobile review check
against its visually reviewed baseline. Eight new cases cover both themes,
keyboard search/Previous, changing totals, empty and unavailable states, no serious
or critical accessibility violations, no page overflow, and no mutation requests.

The first browser run exposed the invalid ARIA label; it was corrected. Existing
state-catalog screenshots changed because totals and the legacy heading now appear.
The initial-failure assertion was updated to require the exact readable server
message instead of a status number. A repeatable 199-pixel difference confined to
the unchanged Source delivery heading in the mobile onboarding screenshot was
inspected at full resolution; that single baseline was refreshed and passed a
separate recheck. No visual tolerance or timeout was increased.

These are presentation and regression checks, not live autonomous acceptance.
The existing real API/controller journey was not rerun for this UI-only slice.
The local Lamina preview runs at port 18444 with read-only fixtures. Opening it in
the app was queued; native interaction was unavailable because the Mac was locked.
Automated browser screenshots were inspected directly. The owner's final walkthrough
and the full scope/filter/route/production/recovery matrix remain M10 gates.

## Subjective assessment

The lists are easier to recover from and less misleading. Keeping search visible
matters more than another decorative control. Compact counts avoid adding a pair
of unusable buttons to every small list. Readable failures are a clear improvement.

The inherited WorkItem rows still repeat technical identifiers and can truncate
the current condition; those are real remaining polish issues. This change preserves
Lamina's navigation, typography and visual identity. It does not finish the dense
detail panels, retirement of competing routes, or production approval presentation.

## Evidence and recovery

[Validation manifest](ASTRA-M10-LIST-VALIDATION.json) records exact file/log hashes.
Representative screenshots are presentation fixtures:

- [Filtered empty list, dark phone](ASTRA-M10-SCREENSHOTS/ASTRA-list-empty-dark-mobile.png).
- [Filtered empty list, light desktop](ASTRA-M10-SCREENSHOTS/ASTRA-list-empty-light-desktop.png).
- [Unavailable legacy list, light phone](ASTRA-M10-SCREENSHOTS/ASTRA-list-unavailable-light-mobile.png).
- [Shrinking result set, dark desktop](ASTRA-M10-SCREENSHOTS/ASTRA-list-unavailable-dark-desktop.png).

This slice advances F03, F04, F07 and F16; none is closed solely by these checks.
It changes no API contract, workflow authority, database schema or production
resource. Release through the program's immutable image procedure. Reverting this
presentation commit preserves workflow records and authority.

[M10 acceptance](../../programs/autonomous-sdlc/ASTRA-10-CONSOLE-CONVERGENCE-AND-POLISH.md)
and the [program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) remain authoritative.
