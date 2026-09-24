#!/usr/bin/env python3
import importlib.util
import io
import tempfile
import tarfile
import unittest
from pathlib import Path


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


if __name__ == "__main__":
    unittest.main()
