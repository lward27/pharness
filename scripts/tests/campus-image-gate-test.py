#!/usr/bin/env python3
"""Unit tests for the Campus image gate, evidence contract and lab Pod controls."""

from __future__ import annotations

import copy
import datetime as dt
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
ROOT = SCRIPTS.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "campus"
sys.path.insert(0, str(SCRIPTS))

import campus_image_gate as gate  # noqa: E402
import campus_image_policy as policy  # noqa: E402

REVISION = "8f7338c792fc88d4b2b700b45b2161108b39e0bf"
PYTHON_IMAGE = "registry.lucas.engineering/pharness-python-runner@sha256:0dcdb9e74355d55c634e6adaf9643b4466557aa728130e752408d62baf2f9756"
NODE_IMAGE = "registry.lucas.engineering/pharness-node-runner@sha256:c478a31d341af1d1c7a13a99236120c870d0b88d1a9b80ac59159dd1a145a7bd"
REPOSITORY = PYTHON_IMAGE.split("@", 1)[0]
TODAY = dt.date(2026, 11, 4)


def load(name: str):
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def match(vulnerability: str, severity: str, package: str, state: str, versions=()):
    return {
        "vulnerability": {
            "id": vulnerability,
            "severity": severity,
            "namespace": "debian:distro:debian:13",
            "fix": {"state": state, "versions": list(versions)},
        },
        "artifact": {"name": package, "version": "1.0", "type": "deb", "purl": f"pkg:deb/debian/{package}@1.0"},
    }


def report(*matches):
    return {
        "descriptor": {
            "name": "grype",
            "version": "0.121.1",
            "db": {"status": {"schemaVersion": "v6.1.10", "built": "2026-11-04T06:20:59Z", "from": "https://grype.anchore.io/db?x=1", "valid": True}},
        },
        "distro": {"name": "debian", "version": "13.6"},
        "matches": list(matches),
    }


SBOM = {"spdxVersion": "SPDX-2.3", "packages": [{"name": "openssl"}], "files": [], "creationInfo": {"creators": ["Tool: syft-1.56.0"], "created": "2026-11-04T21:34:55Z"}}


def exception(**overrides):
    entry = {
        "id": "CVE-2026-0001",
        "package": "openssl",
        "repository": REPOSITORY,
        "owner": "Lucas Ward",
        "reviewer": "Campus Security Reviewer",
        "reason": "Only reachable through TLS server code that lab sessions never run.",
        "created": "2026-11-01",
        "expires": "2026-12-01",
    }
    entry.update(overrides)
    return entry


def exceptions(*entries):
    return {"schema_version": 1, "policy": "test", "exceptions": list(entries)}


class ReleaseRecordTests(unittest.TestCase):
    def setUp(self):
        self.build = load("release/BUILD.json")
        self.artifacts = load("release/ARTIFACTS.json")

    def test_release_digest_is_accepted(self):
        facts = gate.release_check(self.build, self.artifacts, PYTHON_IMAGE, REVISION)
        self.assertEqual(facts["component"], "python-runner")
        self.assertEqual(facts["pipeline_run"], "pharness-python-runner-8f7338c792fc")

    def test_tampered_digest_is_rejected(self):
        tampered = PYTHON_IMAGE[:-1] + "7"
        with self.assertRaisesRegex(gate.GateError, "not exactly one component"):
            gate.release_check(self.build, self.artifacts, tampered, REVISION)

    def test_digest_from_another_component_repository_is_rejected(self):
        swapped = REPOSITORY + "@" + NODE_IMAGE.split("@", 1)[1]
        with self.assertRaises(gate.GateError):
            gate.release_check(self.build, self.artifacts, swapped, REVISION)

    def test_mutable_references_are_rejected(self):
        for reference in (REPOSITORY + ":latest", REPOSITORY + ":git-" + REVISION, REPOSITORY + ":latest@" + PYTHON_IMAGE.split("@")[1]):
            with self.subTest(reference=reference), self.assertRaisesRegex(gate.GateError, "immutable image reference"):
                gate.release_check(self.build, self.artifacts, reference, REVISION)

    def test_other_revision_is_rejected(self):
        with self.assertRaisesRegex(gate.GateError, "different source revision"):
            gate.release_check(self.build, self.artifacts, PYTHON_IMAGE, "0" * 40)

    def test_artifact_record_must_agree(self):
        artifacts = copy.deepcopy(self.artifacts)
        artifacts["images"]["python-runner"]["digest"] = "sha256:" + "1" * 64
        with self.assertRaisesRegex(gate.GateError, "different digest"):
            gate.release_check(self.build, artifacts, PYTHON_IMAGE, REVISION)
        artifacts = copy.deepcopy(self.artifacts)
        artifacts["images"]["python-runner"]["source_verified"] = False
        with self.assertRaisesRegex(gate.GateError, "source-verified"):
            gate.release_check(self.build, artifacts, PYTHON_IMAGE, REVISION)

    def test_bundle_targets_are_not_lab_images(self):
        bundle = next(item["immutable_ref"] for item in self.build["components"] if item["build_target"] == "bundle")
        with self.assertRaises(gate.GateError):
            gate.release_check(self.build, self.artifacts, bundle, REVISION)


class VulnerabilityPolicyTests(unittest.TestCase):
    def evaluate(self, grype, exc=None):
        return policy.evaluate(grype, SBOM, exc or exceptions(), PYTHON_IMAGE, REVISION, TODAY)

    def test_critical_and_high_with_fix_block(self):
        result = self.evaluate(report(match("CVE-2026-0001", "Critical", "openssl", "fixed", ["3.5.7"]), match("CVE-2026-0002", "High", "libc6", "fixed", ["2.41"])))
        self.assertEqual(result["decision"], "blocked")
        self.assertEqual(result["summary"]["blocking"], 2)
        self.assertEqual(len(policy.blocking_findings(result)), 2)

    def test_findings_without_fix_or_below_threshold_do_not_block(self):
        result = self.evaluate(
            report(
                match("CVE-2026-0003", "High", "zlib", "not-fixed"),
                match("CVE-2026-0004", "Critical", "zlib", "wont-fix"),
                match("CVE-2026-0005", "Medium", "zlib", "fixed", ["1.3"]),
                match("CVE-2026-0006", "negligible", "zlib", "unknown"),
            )
        )
        self.assertEqual(result["decision"], "approved")
        self.assertEqual(result["summary"]["dispositions"], {"below-threshold": 2, "contain-no-fix": 2})
        self.assertEqual(result["summary"]["by_severity"]["Negligible"], 1)

    def test_named_unexpired_exception_covers_exact_finding(self):
        result = self.evaluate(report(match("CVE-2026-0001", "Critical", "openssl", "fixed", ["3.5.7"])), exceptions(exception()))
        self.assertEqual(result["decision"], "approved")
        self.assertEqual(result["exceptions_used"], [f"CVE-2026-0001|openssl|{REPOSITORY}"])
        self.assertEqual(result["findings"][0]["disposition"], "excepted")

    def test_expired_exception_blocks(self):
        result = self.evaluate(
            report(match("CVE-2026-0001", "Critical", "openssl", "fixed", ["3.5.7"])),
            exceptions(exception(created="2026-09-01", expires="2026-11-03")),
        )
        self.assertEqual(result["decision"], "blocked")
        self.assertEqual(result["findings"][0]["disposition"], "blocked-exception-expired")

    def test_exception_is_scoped_to_repository_and_package(self):
        for override in ({"repository": "registry.lucas.engineering/pharness-node-runner"}, {"package": "libssl3t64"}, {"id": "CVE-2026-0009"}):
            with self.subTest(override=override):
                result = self.evaluate(report(match("CVE-2026-0001", "Critical", "openssl", "fixed", ["3.5.7"])), exceptions(exception(**override)))
                self.assertEqual(result["decision"], "blocked")

    def test_malformed_exceptions_are_rejected(self):
        bad = [
            exception(reviewer="lucas ward"),
            exception(expires="2027-03-01"),
            exception(expires="2026-10-01"),
            exception(reason="short"),
            exception(id="not a cve"),
            exception(repository=PYTHON_IMAGE),
            {**exception(), "approved": True},
            {key: value for key, value in exception().items() if key != "owner"},
        ]
        for entry in bad:
            with self.subTest(entry=entry), self.assertRaises(policy.GateError):
                policy.validate_exceptions(exceptions(entry))
        with self.assertRaisesRegex(policy.GateError, "duplicates"):
            policy.validate_exceptions(exceptions(exception(), exception()))

    def test_invalid_scanner_database_is_rejected(self):
        grype = report()
        grype["descriptor"]["db"]["status"]["valid"] = False
        with self.assertRaisesRegex(policy.GateError, "valid vulnerability database"):
            self.evaluate(grype)

    def test_reapply_uses_current_exceptions(self):
        result = self.evaluate(report(match("CVE-2026-0001", "Critical", "openssl", "fixed", ["3.5.7"])), exceptions(exception()))
        later = policy.reapply(result, exceptions(exception()), dt.date(2026, 12, 2))
        self.assertEqual(later["decision"], "blocked")

    def test_enforce_exit_status(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "predicate.json")
            path.write_text(json.dumps({"decision": "blocked", "summary": {"blocking": 1}}))
            self.assertEqual(policy.main(["enforce", "--predicate", str(path)]), 3)
            path.write_text(json.dumps({"decision": "approved", "summary": {"blocking": 0}}))
            self.assertEqual(policy.main(["enforce", "--predicate", str(path)]), 0)


class CanonicalDigestTests(unittest.TestCase):
    def test_digest_survives_reencoding(self):
        original = {"b": [1, 2.0, "<&>"], "a": {"score": 7.5, "unicode": "\u00e9"}}
        reencoded = json.loads(json.dumps(original, ensure_ascii=True, indent=4).replace("2.0", "2"))
        self.assertEqual(policy.canonical_sha256(original), policy.canonical_sha256(reencoded))

    def test_digest_detects_content_change(self):
        self.assertNotEqual(policy.canonical_sha256({"a": 1}), policy.canonical_sha256({"a": 2}))


class AdmissionRenderingTests(unittest.TestCase):
    def evidence(self, *entries):
        return {"schema_version": 1, "images": list(entries)}

    def test_only_approved_digests_are_allowlisted(self):
        rendered = gate.render_policy(
            self.evidence({"image": PYTHON_IMAGE, "status": "approved"}, {"image": NODE_IMAGE, "status": "blocked"})
        )
        self.assertIn(f"['{PYTHON_IMAGE}']", rendered)
        self.assertNotIn(NODE_IMAGE, rendered)
        self.assertIn('"pods/ephemeralcontainers"', rendered)
        self.assertIn('"jobs"', rendered)
        self.assertIn('failurePolicy: "Fail"', rendered)
        self.assertIn('pharness.dev/campus-lab: "true"', rendered)

    def test_empty_allowlist_denies_everything(self):
        rendered = gate.render_policy(self.evidence())
        self.assertIn('expression: "[]"', rendered)

    def test_rendering_is_deterministic(self):
        evidence = self.evidence({"image": NODE_IMAGE, "status": "approved"}, {"image": PYTHON_IMAGE, "status": "approved"})
        reordered = self.evidence(*reversed(evidence["images"]))
        self.assertEqual(gate.render_policy(evidence), gate.render_policy(reordered))

    def test_invalid_approved_image_cannot_render(self):
        with self.assertRaises(gate.GateError):
            gate.render_policy(self.evidence({"image": REPOSITORY + ":latest", "status": "approved"}))

    def test_policy_regex_matches_python_regex(self):
        import re

        pattern = re.compile(gate.IMAGE_PATTERN)
        self.assertTrue(pattern.match(PYTHON_IMAGE))
        for bad in (REPOSITORY + ":latest", PYTHON_IMAGE.upper(), "python@sha256:" + "a" * 64, PYTHON_IMAGE + "0", REPOSITORY + ":t@sha256:" + "a" * 64):
            self.assertIsNone(pattern.match(bad), bad)


class TaskRunTests(unittest.TestCase):
    def render(self, exceptions_text=None):
        text = exceptions_text or (ROOT / "deploy/campus/vulnerability-exceptions.json").read_text()
        return gate.render_taskrun("campus-approve-test", PYTHON_IMAGE, REVISION, "python-runner", text, gate.POLICY_SOURCE.read_text())

    def test_signing_only_follows_attestation_and_enforcement(self):
        steps = [step["name"] for step in self.render()["spec"]["taskSpec"]["steps"]]
        self.assertEqual(steps, ["sbom", "scan", "evaluate", "attest-sbom", "attest-vulnerabilities", "enforce", "sign"])

    def test_no_secret_values_are_inlined(self):
        taskrun = self.render()
        for step in taskrun["spec"]["taskSpec"]["steps"]:
            for env in step.get("env", []):
                if env["name"] == "COSIGN_PASSWORD":
                    self.assertEqual(set(env), {"name", "valueFrom"})
        self.assertNotIn("PRIVATE KEY", json.dumps(taskrun))

    def test_every_tool_image_is_digest_pinned(self):
        for step in self.render()["spec"]["taskSpec"]["steps"]:
            self.assertRegex(step["image"], r"@sha256:[0-9a-f]{64}$")
            self.assertTrue(step["securityContext"]["runAsNonRoot"])
            self.assertTrue(step["securityContext"]["readOnlyRootFilesystem"])

    def test_sign_identity_is_public_repository(self):
        sign = self.render()["spec"]["taskSpec"]["steps"][-1]
        self.assertIn(REPOSITORY, sign["args"])
        self.assertTrue(sign["args"][-1].startswith(gate.PUSH_REGISTRY + "/pharness-python-runner@sha256:"))
        self.assertIn(f"dev.pharness.campus/source-revision={REVISION}", sign["args"])

    def test_embedded_script_is_safe_for_tekton(self):
        source = gate.POLICY_SOURCE.read_text()
        self.assertNotIn(gate.TEKTON_REFERENCE, source)
        self.assertLess(len(source.encode()), 48 * 1024, "the embedded policy script must stay well under the 128 KiB argument limit")
        with self.assertRaises(gate.GateError):
            self.render(json.dumps({"schema_version": 1, "exceptions": [], "policy": "$" + "(params.x)"}))

    def test_outcome_interpretation(self):
        def taskrun(status, codes, decision):
            names = ["sbom", "scan", "evaluate", "attest-sbom", "attest-vulnerabilities", "enforce", "sign"]
            return {
                "metadata": {"name": "x"},
                "status": {
                    "conditions": [{"type": "Succeeded", "status": status, "message": "m"}],
                    "results": [{"name": "DECISION", "value": decision}],
                    "steps": [{"name": name, "terminated": {"exitCode": code}} for name, code in zip(names, codes)],
                },
            }

        self.assertEqual(gate.taskrun_outcome(taskrun("True", [0] * 7, "approved"))["outcome"], "approved")
        self.assertEqual(gate.taskrun_outcome(taskrun("False", [0, 0, 0, 0, 0, 3], "blocked"))["outcome"], "blocked")
        with self.assertRaises(gate.GateError):
            gate.taskrun_outcome(taskrun("False", [0, 1], "blocked"))
        with self.assertRaises(gate.GateError):
            gate.taskrun_outcome(taskrun("True", [0, 0, 0, 0, 0, 0, 1], "approved"))
        with self.assertRaises(gate.GateError):
            gate.taskrun_outcome(taskrun("False", [0, 0, 0, 0, 0, 3], "approved"))


class EvidenceContractTests(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp(prefix="campus-evidence-test."))
        shutil.copytree(ROOT / "deploy" / "campus", self.temp / "deploy" / "campus")
        self.campus = self.temp / "deploy" / "campus"

    def tearDown(self):
        shutil.rmtree(self.temp)

    def evidence(self):
        return json.loads((self.campus / "approved-images.json").read_text())

    def save(self, evidence, render=True):
        (self.campus / "approved-images.json").write_text(json.dumps(evidence, indent=2) + "\n")
        if render:
            (self.campus / "admission" / "campus-lab-image-allowlist.yaml").write_text(gate.render_policy(evidence))

    def approved_entry(self, used=()):
        repository, digest = gate.parse_image(PYTHON_IMAGE)
        return {
            "image": PYTHON_IMAGE,
            "repository": repository,
            "digest": digest,
            "status": "approved",
            "source_revision": REVISION,
            "sbom": {"canonical_sha256": "sha256:" + "a" * 64},
            "vulnerability_scan": {
                "canonical_sha256": "sha256:" + "b" * 64,
                "decision": "approved",
                "summary": {"blocking": 0},
                "blocking_findings": [],
                "exceptions_used": list(used),
            },
            "attestations": {"reference": gate.sidecar_reference(PYTHON_IMAGE, "att")},
            "signature": {"reference": gate.sidecar_reference(PYTHON_IMAGE, "sig"), "docker_reference": repository},
        }

    def errors(self, today=TODAY):
        return gate.check_repository(self.temp, today)

    def test_committed_evidence_is_consistent(self):
        self.assertEqual(self.errors(), [])

    def test_admission_manifest_drift_is_rejected(self):
        manifest = self.campus / "admission" / "campus-lab-image-allowlist.yaml"
        original = manifest.read_text()
        manifest.write_text(original.replace('- name: "allowedImages"\n      expression: "[', f"- name: \"allowedImages\"\n      expression: \"['{NODE_IMAGE}', ", 1))
        self.assertNotEqual(manifest.read_text(), original)
        self.assertTrue(any("not the policy rendered" in error for error in self.errors()))

    def test_valid_approved_entry_passes_and_is_allowlisted(self):
        evidence = self.evidence()
        evidence["images"] = [entry for entry in evidence["images"] if entry["image"] != PYTHON_IMAGE] + [self.approved_entry()]
        self.save(evidence)
        self.assertEqual(self.errors(), [])
        self.assertIn(PYTHON_IMAGE, gate.allowlist(evidence))

    def test_approved_entry_without_signature_is_rejected(self):
        evidence = self.evidence()
        entry = self.approved_entry()
        entry["signature"] = None
        evidence["images"] = [item for item in evidence["images"] if item["image"] != PYTHON_IMAGE] + [entry]
        self.save(evidence)
        self.assertTrue(any("signature reference" in error for error in self.errors()))

    def test_approved_entry_with_blocking_findings_is_rejected(self):
        evidence = self.evidence()
        entry = self.approved_entry()
        entry["vulnerability_scan"]["blocking_findings"] = [{"id": "CVE-2026-0001"}]
        entry["vulnerability_scan"]["summary"]["blocking"] = 1
        evidence["images"] = [item for item in evidence["images"] if item["image"] != PYTHON_IMAGE] + [entry]
        self.save(evidence)
        self.assertTrue(any("no blocking findings" in error for error in self.errors()))

    def test_expired_exception_revokes_reliance(self):
        key = f"CVE-2026-0001|openssl|{REPOSITORY}"
        (self.campus / "vulnerability-exceptions.json").write_text(json.dumps(exceptions(exception())))
        evidence = self.evidence()
        evidence["images"] = [item for item in evidence["images"] if item["image"] != PYTHON_IMAGE] + [self.approved_entry([key])]
        self.save(evidence)
        self.assertEqual(self.errors(dt.date(2026, 11, 30)), [])
        self.assertTrue(any("expired" in error for error in self.errors(dt.date(2026, 12, 2))))

    def test_missing_exception_is_rejected(self):
        evidence = self.evidence()
        evidence["images"] = [item for item in evidence["images"] if item["image"] != PYTHON_IMAGE] + [self.approved_entry(["CVE-2026-9|x|" + REPOSITORY])]
        self.save(evidence)
        self.assertTrue(any("not in vulnerability-exceptions.json" in error for error in self.errors()))

    def test_blocked_entry_with_signature_is_rejected(self):
        evidence = self.evidence()
        blocked = [item for item in evidence["images"] if item["status"] == "blocked"]
        if not blocked:
            self.skipTest("no blocked evidence recorded")
        blocked[0]["signature"] = {"reference": "x"}
        self.save(evidence)
        self.assertTrue(any("must not record an approval signature" in error for error in self.errors()))

    def test_tampered_blocked_digest_is_rejected(self):
        evidence = self.evidence()
        if not evidence["images"]:
            self.skipTest("no evidence recorded")
        evidence["images"][0]["image"] = evidence["images"][0]["image"][:-1] + ("0" if evidence["images"][0]["image"][-1] != "0" else "1")
        self.save(evidence)
        self.assertTrue(any("disagree with the image reference" in error for error in self.errors()))

    def test_replaced_public_key_is_rejected(self):
        (self.campus / "cosign.pub").write_text(
            "-----BEGIN PUBLIC KEY-----\n"
            "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n"
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==\n"
            "-----END PUBLIC KEY-----\n"
        )
        self.assertTrue(any("does not match signing.public_key_sha256" in error for error in self.errors()))

    def test_tampered_key_qualification_payload_is_rejected(self):
        evidence = self.evidence()
        if not evidence.get("key_qualification"):
            self.skipTest("no key qualification recorded")
        payload = self.temp / evidence["key_qualification"][0]["payload"]
        payload.write_text(payload.read_text().replace("no approval", "an approval"))
        self.assertTrue(any("payload digest" in error for error in self.errors()))


class LabPodTests(unittest.TestCase):
    def setUp(self):
        self.sources = gate.load_package_sources(ROOT / "deploy/campus/package-sources.json")
        self.pod = gate.render_lab_pod("session-a", "campus-lab-spike", PYTHON_IMAGE, "session-a", self.sources)

    def errors(self, pod):
        return gate.check_lab_pod(pod, self.sources)

    def test_rendered_pod_passes(self):
        self.assertEqual(self.errors(self.pod), [])
        spec = self.pod["spec"]
        self.assertEqual(spec["runtimeClassName"], "gvisor")
        self.assertFalse(spec["automountServiceAccountToken"])
        self.assertTrue(all("emptyDir" in volume for volume in spec["volumes"]))

    def test_shared_host_cache_is_rejected(self):
        pod = copy.deepcopy(self.pod)
        pod["spec"]["volumes"][1] = {"name": "package-cache", "hostPath": {"path": "/var/cache/pip"}}
        self.assertTrue(any("hostPath" in error for error in self.errors(pod)))

    def test_shared_pvc_cache_is_rejected(self):
        pod = copy.deepcopy(self.pod)
        pod["spec"]["volumes"][1] = {"name": "package-cache", "persistentVolumeClaim": {"claimName": "shared-pip-cache"}}
        errors = self.errors(pod)
        self.assertTrue(any("persistentVolumeClaim" in error for error in errors))
        self.assertTrue(any("not a per-Pod emptyDir" in error for error in errors))

    def test_cache_outside_session_volume_is_rejected(self):
        pod = copy.deepcopy(self.pod)
        for env in pod["spec"]["containers"][0]["env"]:
            if env["name"] == "PIP_CACHE_DIR":
                env["value"] = "/opt/shared-pip-cache"
        self.assertTrue(any("PIP_CACHE_DIR" in error for error in self.errors(pod)))

    def test_unpinned_or_redirected_index_is_rejected(self):
        cases = [
            ("PIP_INDEX_URL", "https://evil.example/simple"),
            ("NPM_CONFIG_REGISTRY", "https://evil.example/"),
            ("PIP_EXTRA_INDEX_URL", "https://evil.example/simple"),
            ("PIP_CONFIG_FILE", "/workspace/pip.conf"),
        ]
        for name, value in cases:
            with self.subTest(name=name):
                pod = copy.deepcopy(self.pod)
                env = pod["spec"]["containers"][0]["env"]
                env[:] = [item for item in env if item["name"] != name] + [{"name": name, "value": value}]
                self.assertTrue(self.errors(pod), name)

    def test_missing_index_pin_is_rejected(self):
        pod = copy.deepcopy(self.pod)
        pod["spec"]["containers"][0]["env"] = [item for item in pod["spec"]["containers"][0]["env"] if item["name"] != "NPM_CONFIG_REGISTRY"]
        self.assertTrue(any("NPM_CONFIG_REGISTRY" in error for error in self.errors(pod)))

    def test_sessions_never_share_a_volume(self):
        other = gate.render_lab_pod("session-b", "campus-lab-spike", PYTHON_IMAGE, "session-b", self.sources)
        self.assertNotEqual(self.pod["metadata"]["labels"], other["metadata"]["labels"])
        for volume in self.pod["spec"]["volumes"] + other["spec"]["volumes"]:
            self.assertEqual(set(volume), {"name", "emptyDir"})


if __name__ == "__main__":
    unittest.main(verbosity=1)
