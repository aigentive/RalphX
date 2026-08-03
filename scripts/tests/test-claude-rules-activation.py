#!/usr/bin/env python3
"""Regression-test bounded Claude rule activation costs."""

from __future__ import annotations

import ast
import re
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RULES_DIR = ROOT / ".claude" / "rules"
ALWAYS_ON_ALLOWLIST = {"git-workflow.md"}


@dataclass(frozen=True)
class Scenario:
    name: str
    touched_paths: tuple[str, ...]
    ceiling_bytes: int


# Ceilings include directory-loaded CLAUDE.md files and retain roughly 5% headroom
# over the exact post-diet payload for each bounded touch set.
SCENARIOS = (
    Scenario("S6 rule editing", (".claude/rules/assets.md",), 17_000),
    Scenario("S5 docs only", ("docs/features/plan-verification.md",), 28_500),
    Scenario(
        "S3 small frontend component",
        ("frontend/src/components/ui/Button.tsx",),
        52_000,
    ),
    Scenario(
        "S4 MCP server",
        ("plugins/app/ralphx-mcp-server/src/plan-tools.ts",),
        66_500,
    ),
    Scenario(
        "S1 backend state machine",
        (
            "src-tauri/src/domain/state_machine/transition_handler/merge_outcome_handler.rs",
            "src-tauri/src/application/task_transition_service.rs",
        ),
        96_500,
    ),
    Scenario(
        "S2 frontend chat UI",
        (
            "frontend/src/components/Chat/ChatMessageList.tsx",
            "frontend/src/hooks/useChatEvents.ts",
        ),
        73_000,
    ),
)


def parse_yaml_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        if value[0] == '"':
            return ast.literal_eval(value)
        return value[1:-1].replace("''", "'")
    return value


def parse_paths(path: Path) -> tuple[str, ...] | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "---":
        return None
    try:
        closing_index = lines.index("---", 1)
    except ValueError:
        return None

    frontmatter = lines[1:closing_index]
    paths_index = next(
        (index for index, line in enumerate(frontmatter) if re.fullmatch(r"paths:\s*", line)),
        None,
    )
    if paths_index is None:
        return None

    paths: list[str] = []
    for line in frontmatter[paths_index + 1 :]:
        match = re.fullmatch(r"\s+-\s+(.+?)\s*", line)
        if match:
            paths.append(parse_yaml_scalar(match.group(1)))
        elif line and not line[0].isspace():
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


def inherited_claude_documents(touched_path: str) -> set[Path]:
    document_paths = {ROOT / "CLAUDE.md"}
    parent = (ROOT / touched_path).parent
    while parent != ROOT:
        candidate = parent / "CLAUDE.md"
        if candidate.is_file():
            document_paths.add(candidate)
        parent = parent.parent
    return document_paths


def activated_documents(touched_paths: tuple[str, ...]) -> set[Path]:
    documents: set[Path] = set()
    rules = sorted(RULES_DIR.glob("*.md"))

    for touched_path in touched_paths:
        documents.update(inherited_claude_documents(touched_path))

    for rule_path in rules:
        if rule_path.name in ALWAYS_ON_ALLOWLIST:
            documents.add(rule_path)
            continue
        paths = parse_paths(rule_path)
        if paths and any(
            matches(pattern, touched_path)
            for pattern in paths
            for touched_path in touched_paths
        ):
            documents.add(rule_path)
    return documents


def main() -> None:
    failures: list[str] = []
    for scenario in SCENARIOS:
        documents = activated_documents(scenario.touched_paths)
        actual_bytes = sum(document.stat().st_size for document in documents)
        status = "PASS" if actual_bytes <= scenario.ceiling_bytes else "FAIL"
        print(
            f"{status} | {scenario.name} | {actual_bytes} / {scenario.ceiling_bytes} bytes"
        )
        if actual_bytes > scenario.ceiling_bytes:
            failures.append(
                f"{scenario.name}: {actual_bytes} exceeds {scenario.ceiling_bytes} bytes"
            )

    if failures:
        print("Claude rule activation regression failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
