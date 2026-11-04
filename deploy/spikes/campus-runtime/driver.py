#!/usr/bin/env python3
"""LUC-114: reproducible gVisor versus runc lab qualification harness.

Runs from an operator workstation that has kubectl access to the cluster. All
pods are created in the campus-lab-spike namespace, satisfy Pod Security
"restricted", carry the label pharness.dev/spike=luc-114 and are deleted before
the harness exits (also on error or Ctrl-C).

Only the Python standard library is used. API calls go through a local
`kubectl proxy` bound to 127.0.0.1 so that create/poll latency is not dominated
by kubectl process start-up.
"""
from __future__ import annotations

import argparse
import http.client
import json
import os
import re
import statistics
import subprocess
import sys
import threading
import time
import urllib.parse
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.join(HERE, "scripts")
MANIFESTS = os.path.join(HERE, "manifests")

NAMESPACE = "campus-lab-spike"
NODE = "ubuntu-lucas-engineering-2"
OTHER_NODE = "ubuntu-lucas-engineering"
BUILD_NODE = "ubuntu-lucas-engineering-build"
GVISOR_CLASS = "gvisor"
MISSING_CLASS = "gvisor-luc114-does-not-exist"
SPIKE_LABEL = "pharness.dev/spike"
SPIKE_VALUE = "luc-114"
RUNTIMES = ("gvisor", "runc")

IMAGES = {
    "busybox": "docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662",
    "python": "registry.lucas.engineering/pharness-python-runner@sha256:0dcdb9e74355d55c634e6adaf9643b4466557aa728130e752408d62baf2f9756",
    "node": "registry.lucas.engineering/pharness-node-runner@sha256:c478a31d341af1d1c7a13a99236120c870d0b88d1a9b80ac59159dd1a145a7bd",
}

LABS = {
    "python": {
        "image": "python",
        "script": "lab-python.sh",
        "repo": "https://github.com/pallets/itsdangerous",
        "sha": "096c8d42545d3b68ea21a4f890fb2b2d8979c0bd",
        "ref": "tag 2.2.0",
    },
    "node": {
        "image": "node",
        "script": "lab-node.sh",
        "repo": "https://github.com/uuidjs/uuid",
        "sha": "3d1eba06be81fb3a02e16d06ef6fe959c9bb5c5c",
        "ref": "tag v11.1.0",
    },
}

SIZES = {
    "small": {"cpu": "500m", "memory": "512Mi", "ephemeral-storage": "1Gi"},
    "medium": {"cpu": "1", "memory": "1Gi", "ephemeral-storage": "2Gi"},
    "lab": {"cpu": "2", "memory": "2Gi", "ephemeral-storage": "4Gi"},
}

PY_WRAPPER = (
    'mkdir -p "$HOME"; python3 -c "$LUC114_SCRIPT"; rc=$?; echo "@@EXIT rc=$rc"; '
    'if [ -n "${LUC114_HOLD:-}" ]; then sleep "$LUC114_HOLD"; fi; exit "$rc"'
)


def log(msg: str) -> None:
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {msg}", flush=True)


def read_script(name: str) -> str:
    with open(os.path.join(SCRIPTS, name)) as handle:
        return handle.read()


def shell_workload(name: str) -> str:
    return read_script("common.sh") + "\n" + read_script(name)


# --------------------------------------------------------------------------- #
# Kubernetes access
# --------------------------------------------------------------------------- #
class KubeError(RuntimeError):
    def __init__(self, status: int, body: bytes):
        super().__init__(f"HTTP {status}: {body[:500]!r}")
        self.status = status
        self.body = body


class Kube:
    def __init__(self, context: str | None = None):
        cmd = ["kubectl"] + (["--context", context] if context else []) + ["proxy", "--port=0", "--address=127.0.0.1"]
        self.context = context
        self.proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        line = self.proc.stdout.readline()
        match = re.search(r"127\.0\.0\.1:(\d+)", line)
        if not match:
            self.proc.kill()
            raise RuntimeError(f"kubectl proxy did not start: {line!r}")
        self.port = int(match.group(1))
        self.lock = threading.Lock()

    def close(self) -> None:
        self.proc.terminate()
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()

    def kubectl(self, *args: str, timeout: int = 60, stdin: str | None = None) -> subprocess.CompletedProcess:
        cmd = ["kubectl"] + (["--context", self.context] if self.context else []) + list(args)
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, input=stdin)

    def request(self, method: str, path: str, body=None, timeout: float = 30) -> tuple[int, bytes]:
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        headers = {}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        try:
            conn.request(method, path, body=data, headers=headers)
            resp = conn.getresponse()
            return resp.status, resp.read()
        finally:
            conn.close()

    def json(self, method: str, path: str, body=None, ok=(200, 201, 202)):
        status, raw = self.request(method, path, body)
        if status not in ok:
            raise KubeError(status, raw)
        return json.loads(raw)

    @staticmethod
    def pod_path(name: str = "") -> str:
        return f"/api/v1/namespaces/{NAMESPACE}/pods" + (f"/{name}" if name else "")

    def create_pod(self, manifest: dict, dry_run: bool = False):
        path = self.pod_path() + ("?dryRun=All" if dry_run else "")
        return self.request("POST", path, manifest)

    def get_pod(self, name: str):
        status, raw = self.request("GET", self.pod_path(name))
        return json.loads(raw) if status == 200 else None

    def logs(self, name: str) -> str:
        status, raw = self.request("GET", self.pod_path(name) + "/log", timeout=60)
        return raw.decode(errors="replace") if status == 200 else ""

    def events(self, name: str) -> list[dict]:
        sel = urllib.parse.quote(f"involvedObject.name={name}")
        status, raw = self.request("GET", f"/api/v1/namespaces/{NAMESPACE}/events?fieldSelector={sel}")
        if status != 200:
            return []
        out = []
        for ev in json.loads(raw).get("items", []):
            out.append({
                "type": ev.get("type"),
                "reason": ev.get("reason"),
                "message": ev.get("message"),
                "count": ev.get("count"),
                "source": (ev.get("source") or {}).get("component") or ev.get("reportingComponent"),
                "first": ev.get("firstTimestamp") or ev.get("eventTime"),
            })
        return out

    def metrics(self, name: str):
        status, raw = self.request("GET", f"/apis/metrics.k8s.io/v1beta1/namespaces/{NAMESPACE}/pods/{name}")
        if status != 200:
            return None
        doc = json.loads(raw)
        cpu = sum(parse_cpu(c["usage"]["cpu"]) for c in doc.get("containers", []))
        mem = sum(parse_mem(c["usage"]["memory"]) for c in doc.get("containers", []))
        return {"cpu_cores": cpu, "memory_mib": mem / 1048576, "window": doc.get("window"), "timestamp": doc.get("timestamp")}

    def delete_pod(self, name: str, wait: bool = True, timeout: float = 120) -> None:
        self.request("DELETE", self.pod_path(name) + "?gracePeriodSeconds=0")
        deadline = time.monotonic() + timeout
        while wait and time.monotonic() < deadline:
            status, _ = self.request("GET", self.pod_path(name))
            if status == 404:
                return
            time.sleep(0.5)

    def cleanup(self) -> list[str]:
        sel = urllib.parse.quote(f"{SPIKE_LABEL}={SPIKE_VALUE}")
        status, raw = self.request("GET", self.pod_path() + f"?labelSelector={sel}")
        names = [p["metadata"]["name"] for p in json.loads(raw).get("items", [])] if status == 200 else []
        for name in names:
            self.request("DELETE", self.pod_path(name) + "?gracePeriodSeconds=0")
        for name in names:
            self.delete_pod(name, wait=True)
        return names


def parse_cpu(value: str) -> float:
    units = {"n": 1e-9, "u": 1e-6, "m": 1e-3}
    if value and value[-1] in units:
        return float(value[:-1]) * units[value[-1]]
    return float(value)


def parse_mem(value: str) -> float:
    units = {"Ki": 1024, "Mi": 1024**2, "Gi": 1024**3, "k": 1e3, "M": 1e6, "G": 1e9}
    for suffix, mult in units.items():
        if value.endswith(suffix):
            return float(value[: -len(suffix)]) * mult
    return float(value)


class MetricsSampler(threading.Thread):
    """Samples metrics-server for one pod; resolution is the metrics-server window (about 10-15 s)."""

    def __init__(self, kube: Kube, name: str, interval: float = 5.0):
        super().__init__(daemon=True)
        self.kube, self.name, self.interval = kube, name, interval
        self.samples: list[dict] = []
        self.stop_event = threading.Event()

    def run(self) -> None:
        while not self.stop_event.is_set():
            try:
                sample = self.kube.metrics(self.name)
                if sample and (not self.samples or sample["timestamp"] != self.samples[-1]["timestamp"]):
                    self.samples.append(sample)
            except Exception:  # noqa: BLE001 - sampling is best-effort
                pass
            self.stop_event.wait(self.interval)

    def stop(self) -> dict:
        self.stop_event.set()
        self.join(10)
        if not self.samples:
            return {"samples": 0}
        return {
            "samples": len(self.samples),
            "peak_cpu_cores": round(max(s["cpu_cores"] for s in self.samples), 3),
            "peak_memory_mib": round(max(s["memory_mib"] for s in self.samples), 1),
        }


# --------------------------------------------------------------------------- #
# Manifests
# --------------------------------------------------------------------------- #
def pod_manifest(name: str, runtime: str, image: str, command: list[str], *, env: dict | None = None,
                 ro_root: bool = True, tmp_volume: bool = True, size: str = "small",
                 resources: dict | None = None, labels: dict | None = None) -> dict:
    res = dict(resources or SIZES[size])
    volumes = [{"name": "workspace", "emptyDir": {"sizeLimit": "3Gi"}}]
    mounts = [{"name": "workspace", "mountPath": "/workspace"}]
    if tmp_volume:
        volumes.append({"name": "tmp", "emptyDir": {"sizeLimit": "1Gi"}})
        mounts.append({"name": "tmp", "mountPath": "/tmp"})
    env_list = [{"name": "HOME", "value": "/workspace/home"}]
    env_list += [{"name": k, "value": v} for k, v in (env or {}).items()]
    spec = {
        "restartPolicy": "Never",
        "automountServiceAccountToken": False,
        "enableServiceLinks": False,
        "terminationGracePeriodSeconds": 2,
        "nodeSelector": {"kubernetes.io/hostname": NODE},
        "securityContext": {
            "runAsNonRoot": True,
            "runAsUser": 65532,
            "runAsGroup": 65532,
            "fsGroup": 65532,
            "seccompProfile": {"type": "RuntimeDefault"},
        },
        "containers": [{
            "name": "lab",
            "image": IMAGES[image],
            "imagePullPolicy": "IfNotPresent",
            "command": command,
            "workingDir": "/workspace",
            "env": env_list,
            "resources": {"requests": res, "limits": res},
            "securityContext": {
                "allowPrivilegeEscalation": False,
                "readOnlyRootFilesystem": ro_root,
                "runAsNonRoot": True,
                "capabilities": {"drop": ["ALL"]},
                "seccompProfile": {"type": "RuntimeDefault"},
            },
            "volumeMounts": mounts,
        }],
        "volumes": volumes,
    }
    if runtime == "gvisor":
        spec["runtimeClassName"] = GVISOR_CLASS
    return {
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "namespace": NAMESPACE,
            "labels": {SPIKE_LABEL: SPIKE_VALUE, "pharness.dev/runtime": runtime, **(labels or {})},
        },
        "spec": spec,
    }


def python_pod(name, runtime, script_name, *, hold=None, env=None, **kwargs):
    env = {"LUC114_SCRIPT": read_script(script_name), **(env or {})}
    if hold:
        env["LUC114_HOLD"] = str(hold)
    return pod_manifest(name, runtime, kwargs.pop("image", "python"), ["/bin/sh", "-c", PY_WRAPPER], env=env, **kwargs)


def startup_pod(name, runtime, image):
    return pod_manifest(name, runtime, image, ["/bin/sh", "-c", "echo luc114-first-line; exec sleep 600"],
                        labels={"pharness.dev/test": "startup"})


def lab_pod(name, runtime, lab):
    spec = LABS[lab]
    return pod_manifest(name, runtime, spec["image"], ["/bin/sh", "-c", shell_workload(spec["script"])],
                        env={"LAB_REPO": spec["repo"], "LAB_SHA": spec["sha"]}, size="lab",
                        labels={"pharness.dev/test": f"lab-{lab}"})


def bench_pod(name, runtime):
    return pod_manifest(name, runtime, "python", ["/bin/sh", "-c", shell_workload("bench.sh")], size="lab",
                        labels={"pharness.dev/test": "bench"})


def refusal_manifests(suffix: str) -> dict[str, dict]:
    sleeper = ["/bin/sh", "-c", "echo should-not-run; exec sleep 60"]
    other_node = pod_manifest(f"l114-refuse-gvisor-wrongnode-{suffix}", "gvisor", "busybox", sleeper,
                              labels={"pharness.dev/test": "refusal"})
    other_node["spec"]["nodeSelector"] = {"kubernetes.io/hostname": OTHER_NODE}
    missing = pod_manifest(f"l114-refuse-missing-class-{suffix}", "runc", "busybox", sleeper,
                           labels={"pharness.dev/test": "refusal"})
    missing["spec"]["runtimeClassName"] = MISSING_CLASS
    bypass = pod_manifest(f"l114-refuse-gvisor-nodename-{suffix}", "gvisor", "busybox", sleeper,
                          labels={"pharness.dev/test": "refusal"})
    del bypass["spec"]["nodeSelector"]
    bypass["spec"]["nodeName"] = BUILD_NODE
    noncompliant = pod_manifest(f"l114-psa-noncompliant-{suffix}", "gvisor", "busybox", sleeper,
                                labels={"pharness.dev/test": "psa"})
    del noncompliant["spec"]["securityContext"]
    noncompliant["spec"]["containers"][0]["securityContext"] = {"allowPrivilegeEscalation": True}
    return {"gvisor_wrong_node": other_node, "missing_runtimeclass": missing,
            "gvisor_nodename_bypass": bypass, "psa_noncompliant_dryrun": noncompliant}


# --------------------------------------------------------------------------- #
# Generic runners
# --------------------------------------------------------------------------- #
def run_to_completion(kube: Kube, manifest: dict, timeout: float = 900, sample_metrics: bool = False) -> dict:
    name = manifest["metadata"]["name"]
    t0 = time.monotonic()
    status, raw = kube.create_pod(manifest)
    if status not in (200, 201):
        return {"name": name, "create_error": f"HTTP {status}: {raw[:400]!r}"}
    sampler = MetricsSampler(kube, name) if sample_metrics else None
    if sampler:
        sampler.start()
    t_running = None
    pod = None
    try:
        while time.monotonic() - t0 < timeout:
            pod = kube.get_pod(name)
            phase = pod and pod["status"].get("phase")
            if phase == "Running" and t_running is None:
                t_running = time.monotonic()
            if phase in ("Succeeded", "Failed"):
                break
            time.sleep(0.5)
        t_end = time.monotonic()
        metrics = sampler.stop() if sampler else None
        phase = pod and pod["status"].get("phase")
        term = {}
        if pod:
            for cs in pod["status"].get("containerStatuses", []):
                term = (cs.get("state") or {}).get("terminated") or {}
        return {
            "name": name,
            "phase": phase,
            "exit_code": term.get("exitCode"),
            "reason": term.get("reason"),
            "wall_s": round(t_end - t0, 2),
            "to_running_s": round(t_running - t0, 2) if t_running else None,
            "metrics": metrics,
            "logs": kube.logs(name),
            "events": kube.events(name),
            "timed_out": phase not in ("Succeeded", "Failed"),
        }
    finally:
        if sampler and sampler.is_alive():
            sampler.stop()
        kube.delete_pod(name)


def run_and_hold(kube: Kube, manifest: dict, timeout: float = 300):
    """Create a pod and wait until its workload printed @@EXIT; the pod keeps sleeping."""
    name = manifest["metadata"]["name"]
    status, raw = kube.create_pod(manifest)
    if status not in (200, 201):
        raise KubeError(status, raw)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        logs = kube.logs(name)
        if "@@EXIT" in logs:
            return logs
        pod = kube.get_pod(name)
        if pod and pod["status"].get("phase") in ("Failed", "Succeeded"):
            return kube.logs(name)
        time.sleep(1)
    return kube.logs(name)


def parse_json_lines(logs: str, marker: str) -> list[dict]:
    out = []
    for line in logs.splitlines():
        if line.startswith(marker + " "):
            try:
                out.append(json.loads(line[len(marker) + 1:]))
            except json.JSONDecodeError:
                out.append({"name": "unparsed", "detail": line})
    return out


def parse_phases(logs: str) -> dict:
    phases = {}
    for m in re.finditer(r"^@@PHASE name=(\S+) rc=(\d+) start=([\d.]+) end=([\d.]+)", logs, re.M):
        phases[m.group(1)] = {"rc": int(m.group(2)), "secs": round(float(m.group(4)) - float(m.group(3)), 3)}
    return phases


def parse_cgroup(logs: str) -> dict:
    out = {}
    for m in re.finditer(r"^@@CGROUP (\S+) (.*)$", logs, re.M):
        out[m.group(1)] = m.group(2).strip()
    return out


def parse_test_counts(logs: str, lab: str) -> dict:
    test_lines = "\n".join(line for line in logs.splitlines() if line.startswith("[test] "))
    counts = {}
    if lab == "python":
        for key in ("passed", "failed", "error", "errors", "skipped", "xfailed"):
            m = re.search(rf"(\d+) {key}\b", test_lines)
            if m:
                counts[key] = int(m.group(1))
    else:
        for key in ("tests", "pass", "fail", "skipped", "todo", "cancelled"):
            m = re.search(rf"^\[test\] (?:#|\u2139) {key} (\d+)", test_lines, re.M)
            if m:
                counts[key] = int(m.group(1))
    return counts


def stats(values: list[float]) -> dict:
    values = [v for v in values if v is not None]
    if not values:
        return {"n": 0}
    return {"n": len(values), "median": round(statistics.median(values), 3),
            "min": round(min(values), 3), "max": round(max(values), 3)}


def runtime_order(i: int) -> tuple[str, ...]:
    """Alternate which runtime goes first so slow drift does not favour one side."""
    return RUNTIMES if i % 2 == 0 else tuple(reversed(RUNTIMES))


# --------------------------------------------------------------------------- #
# Experiments
# --------------------------------------------------------------------------- #
def exp_inventory(kube: Kube, args) -> dict:
    node = kube.json("GET", f"/api/v1/nodes/{NODE}")
    rc = kube.json("GET", f"/apis/node.k8s.io/v1/runtimeclasses/{GVISOR_CLASS}")
    ns = kube.json("GET", f"/api/v1/namespaces/{NAMESPACE}")
    ks = kube.json("GET", "/api/v1/namespaces/kube-system/pods")
    sc = kube.json("GET", "/apis/storage.k8s.io/v1/storageclasses")
    version = kube.json("GET", "/version")
    ann = node["metadata"].get("annotations", {})
    return {
        "server_version": version.get("gitVersion"),
        "node": NODE,
        "node_info": node["status"]["nodeInfo"],
        "node_labels": node["metadata"]["labels"],
        "node_capacity": node["status"]["capacity"],
        "node_allocatable": node["status"]["allocatable"],
        "flannel_annotations": {k: v for k, v in ann.items() if "flannel" in k},
        "runtimeclass": {"handler": rc["handler"], "scheduling": rc.get("scheduling"), "overhead": rc.get("overhead")},
        "namespace_labels": ns["metadata"].get("labels"),
        "kube_system_pods": sorted(p["metadata"]["name"] for p in ks["items"]),
        "storageclasses": [{"name": s["metadata"]["name"], "provisioner": s["provisioner"]} for s in sc["items"]],
        "images": IMAGES,
        "labs": LABS,
    }


def measure_startup(kube: Kube, manifest: dict, timeout: float = 180) -> dict:
    name = manifest["metadata"]["name"]
    log_path = kube.pod_path(name) + "/log"
    t0 = time.monotonic()
    status, raw = kube.create_pod(manifest)
    if status not in (200, 201):
        return {"name": name, "create_error": f"HTTP {status}: {raw[:300]!r}"}
    t_sched = t_running = t_log = None
    pod = None
    while time.monotonic() - t0 < timeout and (t_running is None or t_log is None):
        pod = kube.get_pod(name)
        now = time.monotonic()
        if pod:
            if t_sched is None and pod["spec"].get("nodeName"):
                t_sched = now
            phase = pod["status"].get("phase")
            if t_running is None and phase == "Running":
                t_running = now
            if phase in ("Failed", "Succeeded"):
                break
        if t_log is None:
            lstatus, lraw = kube.request("GET", log_path)
            if lstatus == 200 and lraw.strip():
                t_log = time.monotonic()
        time.sleep(0.02)
    started_at = None
    if pod:
        for cs in pod["status"].get("containerStatuses", []):
            started_at = ((cs.get("state") or {}).get("running") or {}).get("startedAt")
    result = {
        "name": name,
        "scheduled_ms": round((t_sched - t0) * 1000) if t_sched else None,
        "running_ms": round((t_running - t0) * 1000) if t_running else None,
        "first_log_ms": round((t_log - t0) * 1000) if t_log else None,
        "created": pod and pod["metadata"].get("creationTimestamp"),
        "started_at": started_at,
        "events": kube.events(name),
    }
    kube.delete_pod(name)
    return result


def exp_startup(kube: Kube, args) -> dict:
    results = {}
    for image in ("busybox", "python", "node"):
        for i in range(args.warmup + args.startup_runs):
            for rt in runtime_order(i):
                name = f"l114-start-{image}-{rt}-{args.suffix}-{i}"
                res = measure_startup(kube, startup_pod(name, rt, image))
                res["warmup"] = i < args.warmup
                results.setdefault(f"{image}/{rt}", []).append(res)
                log(f"startup {image}/{rt} #{i}: running={res.get('running_ms')}ms first_log={res.get('first_log_ms')}ms"
                    + (" (warm-up)" if res["warmup"] else ""))
    return results


def exp_idle(kube: Kube, args) -> dict:
    """Idle and fixed-allocation memory as seen by metrics-server (host cgroup view)."""
    variants = {
        "busybox-idle": ("busybox", ["/bin/sh", "-c", "exec sleep 600"], SIZES["small"]),
        "python-idle": ("python", ["/bin/sh", "-c", "exec sleep 600"], SIZES["small"]),
        "node-idle": ("node", ["/bin/sh", "-c", "exec sleep 600"], SIZES["small"]),
        "python-alloc256": ("python", ["python3", "-c",
                                       "import time\nx = b'\\x01' * (256 * 1024 * 1024)\nprint('allocated', flush=True)\ntime.sleep(600)"],
                            SIZES["medium"]),
    }
    names = {}
    try:
        for key, (image, cmd, res) in variants.items():
            for rt in RUNTIMES:
                name = f"l114-idle-{key}-{rt}-{args.suffix}"
                status, raw = kube.create_pod(pod_manifest(name, rt, image, cmd, resources=res,
                                                           labels={"pharness.dev/test": "idle"}))
                if status not in (200, 201):
                    raise KubeError(status, raw)
                names[(key, rt)] = name
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            phases = [((kube.get_pod(n) or {}).get("status") or {}).get("phase") for n in names.values()]
            if all(p == "Running" for p in phases):
                break
            time.sleep(2)
        samples: dict = {k: [] for k in names}
        log("idle pods running; sampling metrics-server for ~120 s")
        end = time.monotonic() + 120
        while time.monotonic() < end:
            for key, name in names.items():
                m = kube.metrics(name)
                if m and (not samples[key] or samples[key][-1]["timestamp"] != m["timestamp"]):
                    samples[key].append(m)
            time.sleep(5)
        out = {}
        for (key, rt), vals in samples.items():
            out[f"{key}/{rt}"] = {
                "samples": len(vals),
                "memory_mib": stats([v["memory_mib"] for v in vals]),
                "cpu_millicores": stats([v["cpu_cores"] * 1000 for v in vals]),
            }
        return out
    finally:
        for name in names.values():
            kube.delete_pod(name, wait=False)
        for name in names.values():
            kube.delete_pod(name)


def exp_labs(kube: Kube, args) -> dict:
    results = {}
    labs = args.labs.split(",")
    for i in range(args.runs):
        for lab in labs:
            for rt in runtime_order(i):
                name = f"l114-lab-{lab}-{rt}-{args.suffix}-{i}"
                log(f"lab {lab}/{rt} run {i} starting")
                res = run_to_completion(kube, lab_pod(name, rt, lab), timeout=args.lab_timeout, sample_metrics=True)
                logs = res.get("logs", "")
                res.update({
                    "phases": parse_phases(logs),
                    "tests": parse_test_counts(logs, lab),
                    "cgroup": parse_cgroup(logs),
                    "revision": (re.search(r"^@@REVISION (\S+)", logs, re.M) or [None, None])[1],
                    "kernel": (re.search(r"kernel=(\S+)", logs) or [None, None])[1],
                })
                log(f"lab {lab}/{rt} run {i}: phase={res.get('phase')} exit={res.get('exit_code')} wall={res.get('wall_s')}s "
                    f"phases={ {k: v['secs'] for k, v in res['phases'].items()} } tests={res['tests']} metrics={res.get('metrics')}")
                results.setdefault(f"{lab}/{rt}", []).append(res)
    return results


def exp_bench(kube: Kube, args) -> dict:
    results = {}
    for i in range(args.runs):
        for rt in runtime_order(i):
            name = f"l114-bench-{rt}-{args.suffix}-{i}"
            log(f"bench {rt} run {i} starting")
            res = run_to_completion(kube, bench_pod(name, rt), timeout=900, sample_metrics=True)
            res["phases"] = parse_phases(res.get("logs", ""))
            res["cgroup"] = parse_cgroup(res.get("logs", ""))
            log(f"bench {rt} run {i}: { {k: v['secs'] for k, v in res['phases'].items()} } metrics={res.get('metrics')}")
            results.setdefault(rt, []).append(res)
    return results


def exec_with_local_pty(kube: Kube, name: str) -> str:
    """Run `kubectl exec -it` behind a local 30x100 pseudo-terminal, as a web terminal would."""
    import fcntl
    import pty
    import struct
    import termios

    remote = ("tty; if [ -t 0 ]; then echo isatty=yes; else echo isatty=no; fi; stty size; "
              "readlink /proc/self/fd/0; echo tty-done")
    argv = ["kubectl"] + (["--context", kube.context] if kube.context else []) + [
        "exec", "-it", "-n", NAMESPACE, name, "--", "sh", "-c", remote]
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(argv[0], argv)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    out = b""
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        try:
            chunk = os.read(fd, 4096)
        except OSError:
            break
        if not chunk:
            break
        out += chunk
    _, status = os.waitpid(pid, 0)
    os.close(fd)
    text = out.decode(errors="replace").replace("\x00", "").replace("\r", "").strip()
    return f"exit={os.waitstatus_to_exitcode(status)} " + " | ".join(text.splitlines())


def exec_checks(kube: Kube, name: str) -> list[dict]:
    out = []

    def record(check, proc_or_exc, expect=None):
        if isinstance(proc_or_exc, Exception):
            out.append({"name": check, "status": "fail", "detail": str(proc_or_exc)})
            return
        text = (proc_or_exc.stdout + proc_or_exc.stderr).strip().replace("\r", "")
        ok = proc_or_exc.returncode == 0 and (expect is None or expect in text)
        out.append({"name": check, "status": "ok" if ok else "fail", "detail": f"rc={proc_or_exc.returncode} {text[:400]}"})

    ns = ["-n", NAMESPACE]
    try:
        record("kubectl_exec", kube.kubectl("exec", *ns, name, "--", "sh", "-c", "id; echo exec-ok"), "exec-ok")
    except Exception as exc:  # noqa: BLE001
        record("kubectl_exec", exc)
    try:
        record("kubectl_exec_stdin", kube.kubectl("exec", "-i", *ns, name, "--", "cat", stdin="stdin-roundtrip\n"),
               "stdin-roundtrip")
    except Exception as exc:  # noqa: BLE001
        record("kubectl_exec_stdin", exc)
    try:
        text = exec_with_local_pty(kube, name)
        ok = "isatty=yes" in text and "30 100" in text and "tty-done" in text
        out.append({"name": "kubectl_exec_tty", "status": "ok" if ok else "fail", "detail": text})
        out.append({"name": "kubectl_exec_tty_ttyname", "status": "info",
                    "detail": "ttyname resolves to /dev/pts" if "/dev/pts/" in text.split("|")[0] else
                    "ttyname fails inside the container (stdin is not a /dev/pts node)"})
    except Exception as exc:  # noqa: BLE001
        record("kubectl_exec_tty", exc)
    try:
        record("kubectl_exec_write_then_read",
               kube.kubectl("exec", *ns, name, "--", "sh", "-c",
                            "echo via-exec > /workspace/exec.txt && cat /workspace/exec.txt"), "via-exec")
    except Exception as exc:  # noqa: BLE001
        record("kubectl_exec_write_then_read", exc)
    try:
        record("kubectl_exec_root_write_denied",
               kube.kubectl("exec", *ns, name, "--", "sh", "-c",
                            "touch /var/tmp/x 2>&1 && echo WRITABLE || echo denied-as-expected"), "denied-as-expected")
    except Exception as exc:  # noqa: BLE001
        record("kubectl_exec_root_write_denied", exc)
    try:
        dest = f"/tmp/luc114-cp-{name}.txt"
        proc = kube.kubectl("cp", f"{NAMESPACE}/{name}:/workspace/exec.txt", dest)
        content = open(dest).read().strip() if os.path.exists(dest) else ""
        if os.path.exists(dest):
            os.remove(dest)
        out.append({"name": "kubectl_cp_from_pod", "status": "ok" if content == "via-exec" else "fail",
                    "detail": f"rc={proc.returncode} {(proc.stdout + proc.stderr).strip()[:200]} content={content!r}"})
    except Exception as exc:  # noqa: BLE001
        record("kubectl_cp_from_pod", exc)
    return out


def exp_fs(kube: Kube, args) -> dict:
    results = {}
    for rt in RUNTIMES:
        for variant, ro in (("ro-root", True), ("rw-root", False)):
            name = f"l114-fs-{variant}-{rt}-{args.suffix}"
            manifest = python_pod(name, rt, "fs_checks.py", hold=600, env={"LUC114_EXPECT_RO": "1" if ro else "0"},
                                  ro_root=ro, tmp_volume=ro, size="medium", labels={"pharness.dev/test": "fs"})
            try:
                logs = run_and_hold(kube, manifest)
                checks = parse_json_lines(logs, "@@CHECK")
                if ro:
                    checks += exec_checks(kube, name)
                results[f"{variant}/{rt}"] = {"checks": checks, "logs_tail": logs[-3000:]}
                fails = [c["name"] for c in checks if c.get("status") == "fail"]
                log(f"fs {variant}/{rt}: {len(checks)} checks, failures={fails}")
            finally:
                kube.delete_pod(name)
    return results


def exp_negative(kube: Kube, args) -> dict:
    results = {}
    for rt in RUNTIMES:
        name = f"l114-neg-python-{rt}-{args.suffix}"
        res = run_to_completion(kube, python_pod(name, rt, "negative_checks.py", size="small",
                                                 labels={"pharness.dev/test": "negative"}), timeout=300)
        results[f"python/{rt}"] = {"checks": parse_json_lines(res.get("logs", ""), "@@NEG"),
                                   "phase": res.get("phase"), "exit_code": res.get("exit_code")}
        name = f"l114-neg-busybox-{rt}-{args.suffix}"
        res = run_to_completion(kube, pod_manifest(name, rt, "busybox",
                                                   ["/bin/sh", "-c", read_script("negative-busybox.sh")],
                                                   labels={"pharness.dev/test": "negative"}), timeout=300)
        checks = []
        for line in res.get("logs", "").splitlines():
            if line.startswith("@@NEGTXT "):
                key, _, detail = line[len("@@NEGTXT "):].partition(" :: ")
                checks.append({"name": key, "outcome": "cli", "detail": detail.strip()})
        results[f"busybox/{rt}"] = {"checks": checks, "phase": res.get("phase")}
        log(f"negative {rt}: python={len(results[f'python/{rt}']['checks'])} busybox={len(checks)} checks")
    return results


def exp_network(kube: Kube, args) -> dict:
    results = {}
    for i in range(args.runs):
        for rt in runtime_order(i):
            name = f"l114-net-{rt}-{args.suffix}-{i}"
            res = run_to_completion(kube, python_pod(name, rt, "network_checks.py", size="small",
                                                     labels={"pharness.dev/test": "network"}), timeout=300)
            checks = parse_json_lines(res.get("logs", ""), "@@NET")
            results.setdefault(rt, []).append({"checks": checks, "phase": res.get("phase")})
            log(f"network {rt} #{i}: " + ", ".join(f"{c['name']}={c['status']}/{c.get('ms')}" for c in checks if c["status"] != "info"))
    return results


def exp_refusal(kube: Kube, args) -> dict:
    results = {}
    manifests = refusal_manifests(args.suffix)
    for key in ("gvisor_wrong_node", "missing_runtimeclass", "gvisor_nodename_bypass"):
        manifest = manifests[key]
        name = manifest["metadata"]["name"]
        status, raw = kube.create_pod(manifest)
        entry = {"requested": {k: manifest["spec"].get(k) for k in ("runtimeClassName", "nodeSelector", "nodeName")},
                 "create_http_status": status}
        if status not in (200, 201):
            entry["create_error"] = json.loads(raw).get("message", raw.decode(errors="replace")) if raw else ""
            results[key] = entry
            log(f"refusal {key}: rejected at admission HTTP {status}: {entry['create_error']}")
            continue
        try:
            observations = []
            end = time.monotonic() + args.refusal_wait
            pod = None
            while time.monotonic() < end:
                pod = kube.get_pod(name)
                if pod:
                    st = pod["status"]
                    observations.append(st.get("phase"))
                time.sleep(2)
            pod = kube.get_pod(name) or {}
            st = pod.get("status", {})
            entry.update({
                "phases_seen": sorted(set(p for p in observations if p)),
                "final_phase": st.get("phase"),
                "reason": st.get("reason"),
                "message": st.get("message"),
                "conditions": [{k: c.get(k) for k in ("type", "status", "reason", "message")} for c in st.get("conditions", [])],
                "container_states": [cs.get("state") for cs in st.get("containerStatuses", [])],
                "effective_spec": {k: pod.get("spec", {}).get(k) for k in ("runtimeClassName", "nodeSelector", "nodeName")},
                "logs": kube.logs(name)[:500],
                "events": kube.events(name),
            })
            log(f"refusal {key}: phases={entry['phases_seen']} reason={entry['reason']} events={[e['reason'] for e in entry['events']]}")
        finally:
            kube.delete_pod(name)
        results[key] = entry
    status, raw = kube.create_pod(manifests["psa_noncompliant_dryrun"], dry_run=True)
    results["psa_noncompliant_dryrun"] = {"create_http_status": status,
                                          "message": (json.loads(raw).get("message") if raw else "")}
    log(f"refusal psa dry-run: HTTP {status}")
    return results


EXPERIMENTS = {
    "inventory": exp_inventory,
    "refusal": exp_refusal,
    "negative": exp_negative,
    "fs": exp_fs,
    "network": exp_network,
    "startup": exp_startup,
    "idle": exp_idle,
    "bench": exp_bench,
    "labs": exp_labs,
}


# --------------------------------------------------------------------------- #
# Rendering and summary
# --------------------------------------------------------------------------- #
def to_yaml(value, indent: int = 0) -> str:
    pad = " " * indent
    if isinstance(value, dict):
        if not value:
            return "{}"
        lines = []
        for key, item in value.items():
            if isinstance(item, (dict, list)) and item:
                lines.append(f"{pad}{key}:\n{to_yaml(item, indent + 2)}")
            else:
                lines.append(f"{pad}{key}: {scalar(item, indent + 2)}")
        return "\n".join(lines)
    if isinstance(value, list):
        if not value:
            return "[]"
        lines = []
        for item in value:
            if isinstance(item, (dict, list)) and item:
                body = to_yaml(item, indent + 2)
                lines.append(f"{pad}- {body[indent + 2:]}")
            else:
                lines.append(f"{pad}- {scalar(item, indent + 2)}")
        return "\n".join(lines)
    return pad + scalar(value, indent)


def scalar(value, indent: int) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return "null"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, (dict, list)):
        return "{}" if isinstance(value, dict) else "[]"
    if "\n" in value and not value.startswith(" "):
        pad = " " * indent
        body = "\n".join((pad + line) if line else "" for line in value.rstrip("\n").split("\n"))
        return ("|\n" if value.endswith("\n") else "|-\n") + body
    return json.dumps(value)


def cmd_render(args) -> None:
    os.makedirs(MANIFESTS, exist_ok=True)
    docs = {}
    for rt in RUNTIMES:
        docs[f"startup-busybox-{rt}"] = startup_pod(f"l114-start-busybox-{rt}-example", rt, "busybox")
        docs[f"lab-python-{rt}"] = lab_pod(f"l114-lab-python-{rt}-example", rt, "python")
        docs[f"lab-node-{rt}"] = lab_pod(f"l114-lab-node-{rt}-example", rt, "node")
        docs[f"bench-{rt}"] = bench_pod(f"l114-bench-{rt}-example", rt)
        docs[f"fs-ro-root-{rt}"] = python_pod(f"l114-fs-ro-root-{rt}-example", rt, "fs_checks.py", hold=600,
                                              env={"LUC114_EXPECT_RO": "1"}, size="medium")
        docs[f"negative-python-{rt}"] = python_pod(f"l114-neg-python-{rt}-example", rt, "negative_checks.py")
        docs[f"network-{rt}"] = python_pod(f"l114-net-{rt}-example", rt, "network_checks.py")
    for key, manifest in refusal_manifests("example").items():
        docs[f"refusal-{key.replace('_', '-')}"] = manifest
    for key, doc in docs.items():
        path = os.path.join(MANIFESTS, f"{key}.yaml")
        with open(path, "w") as handle:
            handle.write("# Generated by driver.py render; driver.py is the source of truth.\n")
            handle.write(to_yaml(doc) + "\n")
    print(f"wrote {len(docs)} manifests to {MANIFESTS}")


def fmt(s: dict, unit: str = "") -> str:
    if not s or not s.get("n"):
        return "n/a"
    return f"{s['median']}{unit} ({s['min']}-{s['max']}, n={s['n']})"


def summarize(results: dict) -> str:
    out = ["# LUC-114 gVisor versus runc summary", ""]
    inv = results.get("inventory")
    if inv:
        ni = inv["node_info"]
        out += [f"Node {inv['node']}: kubelet {ni['kubeletVersion']}, {ni['containerRuntimeVersion']}, "
                f"kernel {ni['kernelVersion']}, {ni['osImage']}", ""]
    if "startup" in results:
        out += ["## Pod startup (ms from POST)", "", "| image/runtime | to Running | to first log line |", "|---|---|---|"]
        for key, runs in results["startup"].items():
            measured = [r for r in runs if not r.get("warmup")]
            out.append(f"| {key} | {fmt(stats([r.get('running_ms') for r in measured]))} | "
                       f"{fmt(stats([r.get('first_log_ms') for r in measured]))} |")
        out.append("")
    if "labs" in results:
        out += ["## Labs (seconds)", ""]
        for key, runs in results["labs"].items():
            phase_names = []
            for r in runs:
                for p in r.get("phases", {}):
                    if p not in phase_names:
                        phase_names.append(p)
            out.append(f"### {key}")
            out.append("")
            out.append("| metric | median (min-max, n) |")
            out.append("|---|---|")
            for p in phase_names:
                out.append(f"| {p} | {fmt(stats([r['phases'].get(p, {}).get('secs') for r in runs]))} |")
            out.append(f"| pod wall (POST to Succeeded) | {fmt(stats([r.get('wall_s') for r in runs]))} |")
            out.append(f"| metrics-server peak memory MiB | {fmt(stats([(r.get('metrics') or {}).get('peak_memory_mib') for r in runs]))} |")
            out.append(f"| metrics-server peak CPU cores | {fmt(stats([(r.get('metrics') or {}).get('peak_cpu_cores') for r in runs]))} |")
            peaks = []
            for r in runs:
                cg = r.get("cgroup", {})
                raw = cg.get("memory.peak", "")
                if not raw.isdigit():
                    raw = cg.get("memory/memory.max_usage_in_bytes", "")
                peaks.append(int(raw) / 1048576 if raw.isdigit() else None)
            out.append(f"| in-pod cgroup peak memory MiB (v2 memory.peak or gVisor v1 max_usage) | {fmt(stats(peaks))} |")
            out.append(f"| test counts per run | {[r.get('tests') for r in runs]} |")
            out.append(f"| exit codes | {[r.get('exit_code') for r in runs]} |")
            out.append("")
    if "bench" in results:
        out += ["## Micro-benchmarks (seconds)", "", "| benchmark | " + " | ".join(results["bench"]) + " |",
                "|---|" + "---|" * len(results["bench"])]
        names = []
        for runs in results["bench"].values():
            for r in runs:
                for p in r.get("phases", {}):
                    if p not in names:
                        names.append(p)
        for p in names:
            cells = [fmt(stats([r["phases"].get(p, {}).get("secs") for r in runs])) for runs in results["bench"].values()]
            out.append(f"| {p} | " + " | ".join(cells) + " |")
        out.append("")
    if "idle" in results:
        out += ["## Steady-state footprint (metrics-server)", "", "| pod/runtime | memory MiB | CPU millicores |", "|---|---|---|"]
        for key, val in results["idle"].items():
            out.append(f"| {key} | {fmt(val['memory_mib'])} | {fmt(val['cpu_millicores'])} |")
        out.append("")
    if "network" in results:
        out += ["## Network (ms)", ""]
        names = []
        for runs in results["network"].values():
            for r in runs:
                for c in r["checks"]:
                    if c["status"] != "info" and c["name"] not in names:
                        names.append(c["name"])
        out += ["| check | " + " | ".join(results["network"]) + " |", "|---|" + "---|" * len(results["network"])]
        for n in names:
            cells = []
            for runs in results["network"].values():
                vals = [c for r in runs for c in r["checks"] if c["name"] == n]
                ok = sum(1 for c in vals if c["status"] == "ok")
                cells.append(f"{ok}/{len(vals)} ok, {fmt(stats([c.get('ms') for c in vals]))}")
            out.append(f"| {n} | " + " | ".join(cells) + " |")
        out.append("")
    for section, marker in (("fs", "status"), ("negative", "outcome")):
        if section not in results:
            continue
        out += [f"## {section}", ""]
        for key, val in results[section].items():
            out.append(f"### {key}")
            for c in val["checks"]:
                out.append(f"- {c['name']}: {c.get(marker)} - {c.get('detail')}")
            out.append("")
    if "refusal" in results:
        out += ["## Refusal behaviour", "", "```", json.dumps(results["refusal"], indent=2), "```", ""]
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["all", "render", "summary", "cleanup", *EXPERIMENTS])
    parser.add_argument("--context", help="kubectl context (default: current)")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--startup-runs", type=int, default=5, help="measured startup runs per image/runtime")
    parser.add_argument("--warmup", type=int, default=1, help="discarded startup runs per image/runtime")
    parser.add_argument("--labs", default="python,node")
    parser.add_argument("--lab-timeout", type=int, default=900)
    parser.add_argument("--refusal-wait", type=int, default=45)
    parser.add_argument("--out", default=None, help="results directory (default /tmp/luc114-results-<utc>)")
    parser.add_argument("--only", default=None, help="comma-separated experiments for 'all'")
    args = parser.parse_args()
    args.suffix = format(int(time.time()) % 46656, "x")

    if args.command == "render":
        cmd_render(args)
        return 0
    out_dir = args.out or f"/tmp/luc114-results-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
    os.makedirs(out_dir, exist_ok=True)
    results_path = os.path.join(out_dir, "results.json")
    results = json.load(open(results_path)) if os.path.exists(results_path) else {}
    if args.command == "summary":
        print(summarize(results))
        return 0

    kube = Kube(args.context)
    try:
        if args.command == "cleanup":
            print("deleted:", kube.cleanup())
            return 0
        selected = list(EXPERIMENTS) if args.command == "all" else [args.command]
        if args.command == "all" and args.only:
            selected = [e for e in EXPERIMENTS if e in args.only.split(",")]
        for exp in selected:
            log(f"=== {exp} ===")
            started = time.monotonic()
            results[exp] = EXPERIMENTS[exp](kube, args)
            results.setdefault("_meta", {})[exp] = {
                "finished": datetime.now(timezone.utc).isoformat(), "duration_s": round(time.monotonic() - started, 1),
                "runs": args.runs}
            with open(results_path, "w") as handle:
                json.dump(results, handle, indent=2)
        with open(os.path.join(out_dir, "summary.md"), "w") as handle:
            handle.write(summarize(results))
        log(f"results: {results_path}")
        return 0
    finally:
        leftovers = kube.cleanup()
        if leftovers:
            log(f"cleanup deleted leftover pods: {leftovers}")
        kube.close()


if __name__ == "__main__":
    sys.exit(main())
