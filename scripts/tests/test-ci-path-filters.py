#!/usr/bin/env python3
"""Focused regressions for documentation-only CI path selection."""

from __future__ import annotations

import ast
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

from claude_rule_utils import matches  # noqa: E402


def load_path_filters(workflow: str) -> dict[str, tuple[str, ...]]:
    lines = (ROOT / workflow).read_text(encoding="utf-8").splitlines()
    filter_index = next(
        index for index, line in enumerate(lines) if line.strip() == "filters: |"
    )
    filter_indent = len(lines[filter_index]) - len(lines[filter_index].lstrip())
    groups: dict[str, list[str]] = {}
    current_group: str | None = None

    for line in lines[filter_index + 1 :]:
        stripped = line.strip()
        if not stripped:
            continue

        indent = len(line) - len(line.lstrip())
        if indent <= filter_indent:
            break
        if indent == filter_indent + 2 and stripped.endswith(":"):
            current_group = stripped[:-1]
            groups[current_group] = []
            continue
        if indent == filter_indent + 4 and stripped.startswith("- "):
            if current_group is None:
                raise AssertionError(f"path pattern without a filter group in {workflow}")
            groups[current_group].append(ast.literal_eval(stripped[2:]))
            continue
        raise AssertionError(f"unsupported path-filter line in {workflow}: {line}")

    return {name: tuple(patterns) for name, patterns in groups.items()}


def filter_matches(patterns: tuple[str, ...], changed_paths: tuple[str, ...]) -> bool:
    positive_patterns = tuple(pattern for pattern in patterns if not pattern.startswith("!"))
    negative_patterns = tuple(pattern[1:] for pattern in patterns if pattern.startswith("!"))
    return any(
        any(matches(pattern, path) for pattern in positive_patterns)
        and not any(matches(pattern, path) for pattern in negative_patterns)
        for path in changed_paths
    )


class DocumentationOnlyScopeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.ci = load_path_filters(".github/workflows/ci.yml")
        cls.coverage = load_path_filters(".github/workflows/coverage.yml")
        cls.codeql = load_path_filters(".github/workflows/codeql.yml")

    def assert_filter(
        self,
        groups: dict[str, tuple[str, ...]],
        group: str,
        changed_paths: tuple[str, ...],
        expected: bool,
    ) -> None:
        self.assertEqual(
            filter_matches(groups[group], changed_paths),
            expected,
            f"unexpected {group} scope for {changed_paths}",
        )

    def test_instruction_and_validator_changes_select_only_automation(self) -> None:
        changed_paths = (
            "CLAUDE.md",
            ".claude/rules/multi-harness.md",
            "frontend/CLAUDE.md",
            "frontend/src/CLAUDE.md",
            "src-tauri/CLAUDE.md",
            "scripts/claude_rule_utils.py",
            "scripts/validate-claude-rules.py",
            "scripts/tests/test-claude-rule-utils.py",
            "scripts/tests/test-claude-rules-activation.py",
        )

        self.assert_filter(self.ci, "automation", changed_paths, True)
        for group in ("plugins", "rust", "frontend", "visual", "tauri_alignment"):
            self.assert_filter(self.ci, group, changed_paths, False)
        for group in ("plugins", "rust", "frontend"):
            self.assert_filter(self.coverage, group, changed_paths, False)
        for group in ("actions", "javascript", "rust"):
            self.assert_filter(self.codeql, group, changed_paths, False)

    def test_current_workflow_changes_keep_actions_codeql_only(self) -> None:
        changed_paths = (
            ".github/workflows/ci.yml",
            ".github/workflows/coverage.yml",
            ".github/workflows/codeql.yml",
            "frontend/src/CLAUDE.md",
            "src-tauri/CLAUDE.md",
        )

        self.assert_filter(self.codeql, "actions", changed_paths, True)
        self.assert_filter(self.codeql, "javascript", changed_paths, False)
        self.assert_filter(self.codeql, "rust", changed_paths, False)

    def test_product_changes_still_select_their_lanes(self) -> None:
        self.assert_filter(self.ci, "frontend", ("frontend/src/App.tsx",), True)
        self.assert_filter(self.ci, "visual", ("frontend/src/App.tsx",), True)
        self.assert_filter(self.coverage, "frontend", ("frontend/src/App.tsx",), True)
        self.assert_filter(self.codeql, "javascript", ("frontend/src/App.tsx",), True)

        self.assert_filter(self.ci, "rust", ("src-tauri/src/lib.rs",), True)
        self.assert_filter(self.coverage, "rust", ("src-tauri/src/lib.rs",), True)
        self.assert_filter(self.codeql, "rust", ("src-tauri/src/lib.rs",), True)

        plugin_path = "plugins/app/ralphx-mcp-server/src/index.ts"
        self.assert_filter(self.ci, "plugins", (plugin_path,), True)
        self.assert_filter(self.coverage, "plugins", (plugin_path,), True)
        self.assert_filter(self.codeql, "javascript", (plugin_path,), True)

        self.assert_filter(
            self.ci,
            "tauri_alignment",
            ("frontend/package.json",),
            True,
        )

    def test_non_instruction_markdown_remains_product_relevant(self) -> None:
        plugin_doc = "plugins/app/ralphx-mcp-server/skills/review/SKILL.md"

        self.assert_filter(self.ci, "plugins", (plugin_doc,), True)
        self.assert_filter(self.coverage, "plugins", (plugin_doc,), True)
        self.assert_filter(self.codeql, "javascript", (plugin_doc,), True)


if __name__ == "__main__":
    unittest.main()
