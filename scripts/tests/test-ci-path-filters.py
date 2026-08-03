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


PathRule = tuple[str, ...]
PathFilters = dict[str, tuple[PathRule, ...]]


def load_path_filters(workflow: str) -> PathFilters:
    lines = (ROOT / workflow).read_text(encoding="utf-8").splitlines()
    quantifier = next(
        (
            line.split(":", 1)[1].strip().strip("'\"")
            for line in lines
            if line.strip().startswith("predicate-quantifier:")
        ),
        "some",
    )
    if quantifier != "every":
        raise AssertionError(
            f"{workflow} must use predicate-quantifier: every "
            "when filters contain negated patterns"
        )
    filter_index = next(
        index for index, line in enumerate(lines) if line.strip() == "filters: |"
    )
    filter_indent = len(lines[filter_index]) - len(lines[filter_index].lstrip())
    groups: dict[str, list[PathRule]] = {}
    current_group: str | None = None
    current_rule_index: int | None = None

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
            current_rule_index = None
            continue
        if indent == filter_indent + 4 and stripped.startswith("- "):
            if current_group is None:
                raise AssertionError(f"path pattern without a filter group in {workflow}")
            rule = stripped[2:]
            if rule == "added|modified|deleted:":
                current_rule_index = len(groups[current_group])
                groups[current_group].append(())
            else:
                current_rule_index = None
                groups[current_group].append((ast.literal_eval(rule),))
            continue
        if indent == filter_indent + 8 and stripped.startswith("- "):
            if current_group is None or current_rule_index is None:
                raise AssertionError(f"path alternative without a rule in {workflow}")
            groups[current_group][current_rule_index] += (
                ast.literal_eval(stripped[2:]),
            )
            continue
        raise AssertionError(f"unsupported path-filter line in {workflow}: {line}")

    return {name: tuple(rules) for name, rules in groups.items()}


def rule_matches(rule: PathRule, path: str) -> bool:
    if len(rule) == 1 and rule[0].startswith("!"):
        return not matches(rule[0][1:], path)
    return any(matches(pattern, path) for pattern in rule)


def filter_matches(rules: tuple[PathRule, ...], changed_paths: tuple[str, ...]) -> bool:
    return any(
        all(rule_matches(rule, path) for rule in rules)
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
        groups: PathFilters,
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
