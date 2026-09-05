# ASTRA M04: Repair qualification on 48c77b7

Status: **the seeded Repair qualification passed; M04 remains unaccepted**.

The existing gateway ran `repair-kimi-k3-v2@v1` through `fireworks-kimi-k3@v1`, model `accounts/fireworks/models/kimi-k3`, on immutable runtime `48c77b7b4438d621ff9563b913857bcf771f1800`. Protocol calibration passed 30/30 before the two frozen Repair attempts. No model, profile, fixture, threshold or execution limit was changed for this run.

| Result | Attempt 1 | Attempt 2 |
| --- | --- | --- |
| Accepted after seeded repair | 24/24 | 24/24 |
| Rust | 8/8 | 8/8 |
| Python | 8/8 | 8/8 |
| Node | 8/8 | 8/8 |

All 48 cases passed visible acceptance, hidden checks and protected-path checks, with no recorded policy violations. The gate requires at least 23/24 after repair and 7/8 for each stack in each attempt. The report records `gate_passed=true`, `candidate_safe=true`, and `infrastructure_valid=true`.

Each Repair case deliberately starts with a seeded defect. Accordingly, `correction_used=true` and `first_pass=false` in all 48 cases. This is **not** evidence that the successful live Builder run handed an actual failed workspace through Test Diagnosis and one correction. That cross-stage behavior remains an acceptance requirement. The separate Builder qualification passed both first-pass 24-case attempts.

The run completed from **20:21:19.802 to 20:54:00.680 UTC on 2026-09-05**, 1,960.878 seconds of wall time. It recorded 415 turns/tool calls and eight recoverable tool failures within the existing limits. Usage: 2,267,043 prompt tokens; 55,940 completion tokens; 74,336 cached tokens; 5,365 reasoning tokens. These categories are reported as received and are not added together as disjoint billing units.

Evaluation: `infeval_01a0733b942376b19976dac13414a52a`. Qualification: `inferqual_01a0735980e874428eabab35c96b5694`. Kubernetes Job: `pharness-inference-eval-4fe69a798c67`. Canonical report hash, independently checked against the saved report: `sha256:529183ba0dacbaa86b69862263c000557884684b52dbfe964d009368d1d5bb10`.

Evidence: [raw result](ASTRA-M04-48C77B7-REPAIR-LIVE-RESULT.json), [analysis and usage](ASTRA-M04-48C77B7-REPAIR-ANALYSIS.json), [profile and binding pin](ASTRA-M04-48C77B7-REPAIR-PROFILE-PIN.json), [30-case protocol calibration](ASTRA-M04-48C77B7-REPAIR-LIVE-PROTOCOL.json).

All stage qualification attempts on this runtime are now terminal. Builder and Repair passed; Onboarding, Planner, Test Diagnosis and Verifier failed. Hosted creation remains disabled. The already merged harness corrections need a compatible immutable release and fresh evidence. This pin qualifies only the recorded runtime and binding; it does not qualify a later release automatically, and it does not close source delivery, deployment or Finance acceptance.
