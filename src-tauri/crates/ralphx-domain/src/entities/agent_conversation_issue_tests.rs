use super::{canonicalize_agent_conversation_issue, AgentConversationIssueCanonicalInput};

fn identity(
    title: &str,
    summary: &str,
    evidence: Option<&str>,
    blocker_fingerprint: Option<&str>,
    source_task_id: Option<&str>,
) -> super::AgentConversationIssueCanonicalIdentity {
    canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
        issue_kind: "plan_drift",
        blocking_scope: "followup_only",
        title,
        summary,
        evidence,
        recommendation: None,
        blocker_fingerprint,
        source_task_id,
    })
}

#[test]
fn frontend_dependency_setup_dedupes_across_agents_and_tasks() {
    let worker = identity(
        "Frontend validation analysis uses repo-root setup for frontend package",
        "The frontend validation command was evaluated from the repo root and could not find frontend/node_modules.",
        Some("task task-a ran pnpm exec tsc and found missing frontend/node_modules"),
        Some("frontend-validation-cwd-node-modules-setup"),
        Some("task-a"),
    );
    let reviewer = identity(
        "Merge hook cannot find tsc",
        "The merge hook failed because tsc was not found on PATH.",
        Some("task task-b: sh: tsc: command not found"),
        Some("merge-hook-env:task-b:tsc-not-found"),
        Some("task-b"),
    );

    assert_eq!(
        worker.fingerprint,
        "v1:setup:project:frontend-package:missing-frontend-dependency"
    );
    assert_eq!(reviewer.fingerprint, worker.fingerprint);
    assert_eq!(worker.scope_kind, "project");
    assert_eq!(worker.scope_subject, "frontend-package");
    assert!(worker.candidate_match_eligible);
}

#[test]
fn package_lock_drift_dedupes_across_agent_wording() {
    let first = identity(
        "Review setup creates untracked MCP package-lock",
        "Generated ralphx-plugin MCP package-lock changed during setup.",
        Some("plugins/app/ralphx-mcp-server/package-lock.json"),
        Some("review-setup-untracked-ralphx-plugin-mcp-lockfile"),
        Some("task-a"),
    );
    let second = identity(
        "Review setup regenerates untracked MCP package-lock drift",
        "The mcp server package-lock.json was regenerated.",
        Some("ralphx-plugin/ralphx-mcp-server/package-lock.json"),
        Some("review-setup-generated-package-lock:task-b"),
        Some("task-b"),
    );

    assert_eq!(
        first.fingerprint,
        "v1:setup:project:ralphx-plugin-mcp:package-lock-drift"
    );
    assert_eq!(second.fingerprint, first.fingerprint);
}

#[test]
fn validation_and_prerequisite_classes_are_project_scoped() {
    let clippy = identity(
        "Clippy baseline blocks broad validation",
        "cargo clippy --all-targets -- -D warnings fails on pre-existing lints.",
        Some("\u{1b}[31mwarning promoted to error\u{1b}[0m"),
        Some("clippy-baseline:task-a"),
        Some("task-a"),
    );
    let runtime = identity(
        "Runtime index prerequisite missing",
        "The runtime-index API/tab needed by this workflow is missing.",
        None,
        Some("runtime-index-prerequisite:task-b"),
        Some("task-b"),
    );

    assert_eq!(
        clippy.fingerprint,
        "v1:validation:project:backend-clippy:preexisting-baseline"
    );
    assert_eq!(
        runtime.fingerprint,
        "v1:prerequisite:project:runtime-index:missing-runtime-surface"
    );
    assert_eq!(clippy.scope_kind, "project");
    assert_eq!(runtime.scope_subject, "runtime-index");
}

#[test]
fn rails_test_database_setup_dedupes_across_agent_wording() {
    let worker = identity(
        "Rails test DB schema setup blocks focused RSpec validation",
        "Focused RSpec validation cannot run because config/database.yml is missing and db:schema:load fails.",
        Some("PG::UndefinedTable: relation failed_messages_seq does not exist at db/schema.rb"),
        None,
        Some("task-a"),
    );
    let project_chat = identity(
        "Fix test DB/schema setup for Printspeak task worktrees",
        "Repair the Rails test database so task worktrees can run specs.",
        Some("RSpec is blocked by 219 pending migrations and a missing failed_messages_seq table."),
        None,
        Some("task-b"),
    );

    assert_eq!(
        worker.fingerprint,
        "v1:setup:project:rails-test-database:schema-unavailable"
    );
    assert_eq!(project_chat.fingerprint, worker.fingerprint);
    assert_eq!(worker.scope_kind, "project");
    assert_eq!(worker.scope_subject, "rails-test-database");
    assert!(worker.candidate_match_eligible);
}

#[test]
fn scope_drift_stays_task_scoped() {
    let first = canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
        issue_kind: "plan_drift",
        blocking_scope: "current_task",
        title: "Scope drift",
        summary: "Out-of-scope files were changed.",
        evidence: Some("src/unrelated.rs\nsrc/other.rs"),
        recommendation: None,
        blocker_fingerprint: Some("scope-drift:task-a:src/unrelated.rs"),
        source_task_id: Some("task-a"),
    });
    let other_task = canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
        issue_kind: "plan_drift",
        blocking_scope: "current_task",
        title: "Scope drift",
        summary: "Out-of-scope files were changed.",
        evidence: Some("src/unrelated.rs\nsrc/other.rs"),
        recommendation: None,
        blocker_fingerprint: Some("scope-drift:task-b:src/unrelated.rs"),
        source_task_id: Some("task-b"),
    });

    assert!(first
        .fingerprint
        .starts_with("v1:scope-drift:task:task-a:files:"));
    assert!(other_task
        .fingerprint
        .starts_with("v1:scope-drift:task:task-b:files:"));
    assert_ne!(first.fingerprint, other_task.fingerprint);
    assert_eq!(first.scope_kind, "task");
}

#[test]
fn unknown_fallback_preserves_raw_blocker_fingerprint() {
    let raw = identity(
        "Novel blocker",
        "A new issue class appeared.",
        None,
        Some("custom-agent-fingerprint:v1"),
        Some("task-a"),
    );
    assert_eq!(raw.fingerprint, "custom-agent-fingerprint:v1");
    assert!(!raw.candidate_match_eligible);
}
