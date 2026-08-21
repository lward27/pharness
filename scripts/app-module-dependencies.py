#!/usr/bin/env python3
"""Generate the pharness-api app module import graph and reject import cycles."""

from __future__ import annotations

import argparse
import re
from collections import defaultdict
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
APP_ROOT = REPO_ROOT / "crates/pharness-api/src/app"


def module_name(path: Path) -> str:
    parts = list(path.relative_to(APP_ROOT).parts)
    if parts[-1] == "mod.rs":
        parts.pop()
    else:
        parts[-1] = parts[-1][:-3]
    return "::".join(parts) or "<root>"


def split_top_level(value: str) -> list[str]:
    items: list[str] = []
    depth = 0
    start = 0
    for index, character in enumerate(value):
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
        elif character == "," and depth == 0:
            items.append(value[start:index])
            start = index + 1
    items.append(value[start:])
    return [item.strip() for item in items if item.strip()]


def expand_use_tree(value: str, prefix: tuple[str, ...] = ()) -> list[tuple[str, ...]]:
    value = value.strip()
    brace = value.find("{")
    if brace < 0:
        value = re.sub(r"\s+as\s+[_A-Za-z][_A-Za-z0-9]*$", "", value)
        parts = tuple(part for part in value.split("::") if part and part != "self")
        return [prefix + parts]
    head = value[:brace].rstrip().removesuffix("::")
    depth = 0
    close = None
    for index in range(brace, len(value)):
        if value[index] == "{":
            depth += 1
        elif value[index] == "}":
            depth -= 1
            if depth == 0:
                close = index
                break
    if close is None:
        raise ValueError(f"unbalanced use tree: {value}")
    head_parts = tuple(part for part in head.split("::") if part)
    expanded: list[tuple[str, ...]] = []
    for item in split_top_level(value[brace + 1 : close]):
        if item == "self":
            expanded.append(prefix + head_parts)
        else:
            expanded.extend(expand_use_tree(item, prefix + head_parts))
    return expanded


def resolve_import(
    current: str, imported: tuple[str, ...], known: set[str]
) -> str | None:
    current_parts = [] if current == "<root>" else current.split("::")
    imported_parts = list(imported)
    if imported_parts[:2] == ["crate", "app"]:
        base: list[str] = []
        imported_parts = imported_parts[2:]
    elif imported_parts[:1] == ["crate"]:
        return None
    else:
        base = current_parts
        while imported_parts[:1] == ["super"]:
            base = base[:-1]
            imported_parts.pop(0)
        if imported_parts[:1] == ["self"]:
            imported_parts.pop(0)
        elif imported and imported[0] not in {"super", "self"}:
            return None
    candidate = base + imported_parts
    for length in range(len(candidate), 0, -1):
        name = "::".join(candidate[:length])
        if name in known:
            return name
    return None


def graph() -> dict[str, set[str]]:
    modules = {module_name(path): path for path in APP_ROOT.rglob("*.rs")}
    known = set(modules)
    edges: dict[str, set[str]] = defaultdict(set)
    for current, path in modules.items():
        source = re.sub(r"//[^\n]*", "", path.read_text())
        for use_tree in re.findall(r"\buse\s+([^;]+);", source, flags=re.DOTALL):
            for imported in expand_use_tree(" ".join(use_tree.split())):
                target = resolve_import(current, imported, known)
                if target and target != current:
                    edges[current].add(target)
        edges.setdefault(current, set())
    return dict(edges)


def strongly_connected_components(edges: dict[str, set[str]]) -> list[list[str]]:
    index = 0
    indices: dict[str, int] = {}
    lowlinks: dict[str, int] = {}
    stack: list[str] = []
    on_stack: set[str] = set()
    components: list[list[str]] = []

    def visit(node: str) -> None:
        nonlocal index
        indices[node] = index
        lowlinks[node] = index
        index += 1
        stack.append(node)
        on_stack.add(node)
        for target in edges[node]:
            if target not in edges:
                continue
            if target not in indices:
                visit(target)
                lowlinks[node] = min(lowlinks[node], lowlinks[target])
            elif target in on_stack:
                lowlinks[node] = min(lowlinks[node], indices[target])
        if lowlinks[node] == indices[node]:
            component: list[str] = []
            while True:
                target = stack.pop()
                on_stack.remove(target)
                component.append(target)
                if target == node:
                    break
            components.append(component)

    for node in sorted(edges):
        if node not in indices:
            visit(node)
    return components


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--format", choices=("tsv", "mermaid"), default="tsv")
    args = parser.parse_args()
    edges = graph()
    bounded = {
        node: {target for target in targets if target != "<root>"}
        for node, targets in edges.items()
        if node != "<root>"
    }
    cycles = [
        sorted(component)
        for component in strongly_connected_components(bounded)
        if len(component) > 1
    ]
    if args.check:
        if cycles:
            for component in cycles:
                print("app module import cycle: " + " -> ".join(component))
            return 1
        return 0
    if args.format == "mermaid":
        print("flowchart LR")
        for source in sorted(bounded):
            for target in sorted(bounded[source]):
                print(f'  "{source}" --> "{target}"')
    else:
        print("source\ttarget")
        for source in sorted(bounded):
            for target in sorted(bounded[source]):
                print(f"{source}\t{target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
