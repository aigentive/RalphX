use std::fmt;

pub const AUTO_MERGE_ENABLE_WARNING_CODE: &str = "auto_merge_enable_failed";
pub const AUTO_MERGE_ENABLE_FAILURE_SUMMARY_PREFIX: &str = "GitHub auto-merge could not be enabled";
pub const AUTO_MERGE_SUPERVISION_STATUS_WAITING: &str = "waiting";

pub fn auto_merge_enable_failure_summary(error: impl fmt::Display) -> String {
    format!("{AUTO_MERGE_ENABLE_FAILURE_SUMMARY_PREFIX} yet: {error}")
}

pub fn auto_merge_disable_failure_summary(error: impl fmt::Display) -> String {
    format!("GitHub auto-merge could not be disabled yet: {error}")
}
