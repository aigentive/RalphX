#!/usr/bin/env python3
"""Validate Claude rule loading boundaries without third-party dependencies."""

from __future__ import annotations

import ast
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RULES_DIR = ROOT / ".claude" / "rules"
ROOT_CLAUDE = ROOT / "CLAUDE.md"
ALWAYS_ON_ALLOWLIST = {"git-workflow.md"}
ALWAYS_ON_BUDGET_BYTES = 17_000
MARKDOWN_IMPORT_RE = re.compile(
    r"(?:^|[\s(])@[A-Za-z0-9_~./-]+\.(?:md|mdc)", re.MULTILINE
)


@dataclass(frozen=True)
class Rule:
    relative_path: str
    bytes: int
    paths: tuple[str, ...] | None


def git_files(*arguments: str) -> set[str]:
    result = subprocess.run(
        ["git", "ls-files", *arguments],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return {line for line in result.stdout.splitlines() if line}


def repository_files() -> set[str]:
    files = git_files("--cached", "--others", "--exclude-standard")
    files.difference_update(git_files("--deleted"))
    for parent, directory_names, file_names in os.walk(ROOT):
        directory_names[:] = [
            name
            for name in directory_names
            if name not in {".git", "dist", "node_modules", "target"}
        ]
        parent_path = Path(parent)
        files.update(
            (parent_path / name).relative_to(ROOT).as_posix() for name in file_names
        )
    return files


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
    candidates = repository_files()
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
                errors.append(f"{relative_path}: glob matches no tracked or real path: {pattern}")

    for allowlisted_name in ALWAYS_ON_ALLOWLIST:
        if not (RULES_DIR / allowlisted_name).is_file():
            errors.append(f"allowlisted rule is missing: .claude/rules/{allowlisted_name}")

    for claude_path in sorted(
        path for path in candidates if Path(path).name == "CLAUDE.md"
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
