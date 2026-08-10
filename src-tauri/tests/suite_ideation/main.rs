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

mod apply_service;
mod external_ideation_runtime_handlers;
mod ideation_capacity_counting;
mod ideation_commands;
mod ideation_handlers;
mod ideation_model_override;
mod ideation_plan_delivery_test;
mod ideation_runtime_handlers;
mod ideation_service;
mod ideation_webhook_enrichment_test;
