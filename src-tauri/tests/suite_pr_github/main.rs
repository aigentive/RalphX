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

mod pr_mode_integration;
mod pr_mode_fallback;
mod pr_mode_acceptance_paths;
mod pr_poller_tests;
mod pr_reconciler_tests;
mod project_pr_template;
