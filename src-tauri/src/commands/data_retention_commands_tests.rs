use super::data_retention_commands::{policy_update_from, UpdateDataRetentionSettingsInput};

fn input(
    size_budget_bytes: Option<u64>,
    size_budget_confirmed: bool,
) -> UpdateDataRetentionSettingsInput {
    UpdateDataRetentionSettingsInput {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 500,
        size_budget_bytes,
        size_budget_confirmed,
    }
}

#[test]
fn confirming_a_budget_stamps_the_server_clock() {
    let before = chrono::Utc::now();
    let update = policy_update_from(input(Some(1_073_741_824), true));
    let after = chrono::Utc::now();

    let confirmed_at = update
        .size_budget_confirmed_at
        .expect("a confirmed budget carries a server-stamped timestamp");
    assert!(confirmed_at >= before && confirmed_at <= after);
    update.validate().expect("confirmed budget is valid");
}

#[test]
fn an_unconfirmed_budget_never_gains_a_timestamp() {
    let update = policy_update_from(input(Some(1_073_741_824), false));

    assert_eq!(update.size_budget_confirmed_at, None);
    update
        .validate()
        .expect_err("an unconfirmed budget must not be persistable");
}

#[test]
fn clearing_the_budget_clears_consent_even_when_the_caller_still_confirms() {
    let update = policy_update_from(input(None, true));

    assert_eq!(update.size_budget_bytes, None);
    assert_eq!(
        update.size_budget_confirmed_at, None,
        "a stale confirmation flag must not survive clearing the budget"
    );
    update.validate().expect("time-window-only policy is valid");
}
