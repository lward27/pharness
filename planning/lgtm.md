# Decisions

- Start LGTM support with a read-only Prometheus inventory capability instead of a broad observability abstraction.
- Add `prometheus_inventory` as a distinct typed action rather than overloading `prometheus_query`. Inventory has no user-provided query string, so policy and logs can treat it as a bounded health read.
- Inventory reads Prometheus targets, rules, and active alerts through configured `PHARNESS_PROMETHEUS_URL`.
- Compact inventory output before it reaches events, artifacts, model context, or CLI output.
- Keep labels allowlisted and omit alert annotations and rule query bodies from inventory summaries. Those fields can contain noisy or sensitive application details.
- Add `loki_log_summary` as a bounded log read instead of a generic log-query abstraction. It accepts a LogQL selector/query, clamps the time window and line limit, redacts secret-shaped lines, and allowlists stream labels.
- Expose inventory through all current control-plane paths:
  - direct API capability execution
  - `pharness-cli capabilities prometheus-inventory`
  - Fireworks worker tool schema
- Expose Loki log summary through the same control-plane paths:
  - direct API capability execution
  - `pharness-cli capabilities loki-log-summary`
  - Fireworks worker tool schema
- Configure Loki separately through `PHARNESS_LOKI_URL` or `[cluster].loki_url`. The capability should fail as structured tool JSON when the URL is not configured.

# Backlog

- Add typed namespace/pod/container filters on top of `loki_log_summary` once the common cluster log labels are stable.
- Add Tempo trace lookup only after there is a stable service/request correlation shape.
- Add Prometheus target/rule filtering if full inventory becomes too large for common homelab use.
- Consider persisting normalized Observation records once CRD/controller work starts.
- Add live smoke evidence for `prometheus_inventory` after the next Prometheus port-forward run.
- Add live smoke evidence for `loki_log_summary` after the next Loki port-forward run.
