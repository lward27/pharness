# Campus session Jobs in lab slots

`KubernetesCampusProvider` (`crates/pharness-core/src/campus_lab_session/kubernetes.rs`)
is the real `CampusExecutionProvider` for the lab-session journal
([campus-session-journal.md](campus-session-journal.md)). Each session
occupancy runs as one Kubernetes Job in an admin-precreated slot namespace.
This page records the cluster contract, the limits a session gets, how to
qualify a cluster, and the known gaps.

## Cluster contract

The slots and their guardrails are Argo-managed from `lucas_engineering`
(`charts/campus-lab`, Argo app `campus-lab`); the node parts are in
`ops/campus-lab-gvisor`. The controller cannot change any of them.

| Object | Purpose |
| --- | --- |
| RuntimeClass `gvisor` | handler `runsc`, `scheduling.nodeSelector pharness.dev/campus-lab-runtime=gvisor`, `overhead.podFixed` 50m / 128Mi |
| Namespaces `campus-lab-slot-1..5` | Pod Security `restricted` (enforce, latest), labels `pharness.dev/campus-lab=true` and `pharness.dev/campus-slot=<ns>` |
| ResourceQuota `campus-lab-slot` | one Job, one Pod, 16 ConfigMaps, two Secrets (the session's broker token); no Services, PVCs or other workload kinds; 2 CPU / 3 GiB / 256 MiB ephemeral storage (requests and limits) |
| LimitRange, NetworkPolicy `campus-lab-default-deny` | default limits; no ingress or egress |
| NetworkPolicies `campus-lab-session-broker`, `campus-lab-session-egress` | the broker port, and the Campus egress proxy for Pods labelled for egress ([campus-egress.md](campus-egress.md)) |
| default ServiceAccount | `automountServiceAccountToken: false` |
| Role/RoleBinding `pharness-campus-controller` | `campus-poc/pharness-campus-controller` may create/get/list/watch/delete Jobs, create/get/list/delete ConfigMaps, get/list/watch/delete Pods, create/get/list/update/delete Secrets and get/list NetworkPolicies in the slot; nothing else (no exec, patch, watch on Secrets, or policy changes) |
| ClusterRole `pharness-campus-controller-runtime` | get on `runtimeclasses/gvisor` only |
| VAP `pharness-campus-lab-session-guard` | independent admission for Pods and Jobs in `pharness.dev/campus-lab=true` namespaces (below) |
| VAP `pharness-campus-lab-secret-guard` | slot Secrets may only be the session's broker token Secret ([campus-egress.md](campus-egress.md#cluster-objects-chartscampus-lab)) |
| VAP `pharness-campus-lab-image-allowlist` (LUC-121) | digest allowlist; admits the approved runner images |
| node `ubuntu-lucas-engineering-2` | gVisor `release-20261026.0`, `kubepods.slice` `MemorySwapMax=0` |

The guard refuses, whoever the requester is: privileged containers,
privilege escalation, added capabilities, any volume except `emptyDir` and
the session's read-only broker token Secret (so no hostPath, projected
token, other Secret or ConfigMap volumes), a missing `network-gate` init
container, egress labels or hosts outside the approved routes, token
automount (unset or true), hostNetwork/hostPID/hostIPC, host ports, a
runtimeClassName other than `gvisor`, a nodeSelector other than the pool,
`nodeName`, tolerations, tag-only images, environment from Secrets or
ConfigMaps or fields, missing CPU/memory/ephemeral-storage limits, a missing or
over-14400 s `activeDeadlineSeconds`, restartPolicy other than Never,
ephemeral containers, any workload kind other than a session Job, and
ownership labels that are missing, name another controller or slot, or
differ between the Job and its Pod template. The guard also runs on
UPDATE, so relabelling an admitted Pod into another slot is refused.

## Ownership, fencing and idempotency

Every object carries `pharness.dev/campus-controller`, `-tenant`, `-session`,
`-slot`, `-generation` and `-record` (`anchor`, `workload`, `tombstone`).
The Job and its anchor ConfigMap share a name derived from
(session, slot, generation), so a repeated provision finds the existing
objects; the provider reads the Job before creating it because the slot
quota refuses a duplicate create before the API reports AlreadyExists.

- The Job is owned by the anchor ConfigMap. `delete` replaces the anchor with
  a tombstone, so a stale worker cannot recreate the occupancy and the
  garbage collector removes any Job it creates against the deleted anchor.
- A provision is refused when the slot holds an anchor or tombstone of a
  newer generation (`a newer occupant of the lab slot exists`), when the
  operation key is reused with a different spec, and when the occupancy is
  already deleted.
- A missing or unreadable RuntimeClass refuses the provision before anything
  is created; there is no fallback to runc. Security profiles, non-gVisor
  isolation and disabled launch gates are refused because the slots cannot
  provide them.
- A missing or weakened slot NetworkPolicy refuses the provision, and so does
  approved egress without the egress proxy configuration or the slot's
  egress policy ([campus-egress.md](campus-egress.md#provider-enforcement)).

`KubernetesCampusProviderConfig::m1_lab()` maps slot ids to namespaces with
slot id = namespace name (`campus-lab-slot-N`). Journal slot ids are
free-form (`register_campus_lab_slot`), so register the M1 slots under those
names. The slot label on every object must equal the namespace's
`pharness.dev/campus-slot` label, or admission refuses it. The API builds
this provider when `PHARNESS_CAMPUS_LAB_PROVIDER=kubernetes`
([campus-lab-admission.md](campus-lab-admission.md#configuration)); its
catalog lists `lab_slot_ids` `campus-lab-slot-1..5` so the journal ids
equal the namespace labels, and startup refuses a provider whose slot map
differs from the catalog. Deployment: [campus-poc-deployment.md](campus-poc-deployment.md).
Registered ids the catalog no longer lists are retired at startup
([campus-teardown.md](campus-teardown.md#slot-ids)).

## Session limits (research profile)

| Resource | Limit | Enforced by |
| --- | --- | --- |
| CPU | 500m (+50m overhead) | Pod cgroup |
| Memory | 2048 MiB container limit; Pod cgroup 2176 MiB including the RuntimeClass overhead | Pod cgroup `memory.max`; swap closed by `kubepods.slice` `MemorySwapMax=0` |
| `/workspace` | 768 MiB memory-backed `emptyDir` | sizeLimit, counted in memory |
| `/tmp` | 256 MiB memory-backed `emptyDir` | sizeLimit, counted in memory |
| Root filesystem | read-only | `readOnlyRootFilesystem` |
| Container log storage | 128 MiB ephemeral storage; PID 1 discards stdout/stderr | limit, `session_command` |
| Processes | RLIMIT_NPROC 128 (soft and hard) for PID 1 and everything it starts; core dumps off | `session_command` (`ulimit -u/-p`, `ulimit -c 0`) |
| Lifetime | `activeDeadlineSeconds` = session TTL, at most 14400 s; `backoffLimit: 0` | Job and Pod |
| Slot | one Job, one Pod | ResourceQuota |

## Qualifying a cluster

```sh
scripts/campus-session-jobs-cluster-test.sh \
  --chart <lucas_engineering>/charts/campus-lab --out /tmp/campus-session-jobs
```

It needs cluster-admin on `lucas_engineering`, helm, jq and cargo. It
creates five temporary probe slots (`campus-lab-probe-N`) from the same
chart, plus a test-only image allowlist that admits only the research
profile digest. It also checks that an Argo-managed slot admits the
provider's session Job and refuses an unapproved digest. The provider runs as the controller identity
(`kubectl --as system:serviceaccount:campus-poc:pharness-campus-controller`)
through the ignored test `crates/pharness-core/tests/campus_session_jobs_live.rs`.
Phases: preconditions and versions, controller RBAC (30 `can-i` checks),
provider provision, stress, provider delete, admission matrix. The probe
namespaces and policies are deleted on exit, and the script prints what
remains.

Run `ops/campus-lab-gvisor/run-on-node.sh --check` in the GitOps repository
first; it reports drift in the gVisor binaries, the containerd template, the
node label and the swap setting.

## Residual risks

- The real slots admit only the two LUC-121 approved runner digests
  (release `9761707`); the profile fixtures pin those digests.
- Approved egress goes only through the Campus egress proxy; see
  [campus-egress.md](campus-egress.md#residual-risks).
- Node 2 is shared with production. It has no kube/system reservations, no
  taint for the pool, `podPidsLimit` unset, about 10 GiB of free disk and
  4 GiB swap (now closed to Pods).
- The 128-process limit is an rlimit set by PID 1. Browser terminals
  ([campus-browser-terminal.md](campus-browser-terminal.md)) set the same
  limit before their shell starts and refuse to start without it, but an
  operator's own `kubectl exec` is not under it (the controller has no
  `pods/exec`; the separate terminal identity does). gVisor still costs
  host threads per blocked sandbox task.
- gVisor charges the whole sandbox to the Pod cgroup, so a session can use
  its container limit plus the 128 MiB overhead.
