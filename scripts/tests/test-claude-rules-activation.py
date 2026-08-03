#!/usr/bin/env python3
"""Regression-test bounded Claude rule activation costs."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from claude_rule_utils import (  # noqa: E402
    ALWAYS_ON_ALLOWLIST,
    GENERATED_PATH_EXEMPLARS,
    matches,
    parse_paths,
)

ROOT = SCRIPTS_DIR.parent
RULES_DIR = ROOT / ".claude" / "rules"


@dataclass(frozen=True)
class Scenario:
    name: str
    touched_paths: tuple[str, ...]
    ceiling_bytes: int


# Ceilings include directory-loaded CLAUDE.md files and retain roughly 5% headroom
# over the exact post-diet payload for each bounded touch set.
SCENARIOS = (
    Scenario("S6 rule editing", (".claude/rules/assets.md",), 17_000),
    Scenario(
        "S7 generated planning tracker",
        (next(iter(GENERATED_PATH_EXEMPLARS)),),
        24_000,
    ),
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
        paths = parse_paths(rule_path.read_text(encoding="utf-8"))
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
