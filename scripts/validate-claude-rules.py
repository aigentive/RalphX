#!/usr/bin/env python3
"""Validate Claude rule loading boundaries without third-party dependencies."""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from claude_rule_utils import (
    ALWAYS_ON_ALLOWLIST,
    ALWAYS_ON_BUDGET_BYTES,
    GENERATED_PATH_EXEMPLARS,
    matching_paths,
    parse_paths,
)


ROOT = Path(__file__).resolve().parents[1]
RULES_DIR = ROOT / ".claude" / "rules"
ROOT_CLAUDE = ROOT / "CLAUDE.md"
MARKDOWN_IMPORT_RE = re.compile(
    r"(?:^|[\s(])@[A-Za-z0-9_~./-]+\.(?:md|mdc)", re.MULTILINE
)


@dataclass(frozen=True)
class Rule:
    relative_path: str
    bytes: int
    paths: tuple[str, ...] | None


def git_files(*arguments: str, root: Path = ROOT) -> set[str]:
    result = subprocess.run(
        ["git", "ls-files", *arguments],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    return {line for line in result.stdout.splitlines() if line}


def tracked_repository_files(root: Path = ROOT) -> set[str]:
    files = git_files("--cached", root=root)
    files.difference_update(git_files("--deleted", root=root))
    return files


def repository_files(root: Path = ROOT) -> set[str]:
    files = tracked_repository_files(root)
    files.update(GENERATED_PATH_EXEMPLARS)
    return files


def broadest_glob(paths: tuple[str, ...] | None, candidates: set[str]) -> str:
    if not paths:
        return "—"
    return max(paths, key=lambda pattern: len(matching_paths(pattern, candidates)))


def print_report(rules: list[Rule], candidates: set[str]) -> None:
    rows = [("CLAUDE.md", ROOT_CLAUDE.stat().st_size, "root", "—")]
    for rule in rules:
        classification = (
            "always-on"
            if Path(rule.relative_path).name in ALWAYS_ON_ALLOWLIST
            else "scoped" if rule.paths else "invalid"
        )
        rows.append(
            (
                rule.relative_path,
                rule.bytes,
                classification,
                broadest_glob(rule.paths, candidates),
            )
        )

    headers = ("file", "bytes", "class", "broadest glob")
    widths = [len(header) for header in headers]
    for row in rows:
        for index, value in enumerate(row):
            widths[index] = max(widths[index], len(str(value)))

    def format_row(row: tuple[object, ...]) -> str:
        return "| " + " | ".join(
            str(value).ljust(widths[index]) for index, value in enumerate(row)
        ) + " |"

    print(format_row(headers))
    print("| " + " | ".join("-" * width for width in widths) + " |")
    for row in rows:
        print(format_row(row))


def main() -> None:
    tracked_files = tracked_repository_files()
    candidates = tracked_files | GENERATED_PATH_EXEMPLARS
    rules: list[Rule] = []
    errors: list[str] = []

    for rule_path in sorted(RULES_DIR.glob("*.md")):
        relative_path = rule_path.relative_to(ROOT).as_posix()
        paths = parse_paths(rule_path.read_text(encoding="utf-8"))
        rule = Rule(relative_path, rule_path.stat().st_size, paths)
        rules.append(rule)
        name = rule_path.name

        if name in ALWAYS_ON_ALLOWLIST:
            if paths is not None:
                errors.append(f"{relative_path}: allowlisted rule must remain unscoped")
            continue
        if paths is None:
            errors.append(
                f"{relative_path}: must start with line-1 frontmatter containing paths:"
            )
            continue
        if not paths:
            errors.append(f"{relative_path}: paths: must contain at least one glob")
            continue
        for pattern in paths:
            if not matching_paths(pattern, candidates):
                errors.append(
                    f"{relative_path}: glob matches no tracked or generated path: {pattern}"
                )

    for allowlisted_name in ALWAYS_ON_ALLOWLIST:
        if not (RULES_DIR / allowlisted_name).is_file():
            errors.append(f"allowlisted rule is missing: .claude/rules/{allowlisted_name}")

    for claude_path in sorted(
        path for path in tracked_files if Path(path).name == "CLAUDE.md"
    ):
        text = (ROOT / claude_path).read_text(encoding="utf-8")
        if MARKDOWN_IMPORT_RE.search(text):
            errors.append(f"{claude_path}: contains a Markdown @ import")

    always_on_bytes = ROOT_CLAUDE.stat().st_size + sum(
        rule.bytes
        for rule in rules
        if Path(rule.relative_path).name in ALWAYS_ON_ALLOWLIST
    )
    if always_on_bytes > ALWAYS_ON_BUDGET_BYTES:
        errors.append(
            f"always-on bytes {always_on_bytes} exceed budget {ALWAYS_ON_BUDGET_BYTES}"
        )

    print_report(rules, candidates)
    print(f"always-on bytes: {always_on_bytes} / {ALWAYS_ON_BUDGET_BYTES}")
    if errors:
        print("Claude rule validation failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
