"""Shared parsing and matching primitives for Claude rule automation."""

from __future__ import annotations

import ast
import re
from functools import lru_cache


ALWAYS_ON_ALLOWLIST = frozenset({"git-workflow.md"})
# Generated, gitignored scopes need explicit stable examples; never discover them by
# walking a developer's filesystem or local and CI validation will diverge.
GENERATED_PATH_EXEMPLARS = frozenset({".artifacts/specs/example/tracker.md"})


def parse_yaml_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        if value[0] == '"':
            return ast.literal_eval(value)
        return value[1:-1].replace("''", "'")
    return value


def parse_paths(text: str) -> tuple[str, ...] | None:
    lines = text.splitlines()
    if not lines or lines[0] != "---":
        return None

    try:
        closing_index = lines.index("---", 1)
    except ValueError:
        return None

    frontmatter = lines[1:closing_index]
    paths_index = next(
        (
            index
            for index, line in enumerate(frontmatter)
            if re.fullmatch(r"paths:\s*", line)
        ),
        None,
    )
    if paths_index is None:
        return None

    paths: list[str] = []
    for line in frontmatter[paths_index + 1 :]:
        match = re.fullmatch(r"\s+-\s+(.+?)\s*", line)
        if match:
            paths.append(parse_yaml_scalar(match.group(1)))
            continue
        if line and not line[0].isspace():
            break

    return tuple(paths)


@lru_cache(maxsize=None)
def expand_braces(pattern: str) -> tuple[str, ...]:
    start = pattern.find("{")
    if start == -1:
        return (pattern,)
    end = pattern.find("}", start + 1)
    if end == -1:
        return (pattern,)

    alternatives = pattern[start + 1 : end].split(",")
    return tuple(
        expanded
        for alternative in alternatives
        for expanded in expand_braces(pattern[:start] + alternative + pattern[end + 1 :])
    )


@lru_cache(maxsize=None)
def glob_regex(pattern: str) -> re.Pattern[str]:
    parts: list[str] = []
    index = 0
    while index < len(pattern):
        character = pattern[index]
        if character == "*" and index + 1 < len(pattern) and pattern[index + 1] == "*":
            index += 2
            if index < len(pattern) and pattern[index] == "/":
                parts.append(r"(?:.*/)?")
                index += 1
            else:
                parts.append(r".*")
            continue
        if character == "*":
            parts.append(r"[^/]*")
        elif character == "?":
            parts.append(r"[^/]")
        else:
            parts.append(re.escape(character))
        index += 1
    return re.compile("^" + "".join(parts) + "$")


def matches(pattern: str, candidate: str) -> bool:
    return any(glob_regex(expanded).match(candidate) for expanded in expand_braces(pattern))


def matching_paths(pattern: str, candidates: set[str]) -> list[str]:
    return [candidate for candidate in candidates if matches(pattern, candidate)]
