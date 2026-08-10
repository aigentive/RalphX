#!/usr/bin/env python3
"""Ratchet high-risk backend layering edges against a tracked baseline."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = REPO_ROOT / "scripts" / "baselines" / "layering.json"
SCHEMA_VERSION = 1

FILESYSTEM_ENFORCEMENT_SITE = re.compile(
    r"\benforce_filesystem_roots\s*:|\blet\s+enforce_filesystem_roots\s*="
)
FILESYSTEM_ENFORCEMENT_ALLOWLIST = {
    (
        "src-tauri/src/application/chat_service/resolved_conversation_spawn_context.rs",
        "pub enforce_filesystem_roots: bool,",
    ): 1,
    (
        "src-tauri/src/application/chat_service/resolved_conversation_spawn_context.rs",
        "let enforce_filesystem_roots = build_mcp_runtime_context(",
    ): 2,
    (
        "src-tauri/src/application/chat_service/chat_service_context.rs",
        "enforce_filesystem_roots: context_type == ChatContextType::Standalone",
    ): 1,
    (
        "src-tauri/src/infrastructure/agents/mcp_runtime_context.rs",
        "pub enforce_filesystem_roots: bool,",
    ): 1,
}

RULES: list[dict[str, Any]] = [
    {
        "id": "root_domain_no_upward_imports",
        "mode": "baseline",
        "paths": ["src-tauri/src/domain/**/*.rs"],
        "forbidden": [
            "crate::application",
            "crate::infrastructure",
            "crate::http_server",
        ],
    },
    # Phase 4 reached zero here by giving domain its own stats-invalidation
    # port instead of calling into the command layer. Kept as a hard zero so it
    # cannot be re-baselined.
    {
        "id": "root_domain_no_commands_imports",
        "mode": "hard-zero",
        "paths": ["src-tauri/src/domain/**/*.rs"],
        "forbidden": ["crate::commands"],
    },
    {
        "id": "root_domain_no_tauri_usage",
        "mode": "baseline",
        "paths": ["src-tauri/src/domain/**/*.rs"],
        "forbidden": ["tauri::"],
    },
    # Phase 4 drove this to zero (ExecutionState split, spawner move, and the
    # http_server descents). Hard zero so phases 5/12 keep a clean precondition.
    {
        "id": "root_application_no_commands_or_http_imports",
        "mode": "hard-zero",
        "paths": ["src-tauri/src/application/**/*.rs"],
        "forbidden": ["crate::commands", "crate::http_server"],
    },
    {
        "id": "root_infrastructure_no_upper_layer_imports",
        "mode": "baseline",
        "paths": ["src-tauri/src/infrastructure/**/*.rs"],
        "forbidden": [
            "crate::application",
            "crate::commands",
            "crate::http_server",
        ],
    },
    {
        "id": "root_http_no_commands_imports",
        "mode": "baseline",
        "paths": ["src-tauri/src/http_server/**/*.rs"],
        "forbidden": ["crate::commands"],
    },
    {
        "id": "root_commands_no_http_imports",
        "mode": "baseline",
        "paths": ["src-tauri/src/commands/**/*.rs"],
        "forbidden": ["crate::http_server"],
    },
    {
        "id": "bounded_chat_runtime_no_managed_state_lookup",
        "mode": "hard-zero",
        "paths": [
            "src-tauri/src/application/runtime_factory.rs",
            "src-tauri/src/application/chat_service/mod.rs",
            "src-tauri/src/application/chat_service/chat_service_handlers.rs",
            "src-tauri/src/application/chat_service/chat_service_merge.rs",
            "src-tauri/src/application/chat_service/chat_service_queue.rs",
            "src-tauri/src/application/chat_service/chat_service_recovery.rs",
            "src-tauri/src/application/chat_service/chat_service_send_background.rs",
            "src-tauri/src/application/chat_service/chat_service_streaming.rs",
        ],
        "forbidden": [
            "try_state::<AppState>",
            "state::<AppState>",
            "try_state::<ExecutionState>",
            "state::<ExecutionState>",
        ],
    },
    {
        "id": "bounded_chat_start_runtime_no_app_handle",
        "mode": "hard-zero",
        "paths": [
            "src-tauri/src/application/chat_service/mod.rs",
            "src-tauri/src/application/chat_service/chat_service_context.rs",
            "src-tauri/src/application/chat_service/chat_service_handlers.rs",
            "src-tauri/src/application/chat_service/chat_service_merge.rs",
            "src-tauri/src/application/chat_service/chat_service_queue.rs",
            "src-tauri/src/application/chat_service/chat_service_recovery.rs",
            "src-tauri/src/application/chat_service/chat_service_send_background.rs",
            "src-tauri/src/application/chat_service/chat_service_streaming.rs",
            "src-tauri/src/application/chat_service/chat_service_types.rs",
            "src-tauri/src/application/agent_conversation_start_service/mod.rs",
            "src-tauri/src/application/agent_conversation_start_service/start.rs",
            "src-tauri/src/application/agent_conversation_start_service/finish_flow.rs",
            "src-tauri/src/application/agent_conversation_start_service/project_setup.rs",
            "src-tauri/src/application/agent_conversation_start_service/helpers/spawn_glue.rs",
            "src-tauri/src/application/startup_background.rs",
            "src-tauri/src/shell/startup_runtime_builders.rs",
            "src-tauri/src/application/chat_resumption.rs",
            "src-tauri/src/application/task_cleanup_service.rs",
        ],
        "forbidden": ["AppHandle"],
    },
    {
        "id": "bounded_chat_start_runtime_no_tauri_generics",
        "mode": "hard-zero",
        "paths": [
            "src-tauri/src/application/chat_service/mod.rs",
            "src-tauri/src/application/chat_service/chat_service_handlers.rs",
            "src-tauri/src/application/chat_service/chat_service_send_background.rs",
            "src-tauri/src/application/agent_conversation_start_service/mod.rs",
            "src-tauri/src/application/agent_conversation_start_service/start.rs",
            "src-tauri/src/application/agent_conversation_start_service/finish_flow.rs",
            "src-tauri/src/application/agent_conversation_start_service/project_setup.rs",
            "src-tauri/src/application/startup_background.rs",
            "src-tauri/src/shell/startup_runtime_builders.rs",
        ],
        "forbidden": [
            "AppChatService<",
            "BackgroundRunContext<",
            "AgentConversationStartDeps<'a,",
            "AgentConversationStartService<'a,",
            "AgentConversationAutomationRunStarter<",
            "AgentConversationAutomationRunResumer<",
            "StartupSchedulerDeps<",
        ],
    },
    {
        "id": "bounded_completion_producers_no_direct_tauri_emit",
        "mode": "hard-zero",
        "paths": [
            "src-tauri/src/application/chat_service/mod.rs",
            "src-tauri/src/application/chat_service/chat_service_handlers.rs",
            "src-tauri/src/application/chat_service/chat_service_queue.rs",
            "src-tauri/src/application/chat_service/chat_service_send_background.rs",
            "src-tauri/src/application/chat_service/chat_service_streaming.rs",
            "src-tauri/src/application/agent_conversation_start_service/finish_flow.rs",
            "src-tauri/src/application/agent_conversation_start_service/helpers/spawn_glue.rs",
            "src-tauri/src/application/task_cleanup_service.rs",
            "src-tauri/src/http_server/handlers/ideation/verification/lifecycle.rs",
        ],
        "forbidden": [
            "handle.emit(",
            "app.emit(",
            "app_handle.emit(",
            "self.deps.app_handle.emit(",
        ],
    },
    {
        "id": "workspace_domain_no_tauri_or_axum",
        "mode": "hard-zero",
        "paths": ["src-tauri/crates/ralphx-domain/src/**/*.rs"],
        "forbidden": ["tauri::", "axum::"],
    },
    {
        "id": "workspace_domain_no_root_layer_refs",
        "mode": "hard-zero",
        "paths": ["src-tauri/crates/ralphx-domain/src/**/*.rs"],
        "forbidden": [
            "crate::application",
            "crate::commands",
            "crate::infrastructure",
            "crate::http_server",
        ],
    },
]


def normalized_line(line: str) -> str:
    return re.sub(r"\s+", " ", line.strip())


def iter_rule_files(rule: dict[str, Any]) -> list[Path]:
    files: set[Path] = set()
    for pattern in rule["paths"]:
        files.update(path for path in REPO_ROOT.glob(pattern) if path.is_file())
    return sorted(files)


def collect_rule_violations(rule: dict[str, Any]) -> list[dict[str, str]]:
    violations: set[tuple[str, str, str]] = set()
    for path in iter_rule_files(rule):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError as exc:
            raise RuntimeError(f"failed to read {path.relative_to(REPO_ROOT)}: {exc}")

        rel_path = path.relative_to(REPO_ROOT).as_posix()
        for line in lines:
            text = normalized_line(line)
            if not text:
                continue
            for target in rule["forbidden"]:
                if target in text:
                    violations.add((rel_path, target, text))

    return [
        {"path": path, "target": target, "text": text}
        for path, target, text in sorted(violations)
    ]


def collect_violations() -> dict[str, list[dict[str, str]]]:
    return {rule["id"]: collect_rule_violations(rule) for rule in RULES}


def check_filesystem_enforcement_single_derivation() -> int:
    remaining = dict(FILESYSTEM_ENFORCEMENT_ALLOWLIST)
    unexpected: list[tuple[str, int, str]] = []
    source_root = REPO_ROOT / "src-tauri" / "src"

    for path in sorted(source_root.rglob("*.rs")):
        if path.name.endswith("_tests.rs") or path.name == "tests.rs":
            continue
        rel_path = path.relative_to(REPO_ROOT).as_posix()
        for line_number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            text = normalized_line(line)
            if not FILESYSTEM_ENFORCEMENT_SITE.search(text):
                continue
            key = (rel_path, text)
            if remaining.get(key, 0) > 0:
                remaining[key] -= 1
            else:
                unexpected.append((rel_path, line_number, text))

    missing = sorted(key for key, count in remaining.items() if count != 0)
    if not unexpected and not missing:
        return 0

    print(
        "Filesystem enforcement single-derivation invariant failed:",
        file=sys.stderr,
    )
    for path, line_number, text in unexpected:
        print(f"- unexpected {path}:{line_number} | {text}", file=sys.stderr)
    for path, text in missing:
        print(f"- missing allowlisted site {path} | {text}", file=sys.stderr)
    print(
        "Derive enforce_filesystem_roots only in build_mcp_runtime_context; "
        "forward that canonical value elsewhere.",
        file=sys.stderr,
    )
    return 1


def baseline_payload() -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "note": (
            "Generated by scripts/check-layering.py --update. Review every "
            "baseline change; do not add hidden CI bypasses."
        ),
        "rules": RULES,
        "violations": collect_violations(),
    }


def violation_key(violation: dict[str, str]) -> tuple[str, str, str]:
    return (violation["path"], violation["target"], violation["text"])


def validate_baseline_shape(data: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if data.get("schema_version") != SCHEMA_VERSION:
        errors.append(
            f"schema_version must be {SCHEMA_VERSION}, got {data.get('schema_version')}"
        )
    if data.get("rules") != RULES:
        errors.append("rules in baseline do not match scripts/check-layering.py")

    violations = data.get("violations")
    if not isinstance(violations, dict):
        errors.append("violations must be an object keyed by rule id")
        return errors

    expected_ids = {rule["id"] for rule in RULES}
    actual_ids = set(violations)
    missing = sorted(expected_ids - actual_ids)
    extra = sorted(actual_ids - expected_ids)
    if missing:
        errors.append("baseline is missing rule ids: " + ", ".join(missing))
    if extra:
        errors.append("baseline has unknown rule ids: " + ", ".join(extra))

    for rule_id, entries in violations.items():
        if not isinstance(entries, list):
            errors.append(f"{rule_id}: entries must be a list")
            continue
        for index, entry in enumerate(entries):
            if not isinstance(entry, dict):
                errors.append(f"{rule_id}[{index}]: entry must be an object")
                continue
            if set(entry) != {"path", "target", "text"}:
                errors.append(
                    f"{rule_id}[{index}]: expected path/target/text keys, got {sorted(entry)}"
                )
    return errors


def load_baseline(path: Path) -> dict[str, Any]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        raise RuntimeError(f"baseline file not found: {path.relative_to(REPO_ROOT)}")
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"invalid JSON in {path.relative_to(REPO_ROOT)}: {exc}")

    errors = validate_baseline_shape(data)
    if errors:
        raise RuntimeError(
            "layering baseline shape is invalid:\n"
            + "\n".join(f"- {error}" for error in errors)
        )
    return data


def print_delta(
    label: str,
    rule_id: str,
    entries: list[tuple[str, str, str]],
) -> None:
    if not entries:
        return
    print(f"{label} for {rule_id}:", file=sys.stderr)
    for path, target, text in entries[:25]:
        print(f"- {path} | {target} | {text}", file=sys.stderr)
    if len(entries) > 25:
        print(f"- ... {len(entries) - 25} more", file=sys.stderr)


def check_against_baseline(path: Path) -> int:
    expected = load_baseline(path)
    actual = collect_violations()
    failures = 0

    for rule in RULES:
        rule_id = rule["id"]
        actual_keys = {violation_key(entry) for entry in actual[rule_id]}
        expected_keys = {
            violation_key(entry) for entry in expected["violations"].get(rule_id, [])
        }

        if rule["mode"] == "hard-zero":
            if actual_keys:
                failures += 1
                print_delta("Hard-zero violations", rule_id, sorted(actual_keys))
            continue

        new_entries = sorted(actual_keys - expected_keys)
        resolved_entries = sorted(expected_keys - actual_keys)
        if new_entries or resolved_entries:
            failures += 1
            print_delta("New layering violations", rule_id, new_entries)
            print_delta("Resolved baseline entries", rule_id, resolved_entries)

    failures += check_filesystem_enforcement_single_derivation()

    if failures:
        print(
            "\nLayering ratchet failed. Remove new violations or regenerate the "
            "baseline intentionally with `python3 scripts/check-layering.py --update`.",
            file=sys.stderr,
        )
        return 1

    baseline_count = sum(
        len(expected["violations"].get(rule["id"], []))
        for rule in RULES
        if rule["mode"] == "baseline"
    )
    hard_zero_count = sum(1 for rule in RULES if rule["mode"] == "hard-zero")
    print(
        f"Layering ratchet passed: {baseline_count} tracked entries, "
        f"{hard_zero_count} hard-zero rules and filesystem "
        "single-derivation invariant clean."
    )
    return 0


def update_baseline(path: Path) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = baseline_payload()
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tracked = sum(
        len(entries)
        for rule_id, entries in data["violations"].items()
        if next(rule for rule in RULES if rule["id"] == rule_id)["mode"] == "baseline"
    )
    hard_zero = sum(
        len(entries)
        for rule_id, entries in data["violations"].items()
        if next(rule for rule in RULES if rule["id"] == rule_id)["mode"] == "hard-zero"
    )
    print(
        f"Wrote {path.relative_to(REPO_ROOT)} with {tracked} tracked entries "
        f"and {hard_zero} hard-zero entries."
    )
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--baseline",
        type=Path,
        default=DEFAULT_BASELINE,
        help="Path to the tracked layering baseline JSON.",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Regenerate the baseline from the current checkout.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    baseline = args.baseline
    if not baseline.is_absolute():
        baseline = REPO_ROOT / baseline

    try:
        if args.update:
            return update_baseline(baseline)
        return check_against_baseline(baseline)
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
