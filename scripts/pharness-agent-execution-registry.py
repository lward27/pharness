#!/usr/bin/env python3
"""Finalize or update the immutable Codex execution-policy registry."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path


def canonical_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def digest(value: str) -> str:
    if not value.startswith("sha256:") or len(value) != 71:
        raise argparse.ArgumentTypeError("image digests must be lowercase sha256 values")
    if any(character not in "0123456789abcdef" for character in value[7:]):
        raise argparse.ArgumentTypeError("image digests must be lowercase sha256 values")
    return value


def finalize(registry: dict[str, object]) -> dict[str, object]:
    for policy in registry["policies"]:
        material = copy.deepcopy(policy)
        material["policy_hash"] = ""
        policy["policy_hash"] = canonical_hash(material)
    material = copy.deepcopy(registry)
    material["config_hash"] = ""
    registry["config_hash"] = canonical_hash(material)
    return registry


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("registry", type=Path)
    parser.add_argument("--python-runner-digest", type=digest)
    parser.add_argument("--node-runner-digest", type=digest)
    parser.add_argument("--eval-runner-digest", type=digest)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    registry = json.loads(arguments.registry.read_text())
    original = copy.deepcopy(registry)
    repositories = {
        "python-3.11": "registry.lucas.engineering/pharness-python-runner",
        "node-24": "registry.lucas.engineering/pharness-node-runner",
        "evaluation": "registry.lucas.engineering/pharness-eval-runner",
    }
    updates = {
        "python-3.11": arguments.python_runner_digest,
        "node-24": arguments.node_runner_digest,
        "evaluation": arguments.eval_runner_digest,
    }
    for policy in registry["policies"]:
        for profile, image_digest in updates.items():
            if image_digest:
                policy["runner_images"][profile] = f"{repositories[profile]}@{image_digest}"
    finalized = finalize(registry)
    if arguments.check:
        if finalized != original:
            raise SystemExit("agent execution registry hashes or runner images are stale")
        return
    arguments.registry.write_text(json.dumps(finalized, indent=2) + "\n")


if __name__ == "__main__":
    main()
