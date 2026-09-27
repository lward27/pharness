#!/usr/bin/env python3
import importlib.util
import hashlib
import io
import json
import tempfile
import tarfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "pharness-fetch-oci-bundle.py"
SPEC = importlib.util.spec_from_file_location("pharness_fetch_oci_bundle", SCRIPT)
FETCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FETCH)


class OciBundleExtractionTests(unittest.TestCase):
    def make_layer(self, root, entries):
        path = Path(root) / "layer.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name, kind, data in entries:
                member = tarfile.TarInfo(name)
                if kind == "dir":
                    member.type = tarfile.DIRTYPE
                    member.mode = 0o755
                    archive.addfile(member)
                elif kind == "file":
                    member.type = tarfile.REGTYPE
                    member.mode = 0o755
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))
                elif kind == "symlink":
                    member.type = tarfile.SYMTYPE
                    member.linkname = data.decode()
                    archive.addfile(member)
                elif kind == "hardlink":
                    member.type = tarfile.LNKTYPE
                    member.linkname = data.decode()
                    archive.addfile(member)
        return path

    def make_artifact(self, revision, archive_bytes=b"bundle archive"):
        archive_name = FETCH.archive_filename(revision)
        archive_sha256 = hashlib.sha256(archive_bytes).hexdigest()
        checksum = f"{archive_sha256}  {archive_name}\n".encode()
        return archive_name, archive_bytes, checksum, archive_sha256

    def test_extracts_cluster_packaged_archive_and_checksum(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        archive_name, archive_bytes, checksum, archive_sha256 = self.make_artifact(revision)
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                (archive_name, "file", archive_bytes),
                (archive_name + ".sha256", "file", checksum),
            ])
            output = Path(temp) / "out"
            archive_path, checksum_path, extracted_sha256 = FETCH.extract_archive_layer(
                archive, output, revision
            )
            self.assertEqual(archive_path.read_bytes(), archive_bytes)
            self.assertEqual(checksum_path.read_bytes(), checksum)
            self.assertEqual(extracted_sha256, archive_sha256)

    def test_rejects_path_traversal(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        archive_name, archive_bytes, _, _ = self.make_artifact(revision)
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                (archive_name, "file", archive_bytes),
                ("../outside", "file", b"bad"),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_archive_layer(archive, Path(temp) / "out", revision)
            self.assertFalse((Path(temp) / "outside").exists())

    def test_rejects_links_and_unexpected_artifacts(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        archive_name, archive_bytes, checksum, _ = self.make_artifact(revision)
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                (archive_name, "symlink", b"../../outside"),
                (archive_name + ".sha256", "file", checksum),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_archive_layer(archive, Path(temp) / "out", revision)

        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                (archive_name, "file", archive_bytes),
                (archive_name + ".sha256", "file", checksum),
                ("unexpected", "file", b"extra"),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_archive_layer(archive, Path(temp) / "out", revision)

    def test_rejects_bundle_archive_checksum_mismatch(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        archive_name, archive_bytes, _, _ = self.make_artifact(revision)
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                (archive_name, "file", archive_bytes),
                (archive_name + ".sha256", "file", b"0" * 64 + b"  " + archive_name.encode() + b"\n"),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_archive_layer(archive, Path(temp) / "out", revision)

    def test_registry_reference_is_pinned_and_scoped(self):
        digest = "sha256:" + "a" * 64
        self.assertEqual(FETCH.image_parts("registry.lucas.engineering/pharness-codex-host-bundle@" + digest),
                         ("pharness-codex-host-bundle", digest))
        for reference in [
            "registry.example.test/pharness@" + digest,
            "registry.lucas.engineering/../escape@" + digest,
            "registry.lucas.engineering/pharness:latest",
        ]:
            with self.assertRaises(ValueError):
                FETCH.image_parts(reference)

    def test_registry_requests_use_a_registry_accepted_user_agent(self):
        request = FETCH.registry_request("https://registry.lucas.engineering/v2/example/manifests/sha256:abc", "application/vnd.oci.image.manifest.v1+json")
        self.assertEqual(request.get_header("User-agent"), FETCH.USER_AGENT)
        self.assertEqual(request.get_header("Accept"), "application/vnd.oci.image.manifest.v1+json")

    def test_config_requires_matching_source_revision_and_platform(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        config = {
            "os": "linux",
            "architecture": "amd64",
            "config": {"Labels": {
                "org.opencontainers.image.source": "https://github.com/lward27/pharness",
                "org.opencontainers.image.revision": revision,
            }},
        }
        FETCH.verify_config(config, revision)
        with self.assertRaises(ValueError):
            FETCH.verify_config(config, "f" * 40)
        config["architecture"] = "arm64"
        with self.assertRaises(ValueError):
            FETCH.verify_config(config, revision)

    def test_fetch_verifies_and_extracts_an_immutable_oci_bundle(self):
        revision = "0123456789abcdef0123456789abcdef01234567"
        archive_name, archive_bytes, checksum, archive_sha256 = self.make_artifact(revision)
        with tempfile.TemporaryDirectory() as temp:
            layer_path = self.make_layer(temp, [
                (archive_name, "file", archive_bytes),
                (archive_name + ".sha256", "file", checksum),
            ])
            layer = layer_path.read_bytes()
            config = json.dumps({
                "os": "linux",
                "architecture": "amd64",
                "config": {"Labels": {
                    "org.opencontainers.image.source": "https://github.com/lward27/pharness",
                    "org.opencontainers.image.revision": revision,
                }},
            }).encode()
            config_digest = FETCH.digest_bytes(config)
            manifest = json.dumps({
                "schemaVersion": 2,
                "config": {"digest": config_digest},
                "layers": [{
                    "digest": FETCH.digest_bytes(layer),
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "size": len(layer),
                }],
            }).encode()
            manifest_digest = FETCH.digest_bytes(manifest)
            responses = {
                f"https://{FETCH.REGISTRY}/v2/pharness-codex-host-bundle/manifests/{manifest_digest}": manifest,
                f"https://{FETCH.REGISTRY}/v2/pharness-codex-host-bundle/blobs/{config_digest}": config,
                f"https://{FETCH.REGISTRY}/v2/pharness-codex-host-bundle/blobs/{FETCH.digest_bytes(layer)}": layer,
            }

            def fake_urlopen(request, timeout):
                return io.BytesIO(responses[request.full_url])

            output = Path(temp) / "out"
            reference = f"{FETCH.REGISTRY}/pharness-codex-host-bundle@{manifest_digest}"
            with patch.object(FETCH, "urlopen", side_effect=fake_urlopen):
                result = FETCH.fetch_bundle(reference, revision, output)

            self.assertEqual(result["image_digest"], manifest_digest)
            self.assertEqual(result["platform"], "linux/amd64")
            self.assertEqual(result["archive"], archive_name)
            self.assertEqual(result["sha256"], archive_sha256)
            self.assertEqual(Path(result["archive_path"]).read_bytes(), archive_bytes)
            self.assertEqual(Path(result["checksum_path"]).read_bytes(), checksum)
            self.assertEqual(sorted(path.name for path in output.iterdir()),
                             sorted([archive_name, archive_name + ".sha256"]))


if __name__ == "__main__":
    unittest.main()
