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
    ALWAYS_ON_BUDGET_BYTES,
    GENERATED_PATH_EXEMPLARS,
    matches,
    parse_paths,
)

ROOT = SCRIPTS_DIR.parent
RULES_DIR = ROOT / ".claude" / "rules"

# Ceilings are derived, never hand-picked: each scenario records the exact payload
# measured at calibration time and the ceiling is that baseline plus this headroom.
# To recalibrate after an intentional payload change, copy the printed actual into
# baseline_bytes; hand-editing a ceiling reintroduces the drift this ratio prevents.
HEADROOM_RATIO = 0.05


@dataclass(frozen=True)
class Scenario:
    name: str
    touched_paths: tuple[str, ...]
    baseline_bytes: int
    # S6 alone pins to the always-on budget instead of its own baseline: its
    # activation set is exactly the always-on payload the validator already gates.
    pinned_ceiling_bytes: int | None = None

    @property
    def ceiling_bytes(self) -> int:
        if self.pinned_ceiling_bytes is not None:
            return self.pinned_ceiling_bytes
        return int(self.baseline_bytes * (1 + HEADROOM_RATIO))


# Baselines include directory-loaded CLAUDE.md files and were measured on the exact
# payload for each bounded touch set.
SCENARIOS = (
    Scenario(
        "S6 rule editing",
        (".claude/rules/assets.md",),
        16_228,
        pinned_ceiling_bytes=ALWAYS_ON_BUDGET_BYTES,
    ),
    Scenario(
        "S7 generated planning tracker",
        (next(iter(GENERATED_PATH_EXEMPLARS)),),
        22_631,
    ),
    Scenario("S5 docs only", ("docs/features/plan-verification.md",), 26_779),
    Scenario(
        "S3 small frontend component",
        ("frontend/src/components/ui/Button.tsx",),
        50_367,
    ),
    Scenario(
        "S4 MCP server",
        ("plugins/app/ralphx-mcp-server/src/plan-tools.ts",),
        64_073,
    ),
    Scenario(
        "S1 backend state machine",
        (
            "src-tauri/src/domain/state_machine/transition_handler/merge_outcome_handler.rs",
            "src-tauri/src/application/task_transition_service.rs",
        ),
        93_841,
    ),
    Scenario(
        "S2 frontend chat UI",
        (
            "frontend/src/components/Chat/ChatMessageList.tsx",
            "frontend/src/hooks/useChatEvents.ts",
        ),
        72_979,
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
    stale_baselines: list[str] = []
    for scenario in SCENARIOS:
        documents = activated_documents(scenario.touched_paths)
        actual_bytes = sum(document.stat().st_size for document in documents)
        ceiling_bytes = scenario.ceiling_bytes
        status = "PASS" if actual_bytes <= ceiling_bytes else "FAIL"
        headroom_percent = (ceiling_bytes - actual_bytes) / actual_bytes * 100
        print(
            f"{status} | {scenario.name} | {actual_bytes} / {ceiling_bytes} bytes"
            f" | {headroom_percent:+.1f}% headroom"
        )
        if actual_bytes > ceiling_bytes:
            failures.append(
                f"{scenario.name}: {actual_bytes} exceeds {ceiling_bytes} bytes"
            )
        elif actual_bytes < scenario.baseline_bytes:
            stale_baselines.append(
                f"{scenario.name}: payload shrank to {actual_bytes};"
                f" lower baseline_bytes from {scenario.baseline_bytes}"
            )

    for stale_baseline in stale_baselines:
        print(f"note | recalibration available | {stale_baseline}")

    if failures:
        print("Claude rule activation regression failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        print(
            "Trim the activated payload, or record the intended growth by setting"
            f" baseline_bytes to the printed actual (ceiling = baseline"
            f" +{HEADROOM_RATIO:.0%}).",
            file=sys.stderr,
        )
        raise SystemExit(1)


if __name__ == "__main__":
    main()
