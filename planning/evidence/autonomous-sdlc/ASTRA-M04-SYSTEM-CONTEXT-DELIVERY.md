# ASTRA M04: Preserve MiniMax system context

Status: implemented and locally validated; **not deployed or qualified**.
Source: `915f3ee34a089191c0fb08a9d643ba0a3791d90e`.

The [source-19 onboarding failure](ASTRA-M04-19B0C55-ONBOARDING-FAILURE.md)
exposed missing controller context at the provider boundary. A bounded direct
prompt-format diagnostic found that MiniMax M3 rendered only the first of six
separate system messages. Joining them preserved all six. The tested Kimi and GLM
models preserved the six sections in either form.

The shared request builder now joins system sections, in order, into one leading
system message only for Fireworks MiniMax M3. The gateway applies the same
idempotent normalization after resolving the actual model, covering older workers.
It preserves the relative order and content of user, assistant and tool messages,
tool-call IDs and provider replay fields. Stored model requests and durable replay
history remain unchanged. No new backend, prompt instruction, tool, model selection,
execution budget or database migration is introduced.

Regression checks exercise discovery, budgets, late recovery checkpoints and
protocol corrections against the observed first-system-only boundary. They also
cover direct/gateway request construction, unchanged other backends/models,
empty/single-system envelopes and stable grant serialization. **86 distinct tests
passed**: 21 transport/gateway, 28 runner, 35 evaluation and two API calibration
regressions. Clippy, formatting and architecture checks passed, including the five
dependency-parser checks. Source file hashes remained identical through validation.
The evaluation log contains deliberately failing child fixtures; the enclosing
35-test suite passed. Those child outputs are retained verbatim.

[Validation and exact log hashes](ASTRA-M04-SYSTEM-CONTEXT-VALIDATION.json) provide
the local evidence. The original [provider diagnostic](ASTRA-M04-SYSTEM-ENVELOPE-DIAGNOSTIC.json)
is transport diagnosis, not a gateway qualification run. Existing protocol checks
did not cover multiple system messages and cannot prove this correction live.

Deploy all affected worker/gateway artifacts from one merged source revision using
the immutable release procedure. Preserve schema 54 and Finance history; keep
hosted creation and Coding Reliability V2 disabled. A release rollback may use the
recorded compatible reader while no newer incompatible writes exist. Run fresh
gateway and stage qualification on the deployed source before enabling autonomous
work. The failed source-19 and earlier results remain unchanged.
