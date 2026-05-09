// Ideation tool handlers for MCP ralphx-ideation agent

use axum::{http::StatusCode, Json};

mod acceptance;
mod append;
mod dependency_analysis;
mod proposals;
mod runtime;
mod verification;

pub use acceptance::*;
pub use append::*;
pub use dependency_analysis::*;
pub use proposals::*;
pub use runtime::*;
pub use verification::*;
pub(crate) use verification::{
    is_blank_orphaned_active_generation, load_verification_child_state,
    repair_blank_orphaned_verification_generation, stop_and_archive_children,
    stop_verification_children, ChildFilter,
};

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": error.into() })))
}
