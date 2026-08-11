use chrono::Utc;

use super::data_retention::{
    DataRetentionDefaults, DataRetentionPolicyUpdate, DataRetentionSettings,
    MAX_RETENTION_BATCH_ROWS, MIN_SIZE_BUDGET_BYTES,
};
use crate::error::AppError;

fn settings_with_budget(size_budget_bytes: Option<u64>, confirmed: bool) -> DataRetentionSettings {
    let now = Utc::now();
    DataRetentionSettings {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 500,
        size_budget_bytes,
        size_budget_confirmed_at: confirmed.then_some(now),
        seeded_pristine: false,
        size_budget_advised: false,
        last_run_at: None,
        last_run_pruned_rows: None,
        last_run_payload_bytes: None,
        last_run_payload_rows: None,
        updated_at: now,
    }
}

fn update_with_budget(
    size_budget_bytes: Option<u64>,
    confirmed: bool,
) -> DataRetentionPolicyUpdate {
    DataRetentionPolicyUpdate {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 500,
        size_budget_bytes,
        size_budget_confirmed_at: confirmed.then_some(Utc::now()),
    }
}

#[test]
fn zero_batch_rows_from_config_are_clamped_to_a_usable_batch() {
    let clamped = DataRetentionDefaults {
        enabled: true,
        days: 0,
        archived_days: 0,
        batch_rows: 0,
    }
    .clamped();

    assert_eq!(clamped.days, 1);
    assert_eq!(clamped.archived_days, 1);
    assert_eq!(clamped.batch_rows, 1);
}

#[test]
fn oversized_batch_rows_from_config_are_clamped_down() {
    let clamped = DataRetentionDefaults {
        enabled: true,
        days: 90,
        archived_days: 7,
        batch_rows: 999_999,
    }
    .clamped();

    assert_eq!(clamped.batch_rows, MAX_RETENTION_BATCH_ROWS);
}

#[test]
fn size_budget_is_inert_unless_both_budget_and_confirmation_exist() {
    assert!(!settings_with_budget(None, false).size_budget_active());
    assert!(!settings_with_budget(Some(MIN_SIZE_BUDGET_BYTES), false).size_budget_active());
    assert!(!settings_with_budget(None, true).size_budget_active());
    assert!(settings_with_budget(Some(MIN_SIZE_BUDGET_BYTES), true).size_budget_active());

    assert_eq!(
        settings_with_budget(Some(MIN_SIZE_BUDGET_BYTES), false).enforced_size_budget_bytes(),
        None
    );
    assert_eq!(
        settings_with_budget(Some(MIN_SIZE_BUDGET_BYTES), true).enforced_size_budget_bytes(),
        Some(MIN_SIZE_BUDGET_BYTES)
    );
}

#[test]
fn unconfirmed_budget_updates_are_rejected() {
    let error = update_with_budget(Some(MIN_SIZE_BUDGET_BYTES), false)
        .validate()
        .expect_err("unconfirmed budget must be rejected");
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn clearing_the_budget_must_clear_its_confirmation() {
    let error = update_with_budget(None, true)
        .validate()
        .expect_err("dangling confirmation must be rejected");
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn budget_below_the_floor_is_rejected() {
    let error = update_with_budget(Some(MIN_SIZE_BUDGET_BYTES - 1), true)
        .validate()
        .expect_err("tiny budget must be rejected");
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn out_of_range_windows_and_batches_are_rejected() {
    let mut update = update_with_budget(None, false);
    update.days = 0;
    assert!(matches!(
        update.validate().expect_err("zero-day window"),
        AppError::Validation(_)
    ));

    let mut update = update_with_budget(None, false);
    update.batch_rows = MAX_RETENTION_BATCH_ROWS + 1;
    assert!(matches!(
        update.validate().expect_err("oversized batch"),
        AppError::Validation(_)
    ));

    let mut update = update_with_budget(None, false);
    update.batch_rows = 0;
    assert!(matches!(
        update.validate().expect_err("zero batch"),
        AppError::Validation(_)
    ));
}

#[test]
fn time_window_only_policy_validates() {
    update_with_budget(None, false)
        .validate()
        .expect("shipped policy shape must be valid");
    update_with_budget(Some(MIN_SIZE_BUDGET_BYTES), true)
        .validate()
        .expect("confirmed budget must be valid");
}
