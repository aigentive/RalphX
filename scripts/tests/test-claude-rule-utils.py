#!/usr/bin/env python3
"""Focused regressions for shared Claude rule matching and candidate discovery."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

import claude_rule_utils as rule_utils  # noqa: E402


def load_script(module_name: str, path: Path):
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


class SharedMatcherTests(unittest.TestCase):
    def test_both_gates_use_the_same_matching_objects(self) -> None:
        validator = load_script(
            "validate_claude_rules", SCRIPTS_DIR / "validate-claude-rules.py"
        )
        activation = load_script(
            "test_claude_rules_activation",
            SCRIPTS_DIR / "tests" / "test-claude-rules-activation.py",
        )

        for module in (validator, activation):
            self.assertIs(module.ALWAYS_ON_ALLOWLIST, rule_utils.ALWAYS_ON_ALLOWLIST)
            self.assertIs(
                module.GENERATED_PATH_EXEMPLARS,
                rule_utils.GENERATED_PATH_EXEMPLARS,
            )
            self.assertIs(module.parse_paths, rule_utils.parse_paths)
        self.assertIs(validator.matching_paths, rule_utils.matching_paths)
        self.assertIs(activation.matches, rule_utils.matches)

    def test_shared_matcher_handles_braces_and_recursive_globs(self) -> None:
        pattern = "frontend/src/**/*.{ts,tsx}"

        self.assertTrue(rule_utils.matches(pattern, "frontend/src/Button.tsx"))
        self.assertTrue(
            rule_utils.matches(pattern, "frontend/src/components/ui/Button.ts")
        )
        self.assertFalse(rule_utils.matches(pattern, "frontend/src/Button.css"))


class GitCandidateTests(unittest.TestCase):
    def test_validator_candidates_exclude_untracked_ignored_and_deleted_files(self) -> None:
        validator = load_script(
            "validate_claude_rules_candidates",
            SCRIPTS_DIR / "validate-claude-rules.py",
        )

        with tempfile.TemporaryDirectory() as temporary_directory:
            repository = Path(temporary_directory)
            subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
            (repository / ".gitignore").write_text("ignored/\n", encoding="utf-8")
            (repository / "docs").mkdir()
            (repository / "docs" / "CLAUDE.md").write_text("# tracked\n", encoding="utf-8")
            (repository / "deleted.md").write_text("delete me\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=repository, check=True)

            (repository / "deleted.md").unlink()
            (repository / "untracked.md").write_text("untracked\n", encoding="utf-8")
            (repository / "ignored").mkdir()
            (repository / "ignored" / "CLAUDE.md").write_text(
                "See @../../ignored.md\n", encoding="utf-8"
            )

            tracked = validator.tracked_repository_files(repository)
            candidates = validator.repository_files(repository)

        self.assertEqual(tracked, {".gitignore", "docs/CLAUDE.md"})
        self.assertEqual(candidates, tracked | rule_utils.GENERATED_PATH_EXEMPLARS)
        self.assertFalse(rule_utils.matching_paths("ignored/**", candidates))
        self.assertEqual(
            [path for path in tracked if Path(path).name == "CLAUDE.md"],
            ["docs/CLAUDE.md"],
        )


if __name__ == "__main__":
    unittest.main()
