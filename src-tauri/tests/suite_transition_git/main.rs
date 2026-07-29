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

mod transition_handler_freshness;
mod transition_handler_freshness_integration;
mod transition_handler_concurrent_freshness;
mod webhook_pipeline_integration;
mod reviewing_initial_recovery;
mod startup_jobs_runner;
mod merge_system_hardening;
mod deferred_main_merge_integration;
mod steps_handlers;
mod reviews_handlers;
mod git_handlers;
mod external_handlers;
