#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

#[path = "../support/mod.rs"]
mod support;

#[path = "../common/mod.rs"]
mod common;

mod agent_workspace_pr_fix_review_autopublish;
mod agent_workspace_publish_recovery;
mod agent_workspace_pr_review_notifications;
mod agent_workspace_repair_auto_publish;
mod agent_workspace_review;
mod linked_workspace_diff;
mod terminal_workspace_diff;
