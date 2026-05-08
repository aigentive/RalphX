use serde::Serialize;
use thiserror::Error;

use crate::agents::error::AgentError;
use crate::entities::ideation::VerificationError;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Invalid status transition: {from} → {to}")]
    InvalidTransition { from: String, to: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Claude session expired: {session_id}")]
    StaleSession {
        session_id: String,
        conversation_id: String,
    },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Infrastructure error: {0}")]
    Infrastructure(String),

    #[error("Git operation error: {0}")]
    GitOperation(String),

    #[error("Git authentication error: {0}")]
    GitAuth(String),

    #[error("Execution blocked: {0}")]
    ExecutionBlocked(String),

    #[error("Branch freshness conflict: branches need updating before execution can proceed")]
    BranchFreshnessConflict,

    #[error("Review worktree missing: worktree directory does not exist")]
    ReviewWorktreeMissing,

    #[error("Duplicate pull request: branch already has an open PR")]
    DuplicatePr,

    #[error("IMPORT_VERSION_UNSUPPORTED: Schema version {version} is not supported")]
    ImportVersionUnsupported { version: u32 },

    #[error("IMPORT_INVALID_FORMAT: {detail}")]
    ImportInvalidFormat { detail: String },

    #[error("IMPORT_INVALID_DEPENDENCY: {detail}")]
    ImportInvalidDependency { detail: String },

    #[error("Conflict: {0}")]
    Conflict(String),
}

impl From<AgentError> for AppError {
    fn from(err: AgentError) -> Self {
        Self::Agent(err.to_string())
    }
}

impl From<VerificationError> for AppError {
    fn from(err: VerificationError) -> Self {
        Self::Validation(err.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
