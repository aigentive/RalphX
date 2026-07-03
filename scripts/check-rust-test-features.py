#!/usr/bin/env python3
"""Validate root integration tests that require the test-utils feature."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
MANIFEST = REPO_ROOT / "src-tauri" / "Cargo.toml"

EXPECTED_TEST_UTILS_TARGETS = {
    "activity_commands",
    "agent_profile_commands",
    "api_key_commands",
    "chat_service_context",
    "git_commands",
    "harness_provider_commands",
    "ideation_commands",
    "ideation_plan_delivery_test",
    "merge_system_hardening",
    "methodology_commands",
    "metrics_commands",
    "project_commands",
    "qa_commands",
    "question_commands",
    "release_notes_commands",
    "research_commands",
    "task_step_commands",
    "unified_chat_commands",
    "workflow_commands",
}


def metadata() -> dict:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            str(MANIFEST),
            "--no-deps",
        ],
        cwd=REPO_ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    )
    return json.loads(result.stdout)


def required_features(target: dict) -> list[str]:
    return target.get("required-features") or target.get("required_features") or []


def main() -> int:
    package = next(
        pkg for pkg in metadata()["packages"] if pkg["manifest_path"] == str(MANIFEST)
    )
    test_targets = {
        target["name"]: target for target in package["targets"] if "test" in target["kind"]
    }

    errors: list[str] = []
    for name in sorted(EXPECTED_TEST_UTILS_TARGETS):
        target = test_targets.get(name)
        expected_path = (MANIFEST.parent / "tests" / f"{name}.rs").resolve()
        if target is None:
            errors.append(f"missing [[test]] target: {name}")
            continue

        features = required_features(target)
        if features != ["test-utils"]:
            errors.append(
                f"{name}: expected required-features ['test-utils'], got {features}"
            )

        actual_path = Path(target["src_path"]).resolve()
        if actual_path != expected_path:
            errors.append(f"{name}: expected path {expected_path}, got {actual_path}")

    unexpected = sorted(
        name
        for name, target in test_targets.items()
        if required_features(target) == ["test-utils"]
        and name not in EXPECTED_TEST_UTILS_TARGETS
    )
    if unexpected:
        errors.append(
            "unexpected test-utils-gated test targets: " + ", ".join(unexpected)
        )

    if errors:
        print("Rust test feature gate validation failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print(
        f"Validated {len(EXPECTED_TEST_UTILS_TARGETS)} test-utils-gated root test targets."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
