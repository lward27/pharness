# ASTRA M04E Slice 3 — transport observability implementation

Date: 2026-09-23. **Local source implementation and test evidence only.** No image was built or released, no cluster resource was changed, and no model-backed operation was dispatched.

## Change and correlation contract

The worktree is based on PHarness `cffa63f2cf41168883e798ded07d2bb9c2e65639` on branch `codex/astra-m04e-slice3`. The exact-policy Test Diagnosis preflight already creates a signed model grant whose `run_id` is `verify_{verification_id}_{case_index}_{attempt}` in `crates/pharness-api/src/app/inference.rs::execute_protocol_calibration_case`. The gateway now uses that verified, per-case value as the log correlation ID. This ties transport evidence to the recorded preflight, case index, and attempt without adding an HTTP header or changing the grant schema.

Before logging, a correlation ID is retained only when it is 1–128 ASCII letters, digits, hyphens, or underscores. Any other value is replaced in logs by its SHA-256 digest. Gateway events include target and policy identities, request sequence, attempt, status code, elapsed milliseconds, retry decision, and a fixed error class. They distinguish request admission, upstream attempt start, response headers, first stream chunk, stream completion, first-response timeout, connection/request error class, non-success status, stream-read failure, and stream-idle timeout. Successful phase details are debug-level; attempt starts are info-level; failures are warnings.

No provider URL, grant, credential, prompt, request/response body, raw `reqwest::Error`, or response header is logged. The gateway constructs a new upstream request rather than forwarding the API grant or correlation value as provider headers.

## Related proxy noise correction

The worker egress proxy now treats a client EOF before any CONNECT request as a normal no-op and emits only a debug event. Its 5-second readiness and 10-second liveness probes intentionally use bare TCP sockets; previously each close was logged as a warning. This does not change allowed hosts or proxy traffic behavior.

## Validation

- `cargo test -p pharness-model-gateway`: 15 passed; 0 failed. The test suite reported three Helm-specific tests ignored because they require Helm.
- `cargo test -p pharness-worker`: 48 unit tests and 1 hosted-build integration test passed, including the new pre-CONNECT EOF regression.
- `cargo clippy -p pharness-model-gateway --all-targets -- -D warnings` and `cargo clippy -p pharness-worker --all-targets -- -D warnings`: passed.
- `cargo fmt -p pharness-model-gateway -- --check` and `git diff --check`: passed.
- Captured-log tests correlate successful stream, loopback connection refusal, simulated 1-second first-response timeout, non-success HTTP response, and stream-idle timeout while asserting the test grant, credential, prompt sentinel, provider-response sentinel, and upstream error-body sentinel are absent from logs.

## Release and operation boundary

This is not deployed evidence. The current serving gateway has no request-level logs from this change, so the 2026-09-23 timeout remains unexplained. Do not retry Test Diagnosis yet. Next, use the repository's immutable build/release path for the compatible gateway/worker artifacts, retain their source/image digests, and complete the program's required review/approval boundaries. After the image is observed serving, verify the exact API/gateway runtime and registry, then decide whether one exact-policy Test Diagnosis preflight is justified. Any fresh failure remains terminal for that attempt; it does not authorize qualification, Verifier, V2 activation, or connected WorkItem dispatch.
