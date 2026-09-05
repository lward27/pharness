# ASTRA M08: Restore Finance log delivery

Status: reviewed recovery change; deployment and a fresh five-minute Finance log window are required before acceptance.

The Finance staging observation window at 22:38:30–22:43:30 UTC on 2026-09-05 was inconclusive because both application log streams stopped arriving in Loki. Direct, bounded Pod log reads showed the applications still writing through 22:44 UTC. The [collector preflight](ASTRA-M08-PROMTAIL-DELIVERY-GAP-PREFLIGHT.json) records `monitoring/promtail-2d7rh` as unready with 333 restarts and its latest termination as `OOMKilled`, exit 137, at 22:40:20 UTC. Its 128 MiB memory limit is an observed failure boundary, not a healthy operating budget.

## Scoped recovery

Change the existing `charts/promtail/values.yaml` memory reservation from 64 to 128 MiB and its limit from 128 to 512 MiB. The two nodes each have about 16 GiB allocatable memory, reported 48% and 38% usage, and no MemoryPressure. This reserves an additional 64 MiB per node and allows a bounded additional 384 MiB per collector. The established one-at-a-time DaemonSet rollout and host-mounted position/log storage remain in place.

The image, chart version, scraping configuration, destinations, permissions, CPU request and retained logs are unchanged. Recovery stays under the existing `promtail` Argo application in `lucas_engineering`; no direct workload patch, restart, deletion, prune or force sync is part of this change.

## Validation and acceptance

[Premerge validation](ASTRA-M08-PROMTAIL-MEMORY-VALIDATION.json) includes Helm dependency resolution for the unchanged chart version, successful lint/render and a successful scoped server dry-run. Comparing the old and new rendered DaemonSets found exactly the two memory fields above. Lint reports only the wrapper chart's existing icon/no-template warnings.

Observe the exact merged Argo revision and the completed two-node rollout. Require both new collector Pods ready, stable restart counts, measured memory below the configured ceiling, and a complete new five-minute application-scoped Loki window. Pod readiness alone cannot close the telemetry gap. Preserve the two failed Finance observation windows and do not relabel them after delayed logs arrive.

## Recovery limits

If this finite memory change is insufficient, retain the failed observation and diagnose the actual new termination or delivery error before any further adjustment. Reverting these two values through GitOps restores the previous allocation but is expected to restore the demonstrated out-of-memory risk; it is not a recommended service recovery. Existing positions and log files must not be deleted. This correction does not establish autonomous deployment, production approval, or the program's 24-hour operating acceptance.
