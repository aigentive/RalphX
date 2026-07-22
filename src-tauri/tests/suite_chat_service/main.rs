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

mod chat_service_context;
mod chat_service_errors;
mod chat_service_merge;
mod chat_service_pause_flows;
mod chat_service_review_immediate_start;
mod chat_session_recovery_integration;
mod http_helpers;
mod pending_session_drain;
mod persona_feature_flag_attribution;
mod persona_feature_flag_builder;
mod persona_feature_flag_queue;
mod persona_feature_flag_standalone;
mod persona_feature_flag_support;
mod persona_prompt_composition;
mod persona_run_attribution;
mod session_linking_integration;
