//! Turns AppState construction failures into what the bootstrap surface shows.
//!
//! Most construction failures are opaque to the user, so they collapse into one
//! generic sentence. Disk exhaustion is different: the workspace is intact and
//! the user can fix it, so it gets its own code and keeps the measured numbers.

use crate::application::startup_status::{StartupFailure, StartupFailureCode};
use crate::error::AppError;

const GENERIC_CONSTRUCTION_SUMMARY: &str = "RalphX could not open its local workspace.";

pub fn classify_app_state_construction_failure(error: &AppError) -> StartupFailure {
    match error {
        AppError::InsufficientDiskSpace {
            operation,
            required_bytes,
            available_bytes,
        } => StartupFailure {
            code: StartupFailureCode::InsufficientDiskSpace,
            diagnostic_summary: format!(
                "RalphX needs {} free to finish {operation}, but only {} is available. \
                 Free up disk space, then try again.",
                format_bytes(*required_bytes),
                format_bytes(*available_bytes)
            ),
        },
        _ => StartupFailure {
            code: StartupFailureCode::AppStateConstruction,
            diagnostic_summary: GENERIC_CONSTRUCTION_SUMMARY.to_string(),
        },
    }
}

pub fn generic_app_state_construction_failure() -> StartupFailure {
    StartupFailure {
        code: StartupFailureCode::AppStateConstruction,
        diagnostic_summary: GENERIC_CONSTRUCTION_SUMMARY.to_string(),
    }
}

/// Decimal units, matching how macOS reports free space to the same user who is
/// about to go looking for it in Finder.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} bytes")
    }
}
