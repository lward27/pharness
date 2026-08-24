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
- Prometheus inventory/query and Loki log summary observations can now be attached to a Release as V1 observability evidence.
- Keep Release observability evidence attached by observation id instead of introducing a broad LGTM abstraction. The normalized LGTM model should wait until there are at least two concrete runtime verification policies that need it.
- Release-attached observability evidence now promotes `attention_required` findings into durable Incident candidates. Clean and unknown evidence remains evidence-only.
- Release observability incidents now create conservative draft remediation plans that start with read-only Prometheus/Loki/Argo rechecks and require approval before any file, pipeline, cluster, or production-impacting mutation.

# Backlog

- Add typed namespace/pod/container filters on top of `loki_log_summary` once the common cluster log labels are stable.
- Add Tempo trace lookup only after there is a stable service/request correlation shape.
- Add Prometheus target/rule filtering if full inventory becomes too large for common homelab use.
- Inspect Loki summaries for suspicious but successful log patterns before promoting them to incidents; V1 only promotes explicit error status.
- Add live smoke evidence for `prometheus_inventory` and `loki_log_summary` after the next Prometheus/Loki port-forward run.
