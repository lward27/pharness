#!/usr/bin/env python3
"""Campus image vulnerability policy and canonical evidence encoding.

This module uses only the Python standard library because the approval
TaskRun embeds it verbatim as the ``evaluate`` and key-qualification step
script. ``campus_image_gate.py`` imports it for workstation and CI checks, so
the decision that blocks signing in the cluster is the same code that CI and
reviewers re-apply to the attested findings.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
REPOSITORY_PATTERN = (
    r"[a-z0-9]+(?:[.-][a-z0-9]+)*(?::[0-9]+)?(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)+"
)
IMAGE_PATTERN = "^" + REPOSITORY_PATTERN + "@sha256:[0-9a-f]{64}$"
IMAGE_RE = re.compile(
    "^(?P<repository>" + REPOSITORY_PATTERN + ")@(?P<digest>sha256:[0-9a-f]{64})$"
)
REPOSITORY_RE = re.compile("^" + REPOSITORY_PATTERN + "$")
REVISION_RE = re.compile(r"^[0-9a-f]{40}$")
DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
VULN_ID_RE = re.compile(r"^[A-Z][A-Z0-9]*-[A-Za-z0-9][A-Za-z0-9-]*$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
K8S_NAME_RE = re.compile(r"^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?$")

BLOCKING_SEVERITIES = ("Critical", "High")
SEVERITIES = ("Critical", "High", "Medium", "Low", "Negligible", "Unknown")
MAX_EXCEPTION_DAYS = 90
SIGNING_SECRET_REF = "tekton-pipelines/campus-cosign-signing"


class GateError(Exception):
    """A policy, evidence or input failure with an operator-facing message."""


# ---------------------------------------------------------------------------
# Canonical JSON and small helpers


def _normalise(value: Any) -> Any:
    if isinstance(value, float) and value.is_integer():
        return int(value)
    if isinstance(value, dict):
        return {key: _normalise(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_normalise(item) for item in value]
    return value


def canonical_json(value: Any) -> bytes:
    """Encode JSON so that re-encoding by cosign does not change the digest.

    cosign decodes predicates into generic maps before embedding them in the
    in-toto statement, so key order, whitespace and escaping are not preserved.
    Sorting keys, using compact separators and folding integral floats gives a
    form that is identical before signing and after extraction.
    """
    return json.dumps(
        _normalise(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def canonical_sha256(value: Any) -> str:
    return "sha256:" + hashlib.sha256(canonical_json(value)).hexdigest()


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise GateError(f"{path} does not exist") from error
    except json.JSONDecodeError as error:
        raise GateError(f"{path} is not valid JSON: {error}") from error


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def parse_image(reference: str) -> tuple[str, str]:
    match = IMAGE_RE.match(reference or "")
    if not match:
        raise GateError(
            f"{reference!r} is not an immutable image reference of the form repository@sha256:<64 hex>"
        )
    return match.group("repository"), match.group("digest")


def parse_date(value: Any, field: str) -> dt.date:
    if not isinstance(value, str) or not DATE_RE.match(value):
        raise GateError(f"{field} must be a YYYY-MM-DD date")
    try:
        return dt.date.fromisoformat(value)
    except ValueError as error:
        raise GateError(f"{field} is not a valid date: {value}") from error


def today_utc() -> dt.date:
    return dt.datetime.now(dt.timezone.utc).date()


def public_key_sha256(pem_text: str) -> str:
    """Fingerprint the DER SubjectPublicKeyInfo inside a PEM public key."""
    lines = [line.strip() for line in pem_text.strip().splitlines()]
    if not lines or lines[0] != "-----BEGIN PUBLIC KEY-----" or lines[-1] != "-----END PUBLIC KEY-----":
        raise GateError("cosign public key must be a single PEM PUBLIC KEY block")
    der = base64.b64decode("".join(lines[1:-1]), validate=True)
    return "sha256:" + hashlib.sha256(der).hexdigest()


# ---------------------------------------------------------------------------
# Exceptions and vulnerability policy


def exception_key(vulnerability: str, package: str, repository: str) -> str:
    return f"{vulnerability}|{package}|{repository}"


def validate_exceptions(document: Any) -> list[dict]:
    """Validate the reviewed exceptions file and return its entries.

    Expiry is not checked here; an expired entry stays syntactically valid but
    no longer covers a finding.
    """
    if not isinstance(document, dict) or document.get("schema_version") != SCHEMA_VERSION:
        raise GateError("vulnerability exceptions must be an object with schema_version 1")
    entries = document.get("exceptions")
    if not isinstance(entries, list):
        raise GateError("vulnerability exceptions must contain an exceptions list")
    seen: set[str] = set()
    for index, entry in enumerate(entries):
        where = f"exceptions[{index}]"
        if not isinstance(entry, dict):
            raise GateError(f"{where} must be an object")
        required = ("id", "package", "repository", "owner", "reviewer", "reason", "created", "expires")
        for field in required:
            if not isinstance(entry.get(field), str) or not entry[field].strip():
                raise GateError(f"{where}.{field} is required")
        unknown = sorted(set(entry) - set(required))
        if unknown:
            raise GateError(f"{where} has unsupported fields: {', '.join(unknown)}")
        if not VULN_ID_RE.match(entry["id"]):
            raise GateError(f"{where}.id must be a vulnerability identifier such as CVE-2026-1234")
        if not REPOSITORY_RE.match(entry["repository"]):
            raise GateError(f"{where}.repository must be an untagged image repository")
        if entry["owner"].strip().lower() == entry["reviewer"].strip().lower():
            raise GateError(f"{where} must name a reviewer who is not the owner")
        if len(entry["reason"].strip()) < 20:
            raise GateError(f"{where}.reason must explain the containment decision")
        created = parse_date(entry["created"], f"{where}.created")
        expires = parse_date(entry["expires"], f"{where}.expires")
        if expires <= created:
            raise GateError(f"{where}.expires must be after created")
        if (expires - created).days > MAX_EXCEPTION_DAYS:
            raise GateError(f"{where} may not last longer than {MAX_EXCEPTION_DAYS} days")
        key = exception_key(entry["id"], entry["package"], entry["repository"])
        if key in seen:
            raise GateError(f"{where} duplicates an earlier exception")
        seen.add(key)
    return entries


def _severity(value: Any) -> str:
    text = str(value or "Unknown").strip().capitalize()
    return text if text in SEVERITIES else "Unknown"


def classify(finding: dict, repository: str, exceptions: list[dict], today: dt.date) -> tuple[str, str | None]:
    """Return the disposition of one finding and the exception key it used."""
    blocking = finding["severity"] in BLOCKING_SEVERITIES and finding["fix_state"] == "fixed"
    if not blocking:
        if finding["severity"] in BLOCKING_SEVERITIES:
            return "contain-no-fix", None
        return "below-threshold", None
    key = exception_key(finding["id"], finding["package"], repository)
    for entry in exceptions:
        if exception_key(entry["id"], entry["package"], entry["repository"]) != key:
            continue
        if parse_date(entry["expires"], "expires") < today:
            return "blocked-exception-expired", key
        return "excepted", key
    return "blocked", None


def compact_findings(report: dict) -> list[dict]:
    findings = []
    for match in report.get("matches") or []:
        vulnerability = match.get("vulnerability") or {}
        artifact = match.get("artifact") or {}
        fix = vulnerability.get("fix") or {}
        findings.append(
            {
                "id": str(vulnerability.get("id") or ""),
                "severity": _severity(vulnerability.get("severity")),
                "package": str(artifact.get("name") or ""),
                "version": str(artifact.get("version") or ""),
                "type": str(artifact.get("type") or ""),
                "purl": str(artifact.get("purl") or ""),
                "fix_state": str(fix.get("state") or "unknown"),
                "fix_versions": sorted(str(item) for item in fix.get("versions") or []),
                "namespace": str(vulnerability.get("namespace") or ""),
            }
        )
    findings.sort(key=lambda item: (SEVERITIES.index(item["severity"]), item["id"], item["package"], item["version"]))
    return findings


def apply_policy(findings: list[dict], repository: str, exceptions: list[dict], today: dt.date) -> dict:
    by_severity = {severity: 0 for severity in SEVERITIES}
    fixable = {severity: 0 for severity in SEVERITIES}
    dispositions: dict[str, int] = {}
    used_exceptions: set[str] = set()
    for finding in findings:
        disposition, key = classify(finding, repository, exceptions, today)
        finding["disposition"] = disposition
        if key:
            finding["exception"] = key
            if disposition == "excepted":
                used_exceptions.add(key)
        else:
            finding.pop("exception", None)
        by_severity[finding["severity"]] += 1
        if finding["fix_state"] == "fixed":
            fixable[finding["severity"]] += 1
        dispositions[disposition] = dispositions.get(disposition, 0) + 1
    blocking = dispositions.get("blocked", 0) + dispositions.get("blocked-exception-expired", 0)
    return {
        "decision": "approved" if blocking == 0 else "blocked",
        "summary": {
            "total": len(findings),
            "unique_vulnerabilities": len({finding["id"] for finding in findings}),
            "by_severity": by_severity,
            "fix_available_by_severity": fixable,
            "dispositions": dict(sorted(dispositions.items())),
            "blocking": blocking,
        },
        "exceptions_used": sorted(used_exceptions),
    }


def sbom_facts(sbom: dict) -> dict:
    creation = sbom.get("creationInfo") or {}
    return {
        "format": "spdx-json",
        "spdx_version": sbom.get("spdxVersion"),
        "canonical_sha256": canonical_sha256(sbom),
        "packages": len(sbom.get("packages") or []),
        "files": len(sbom.get("files") or []),
        "creators": creation.get("creators") or [],
        "created": creation.get("created"),
    }


def evaluate(report: dict, sbom: dict, exceptions_document: Any, image: str, revision: str, today: dt.date) -> dict:
    repository, digest = parse_image(image)
    if not REVISION_RE.match(revision or ""):
        raise GateError("source revision must be a full 40-character Git SHA")
    exceptions = validate_exceptions(exceptions_document)
    descriptor = report.get("descriptor") or {}
    status = ((descriptor.get("db") or {}).get("status")) or {}
    if descriptor.get("name") != "grype":
        raise GateError("vulnerability report was not produced by grype")
    if status.get("valid") is not True:
        raise GateError("grype did not use a valid vulnerability database")
    findings = compact_findings(report)
    outcome = apply_policy(findings, repository, exceptions, today)
    return {
        "schema_version": SCHEMA_VERSION,
        "image": image,
        "repository": repository,
        "digest": digest,
        "source_revision": revision,
        "evaluated_on": today.isoformat(),
        "policy": {
            "blocking": "Critical or High severity with a fix available",
            "exception_fields": ["id", "package", "repository", "owner", "reviewer", "reason", "created", "expires"],
            "exceptions_sha256": canonical_sha256(exceptions_document),
        },
        "scanner": {
            "name": "grype",
            "version": descriptor.get("version"),
            "db_schema": status.get("schemaVersion"),
            "db_built": status.get("built"),
            "db_source": str(status.get("from") or "").split("?", 1)[0],
            "distro": report.get("distro") or {},
        },
        "sbom": sbom_facts(sbom),
        "decision": outcome["decision"],
        "summary": outcome["summary"],
        "exceptions_used": outcome["exceptions_used"],
        "findings": findings,
    }


def reapply(predicate: dict, exceptions_document: Any, today: dt.date) -> dict:
    """Re-run the policy over attested findings with the current exceptions."""
    exceptions = validate_exceptions(exceptions_document)
    findings = [dict(finding) for finding in predicate.get("findings") or []]
    return apply_policy(findings, predicate["repository"], exceptions, today)


def blocking_findings(predicate: dict) -> list[dict]:
    return [
        {
            "id": finding["id"],
            "severity": finding["severity"],
            "package": finding["package"],
            "version": finding["version"],
            "type": finding["type"],
            "fix_versions": finding["fix_versions"],
            "disposition": finding["disposition"],
        }
        for finding in predicate.get("findings") or []
        if finding.get("disposition") in ("blocked", "blocked-exception-expired")
    ]


def canary_payload(created: str) -> dict:
    return {
        "purpose": "campus-signing-key-qualification",
        "secret": SIGNING_SECRET_REF,
        "created": created,
        "statement": "This payload carries no approval. It proves that the cluster signing Secret matches the committed public key.",
    }


def write_results(directory: str | None, values: dict[str, str]) -> None:
    if not directory:
        return
    for key, value in values.items():
        Path(directory, key).write_text(value, encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("evaluate", help="apply the vulnerability policy to a grype report")
    p.add_argument("--report", required=True)
    p.add_argument("--sbom", required=True)
    group = p.add_mutually_exclusive_group(required=True)
    group.add_argument("--exceptions")
    group.add_argument("--exceptions-env")
    p.add_argument("--image", required=True)
    p.add_argument("--revision", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--results-dir")
    p.add_argument("--today")

    p = sub.add_parser("enforce", help="exit 3 when an evaluated predicate is blocked")
    p.add_argument("--predicate", required=True)

    p = sub.add_parser("canary-payload", help="write the key qualification payload")
    p.add_argument("--created", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--results-dir")

    args = parser.parse_args(argv)
    try:
        if args.command == "evaluate":
            today = parse_date(args.today, "--today") if args.today else today_utc()
            if args.exceptions_env:
                exceptions_document = json.loads(os.environ[args.exceptions_env])
            else:
                exceptions_document = load_json(Path(args.exceptions))
            predicate = evaluate(
                load_json(Path(args.report)),
                load_json(Path(args.sbom)),
                exceptions_document,
                args.image,
                args.revision,
                today,
            )
            Path(args.output).write_bytes(canonical_json(predicate))
            write_results(
                args.results_dir,
                {
                    "SBOM_SHA256": predicate["sbom"]["canonical_sha256"],
                    "VULN_SHA256": canonical_sha256(predicate),
                    "DECISION": predicate["decision"],
                    "SUMMARY": json.dumps(predicate["summary"], separators=(",", ":"), sort_keys=True),
                },
            )
            print(json.dumps({"decision": predicate["decision"], "summary": predicate["summary"]}, indent=2))
            for finding in blocking_findings(predicate):
                fixes = ",".join(finding["fix_versions"])
                print(f"BLOCKING {finding['severity']} {finding['id']} {finding['package']}@{finding['version']} fix={fixes}")
        elif args.command == "enforce":
            predicate = load_json(Path(args.predicate))
            if predicate.get("decision") != "approved":
                blocking = predicate.get("summary", {}).get("blocking")
                print(f"blocked: {blocking} Critical/High findings have a fix and no valid exception", file=sys.stderr)
                return 3
            print("approved: no blocking findings")
        elif args.command == "canary-payload":
            payload = canonical_json(canary_payload(args.created))
            Path(args.output).write_bytes(payload)
            write_results(args.results_dir, {"PAYLOAD": payload.decode("utf-8")})
    except GateError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
