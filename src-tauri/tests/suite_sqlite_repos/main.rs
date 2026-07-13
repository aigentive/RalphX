#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

mod sqlite_chat_message_repo;
mod sqlite_ideation_session_repo;
mod external_issue_links;
mod clickup_integration_settings;
mod granola_integration_settings;
mod linear_integration_settings;
mod ui_feature_flag_overrides;
