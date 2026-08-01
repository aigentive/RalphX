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

mod deferred_main_merge_integration;
mod external_handlers;
mod git_handlers;
mod merge_system_hardening;
mod reviewing_initial_recovery;
mod reviews_handlers;
mod startup_jobs_runner;
mod steps_handlers;
mod transition_handler_concurrent_freshness;
mod transition_handler_freshness;
mod transition_handler_freshness_integration;
mod webhook_pipeline_integration;
