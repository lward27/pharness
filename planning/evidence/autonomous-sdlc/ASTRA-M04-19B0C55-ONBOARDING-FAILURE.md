# ASTRA M04: Source-19 onboarding qualification failed

Status: **not qualified**. The evaluation finished at 2026-09-06T00:42:00.114000+00:00.
Runtime: `19b0c55c48e3614d0b4507d56df3029a52475618`.
Evaluation: `infeval_01a073fe89947b33a1e8d1a24b7dc200`; qualification: `inferqual_01a0742a3c327a23882ff985877ba392`.
Policy: `onboarding-minimax-m3-v2@v1`, through the existing gateway.

## Objective results

| Boundary | Result |
| --- | --- |
| Existing protocol calibration | 30/30 passed |
| First Onboarding V2.1 attempt | 0/12 passed |
| Second Onboarding V2.1 attempt | 0/12 passed |
| Accepted typed proposals | 0/24 |
| Submission tool attempts | 79 |
| Final recoverable validation error | Discovery binding mismatch in all 24 cases |
| Terminal reasons | 9 soft budget boundaries; 7 missing actions; 8 recovery ceilings |
| Changed workspace paths | None reported |
| Qualification / activation | Failed / disabled |

The report's `infrastructure_valid` flag means all fixture results were present;
it does not establish that the model received the complete intended context.
`typed_submission_missing` appears in all 24 reported violation lists. No false
approval or false rejection was recorded, because no proposal reached that gate.
The last recoverable error cannot establish whether the ID, hash or both were wrong:
rejected proposal arguments were not retained. Every missing document remains missing.

Usage totals: 1,716,688 prompt tokens, 251,737
completion tokens, 1,398,159 cached tokens and 157,098
reasoning tokens over 329 turns. Cached/reasoning categories overlap
the prompt/completion totals. There were 71 recoverable failures,
zero recorded context-budget failures and zero compactions. Limits were unchanged.

## Source and transport diagnosis

The current runner sends the base instruction, repository instructions, environment,
repository map, stage prompt and controller context as separate system messages;
execution-ledger/budget messages add more. The shared transport and gateway preserved
that arrangement. The onboarding validator requires exact discovery identity/hash.

A separate six-request provider-format diagnostic used synthetic markers and the
documented Fireworks prompt-inspection interface. MiniMax M3's returned prompt
contained only the first of six separate system sections. Joining the same six
sections preserved all six. Kimi K3 and GLM 5.3 preserved all six in both arrangements.
This establishes a current MiniMax context-delivery defect and explains how the
controller discovery can be absent despite a successful HTTP/protocol check. It
does not reconstruct rejected arguments or guarantee the remaining stage behavior.

The existing 30-case protocol calibration uses one initial system message and also
supplies its marker in the user message and tool enum. It therefore did not exercise
this actual context boundary. Its historical pass is retained with that limitation.

[Diagnostic observations](ASTRA-M04-SYSTEM-ENVELOPE-DIAGNOSTIC.json) contain the
synthetic requests, provider-response hashes, prompt-field hashes, marker positions
and usage. They retain no credentials or generated reasoning text. Each diagnostic
request allowed only 32 output tokens; those diagnostic settings did not alter the
live policy or qualify a model. [Fireworks prompt inspection](https://docs.fireworks.ai/guides/querying-text-models#configuration--debugging)
documents the echo/raw-output mechanism used here.

The transport correction must preserve every system section for MiniMax, including
checkpoints and recovery instructions, without changing stored replay history,
tool results, tools, limits or other models. It needs local regression checks and
an immutable deployment followed by fresh gateway and stage qualification. This
failed run and the preceding 48c77b7 failures must never be relabeled as passes.

[Raw result](ASTRA-M04-19B0C55-ONBOARDING-LIVE-RESULT.json) and
[analysis](ASTRA-M04-19B0C55-ONBOARDING-ANALYSIS.json) retain the exact report hash
and measured result. M04, hosted activation and Finance end-to-end acceptance remain open.
