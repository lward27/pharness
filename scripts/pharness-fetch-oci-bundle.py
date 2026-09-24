#!/usr/bin/env python3
"""Fetch and safely extract the single-layer PHarness bundle OCI artifact."""
import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import tarfile
import tempfile
from pathlib import Path, PurePosixPath
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


REGISTRY = "registry.lucas.engineering"
MAX_METADATA_BYTES = 8 * 1024 * 1024
MAX_LAYER_BYTES = 2 * 1024 * 1024 * 1024
MAX_UNPACKED_BYTES = 3 * 1024 * 1024 * 1024
MANIFEST_ACCEPT = ", ".join((
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
))
ROOT_DIRECTORY = "pharness-codex-host"


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


def read_metadata(url, accept=None):
    headers = {"Accept": accept} if accept else {}
    request = Request(url, headers=headers)
    with urlopen(request, timeout=30) as response:
        payload = response.read(MAX_METADATA_BYTES + 1)
    if len(payload) > MAX_METADATA_BYTES:
        raise ValueError("OCI manifest/config exceeds the metadata size limit")
    return payload


def safe_parts(name):
    if not name or name.startswith("/") or "\\" in name:
        raise ValueError("OCI layer contains an absolute or invalid path")
    parts = tuple(part for part in PurePosixPath(name).parts if part not in ("", "."))
    if not parts or any(part == ".." for part in parts) or parts[0] != ROOT_DIRECTORY:
        raise ValueError("OCI layer contains a path outside the expected bundle root")
    return parts


def safe_link_target(parts, target, hardlink=False):
    if not target or target.startswith("/") or "\\" in target:
        raise ValueError("OCI layer contains an absolute or invalid link target")
    if hardlink:
        return safe_parts(target)
    resolved = list(parts[:-1])
    for part in PurePosixPath(target).parts:
        if part in ("", "."):
            continue
        if part == "..":
            if len(resolved) <= 1:
                raise ValueError("OCI symlink escapes the bundle root")
            resolved.pop()
        else:
            resolved.append(part)
    if not resolved or resolved[0] != ROOT_DIRECTORY:
        raise ValueError("OCI symlink escapes the bundle root")
    return tuple(resolved)


def extract_layer(layer_path, destination):
    destination = Path(destination).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        raise ValueError("OCI extraction destination must be empty")

    directories = []
    hardlinks = []
    symlinks = []
    extracted_bytes = 0
    with tarfile.open(layer_path, mode="r:gz") as archive:
        members = archive.getmembers()
        if len(members) > 20000:
            raise ValueError("OCI layer contains too many entries")
        for member in members:
            parts = safe_parts(member.name)
            target = destination.joinpath(*parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                directories.append((target, member.mode))
            elif member.isfile():
                if member.size < 0 or member.size > MAX_UNPACKED_BYTES - extracted_bytes:
                    raise ValueError("OCI layer exceeds the unpacked size limit")
                target.parent.mkdir(parents=True, exist_ok=True)
                if target.exists() or target.is_symlink():
                    raise ValueError("OCI layer contains duplicate file paths")
                source = archive.extractfile(member)
                if source is None:
                    raise ValueError("OCI layer file contents are unavailable")
                with source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, length=1024 * 1024)
                os.chmod(target, stat.S_IMODE(member.mode) & 0o777)
                extracted_bytes += member.size
            elif member.islnk():
                hardlinks.append((parts, safe_link_target(parts, member.linkname, hardlink=True)))
            elif member.issym():
                safe_link_target(parts, member.linkname)
                symlinks.append((parts, member.linkname))
            else:
                raise ValueError("OCI layer contains an unsupported special file")

    for parts, target_parts in hardlinks:
        destination_path = destination.joinpath(*parts)
        source_path = destination.joinpath(*target_parts)
        if destination_path.exists() or destination_path.is_symlink() or source_path.is_symlink() or not source_path.is_file():
            raise ValueError("OCI layer hardlink does not refer to a regular bundle file")
        destination_path.parent.mkdir(parents=True, exist_ok=True)
        os.link(source_path, destination_path)

    for parts, target in symlinks:
        destination_path = destination.joinpath(*parts)
        if destination_path.exists() or destination_path.is_symlink():
            raise ValueError("OCI layer contains duplicate link paths")
        destination_path.parent.mkdir(parents=True, exist_ok=True)
        os.symlink(target, destination_path)

    for directory, mode in reversed(directories):
        os.chmod(directory, stat.S_IMODE(mode) & 0o777)

    bundle = destination / ROOT_DIRECTORY
    if not (bundle / "REVISION").is_file() or not (bundle / "CHECKSUMS.sha256").is_file():
        raise ValueError("OCI layer is missing the verified PHarness bundle markers")
    return bundle


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
    descriptor, layer_path = tempfile.mkstemp(prefix=".pharness-oci-layer-", suffix=".tar.gz", dir=destination)
    digest = hashlib.sha256()
    total = 0
    try:
        with os.fdopen(descriptor, "wb") as output:
            request = Request(base + "blobs/" + quote(layer_digest, safe=":"))
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
        bundle = extract_layer(layer_path, destination)
    finally:
        try:
            os.unlink(layer_path)
        except FileNotFoundError:
            pass
    return {"bundle_dir": str(bundle), "image_digest": manifest_digest,
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
    except (HTTPError, URLError, OSError, ValueError, tarfile.TarError, json.JSONDecodeError) as error:
        print(json.dumps({"status": "failed", "reason": type(error).__name__}), file=__import__("sys").stderr)
        return 1
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
