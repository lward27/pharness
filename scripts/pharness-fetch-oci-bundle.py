#!/usr/bin/env python3
"""Fetch and verify the packaged PHarness bundle archive from its OCI artifact."""
import argparse
import hashlib
import json
import os
import re
import shutil
import tarfile
import tempfile
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


REGISTRY = "registry.lucas.engineering"
USER_AGENT = "pharness-oci-fetch/1.0"
MAX_METADATA_BYTES = 8 * 1024 * 1024
MAX_LAYER_BYTES = 2 * 1024 * 1024 * 1024
MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024
MANIFEST_ACCEPT = ", ".join((
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
))


def archive_filename(revision):
    return f"pharness-codex-host-{revision}-linux-amd64.tar.gz"


def image_parts(reference):
    match = re.fullmatch(
        r"registry\.lucas\.engineering/([A-Za-z0-9][A-Za-z0-9._/-]*)@(sha256:[0-9a-f]{64})",
        reference,
    )
    if not match:
        raise ValueError("expected an immutable digest in the PHarness registry")
    repository, digest = match.groups()
    if ".." in repository.split("/") or "//" in repository:
        raise ValueError("unsafe OCI repository path")
    return repository, digest


def digest_bytes(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def digest_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            chunk = source.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def verify_config(config, revision):
    if not isinstance(config, dict):
        raise ValueError("bundle OCI config must be an object")
    if (config.get("os"), config.get("architecture")) != ("linux", "amd64"):
        raise ValueError("bundle OCI artifact is not linux/amd64")
    image_config = config.get("config", {})
    if not isinstance(image_config, dict):
        raise ValueError("bundle OCI runtime config must be an object")
    labels = image_config.get("Labels", {})
    if not isinstance(labels, dict):
        raise ValueError("bundle OCI labels must be an object")
    if labels.get("org.opencontainers.image.revision") != revision:
        raise ValueError("bundle OCI revision label differs from the requested source revision")
    if labels.get("org.opencontainers.image.source") != "https://github.com/lward27/pharness":
        raise ValueError("bundle OCI source label is unexpected")


def registry_request(url, accept=None):
    headers = {"User-Agent": USER_AGENT}
    if accept:
        headers["Accept"] = accept
    return Request(url, headers=headers)


def read_metadata(url, accept=None):
    with urlopen(registry_request(url, accept), timeout=30) as response:
        payload = response.read(MAX_METADATA_BYTES + 1)
    if len(payload) > MAX_METADATA_BYTES:
        raise ValueError("OCI manifest/config exceeds the metadata size limit")
    return payload


def extract_archive_layer(layer_path, destination, revision):
    destination = Path(destination).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        raise ValueError("OCI artifact destination must be empty")
    archive_name = archive_filename(revision)
    checksum_name = archive_name + ".sha256"
    expected_names = {archive_name, checksum_name}
    extracted = set()
    with tarfile.open(layer_path, mode="r:gz") as archive:
        members = archive.getmembers()
        if len(members) != len(expected_names):
            raise ValueError("OCI bundle artifact must contain exactly the archive and checksum files")
        for member in members:
            if member.name not in expected_names or not member.isfile() or member.name in extracted:
                raise ValueError("OCI bundle artifact contains an unexpected or duplicate entry")
            size_limit = 256 if member.name == checksum_name else MAX_ARTIFACT_BYTES
            if member.size < 0 or member.size > size_limit:
                raise ValueError("OCI bundle artifact file exceeds the size limit")
            source = archive.extractfile(member)
            if source is None:
                raise ValueError("OCI bundle artifact file contents are unavailable")
            target = destination / member.name
            with source, target.open("xb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)
            extracted.add(member.name)

    if extracted != expected_names:
        raise ValueError("OCI bundle artifact is missing an expected file")
    archive_path = destination / archive_name
    checksum_path = destination / checksum_name
    archive_sha256 = digest_file(archive_path)
    expected_checksum = f"{archive_sha256}  {archive_name}\n".encode()
    if checksum_path.read_bytes() != expected_checksum:
        raise ValueError("cluster-built bundle archive does not match its checksum file")
    return archive_path, checksum_path, archive_sha256


def fetch_bundle(reference, revision, output_dir):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("expected a full lowercase source revision")
    repository, manifest_digest = image_parts(reference)
    base = f"https://{REGISTRY}/v2/{quote(repository, safe='/')}/"
    manifest_raw = read_metadata(base + "manifests/" + quote(manifest_digest, safe=":"), MANIFEST_ACCEPT)
    if digest_bytes(manifest_raw) != manifest_digest:
        raise ValueError("registry manifest digest differs from the requested image digest")
    manifest = json.loads(manifest_raw)
    if not isinstance(manifest, dict):
        raise ValueError("OCI manifest must be an object")
    if manifest.get("schemaVersion") != 2 or "manifests" in manifest:
        raise ValueError("expected a single-platform OCI/Docker manifest")
    layers = manifest.get("layers")
    config_descriptor = manifest.get("config", {})
    if not isinstance(layers, list) or len(layers) != 1 or not isinstance(layers[0], dict):
        raise ValueError("expected one bundle layer in the OCI artifact")
    if not isinstance(config_descriptor, dict):
        raise ValueError("OCI image config descriptor must be an object")
    layer = layers[0]
    layer_digest = layer.get("digest", "")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", layer_digest):
        raise ValueError("OCI bundle layer digest is invalid")
    if layer.get("mediaType") not in {
        "application/vnd.oci.image.layer.v1.tar+gzip",
        "application/vnd.docker.image.rootfs.diff.tar.gzip",
    }:
        raise ValueError("OCI bundle layer is not a supported gzip tar")

    config_digest = config_descriptor.get("digest", "")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", config_digest):
        raise ValueError("OCI image config digest is invalid")
    config_raw = read_metadata(base + "blobs/" + quote(config_digest, safe=":"))
    if digest_bytes(config_raw) != config_digest:
        raise ValueError("registry config digest mismatch")
    config = json.loads(config_raw)
    verify_config(config, revision)

    destination = Path(output_dir).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    descriptor_size = layer.get("size")
    if not isinstance(descriptor_size, int) or descriptor_size < 0 or descriptor_size > MAX_LAYER_BYTES:
        raise ValueError("OCI layer size is invalid or exceeds the download limit")
    descriptor, layer_path = tempfile.mkstemp(prefix=".pharness-oci-layer-", suffix=".tar.gz",
                                               dir=destination.parent)
    digest = hashlib.sha256()
    total = 0
    try:
        with os.fdopen(descriptor, "wb") as output:
            request = registry_request(base + "blobs/" + quote(layer_digest, safe=":"))
            with urlopen(request, timeout=60) as response:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > MAX_LAYER_BYTES:
                        raise ValueError("OCI layer exceeds the download size limit")
                    digest.update(chunk)
                    output.write(chunk)
        if total != descriptor_size or "sha256:" + digest.hexdigest() != layer_digest:
            raise ValueError("registry layer size or digest mismatch")
        archive_path, checksum_path, archive_sha256 = extract_archive_layer(
            layer_path, destination, revision
        )
    finally:
        try:
            os.unlink(layer_path)
        except FileNotFoundError:
            pass
    return {"archive": archive_path.name, "archive_path": str(archive_path),
            "checksum_path": str(checksum_path), "sha256": archive_sha256,
            "image_digest": manifest_digest,
            "config_digest": config_digest, "layer_digest": layer_digest,
            "layer_size_bytes": total, "platform": "linux/amd64"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, help="registry.lucas.engineering/repository@sha256:...")
    parser.add_argument("--revision", required=True, help="full source commit SHA expected in OCI labels")
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    try:
        result = fetch_bundle(args.image, args.revision, args.output_dir)
    except HTTPError as error:
        print(json.dumps({"status": "failed", "reason": "HTTPError", "http_status": error.code}),
              file=__import__("sys").stderr)
        return 1
    except ValueError as error:
        print(json.dumps({"status": "failed", "reason": "ValueError", "detail": str(error)}),
              file=__import__("sys").stderr)
        return 1
    except (URLError, OSError, tarfile.TarError) as error:
        print(json.dumps({"status": "failed", "reason": type(error).__name__}), file=__import__("sys").stderr)
        return 1
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
