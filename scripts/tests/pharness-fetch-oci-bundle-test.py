#!/usr/bin/env python3
import importlib.util
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

    def test_extracts_files_and_safe_links(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                ("pharness-codex-host", "dir", b""),
                ("pharness-codex-host/bin", "dir", b""),
                ("pharness-codex-host/bin/codex", "file", b"codex-binary"),
                ("pharness-codex-host/bin/codex-linux-sandbox", "hardlink", b"pharness-codex-host/bin/codex"),
                ("pharness-codex-host/CHECKSUMS.sha256", "file", b"verified\n"),
                ("pharness-codex-host/REVISION", "file", b"0" * 40 + b"\n"),
                ("pharness-codex-host/current", "symlink", b"bin/codex"),
            ])
            output = Path(temp) / "out"
            bundle = FETCH.extract_layer(archive, output)
            self.assertEqual((bundle / "bin/codex").read_bytes(), b"codex-binary")
            self.assertEqual((bundle / "bin/codex-linux-sandbox").read_bytes(), b"codex-binary")
            self.assertTrue((bundle / "current").is_symlink())

    def test_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                ("pharness-codex-host/REVISION", "file", b"0" * 40),
                ("../outside", "file", b"bad"),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_layer(archive, Path(temp) / "out")
            self.assertFalse((Path(temp) / "outside").exists())

    def test_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = self.make_layer(temp, [
                ("pharness-codex-host/REVISION", "file", b"0" * 40),
                ("pharness-codex-host/escape", "symlink", b"../../outside"),
            ])
            with self.assertRaises(ValueError):
                FETCH.extract_layer(archive, Path(temp) / "out")

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
        with tempfile.TemporaryDirectory() as temp:
            layer_path = self.make_layer(temp, [
                ("pharness-codex-host", "dir", b""),
                ("pharness-codex-host/CHECKSUMS.sha256", "file", b"verified\n"),
                ("pharness-codex-host/REVISION", "file", (revision + "\n").encode()),
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
            self.assertEqual((Path(result["bundle_dir"]) / "REVISION").read_text().strip(), revision)
            self.assertEqual([path.name for path in output.iterdir()], ["pharness-codex-host"])


if __name__ == "__main__":
    unittest.main()
