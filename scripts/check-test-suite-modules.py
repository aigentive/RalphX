#!/usr/bin/env python3
"""Fail if a directory-backed integration suite omits a module file."""

from __future__ import annotations

import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
ROOTS = [
    REPO_ROOT / "src-tauri" / "tests",
    *(REPO_ROOT / "src-tauri" / "crates").glob("*/tests"),
]


def module_names(main_rs: Path) -> set[str]:
    content = main_rs.read_text(encoding="utf-8")
    return set(re.findall(r"(?m)^\s*(?:pub\s+)?mod\s+([a-zA-Z0-9_]+)\s*;", content))


def check_suite(suite_dir: Path) -> list[str]:
    errors: list[str] = []
    main_rs = suite_dir / "main.rs"
    if not main_rs.exists():
        return [f"{suite_dir.relative_to(REPO_ROOT)}: missing main.rs"]

    declared = module_names(main_rs)
    for test_file in sorted(suite_dir.glob("*.rs")):
        if test_file.name == "main.rs":
            continue
        expected = test_file.stem
        if expected not in declared:
            errors.append(
                f"{test_file.relative_to(REPO_ROOT)} is not declared in "
                f"{main_rs.relative_to(REPO_ROOT)}"
            )
    return errors


def main() -> int:
    errors: list[str] = []
    for root in ROOTS:
        if not root.exists():
            continue
        for suite_dir in sorted(root.glob("suite_*")):
            if suite_dir.is_dir():
                errors.extend(check_suite(suite_dir))

    if errors:
        print("Integration suite module check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print("Integration suite module check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
