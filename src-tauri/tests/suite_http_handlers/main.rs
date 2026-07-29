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

mod agent_workspace_pr_review_context;
mod agent_workspace_repair_completion;
mod agent_workspace_review_context;
mod agent_workspace_review_diff;
mod api_keys_handlers;
mod artifacts_handlers;
mod automations_handlers;
mod chat_service_streaming;
mod conversations_handlers;
mod delegation_handlers;
mod ideation_event_emission;
mod internal_handlers;
mod personas_handlers;
mod projects_handlers;
mod reliability_tests;
mod session_linking_handlers;
