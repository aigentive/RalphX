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

mod api_keys_handlers;
mod artifacts_handlers;
mod conversations_handlers;
mod delegation_handlers;
mod internal_handlers;
mod projects_handlers;
mod reliability_tests;
mod session_linking_handlers;
mod teams_handlers;
mod chat_service_streaming;
mod ideation_event_emission;
mod ticket_attachment_handlers;
