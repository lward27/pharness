#!/usr/bin/env python3
"""Campus image approval evidence, admission allowlist and lab Pod controls.

Workstation and CI commands: ``release-check``, ``render-taskrun``,
``render-canary-taskrun``, ``taskrun-outcome``, ``collect``, ``record``,
``render-policy``, ``check``, ``verify-online``, ``render-lab-pod`` and
``check-lab-pod``. The vulnerability policy itself lives in
``campus_image_policy.py``, which the approval TaskRun embeds as its step
script.

``deploy/campus/approved-images.json`` is the reviewed evidence record. Only
entries with status ``approved`` become admission allowlist digests.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from campus_image_policy import (  # noqa: E402
    DIGEST_RE,
    GateError,
    IMAGE_PATTERN,
    K8S_NAME_RE,
    REVISION_RE,
    SCHEMA_VERSION,
    blocking_findings,
    canonical_sha256,
    exception_key,
    file_sha256,
    load_json,
    parse_date,
    parse_image,
    public_key_sha256,
    reapply,
    today_utc,
    validate_exceptions,
    write_json,
)

POLICY_SOURCE = Path(__file__).resolve().with_name("campus_image_policy.py")

PUBLIC_REGISTRY = "registry.lucas.engineering"
PUSH_REGISTRY = "registry-write-gateway.registry.svc.cluster.local:8443"
SIGNING_SECRET = "campus-cosign-signing"
SIGNING_NAMESPACE = "tekton-pipelines"
SERVICE_ACCOUNT = "tekton-ci-build"
VULN_PREDICATE_TYPE = "https://pharness.dev/attestations/campus-vulnerability-disposition/v1"
SBOM_PREDICATE_TYPE = "https://spdx.dev/Document"
POLICY_NAME = "pharness-campus-lab-image-allowlist"
LAB_NAMESPACE_LABEL = "pharness.dev/campus-lab"
ANNOTATION_PREFIX = "dev.pharness.campus"

STATUSES = ("approved", "blocked", "revoked")

# Pinned tool images for the approval TaskRun. Versions match
# scripts/campus-install-supply-chain-tools.sh so workstation verification and
# in-cluster assessment use the same releases.
TOOL_IMAGES = {
    "syft": "docker.io/anchore/syft:v1.56.0@sha256:697ae864af1299cfa43c54b161cea3dd65c6d54de82e3dba66ea3dccb3acc313",
    "grype": "docker.io/anchore/grype:v0.121.1@sha256:6b06708b036f74b2cd70e94b706b4586af96d1932e6ac46255940b2e39dd44a9",
    "cosign": "ghcr.io/sigstore/cosign/cosign:v3.1.4@sha256:0f06154836d2de34f887a5c273506d52ca43edaa1d9e53e6510fa78a48f69c2d",
    "python": "docker.io/library/python:3.11-slim@sha256:a630a63cdb314e2d138a2fca3e375e319e8568346ffafac5b980f888630ac4f1",
}
# Tekton expands dollar-parenthesis references inside step scripts and env
# values; the gate source is embedded verbatim, so it must never contain one.
TEKTON_REFERENCE = "$" + "("

# Tekton passes every step script through one init container argument, which
# Linux caps at 128 KiB, so only the evaluate step embeds the full module.
ENFORCE_SCRIPT = """#!/usr/bin/env python3
import json
import sys

with open("/work/vulnerability-disposition.json", encoding="utf-8") as handle:
    predicate = json.load(handle)
if predicate.get("decision") != "approved":
    blocking = predicate.get("summary", {}).get("blocking")
    print(f"blocked: {blocking} Critical/High findings have a fix and no valid exception", file=sys.stderr)
    sys.exit(3)
print("approved: no blocking findings")
"""
COSIGN_LEGACY_FLAGS = [
    "--tlog-upload=false",
    "--new-bundle-format=false",
    "--use-signing-config=false",
]


# ---------------------------------------------------------------------------
# Release record


def release_check(build: dict, artifacts: dict, image: str, revision: str) -> dict:
    """Prove that an image digest is a component of one immutable release."""
    repository, digest = parse_image(image)
    if not REVISION_RE.match(revision or ""):
        raise GateError("source revision must be a full lowercase 40-character Git SHA")
    if not repository.startswith(PUBLIC_REGISTRY + "/"):
        raise GateError(f"Campus lab images must come from {PUBLIC_REGISTRY}")
    if build.get("source_revision") != revision:
        raise GateError("BUILD.json was produced for a different source revision")
    if build.get("status") != "builds_completed":
        raise GateError("BUILD.json does not record completed builds")
    if artifacts.get("source_revision") != revision:
        raise GateError("ARTIFACTS.json was produced for a different source revision")
    if artifacts.get("status") != "verified_artifacts":
        raise GateError("ARTIFACTS.json does not record verified artifacts")

    components = [
        component
        for component in build.get("components", [])
        if component.get("immutable_ref") == image
    ]
    if len(components) != 1:
        raise GateError(f"{image} is not exactly one component of the release record for {revision}")
    component = components[0]
    if component.get("source_revision") != revision or component.get("digest") != digest:
        raise GateError("the BUILD.json component does not bind this digest to the requested revision")
    if component.get("build_target"):
        raise GateError("only final runtime image targets can be approved for Campus labs")

    name = component.get("component")
    artifact = (artifacts.get("images") or {}).get(name)
    if not isinstance(artifact, dict):
        raise GateError(f"ARTIFACTS.json has no verified image for component {name}")
    if artifact.get("digest") != digest:
        raise GateError("ARTIFACTS.json records a different digest for this component")
    if PUBLIC_REGISTRY + "/" + str(artifact.get("repository")) != repository:
        raise GateError("ARTIFACTS.json records a different repository for this component")
    if artifact.get("source_verified") is not True:
        raise GateError("ARTIFACTS.json does not mark this image as source-verified")
    if artifact.get("platform") != "linux/amd64":
        raise GateError("Campus lab images must be linux/amd64")

    return {
        "component": name,
        "repository": repository,
        "digest": digest,
        "source_revision": revision,
        "pipeline_run": component.get("pipeline_run"),
        "build_executor": component.get("build_executor"),
        "config_digest": artifact.get("config_digest"),
        "platform": artifact.get("platform"),
        "artifacts_observed_at": artifacts.get("observed_at"),
    }


# ---------------------------------------------------------------------------
# Approval TaskRun


def render_taskrun(name: str, image: str, revision: str, component: str, exceptions_text: str, script_text: str) -> dict:
    repository, digest = parse_image(image)
    if not K8S_NAME_RE.match(name):
        raise GateError("TaskRun name must be a DNS label")
    if not repository.startswith(PUBLIC_REGISTRY + "/"):
        raise GateError(f"Campus lab images must come from {PUBLIC_REGISTRY}")
    if TEKTON_REFERENCE in script_text or TEKTON_REFERENCE in exceptions_text:
        raise GateError("Tekton would treat a dollar-parenthesis sequence in the gate script or exceptions as a variable")
    validate_exceptions(json.loads(exceptions_text))
    path = repository[len(PUBLIC_REGISTRY) + 1 :]
    push_reference = f"{PUSH_REGISTRY}/{path}@{digest}"

    nonroot = {
        "runAsNonRoot": True,
        "runAsUser": 65532,
        "runAsGroup": 65532,
        "allowPrivilegeEscalation": False,
        "readOnlyRootFilesystem": True,
        "capabilities": {"drop": ["ALL"]},
        "seccompProfile": {"type": "RuntimeDefault"},
    }
    work_mounts = [
        {"name": "work", "mountPath": "/work"},
        {"name": "tmp", "mountPath": "/tmp"},
    ]
    cosign_mounts = [
        {"name": "work", "mountPath": "/work", "readOnly": True},
        {"name": "tmp", "mountPath": "/tmp"},
        {"name": "signing-key", "mountPath": "/var/run/campus-signing", "readOnly": True},
        {"name": "registry-auth", "mountPath": "/var/run/registry-auth", "readOnly": True},
        {"name": "registry-ca", "mountPath": "/var/run/registry-ca", "readOnly": True},
    ]
    cosign_env = [
        {"name": "HOME", "value": "/tmp"},
        {"name": "DOCKER_CONFIG", "value": "/var/run/registry-auth"},
        {
            "name": "COSIGN_PASSWORD",
            "valueFrom": {"secretKeyRef": {"name": SIGNING_SECRET, "key": "cosign.password"}},
        },
    ]
    cosign_common = [
        "--key",
        "/var/run/campus-signing/cosign.key",
        *COSIGN_LEGACY_FLAGS,
        "--registry-cacert",
        "/var/run/registry-ca/ca.crt",
        "--yes",
    ]
    heavy = {"requests": {"cpu": "250m", "memory": "512Mi"}, "limits": {"memory": "4Gi"}}
    light = {"requests": {"cpu": "50m", "memory": "64Mi"}, "limits": {"memory": "512Mi"}}

    steps = [
        {
            "name": "sbom",
            "image": TOOL_IMAGES["syft"],
            "args": ["scan", f"registry:{image}", "--output", "spdx-json=/work/sbom.spdx.json"],
            "env": [
                {"name": "HOME", "value": "/tmp"},
                {"name": "TMPDIR", "value": "/tmp"},
                {"name": "SYFT_CHECK_FOR_APP_UPDATE", "value": "false"},
            ],
            "computeResources": heavy,
            "securityContext": nonroot,
            "volumeMounts": work_mounts,
        },
        {
            "name": "scan",
            "image": TOOL_IMAGES["grype"],
            "args": ["sbom:/work/sbom.spdx.json", "--output", "json", "--file", "/work/grype.json"],
            "env": [
                {"name": "HOME", "value": "/tmp"},
                {"name": "TMPDIR", "value": "/tmp"},
                {"name": "GRYPE_DB_CACHE_DIR", "value": "/tmp/grype-db"},
                {"name": "GRYPE_CHECK_FOR_APP_UPDATE", "value": "false"},
            ],
            "computeResources": heavy,
            "securityContext": nonroot,
            "volumeMounts": work_mounts,
        },
        {
            "name": "evaluate",
            "image": TOOL_IMAGES["python"],
            "script": script_text,
            "args": [
                "evaluate",
                "--report", "/work/grype.json",
                "--sbom", "/work/sbom.spdx.json",
                "--exceptions-env", "CAMPUS_VULNERABILITY_EXCEPTIONS",
                "--image", image,
                "--revision", revision,
                "--output", "/work/vulnerability-disposition.json",
                "--results-dir", "/tekton/results",
            ],
            "env": [
                {"name": "CAMPUS_VULNERABILITY_EXCEPTIONS", "value": exceptions_text},
                {"name": "PYTHONDONTWRITEBYTECODE", "value": "1"},
            ],
            "computeResources": light,
            "securityContext": nonroot,
            "volumeMounts": work_mounts,
        },
        {
            "name": "attest-sbom",
            "image": TOOL_IMAGES["cosign"],
            "args": ["attest", *cosign_common, "--type", "spdxjson", "--predicate", "/work/sbom.spdx.json", push_reference],
            "env": cosign_env,
            "computeResources": light,
            "securityContext": nonroot,
            "volumeMounts": cosign_mounts,
        },
        {
            "name": "attest-vulnerabilities",
            "image": TOOL_IMAGES["cosign"],
            "args": [
                "attest", *cosign_common,
                "--type", VULN_PREDICATE_TYPE,
                "--predicate", "/work/vulnerability-disposition.json",
                push_reference,
            ],
            "env": cosign_env,
            "computeResources": light,
            "securityContext": nonroot,
            "volumeMounts": cosign_mounts,
        },
        {
            # Exit status 3 marks a policy block; later steps, including the
            # approval signature, never run for a blocked digest.
            "name": "enforce",
            "image": TOOL_IMAGES["python"],
            "script": ENFORCE_SCRIPT,
            "env": [{"name": "PYTHONDONTWRITEBYTECODE", "value": "1"}],
            "computeResources": light,
            "securityContext": nonroot,
            "volumeMounts": work_mounts,
        },
        {
            "name": "sign",
            "image": TOOL_IMAGES["cosign"],
            "args": [
                "sign", *cosign_common,
                "--sign-container-identity", repository,
                "-a", f"{ANNOTATION_PREFIX}/source-revision={revision}",
                "-a", f"{ANNOTATION_PREFIX}/component={component}",
                "-a", f"{ANNOTATION_PREFIX}/approval-run={name}",
                push_reference,
            ],
            "env": cosign_env,
            "computeResources": light,
            "securityContext": nonroot,
            "volumeMounts": cosign_mounts,
        },
    ]
    return {
        "apiVersion": "tekton.dev/v1",
        "kind": "TaskRun",
        "metadata": {
            "name": name,
            "namespace": SIGNING_NAMESPACE,
            "labels": {
                "app.kubernetes.io/part-of": "pharness",
                "app.kubernetes.io/component": "campus-image-approval",
                "pharness.dev/campus-component": component,
            },
            "annotations": {
                "pharness.dev/source-revision": revision,
                "pharness.dev/image": image,
            },
        },
        "spec": {
            "serviceAccountName": SERVICE_ACCOUNT,
            "timeout": "45m",
            "podTemplate": {
                "securityContext": {
                    "runAsNonRoot": True,
                    "runAsUser": 65532,
                    "runAsGroup": 65532,
                    "fsGroup": 65532,
                    "seccompProfile": {"type": "RuntimeDefault"},
                },
            },
            "taskSpec": {
                "description": "Generate an SBOM, scan it, attest both and sign only an approved Campus image digest.",
                "results": [
                    {"name": "SBOM_SHA256", "description": "Canonical SHA-256 of the SPDX SBOM predicate."},
                    {"name": "VULN_SHA256", "description": "Canonical SHA-256 of the vulnerability disposition predicate."},
                    {"name": "DECISION", "description": "approved or blocked."},
                    {"name": "SUMMARY", "description": "Compact vulnerability totals."},
                ],
                "volumes": [
                    {"name": "work", "emptyDir": {"sizeLimit": "2Gi"}},
                    {"name": "tmp", "emptyDir": {"sizeLimit": "8Gi"}},
                    {
                        "name": "signing-key",
                        "secret": {
                            "secretName": SIGNING_SECRET,
                            "defaultMode": 0o440,
                            "items": [{"key": "cosign.key", "path": "cosign.key"}],
                        },
                    },
                    {
                        "name": "registry-auth",
                        "secret": {
                            "secretName": "lucas-registry-push",
                            "defaultMode": 0o440,
                            "items": [{"key": ".dockerconfigjson", "path": "config.json"}],
                        },
                    },
                    {
                        "name": "registry-ca",
                        "secret": {
                            "secretName": "registry-internal-ca",
                            "defaultMode": 0o444,
                            "items": [{"key": "ca.crt", "path": "ca.crt"}],
                        },
                    },
                ],
                "steps": steps,
            },
        },
    }


def render_canary_taskrun(name: str, created: str, script_text: str) -> dict:
    """A TaskRun that proves the Secret key matches the committed public key."""
    if not K8S_NAME_RE.match(name):
        raise GateError("TaskRun name must be a DNS label")
    if TEKTON_REFERENCE in script_text:
        raise GateError("Tekton would treat a dollar-parenthesis sequence in the gate script as a variable")
    nonroot = {
        "runAsNonRoot": True,
        "runAsUser": 65532,
        "runAsGroup": 65532,
        "allowPrivilegeEscalation": False,
        "readOnlyRootFilesystem": True,
        "capabilities": {"drop": ["ALL"]},
        "seccompProfile": {"type": "RuntimeDefault"},
    }
    light = {"requests": {"cpu": "50m", "memory": "64Mi"}, "limits": {"memory": "256Mi"}}
    return {
        "apiVersion": "tekton.dev/v1",
        "kind": "TaskRun",
        "metadata": {
            "name": name,
            "namespace": SIGNING_NAMESPACE,
            "labels": {
                "app.kubernetes.io/part-of": "pharness",
                "app.kubernetes.io/component": "campus-key-qualification",
            },
        },
        "spec": {
            "serviceAccountName": SERVICE_ACCOUNT,
            "timeout": "10m",
            "podTemplate": {
                "securityContext": {
                    "runAsNonRoot": True,
                    "runAsUser": 65532,
                    "runAsGroup": 65532,
                    "fsGroup": 65532,
                    "seccompProfile": {"type": "RuntimeDefault"},
                },
            },
            "taskSpec": {
                "results": [
                    {"name": "PAYLOAD", "description": "Canary payload that was signed."},
                    {"name": "SIGNATURE", "description": "Base64 signature over the payload."},
                ],
                "volumes": [
                    {"name": "work", "emptyDir": {"sizeLimit": "16Mi"}},
                    {"name": "tmp", "emptyDir": {"sizeLimit": "64Mi"}},
                    {
                        "name": "signing-key",
                        "secret": {
                            "secretName": SIGNING_SECRET,
                            "defaultMode": 0o440,
                            "items": [{"key": "cosign.key", "path": "cosign.key"}],
                        },
                    },
                ],
                "steps": [
                    {
                        "name": "payload",
                        "image": TOOL_IMAGES["python"],
                        "script": script_text,
                        "args": ["canary-payload", "--created", created, "--output", "/work/payload.json", "--results-dir", "/tekton/results"],
                        "computeResources": light,
                        "securityContext": nonroot,
                        "volumeMounts": [{"name": "work", "mountPath": "/work"}],
                    },
                    {
                        "name": "sign-blob",
                        "image": TOOL_IMAGES["cosign"],
                        "args": [
                            "sign-blob",
                            "--key", "/var/run/campus-signing/cosign.key",
                            *COSIGN_LEGACY_FLAGS,
                            "--yes",
                            "--output-signature", "/tekton/results/SIGNATURE",
                            "/work/payload.json",
                        ],
                        "env": [
                            {"name": "HOME", "value": "/tmp"},
                            {
                                "name": "COSIGN_PASSWORD",
                                "valueFrom": {"secretKeyRef": {"name": SIGNING_SECRET, "key": "cosign.password"}},
                            },
                        ],
                        "computeResources": light,
                        "securityContext": nonroot,
                        "volumeMounts": [
                            {"name": "work", "mountPath": "/work", "readOnly": True},
                            {"name": "tmp", "mountPath": "/tmp"},
                            {"name": "signing-key", "mountPath": "/var/run/campus-signing", "readOnly": True},
                        ],
                    },
                ],
            },
        },
    }


def taskrun_outcome(taskrun: dict) -> dict:
    """Interpret a finished approval TaskRun without trusting free-form logs."""
    status = taskrun.get("status") or {}
    conditions = [item for item in status.get("conditions") or [] if item.get("type") == "Succeeded"]
    if not conditions:
        raise GateError("the approval TaskRun has not finished")
    condition = conditions[0]
    results = {item.get("name"): item.get("value") for item in status.get("results") or []}
    steps = {item.get("name"): item for item in status.get("steps") or []}

    def exit_code(step: str) -> int | None:
        terminated = (steps.get(step) or {}).get("terminated") or {}
        code = terminated.get("exitCode")
        return code if isinstance(code, int) else None

    for required in ("sbom", "scan", "evaluate", "attest-sbom", "attest-vulnerabilities"):
        if exit_code(required) != 0:
            raise GateError(f"approval step {required} did not succeed; no evidence was recorded")
    if condition.get("status") == "True":
        if exit_code("enforce") != 0 or exit_code("sign") != 0:
            raise GateError("the TaskRun succeeded without a successful enforce and sign step")
        outcome = "approved"
    elif condition.get("status") == "False" and exit_code("enforce") == 3:
        outcome = "blocked"
    else:
        raise GateError(f"the approval TaskRun failed outside the policy gate: {condition.get('message', '')}")
    if results.get("DECISION") not in (None, outcome):
        raise GateError("the TaskRun DECISION result disagrees with its step outcome")
    for key in ("SBOM_SHA256", "VULN_SHA256"):
        if key in results and not DIGEST_RE.match(str(results[key])):
            raise GateError(f"the TaskRun {key} result is not a sha256 digest")
    return {
        "outcome": outcome,
        "results": results,
        "completed_at": status.get("completionTime"),
        "started_at": status.get("startTime"),
        "pod": status.get("podName"),
    }


# ---------------------------------------------------------------------------
# Registry and cosign verification (workstation only)


def registry_manifest_digest(reference: str) -> str | None:
    """HEAD a tag on the public registry and return its digest, or None if absent."""
    repository, _, tag = reference.rpartition(":")
    host, _, path = repository.partition("/")
    request = urllib.request.Request(
        f"https://{host}/v2/{path}/manifests/{tag}",
        method="HEAD",
        # The public route rejects urllib's default User-Agent.
        headers={
            "User-Agent": "pharness-campus-image-gate/1",
            "Accept": ", ".join(
                [
                    "application/vnd.oci.image.manifest.v1+json",
                    "application/vnd.docker.distribution.manifest.v2+json",
                    "application/vnd.oci.image.index.v1+json",
                ]
            )
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.headers.get("Docker-Content-Digest")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise GateError(f"registry returned HTTP {error.code} for {reference}") from error


def sidecar_reference(image: str, suffix: str) -> str:
    repository, digest = parse_image(image)
    return f"{repository}:sha256-{digest.split(':', 1)[1]}.{suffix}"


def run_cosign(cosign: str, arguments: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run([cosign, *arguments], capture_output=True, text=True, check=False)


def verified_attestations(cosign: str, public_key: Path, image: str, predicate_type: str) -> list[dict]:
    """Return statements whose DSSE signatures verify against the public key."""
    result = run_cosign(
        cosign,
        [
            "verify-attestation",
            "--key", str(public_key),
            "--insecure-ignore-tlog=true",
            "--type", predicate_type,
            "--output", "json",
            image,
        ],
    )
    if result.returncode != 0:
        return []
    statements = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        envelope = json.loads(line)
        statement = json.loads(base64.b64decode(envelope["payload"]))
        _, digest = parse_image(image)
        subjects = statement.get("subject") or []
        if not any(("sha256:" + (subject.get("digest") or {}).get("sha256", "")) == digest for subject in subjects):
            continue
        if statement.get("predicateType") != predicate_type:
            continue
        statements.append(statement)
    return statements


def find_attested_predicate(cosign: str, public_key: Path, image: str, predicate_type: str, expected: str) -> dict:
    for statement in verified_attestations(cosign, public_key, image, predicate_type):
        if canonical_sha256(statement.get("predicate")) == expected:
            return statement["predicate"]
    raise GateError(f"no attestation of type {predicate_type} with digest {expected} verifies for {image}")


def verified_signatures(cosign: str, public_key: Path, image: str) -> list[dict]:
    result = run_cosign(
        cosign,
        ["verify", "--key", str(public_key), "--insecure-ignore-tlog=true", "--output", "json", image],
    )
    if result.returncode != 0:
        return []
    payloads = json.loads(result.stdout)
    return payloads if isinstance(payloads, list) else []


def approval_signature(cosign: str, public_key: Path, image: str, revision: str) -> dict:
    repository, digest = parse_image(image)
    for payload in verified_signatures(cosign, public_key, image):
        critical = payload.get("critical") or {}
        annotations = payload.get("optional") or {}
        if (critical.get("image") or {}).get("docker-manifest-digest") != digest:
            continue
        if (critical.get("identity") or {}).get("docker-reference") != repository:
            continue
        if annotations.get(f"{ANNOTATION_PREFIX}/source-revision") != revision:
            continue
        return {
            "docker_reference": repository,
            "annotations": {key: annotations[key] for key in sorted(annotations) if key.startswith(ANNOTATION_PREFIX)},
        }
    raise GateError(f"no approval signature for {image} verifies against the committed public key")


# ---------------------------------------------------------------------------
# Evidence record


def load_evidence(path: Path) -> dict:
    document = load_json(path)
    if not isinstance(document, dict) or document.get("schema_version") != SCHEMA_VERSION:
        raise GateError(f"{path} must be an object with schema_version 1")
    if not isinstance(document.get("images"), list):
        raise GateError(f"{path} must contain an images list")
    return document


def collect(args: argparse.Namespace) -> dict:
    """Build one evidence entry from a finished TaskRun and the registry."""
    taskrun = load_json(Path(args.taskrun))
    release = load_json(Path(args.release_check))
    exceptions_document = load_json(Path(args.exceptions))
    outcome = taskrun_outcome(taskrun)
    image = args.image
    repository, digest = parse_image(image)
    if (taskrun.get("metadata") or {}).get("annotations", {}).get("pharness.dev/image") != image:
        raise GateError("the TaskRun was created for a different image")
    results = outcome["results"]
    for key in ("SBOM_SHA256", "VULN_SHA256"):
        if key not in results:
            raise GateError(f"the TaskRun did not publish {key}; cannot bind attestations to this run")

    public_key = Path(args.public_key)
    sbom = find_attested_predicate(args.cosign, public_key, image, SBOM_PREDICATE_TYPE, results["SBOM_SHA256"])
    predicate = find_attested_predicate(args.cosign, public_key, image, VULN_PREDICATE_TYPE, results["VULN_SHA256"])
    if predicate.get("image") != image or predicate.get("source_revision") != release["source_revision"]:
        raise GateError("the attested vulnerability disposition names a different image or revision")
    if predicate.get("sbom", {}).get("canonical_sha256") != results["SBOM_SHA256"]:
        raise GateError("the vulnerability disposition does not cite the attested SBOM")
    if predicate.get("decision") != outcome["outcome"]:
        raise GateError("the attested decision does not match the TaskRun outcome")
    today = parse_date(args.today, "--today") if args.today else today_utc()
    recheck = reapply(predicate, exceptions_document, today)
    if recheck["decision"] != predicate["decision"]:
        raise GateError("re-applying the policy with the current exceptions gives a different decision")

    signature = None
    if outcome["outcome"] == "approved":
        facts = approval_signature(args.cosign, public_key, image, release["source_revision"])
        reference = sidecar_reference(image, "sig")
        facts.update({"reference": reference, "manifest_digest": registry_manifest_digest(reference)})
        signature = facts
    elif verified_signatures(args.cosign, public_key, image):
        raise GateError("a blocked image carries a signature from the Campus key; investigate before recording")

    attestation_reference = sidecar_reference(image, "att")
    attestation_digest = registry_manifest_digest(attestation_reference)
    if not attestation_digest:
        raise GateError("attestations verified but their registry manifest could not be resolved")

    return {
        "image": image,
        "repository": repository,
        "digest": digest,
        "component": release["component"],
        "status": outcome["outcome"],
        "source_revision": release["source_revision"],
        "release_record": {
            "pipeline_run": release.get("pipeline_run"),
            "build_executor": release.get("build_executor"),
            "config_digest": release.get("config_digest"),
            "platform": release.get("platform"),
            "build_json_sha256": args.build_sha256,
            "artifacts_json_sha256": args.artifacts_sha256,
        },
        "assessed_at": outcome["completed_at"],
        "approval_run": {
            "namespace": SIGNING_NAMESPACE,
            "taskrun": taskrun["metadata"]["name"],
            "tool_images": dict(TOOL_IMAGES),
        },
        "sbom": {
            "format": "spdx-json",
            "predicate_type": SBOM_PREDICATE_TYPE,
            "canonical_sha256": results["SBOM_SHA256"],
            "packages": len(sbom.get("packages") or []),
            "files": len(sbom.get("files") or []),
            "generator": next((item for item in (sbom.get("creationInfo") or {}).get("creators", []) if item.startswith("Tool:")), None),
        },
        "vulnerability_scan": {
            "predicate_type": VULN_PREDICATE_TYPE,
            "canonical_sha256": results["VULN_SHA256"],
            "scanner": predicate["scanner"],
            "decision": predicate["decision"],
            "summary": predicate["summary"],
            "exceptions_sha256": predicate["policy"]["exceptions_sha256"],
            "exceptions_used": predicate.get("exceptions_used", []),
            "blocking_findings": blocking_findings(predicate),
        },
        "attestations": {
            "reference": attestation_reference,
            "manifest_digest": attestation_digest,
        },
        "signature": signature,
    }


def record(evidence_path: Path, entry: dict) -> dict:
    evidence = load_evidence(evidence_path)
    images = [item for item in evidence["images"] if item.get("image") != entry["image"]]
    images.append(entry)
    images.sort(key=lambda item: (item["repository"], item["digest"]))
    evidence["images"] = images
    write_json(evidence_path, evidence)
    return evidence


def allowlist(evidence: dict) -> list[str]:
    return sorted(item["image"] for item in evidence["images"] if item.get("status") == "approved")


# ---------------------------------------------------------------------------
# Admission policy rendering


def _yaml_scalar(value: Any) -> str:
    if value is True:
        return "true"
    if value is False:
        return "false"
    if value is None:
        return "null"
    if isinstance(value, int):
        return str(value)
    return json.dumps(str(value))


def to_yaml(value: Any, indent: int = 0) -> str:
    """Emit the small, fixed subset of YAML that the generated manifests need."""
    pad = " " * indent
    lines: list[str] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if isinstance(item, str) and "\n" in item:
                lines.append(f"{pad}{key}: |-")
                lines.extend(f"{pad}  {line}" if line else "" for line in item.split("\n"))
            elif isinstance(item, (dict, list)) and item:
                lines.append(f"{pad}{key}:")
                lines.append(to_yaml(item, indent + 2))
            elif isinstance(item, dict):
                lines.append(f"{pad}{key}: {{}}")
            elif isinstance(item, list):
                lines.append(f"{pad}{key}: []")
            else:
                lines.append(f"{pad}{key}: {_yaml_scalar(item)}")
    elif isinstance(value, list):
        for item in value:
            if isinstance(item, (dict, list)) and item:
                nested = to_yaml(item, indent + 2).split("\n")
                lines.append(f"{pad}- {nested[0].lstrip()}")
                lines.extend(nested[1:])
            else:
                lines.append(f"{pad}- {_yaml_scalar(item)}")
    else:
        lines.append(pad + _yaml_scalar(value))
    return "\n".join(lines)


def pod_spec_expression(root: str) -> str:
    return (
        f"{root}.kind == 'Pod' ? {root}.spec : "
        f"({root}.kind == 'CronJob' ? {root}.spec.jobTemplate.spec.template.spec : {root}.spec.template.spec)"
    )


def images_expression(spec: str) -> str:
    parts = []
    for field in ("containers", "initContainers", "ephemeralContainers"):
        parts.append(
            f"(has({spec}.{field}) ? {spec}.{field}.map(c, has(c.image) ? c.image : '') : [])"
        )
    return " +\n".join(parts)


def render_policy(evidence: dict, name: str = POLICY_NAME, label: str = LAB_NAMESPACE_LABEL) -> str:
    images = allowlist(evidence)
    for image in images:
        parse_image(image)
    allowed_literal = "[" + ", ".join("'" + image + "'" for image in images) + "]"
    pattern = IMAGE_PATTERN
    common_labels = {"app.kubernetes.io/part-of": "pharness", "app.kubernetes.io/component": "campus-image-admission"}
    policy = {
        "apiVersion": "admissionregistration.k8s.io/v1",
        "kind": "ValidatingAdmissionPolicy",
        "metadata": {
            "name": name,
            "labels": common_labels,
            "annotations": {
                "pharness.dev/generated-from": "deploy/campus/approved-images.json",
                "pharness.dev/allowlist-sha256": canonical_sha256(images),
            },
        },
        "spec": {
            "failurePolicy": "Fail",
            "matchConstraints": {
                "namespaceSelector": {"matchLabels": {label: "true"}},
                "resourceRules": [
                    {
                        "apiGroups": [""],
                        "apiVersions": ["v1"],
                        "operations": ["CREATE", "UPDATE"],
                        "resources": ["pods", "pods/ephemeralcontainers"],
                    },
                    {
                        "apiGroups": ["batch"],
                        "apiVersions": ["v1"],
                        "operations": ["CREATE", "UPDATE"],
                        "resources": ["jobs", "cronjobs"],
                    },
                    {
                        "apiGroups": ["apps"],
                        "apiVersions": ["v1"],
                        "operations": ["CREATE", "UPDATE"],
                        "resources": ["deployments", "replicasets", "statefulsets", "daemonsets"],
                    },
                ],
            },
            "variables": [
                {"name": "allowedImages", "expression": allowed_literal},
                {"name": "podSpec", "expression": pod_spec_expression("object")},
                {"name": "images", "expression": images_expression("variables.podSpec")},
                # Only images introduced by this request are judged, so metadata
                # updates and finalizer removal on Pods admitted under an older
                # allowlist still succeed after a digest is revoked.
                {
                    "name": "previousPodSpec",
                    "expression": "oldObject == null ? null : (" + pod_spec_expression("oldObject") + ")",
                },
                {
                    "name": "previousImages",
                    "expression": "variables.previousPodSpec == null ? [] : (\n"
                    + images_expression("variables.previousPodSpec")
                    + ")",
                },
                {
                    "name": "newImages",
                    "expression": "variables.images.filter(i, !(i in variables.previousImages))",
                },
            ],
            "validations": [
                {
                    "expression": f"variables.newImages.all(i, i.matches('{pattern}'))",
                    "messageExpression": (
                        "'Campus lab images must be fully qualified immutable references "
                        "(repository@sha256:<64 lowercase hex>, no tag); rejected: ' + "
                        f"variables.newImages.filter(i, !i.matches('{pattern}')).join(', ')"
                    ),
                    "reason": "Forbidden",
                },
                {
                    "expression": "variables.newImages.all(i, i in variables.allowedImages)",
                    "messageExpression": (
                        "'Campus lab image digest is not approved in deploy/campus/approved-images.json; rejected: ' + "
                        "variables.newImages.filter(i, !(i in variables.allowedImages)).join(', ')"
                    ),
                    "reason": "Forbidden",
                },
            ],
        },
    }
    binding = {
        "apiVersion": "admissionregistration.k8s.io/v1",
        "kind": "ValidatingAdmissionPolicyBinding",
        "metadata": {"name": name, "labels": common_labels},
        "spec": {
            "policyName": name,
            "validationActions": ["Deny"],
            "matchResources": {"namespaceSelector": {"matchLabels": {label: "true"}}},
        },
    }
    header = (
        "# Generated by scripts/campus_image_gate.py render-policy from\n"
        "# deploy/campus/approved-images.json. Do not edit by hand; CI rejects drift.\n"
    )
    return header + "---\n" + to_yaml(policy) + "\n---\n" + to_yaml(binding) + "\n"


# ---------------------------------------------------------------------------
# Repository consistency checks


def check_repository(root: Path, today: dt.date) -> list[str]:
    campus = root / "deploy" / "campus"
    errors: list[str] = []
    warnings: list[str] = []
    try:
        evidence = load_evidence(campus / "approved-images.json")
        exceptions_document = load_json(campus / "vulnerability-exceptions.json")
        exceptions = validate_exceptions(exceptions_document)
        key_digest = public_key_sha256((campus / "cosign.pub").read_text(encoding="utf-8"))
    except (GateError, OSError) as error:
        return [str(error)]

    signing = evidence.get("signing") or {}
    if signing.get("public_key_sha256") != key_digest:
        errors.append("deploy/campus/cosign.pub does not match signing.public_key_sha256 in the evidence record")
    if signing.get("secret") != f"{SIGNING_NAMESPACE}/{SIGNING_SECRET}":
        errors.append("the evidence record names an unexpected signing Secret")

    active = {exception_key(item["id"], item["package"], item["repository"]): item for item in exceptions}
    seen: set[str] = set()
    for index, entry in enumerate(evidence["images"]):
        where = f"images[{index}]"
        image = entry.get("image")
        try:
            repository, digest = parse_image(image)
        except GateError as error:
            errors.append(f"{where}: {error}")
            continue
        if image in seen:
            errors.append(f"{where}: {image} is recorded twice")
        seen.add(image)
        if entry.get("repository") != repository or entry.get("digest") != digest:
            errors.append(f"{where}: repository and digest fields disagree with the image reference")
        status = entry.get("status")
        if status not in STATUSES:
            errors.append(f"{where}: status must be one of {', '.join(STATUSES)}")
            continue
        if not REVISION_RE.match(str(entry.get("source_revision") or "")):
            errors.append(f"{where}: source_revision must be a full Git SHA")
        scan = entry.get("vulnerability_scan") or {}
        sbom = entry.get("sbom") or {}
        for label, value in (("sbom.canonical_sha256", sbom.get("canonical_sha256")), ("vulnerability_scan.canonical_sha256", scan.get("canonical_sha256"))):
            if not DIGEST_RE.match(str(value or "")):
                errors.append(f"{where}: {label} must be a sha256 digest")
        attestations = entry.get("attestations") or {}
        if attestations.get("reference") != sidecar_reference(image, "att"):
            errors.append(f"{where}: attestations.reference must be the cosign .att tag for this digest")
        summary = scan.get("summary") or {}
        blocking = scan.get("blocking_findings")
        if not isinstance(blocking, list) or summary.get("blocking") != len(blocking):
            errors.append(f"{where}: blocking_findings must list every blocking finding in the summary")
        if status == "approved":
            if scan.get("decision") != "approved" or blocking:
                errors.append(f"{where}: an approved image must have an approved scan with no blocking findings")
            signature = entry.get("signature") or {}
            if signature.get("reference") != sidecar_reference(image, "sig"):
                errors.append(f"{where}: an approved image must record its cosign signature reference")
            if signature.get("docker_reference") != repository:
                errors.append(f"{where}: the signature identity must be the image repository")
            for key in scan.get("exceptions_used") or []:
                exception = active.get(key)
                if exception is None:
                    errors.append(f"{where}: relies on exception {key}, which is not in vulnerability-exceptions.json")
                elif parse_date(exception["expires"], "expires") < today:
                    errors.append(f"{where}: relies on exception {key}, which expired on {exception['expires']}; re-assess or revoke")
        elif status == "blocked":
            if scan.get("decision") != "blocked" or not blocking:
                errors.append(f"{where}: a blocked image must record the blocking scan decision and findings")
            if entry.get("signature") is not None:
                errors.append(f"{where}: a blocked image must not record an approval signature")
        elif status == "revoked" and not str(entry.get("revocation_reason") or "").strip():
            errors.append(f"{where}: a revoked image must record revocation_reason")

    for key, exception in active.items():
        if parse_date(exception["expires"], "expires") < today:
            warnings.append(f"exception {key} expired on {exception['expires']} and should be removed")

    for index, item in enumerate(evidence.get("key_qualification") or []):
        payload_path = root / str(item.get("payload", ""))
        signature_path = root / str(item.get("signature", ""))
        if not payload_path.is_file() or not signature_path.is_file():
            errors.append(f"key_qualification[{index}]: payload and signature files must exist")
            continue
        if file_sha256(payload_path) != item.get("payload_sha256"):
            errors.append(f"key_qualification[{index}]: payload digest does not match the committed file")
        if item.get("public_key_sha256") != key_digest:
            errors.append(f"key_qualification[{index}]: was produced for a different public key")

    rendered = render_policy(evidence)
    manifest = campus / "admission" / "campus-lab-image-allowlist.yaml"
    try:
        committed = manifest.read_text(encoding="utf-8")
    except OSError:
        committed = ""
    if committed != rendered:
        errors.append(f"{manifest.relative_to(root)} is not the policy rendered from approved-images.json")

    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    return errors


def verify_online(root: Path, cosign: str, today: dt.date) -> list[str]:
    campus = root / "deploy" / "campus"
    evidence = load_evidence(campus / "approved-images.json")
    exceptions_document = load_json(campus / "vulnerability-exceptions.json")
    public_key = campus / "cosign.pub"
    errors: list[str] = []
    for entry in evidence["images"]:
        image = entry["image"]
        try:
            find_attested_predicate(cosign, public_key, image, SBOM_PREDICATE_TYPE, entry["sbom"]["canonical_sha256"])
            predicate = find_attested_predicate(
                cosign, public_key, image, VULN_PREDICATE_TYPE, entry["vulnerability_scan"]["canonical_sha256"]
            )
            if predicate.get("decision") != entry["vulnerability_scan"]["decision"]:
                raise GateError("attested decision differs from the evidence record")
            if predicate.get("summary") != entry["vulnerability_scan"]["summary"]:
                raise GateError("attested summary differs from the evidence record")
            if entry["status"] == "approved":
                if reapply(predicate, exceptions_document, today)["decision"] != "approved":
                    raise GateError("the current exceptions no longer approve this digest")
                approval_signature(cosign, public_key, image, entry["source_revision"])
            elif entry["status"] == "blocked" and verified_signatures(cosign, public_key, image):
                raise GateError("a blocked digest carries a Campus approval signature")
            print(f"verified {entry['status']} {image}")
        except (GateError, KeyError) as error:
            errors.append(f"{image}: {error}")
    for item in evidence.get("key_qualification") or []:
        result = run_cosign(
            cosign,
            [
                "verify-blob",
                "--key", str(public_key),
                "--insecure-ignore-tlog=true",
                "--signature", str(root / item["signature"]),
                str(root / item["payload"]),
            ],
        )
        if result.returncode != 0:
            errors.append(f"key qualification {item['payload']} does not verify: {result.stderr.strip()}")
        else:
            print(f"verified key qualification {item['payload']}")
    return errors


# ---------------------------------------------------------------------------
# Lab session dependency controls


def load_package_sources(path: Path) -> dict:
    document = load_json(path)
    if document.get("schema_version") != SCHEMA_VERSION:
        raise GateError("package sources must have schema_version 1")
    for field in ("pip_index_urls", "npm_registries"):
        values = document.get(field)
        if not isinstance(values, list) or not values or not all(isinstance(v, str) and v.startswith("https://") for v in values):
            raise GateError(f"package sources {field} must be a non-empty list of https URLs")
    return document


CACHE_ROOT = "/var/cache/campus"
CACHE_ENV = {
    "PIP_CACHE_DIR": f"{CACHE_ROOT}/pip",
    "UV_CACHE_DIR": f"{CACHE_ROOT}/uv",
    "NPM_CONFIG_CACHE": f"{CACHE_ROOT}/npm",
    "XDG_CACHE_HOME": f"{CACHE_ROOT}/xdg",
}


def render_lab_pod(name: str, namespace: str, image: str, session: str, sources: dict, command: list[str] | None = None) -> dict:
    parse_image(image)
    if not K8S_NAME_RE.match(name) or not K8S_NAME_RE.match(session):
        raise GateError("Pod name and session id must be DNS labels")
    pip_index = sources["pip_index_urls"][0]
    npm_registry = sources["npm_registries"][0]
    env = [
        {"name": "HOME", "value": "/workspace"},
        *({"name": key, "value": value} for key, value in CACHE_ENV.items()),
        {"name": "PIP_INDEX_URL", "value": pip_index},
        {"name": "UV_INDEX_URL", "value": pip_index},
        {"name": "PIP_CONFIG_FILE", "value": "/dev/null"},
        {"name": "PIP_DISABLE_PIP_VERSION_CHECK", "value": "1"},
        {"name": "NPM_CONFIG_REGISTRY", "value": npm_registry},
        {"name": "NPM_CONFIG_USERCONFIG", "value": "/dev/null"},
        {"name": "NPM_CONFIG_UPDATE_NOTIFIER", "value": "false"},
    ]
    container = {
        "name": "session",
        "image": image,
        "env": env,
        "securityContext": {
            "allowPrivilegeEscalation": False,
            "readOnlyRootFilesystem": True,
            "capabilities": {"drop": ["ALL"]},
        },
        "resources": {
            "requests": {"cpu": "100m", "memory": "256Mi", "ephemeral-storage": "256Mi"},
            "limits": {"memory": "1Gi", "ephemeral-storage": "4Gi"},
        },
        "volumeMounts": [
            {"name": "workspace", "mountPath": "/workspace"},
            {"name": "package-cache", "mountPath": CACHE_ROOT},
            {"name": "tmp", "mountPath": "/tmp"},
        ],
    }
    if command:
        container["command"] = command
    return {
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/part-of": "pharness",
                "app.kubernetes.io/component": "campus-lab-session",
                "pharness.dev/campus-session": session,
            },
        },
        "spec": {
            "runtimeClassName": "gvisor",
            "automountServiceAccountToken": False,
            "enableServiceLinks": False,
            "restartPolicy": "Never",
            "securityContext": {
                "runAsNonRoot": True,
                "runAsUser": 65532,
                "runAsGroup": 65532,
                "fsGroup": 65532,
                "seccompProfile": {"type": "RuntimeDefault"},
            },
            "containers": [container],
            "volumes": [
                {"name": "workspace", "emptyDir": {"sizeLimit": "2Gi"}},
                {"name": "package-cache", "emptyDir": {"sizeLimit": "2Gi"}},
                {"name": "tmp", "emptyDir": {"sizeLimit": "512Mi"}},
            ],
        },
    }


def check_lab_pod(pod: dict, sources: dict) -> list[str]:
    """Reject lab Pods whose package caches or indexes could be shared or redirected."""
    errors: list[str] = []
    spec = pod.get("spec") or {}
    volumes = {volume.get("name"): volume for volume in spec.get("volumes") or []}
    for name, volume in volumes.items():
        if "emptyDir" not in volume:
            kinds = ", ".join(sorted(key for key in volume if key != "name"))
            errors.append(f"volume {name} uses {kinds}; lab session volumes must be per-Pod emptyDir")
        elif (volume.get("emptyDir") or {}).get("medium") not in (None, "", "Memory"):
            errors.append(f"volume {name} uses an unsupported emptyDir medium")
    containers = (spec.get("initContainers") or []) + (spec.get("containers") or [])
    if not containers:
        errors.append("the lab Pod has no containers")
    for container in containers:
        name = container.get("name")
        env = {item.get("name"): item for item in container.get("env") or []}
        mounts = sorted(
            (mount.get("mountPath", ""), mount.get("name")) for mount in container.get("volumeMounts") or []
        )
        for variable in ("PIP_INDEX_URL", "UV_INDEX_URL", "NPM_CONFIG_REGISTRY", *CACHE_ENV):
            item = env.get(variable)
            if item is None or "value" not in item or "valueFrom" in item:
                errors.append(f"container {name} must set {variable} to a literal value")
        for variable in ("PIP_EXTRA_INDEX_URL", "PIP_TRUSTED_HOST", "PIP_FIND_LINKS", "UV_EXTRA_INDEX_URL"):
            if variable in env:
                errors.append(f"container {name} must not set {variable}; it adds an unreviewed package source")
        if (env.get("PIP_CONFIG_FILE") or {}).get("value") != "/dev/null":
            errors.append(f"container {name} must set PIP_CONFIG_FILE=/dev/null so image or user pip config cannot redirect the index")
        for variable in ("PIP_INDEX_URL", "UV_INDEX_URL"):
            value = (env.get(variable) or {}).get("value")
            if value is not None and value not in sources["pip_index_urls"]:
                errors.append(f"container {name} {variable}={value} is not an approved Python index")
        value = (env.get("NPM_CONFIG_REGISTRY") or {}).get("value")
        if value is not None and value not in sources["npm_registries"]:
            errors.append(f"container {name} NPM_CONFIG_REGISTRY={value} is not an approved npm registry")
        for variable in CACHE_ENV:
            path = (env.get(variable) or {}).get("value")
            if not path:
                continue
            owner = None
            for mount_path, volume in mounts:
                if path == mount_path or path.startswith(mount_path.rstrip("/") + "/"):
                    owner = volume
            if owner is None:
                errors.append(f"container {name} {variable}={path} is not on a mounted per-session volume")
            elif "emptyDir" not in (volumes.get(owner) or {}):
                errors.append(f"container {name} {variable}={path} is on volume {owner}, which is not a per-Pod emptyDir")
    return errors


# ---------------------------------------------------------------------------
# Command line


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("release-check", help="prove a digest belongs to a release record")
    p.add_argument("--build", required=True)
    p.add_argument("--artifacts", required=True)
    p.add_argument("--image", required=True)
    p.add_argument("--revision", required=True)

    p = sub.add_parser("render-taskrun", help="print the approval TaskRun")
    p.add_argument("--name", required=True)
    p.add_argument("--image", required=True)
    p.add_argument("--revision", required=True)
    p.add_argument("--component", required=True)
    p.add_argument("--exceptions", required=True)

    p = sub.add_parser("render-canary-taskrun", help="print the key qualification TaskRun")
    p.add_argument("--name", required=True)
    p.add_argument("--created", required=True)

    p = sub.add_parser("taskrun-outcome", help="interpret a finished approval TaskRun")
    p.add_argument("--taskrun", required=True)

    p = sub.add_parser("collect", help="verify registry evidence for a finished TaskRun")
    p.add_argument("--taskrun", required=True)
    p.add_argument("--release-check", required=True)
    p.add_argument("--image", required=True)
    p.add_argument("--exceptions", required=True)
    p.add_argument("--public-key", required=True)
    p.add_argument("--cosign", default="cosign")
    p.add_argument("--build-sha256", required=True)
    p.add_argument("--artifacts-sha256", required=True)
    p.add_argument("--today")

    p = sub.add_parser("record", help="write one evidence entry into approved-images.json")
    p.add_argument("--evidence", required=True)
    p.add_argument("--entry", required=True)

    p = sub.add_parser("render-policy", help="print the admission policy for the allowlist")
    p.add_argument("--evidence", required=True)
    p.add_argument("--name", default=POLICY_NAME)
    p.add_argument("--namespace-label", default=LAB_NAMESPACE_LABEL)

    p = sub.add_parser("check", help="offline evidence, exception and policy consistency")
    p.add_argument("--root", default=".")
    p.add_argument("--today")

    p = sub.add_parser("verify-online", help="verify signatures and attestations in the registry")
    p.add_argument("--root", default=".")
    p.add_argument("--cosign", default="cosign")
    p.add_argument("--today")

    p = sub.add_parser("render-lab-pod", help="print a lab session Pod with dependency controls")
    p.add_argument("--name", required=True)
    p.add_argument("--namespace", required=True)
    p.add_argument("--image", required=True)
    p.add_argument("--session", required=True)
    p.add_argument("--sources", required=True)
    p.add_argument("--container-command", nargs=argparse.REMAINDER)

    p = sub.add_parser("check-lab-pod", help="validate a lab Pod's package cache and index controls")
    p.add_argument("--pod", required=True)
    p.add_argument("--sources", required=True)

    args = parser.parse_args(argv)
    try:
        if args.command not in sub.choices:
            raise GateError(f"unsupported command {args.command}")
        today = parse_date(args.today, "--today") if getattr(args, "today", None) else today_utc()
        if args.command == "release-check":
            print(json.dumps(release_check(load_json(Path(args.build)), load_json(Path(args.artifacts)), args.image, args.revision), indent=2))
        elif args.command == "render-taskrun":
            script = POLICY_SOURCE.read_text(encoding="utf-8")
            exceptions_text = Path(args.exceptions).read_text(encoding="utf-8")
            print(json.dumps(render_taskrun(args.name, args.image, args.revision, args.component, exceptions_text, script), indent=2))
        elif args.command == "render-canary-taskrun":
            script = POLICY_SOURCE.read_text(encoding="utf-8")
            print(json.dumps(render_canary_taskrun(args.name, args.created, script), indent=2))
        elif args.command == "taskrun-outcome":
            print(json.dumps(taskrun_outcome(load_json(Path(args.taskrun))), indent=2))
        elif args.command == "collect":
            print(json.dumps(collect(args), indent=2))
        elif args.command == "record":
            record(Path(args.evidence), load_json(Path(args.entry)))
        elif args.command == "render-policy":
            sys.stdout.write(render_policy(load_evidence(Path(args.evidence)), args.name, args.namespace_label))
        elif args.command == "check":
            errors = check_repository(Path(args.root), today)
            for error in errors:
                print(f"error: {error}", file=sys.stderr)
            if errors:
                return 1
            print("campus image evidence, exceptions and admission allowlist are consistent")
        elif args.command == "verify-online":
            errors = verify_online(Path(args.root), args.cosign, today)
            for error in errors:
                print(f"error: {error}", file=sys.stderr)
            return 1 if errors else 0
        elif args.command == "render-lab-pod":
            sources = load_package_sources(Path(args.sources))
            print(json.dumps(render_lab_pod(args.name, args.namespace, args.image, args.session, sources, args.container_command or None), indent=2))
        elif args.command == "check-lab-pod":
            sources = load_package_sources(Path(args.sources))
            errors = check_lab_pod(load_json(Path(args.pod)), sources)
            for error in errors:
                print(f"error: {error}", file=sys.stderr)
            if errors:
                return 1
            print("lab Pod package caches are per-session and indexes are pinned")
    except GateError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
