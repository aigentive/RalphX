#!/usr/bin/env python3
"""Mechanically consolidate PR 0.3 integration tests into suite directories."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
TESTS = REPO_ROOT / "src-tauri" / "tests"

SUITES: dict[str, list[str]] = {
    "suite_ipc_commands": [
        "task_commands",
        "api_key_commands",
        "project_commands",
        "unified_chat_commands",
        "task_step_commands",
        "harness_provider_commands",
    ],
    "suite_commands": [
        "activity_commands",
        "agent_profile_commands",
        "methodology_commands",
        "metrics_commands",
        "qa_commands",
        "release_notes_commands",
        "research_commands",
        "workflow_commands",
        "artifact_commands",
        "review_commands",
        "review_service",
        "plan_branch_commands",
        "conversation_stats_commands",
        "execution_commands_running_count",
        "question_commands",
        "git_commands",
    ],
    "suite_http_handlers": [
        "api_keys_handlers",
        "artifacts_handlers",
        "conversations_handlers",
        "delegation_handlers",
        "internal_handlers",
        "projects_handlers",
        "reliability_tests",
        "session_linking_handlers",
        "teams_handlers",
        "chat_service_streaming",
        "ideation_event_emission",
    ],
    "suite_sqlite_repos": [
        "sqlite_chat_message_repo",
        "sqlite_ideation_session_repo",
        "external_issue_links",
        "clickup_integration_settings",
        "granola_integration_settings",
        "linear_integration_settings",
    ],
    "suite_sqlite_flows": [
        "state_machine_flows",
        "qa_system_flows",
        "review_flows",
        "execution_control_flows",
        "per_project_execution_scoping",
        "workflow_integration",
        "artifact_integration",
        "methodology_integration",
        "gsd_integration",
        "research_integration",
        "repository_swapping",
        "linear_webhook_reconciliation",
    ],
    "suite_metrics": [
        "metrics_integration",
        "metrics_schema_validation",
        "metrics_delivery_trends",
        "metrics_pr_insights",
    ],
    "suite_chat_service": [
        "chat_service_errors",
        "chat_service_context",
        "chat_service_merge",
        "chat_service_pause_flows",
        "chat_session_recovery_integration",
        "pending_session_drain",
        "session_fixes_integration",
        "session_linking_integration",
        "http_helpers",
    ],
    "suite_ideation": [
        "ideation_service",
        "ideation_capacity_counting",
        "ideation_webhook_enrichment_test",
        "ideation_model_override",
        "ideation_commands",
        "ideation_runtime_handlers",
        "external_ideation_runtime_handlers",
        "ideation_plan_delivery_test",
        "ideation_handlers",
        "apply_service",
    ],
    "suite_transition_git": [
        "transition_handler_freshness",
        "transition_handler_freshness_integration",
        "transition_handler_concurrent_freshness",
        "webhook_pipeline_integration",
        "reviewing_initial_recovery",
        "startup_jobs_runner",
        "merge_system_hardening",
        "deferred_main_merge_integration",
        "steps_handlers",
        "reviews_handlers",
        "git_handlers",
        "external_handlers",
    ],
    "suite_pr_github": [
        "pr_mode_integration",
        "pr_mode_fallback",
        "pr_mode_acceptance_paths",
        "pr_poller_tests",
        "pr_reconciler_tests",
        "project_pr_template",
    ],
    "suite_interactive_process": [
        "gate1_ipr_fast_path_tests",
        "ipr_cleanup_guard_tests",
        "interactive_mode_integration",
        "team_nudge_running_count_tests",
        "task_cleanup_service",
        "reconciliation_runner",
        "agentic_client_flows",
        "supervisor_integration",
        "codex_stream_processor",
        "codex_cli_capabilities",
        "execution_types_serde",
        "task_scheduler_service",
    ],
    "suite_agent_workspace": [
        "agent_workspace_publish_recovery",
        "agent_workspace_repair_auto_publish",
        "agent_workspace_review",
    ],
}


def run(args: list[str]) -> None:
    subprocess.run(args, cwd=REPO_ROOT, check=True)


def rewrite_member(path: Path) -> tuple[bool, bool]:
    content = path.read_text(encoding="utf-8")
    needed_common = bool(re.search(r"(?m)^\s*mod\s+common\s*;", content)) or "common::" in content
    needed_support = bool(re.search(r"(?m)^\s*mod\s+support\s*;", content)) or "support::" in content

    content = re.sub(r"(?m)^\s*mod\s+common\s*;\n", "", content)
    content = re.sub(r"(?m)^\s*mod\s+support\s*;\n", "", content)
    content = re.sub(r"\bsuper::common::", "crate::common::", content)
    content = re.sub(r"\bsuper::support::", "crate::support::", content)
    content = re.sub(r"\bcommon::", "crate::common::", content)
    content = re.sub(r"\bsupport::", "crate::support::", content)

    path.write_text(content, encoding="utf-8")
    return needed_common, needed_support


def guard_test() -> str:
    return '''#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

'''


def write_main(suite: str, modules: list[str], needs_common: bool, needs_support: bool) -> None:
    suite_dir = TESTS / suite
    lines: list[str] = [guard_test()]
    if suite == "suite_commands":
        lines.append(
            "fn tauri_context() -> tauri::Context<tauri::test::MockRuntime> {\n"
            "    tauri::generate_context!()\n"
            "}\n\n"
        )
    if needs_common or needs_support:
        lines.append('#[path = "../support/mod.rs"]\nmod support;\n\n')
    if needs_common:
        lines.append('#[path = "../common/mod.rs"]\nmod common;\n\n')
    lines.extend(f"mod {module};\n" for module in modules)
    (suite_dir / "main.rs").write_text("".join(lines), encoding="utf-8")


def main() -> int:
    moved = 0
    for suite, modules in SUITES.items():
        suite_dir = TESTS / suite
        suite_dir.mkdir(exist_ok=True)
        needs_common = False
        needs_support = False
        for module in modules:
            source = TESTS / f"{module}.rs"
            dest = suite_dir / f"{module}.rs"
            if source.exists():
                run(["git", "mv", str(source), str(dest)])
                moved += 1
            elif not dest.exists():
                raise FileNotFoundError(f"missing {source.relative_to(REPO_ROOT)}")
            module_common, module_support = rewrite_member(dest)
            needs_common = needs_common or module_common
            needs_support = needs_support or module_support
        write_main(suite, modules, needs_common, needs_support)

    project_commands = TESTS / "suite_ipc_commands" / "project_commands.rs"
    if project_commands.exists():
        content = project_commands.read_text(encoding="utf-8")
        content = content.replace(
            '#[path = "project_pr_template.rs"]',
            '#[path = "../suite_pr_github/project_pr_template.rs"]',
        )
        project_commands.write_text(content, encoding="utf-8")

    for context_user in (
        TESTS / "suite_commands" / "metrics_commands.rs",
        TESTS / "suite_commands" / "release_notes_commands.rs",
    ):
        if context_user.exists():
            content = context_user.read_text(encoding="utf-8")
            content = content.replace("tauri::generate_context!()", "crate::tauri_context()")
            context_user.write_text(content, encoding="utf-8")

    print(f"Consolidated {moved} files into {len(SUITES)} suite directories.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
