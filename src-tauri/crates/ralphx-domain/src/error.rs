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

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    // Bare passthrough: constructors embed the [Personas disabled: prefix so
    // IPC/HTTP strings start with the A15 family constant (matches PersonaUnavailable).
    #[error("{0}")]
    FeatureDisabled(String),

    #[error("{0}")]
    PersonaUnavailable(String),

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

    #[error("Review worktree contains unresolved conflict markers")]
    ReviewWorktreeConflictMarkers,

    #[error(
        "Resolve conflicts and complete or abort the merge or rebase before retrying Workspace Review."
    )]
    WorkspaceReviewUnfinishedGitOperation,

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

    #[error(
        "PERSONA_DRAFT_CONFLICT: expected content hash `{expected}` but current hash is `{actual}`"
    )]
    PersonaDraftConflict { expected: String, actual: String },

    #[error("PERSONA_ALREADY_APPROVED: persona already approved — start a refine build")]
    PersonaAlreadyApproved,

    #[error(
        "CONVERSATION_FOLDER_REFERENCE_LIMIT: conversation `{conversation_id}` already has the maximum of {limit} live folder references; remove one before adding another"
    )]
    ConversationFolderReferenceLimit {
        conversation_id: String,
        limit: usize,
    },

    #[error(
        "CONVERSATION_FOLDER_REFERENCE_DUPLICATE: conversation `{conversation_id}` already references `{folder_path}`"
    )]
    ConversationFolderReferenceDuplicate {
        conversation_id: String,
        folder_path: String,
    },

    #[error("CONVERSATION_FOLDER_REFERENCE_UNSUPPORTED_CONTEXT: folder references require a Project conversation")]
    ConversationFolderReferenceUnsupportedContext,

    #[error("CONVERSATION_FOLDER_REFERENCE_APP_DATA_UNAVAILABLE: RalphX application data could not be canonicalized: {detail}")]
    ConversationFolderReferenceAppDataUnavailable { detail: String },

    #[error("SESSION_NAMER_STANDALONE_WORKSPACE_UNAVAILABLE: standalone conversation `{conversation_id}` could not obtain an app-owned naming workspace: {detail}")]
    SessionNamerStandaloneWorkspaceUnavailable {
        conversation_id: String,
        detail: String,
    },

    #[error("SEEDED_AGENT_CONVERSATION_ALREADY_STARTED: conversation `{conversation_id}` has already started and cannot be aborted as a seed")]
    SeededAgentConversationAlreadyStarted { conversation_id: String },

    #[error("STANDALONE_WORKSPACE_MISSING: workspace for conversation `{conversation_id}` does not exist")]
    StandaloneWorkspaceMissing { conversation_id: String },

    #[error("The persona builder can only read text context — PDFs/images aren't supported")]
    PersonaBuilderTextAttachmentOnly,

    /// Carries the measured numbers rather than a prebuilt sentence so callers
    /// that surface this to a user can phrase and format it themselves.
    #[error("INSUFFICIENT_DISK_SPACE: {operation} needs {required_bytes} free bytes but only {available_bytes} are available")]
    InsufficientDiskSpace {
        operation: String,
        required_bytes: u64,
        available_bytes: u64,
    },
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
