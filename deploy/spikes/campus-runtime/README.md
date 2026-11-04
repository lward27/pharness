# LUC-114: gVisor versus runc lab qualification harness

This harness compares the gVisor sandbox (`runtimeClassName: gvisor`, handler
`runsc`) with the default runc runtime for PHarness Campus lab pods. It runs
from an operator workstation with `kubectl` access and Python 3.10 or newer.
It uses only the standard library.

It is a spike harness. It is not deployed by Helm or Argo, and nothing in
`deploy/helm` references it.

## Prerequisites (created out of band, not by this harness)

- Node `ubuntu-lucas-engineering-2` has gVisor installed, the `runsc` handler
  is registered in containerd, and the node carries the label
  `pharness.dev/campus-lab-runtime=gvisor`.
- `RuntimeClass/gvisor` exists with handler `runsc` and
  `scheduling.nodeSelector: {pharness.dev/campus-lab-runtime: gvisor}`.
- `Namespace/campus-lab-spike` exists with
  `pod-security.kubernetes.io/enforce=restricted`.

## Running it

```sh
cd deploy/spikes/campus-runtime
python3 driver.py all --runs 3 --startup-runs 5 --out /tmp/luc114-results
python3 driver.py summary --out /tmp/luc114-results   # print the Markdown summary again
python3 driver.py cleanup                             # delete any leftover spike pods
```

A full run takes about 25 minutes. You can run a single experiment by name:
`inventory`, `refusal`, `negative`, `fs`, `network`, `startup`, `idle`,
`bench` or `labs`. You can also pass `all --only fs,network`. Use
`--context` to select a kubectl context.

The harness writes `results.json` (raw logs, events and timings) and
`summary.md` into the output directory. It does not write them into the
repository.

`python3 driver.py render` regenerates `manifests/`, one example pod per
experiment and runtime. The rendered files are for review and for
`kubectl create -f` by hand. `driver.py` remains the source of truth.

## Guarantees

- Pods are created only in `campus-lab-spike` and carry the label
  `pharness.dev/spike=luc-114`. The harness deletes every pod it creates,
  including on error or Ctrl-C (`try`/`finally` cleanup by label).
- Every pod passes Pod Security `restricted`. Each pod sets
  `runAsNonRoot`/`runAsUser: 65532`, `capabilities.drop: [ALL]`,
  `seccompProfile: RuntimeDefault` and `allowPrivilegeEscalation: false`.
  Each pod also sets `automountServiceAccountToken: false` and
  `enableServiceLinks: false`. Requests equal limits, including
  `ephemeral-storage`.
- Lab and check pods use `readOnlyRootFilesystem: true` with `emptyDir` for
  `/workspace` (also `$HOME`) and `/tmp`. The `fs` experiment also runs a
  writable-root variant to compare behaviour.
- runc pods are pinned to the same node as gVisor pods with
  `nodeSelector: kubernetes.io/hostname: ubuntu-lucas-engineering-2`.
- Runtimes alternate order between iterations so that drift does not favour
  one runtime. Startup runs discard one warm-up per image and runtime, so the
  results show warm-image-cache numbers.
- The negative checks never attempt an exploit. Each probe is a single
  syscall or CLI call that is expected to fail. Probes that would change
  state if they succeeded run in a forked child. Probes against sysfs and
  procfs only open the file and never write data.
- The harness uses no credentials and no private repositories. Lab sources are
  public GitHub repositories fetched at exact commit SHAs.

## Pinned identities

| Item | Value |
|---|---|
| Node | `ubuntu-lucas-engineering-2`, Ubuntu 24.04.5, kernel `6.8.0-147-generic`, k3s `v1.34.5+k3s1`, containerd `2.1.5-k3s1`, 16 vCPU / 32 GiB |
| gVisor | `release-20261026.0` (`runsc`), installed out of band |
| busybox | `docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662` |
| Python runner | `registry.lucas.engineering/pharness-python-runner@sha256:0dcdb9e74355d55c634e6adaf9643b4466557aa728130e752408d62baf2f9756` (Python 3.11.16, user 65532) |
| Node runner | `registry.lucas.engineering/pharness-node-runner@sha256:c478a31d341af1d1c7a13a99236120c870d0b88d1a9b80ac59159dd1a145a7bd` (Node 24.20.0, user 65532) |
| Python lab | `https://github.com/pallets/itsdangerous` at `096c8d42545d3b68ea21a4f890fb2b2d8979c0bd` (tag 2.2.0): venv, `pip install -r requirements/tests.txt -e .`, `pytest` |
| Node lab | `https://github.com/uuidjs/uuid` at `3d1eba06be81fb3a02e16d06ef6fe959c9bb5c5c` (tag v11.1.0): `npm ci`, `npm test` (TypeScript build plus `node --test`) |

The runner images' entrypoint (`pharness-worker`) is overridden with an
explicit `command`.

## Experiments

| Name | What it measures |
|---|---|
| `inventory` | Node info, RuntimeClass, namespace labels, kube-system pods and storage classes |
| `refusal` | gVisor with an unsatisfiable nodeSelector; a nonexistent RuntimeClass; gVisor with `spec.nodeName` set to a node without gVisor (scheduler bypass); a server-side dry run of a non-compliant pod against Pod Security |
| `negative` | `scripts/negative_checks.py` (Python ctypes syscalls plus CLI tools) and `scripts/negative-busybox.sh` |
| `fs` | `scripts/fs_checks.py` (read-only root, emptyDir, permissions, symlinks, locks, inotify, PTY, `/dev/shm`), plus `kubectl exec` with and without a TTY, stdin streaming and `kubectl cp` |
| `network` | `scripts/network_checks.py`: DNS, HTTPS to GitHub, PyPI and npm, an npm tarball download, and interface MTU |
| `startup` | Time from the pod POST to `Running` and to the first log line, for busybox, Python and Node images (20 ms polling through `kubectl proxy`) |
| `idle` | metrics-server memory and CPU for idle pods and for a pod that holds 256 MiB |
| `bench` | `scripts/bench.sh`: Python CPU loop, sha256 of 1 GiB, 1M `getppid`, 1k fork/exec, 20 Python start-ups, 10k file create/stat/delete, and `dd` 512 MiB write (fsync) and read |
| `labs` | `scripts/lab-python.sh` and `scripts/lab-node.sh`: per-phase wall time, test counts, metrics-server peak, and in-pod cgroup peak |

## Caveats

- metrics-server samples about every 15 seconds, so peaks for short pods are
  approximate and may be missing. runc pods also report the container's
  cgroup v2 `memory.peak`. gVisor pods report the Sentry's emulated cgroup
  v1 counters, which exclude the Sentry and Gofer host processes.
- Network-bound phases (`clone`, `pip install`, `npm ci`) include public
  internet variance.
- The node is shared with other workloads, including CoreDNS and
  local-path-provisioner, so expect some noise.
