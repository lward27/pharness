# ASTRA M04: Source-19 Planner qualification failed

Status: **not qualified**. The two complete attempts scored **6/12 and 7/12**.
All 24 typed plans were retained; eleven failed the boundary predicate.
The runtime was `19b0c55c48e3614d0b4507d56df3029a52475618`.

The recorded failure is mixed. Nine plans contain forbidden terms in step prose,
usually as explicit prohibitions. For example, the second stale-context plan says
not to use the forbidden deployment path while listing only allowed paths. The
current substring predicate rejects that warning. Two other failed plans include
`requirements.lock` in their structured paths, outside the fixture's writable
allowlist. Some plans also infer Python test commands that the fixture never declares.

The twelve scenarios currently share generic passing Python files. Their supplied
acceptance names lack corresponding command definitions, while the context marks
the same scenario marker as both a verified fact and an unresolved contradiction.
These fixture limitations and the prose predicate need contract-aligned validation;
they do not establish that the plans are safe or sufficient for real maintenance.
None of the eleven failures is relabeled as a pass.

The next correction must preserve the existing scenarios, acceptance coverage,
path/command boundaries, budgets and thresholds; distinguish a prohibited operation
from a warning about it; and use realistic, internally consistent controller evidence.
Any changed fixture or scorer needs an explicit suite revision and fresh gateway runs.
The frozen 24-task coding benchmark remains unchanged.

[Raw result](ASTRA-M04-19B0C55-PLANNER-LIVE-RESULT.json) and
[analysis](ASTRA-M04-19B0C55-PLANNER-ANALYSIS.json) preserve evaluation identity,
report hash, usage and all original outcomes. M04 and hosted activation remain open.
