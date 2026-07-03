#!/usr/bin/env python3
"""Compare nextest test IDs across suite consolidation.

The script accepts JSON output from `cargo nextest list --message-format json`.
For consolidated suites, it normalizes IDs shaped like
`suite_name::old_binary::test_path` back to `old_binary::test_path`.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


DEFAULT_ALLOWED_ADDED = re.compile(r"^suite_[^:]+::merged_suite_requires_nextest$")


def load_json(path: Path) -> Any:
    text = path.read_text(encoding="utf-8")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        records = []
        for line in text.splitlines():
            line = line.strip()
            if not line:
                continue
            records.append(json.loads(line))
        return records


def binary_name(raw: str) -> str:
    raw = raw.rsplit("/", 1)[-1]
    raw = raw.rsplit("::", 1)[-1]
    return raw


def ids_from_nextest(data: Any) -> set[str]:
    ids: set[str] = set()

    if isinstance(data, dict) and isinstance(data.get("rust-suites"), dict):
        for suite in data["rust-suites"].values():
            if not isinstance(suite, dict):
                continue
            binary = binary_name(str(suite.get("binary-name") or suite.get("binary-id") or ""))
            testcases = suite.get("testcases")
            if not binary or not isinstance(testcases, dict):
                continue
            for test_name in testcases.keys():
                ids.add(f"{binary}::{test_name}")
        return ids

    if isinstance(data, dict) and isinstance(data.get("rust-tests"), dict):
        for binary_id, tests in data["rust-tests"].items():
            binary = binary_name(str(binary_id))
            if isinstance(tests, dict):
                for test_name in tests.keys():
                    ids.add(f"{binary}::{test_name}")
        return ids

    records = data if isinstance(data, list) else [data]
    for record in records:
        if not isinstance(record, dict):
            continue
        if record.get("type") != "test":
            continue
        test = record.get("test") or record
        if not isinstance(test, dict):
            continue
        test_name = (
            test.get("test_id")
            or test.get("name")
            or test.get("test_name")
            or test.get("id")
        )
        raw_binary = (
            test.get("binary_id")
            or test.get("binary")
            or test.get("suite")
            or test.get("target")
        )
        if test_name and raw_binary:
            ids.add(f"{binary_name(str(raw_binary))}::{test_name}")

    return ids


def normalize(test_id: str) -> str:
    binary, _, rest = test_id.partition("::")
    if binary.startswith("suite_") and "::" in rest:
        return rest
    return test_id


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument(
        "--allow-added-regex",
        action="append",
        default=[],
        help="Additional normalized added IDs to allow.",
    )
    args = parser.parse_args()

    before = {normalize(test_id) for test_id in ids_from_nextest(load_json(args.before))}
    after = {normalize(test_id) for test_id in ids_from_nextest(load_json(args.after))}

    allowed_added_patterns = [DEFAULT_ALLOWED_ADDED, *[re.compile(p) for p in args.allow_added_regex]]
    added = sorted(
        test_id
        for test_id in after - before
        if not any(pattern.search(test_id) for pattern in allowed_added_patterns)
    )
    removed = sorted(before - after)

    if added or removed:
        print("Test ID bijection check failed.", file=sys.stderr)
        if removed:
            print("Removed IDs:", file=sys.stderr)
            for test_id in removed[:200]:
                print(f"- {test_id}", file=sys.stderr)
            if len(removed) > 200:
                print(f"... {len(removed) - 200} more", file=sys.stderr)
        if added:
            print("Unexpected added IDs:", file=sys.stderr)
            for test_id in added[:200]:
                print(f"- {test_id}", file=sys.stderr)
            if len(added) > 200:
                print(f"... {len(added) - 200} more", file=sys.stderr)
        return 1

    print(f"Test ID bijection check passed for {len(before)} normalized IDs.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
