# Decisions

- Direct capability execution now accepts `timeout_ms` and wraps the capability future with an API-level timeout.
- Timeout cancellation returns `status = cancelled`, `executed = true`, `cancelled = true`, and no tool result.
- Timeout cancellation writes a durable `direct_capability.cancelled` audit event with `executed`, `cancelled`, `timeout_ms`, action id, and policy decision.
- The CLI exposes `--timeout-ms` on every direct capability command and sends it to `POST /api/capabilities/execute`.
- The API clamps direct capability timeouts to a bounded local range so clients cannot request unbounded blocking work through the direct capability path.
- Run cancellation has a CLI path: `pharness-cli runs cancel --run-id ...`.
- `runs cancel --with-events` returns the cancelled run and durable event log so smoke tests can validate cancellation without raw curl.

# Backlog

- Add direct capability request IDs only if operators need asynchronous direct capability execution with a separate cancel endpoint.
- Add cancellation metrics once the observability story includes API-level counters.
- Consider per-capability default timeout profiles after more homelab dogfooding.
