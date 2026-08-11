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
pub(crate) use crate::application::verification_child_lifecycle::stop_verification_children;

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": error.into() })))
}
