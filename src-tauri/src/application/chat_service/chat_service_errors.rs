// Error classification module for chat service
//
// Provides structured error type classification from string-based agent errors,
// enabling specific error handling strategies. Also defines StreamError for typed
// stream processing failures, replacing String-based error returns.

use crate::application::persona_resolver::PersonaError;
use crate::application::personas::PERSONA_UNAVAILABLE_PREFIX;
use crate::domain::entities::{ChatContextType, ChatConversationId, InternalStatus};
use crate::error::AppError;
use crate::infrastructure::agents::limits_config;
use crate::utils::truncate_str;
use chrono::{Datelike, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// Claude CLI error message indicating an expired/invalid session.
/// Source: Claude CLI stderr when resuming with a stale session ID.
pub const STALE_SESSION_ERROR: &str = "No conversation found with session ID";
/// Stable stem shared by Claude subscription/usage limit banners. The trailing
/// scope word varies ("You've hit your limit", "You've hit your session limit",
/// "You've hit your usage limit", ...), so we match the stem plus "limit"
/// instead of a fixed full phrase. ❌ Don't narrow this back to a single phrase:
/// a missed banner is hard-failed as `AgentExit` instead of paused/auto-resumed.
const CLAUDE_USAGE_LIMIT_STEM: &str = "you've hit your";
const CLAUDE_EXTRA_USAGE_PREFIX: &str = "you're out of extra usage";

impl From<PersonaError> for super::ChatServiceError {
    fn from(error: PersonaError) -> Self {
        Self::PersonaUnavailable(format!("{PERSONA_UNAVAILABLE_PREFIX} {error}]"))
    }
}

/// True when `lower` (already lowercased) is a Claude usage/session limit banner.
fn is_claude_usage_limit_banner(lower: &str) -> bool {
    lower.contains(CLAUDE_USAGE_LIMIT_STEM) && lower.contains("limit")
}

/// Category of provider/API error for recovery decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    /// HTTP 429 or usage limit exceeded
    RateLimit,
    /// HTTP 401/403 or invalid API key
    AuthError,
    /// HTTP 5xx from provider
    ServerError,
    /// Connection refused, DNS failure, network timeout
    NetworkError,
    /// Overloaded API (Claude-specific overloaded_error)
    Overloaded,
}

impl std::fmt::Display for ProviderErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimit => write!(f, "rate_limit"),
            Self::AuthError => write!(f, "auth_error"),
            Self::ServerError => write!(f, "server_error"),
            Self::NetworkError => write!(f, "network_error"),
            Self::Overloaded => write!(f, "overloaded"),
        }
    }
}

/// Metadata stored in task.metadata when paused due to provider error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderErrorMetadata {
    pub category: ProviderErrorCategory,
    pub message: String,
    /// ISO 8601 timestamp when the error's limit resets (parsed from error message)
    pub retry_after: Option<String>,
    /// The task status before pausing (for resuming to correct state)
    pub previous_status: String,
    /// When the task was paused
    pub paused_at: String,
    /// Whether the system should auto-resume this task
    pub auto_resumable: bool,
    /// Number of auto-resume attempts so far
    #[serde(default)]
    pub resume_attempts: u32,
}

impl ProviderErrorMetadata {
    /// Maximum auto-resume attempts before giving up (read from runtime config).
    pub fn max_resume_attempts() -> u32 {
        limits_config().max_resume_attempts as u32
    }

    /// Read provider_error metadata from task metadata JSON string.
    pub fn from_task_metadata(metadata: Option<&str>) -> Option<Self> {
        let json: serde_json::Value = serde_json::from_str(metadata?).ok()?;
        let provider_error = json.get("provider_error")?;
        serde_json::from_value(provider_error.clone()).ok()
    }

    /// Write provider_error metadata into task metadata JSON string.
    pub fn write_to_task_metadata(&self, existing_metadata: Option<&str>) -> String {
        let mut json: serde_json::Value = existing_metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "provider_error".to_string(),
                serde_json::to_value(self).unwrap_or_default(),
            );
        }

        json.to_string()
    }

    /// Remove provider_error metadata from task metadata (on successful resume).
    pub fn clear_from_task_metadata(existing_metadata: Option<&str>) -> String {
        let mut json: serde_json::Value = existing_metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(obj) = json.as_object_mut() {
            obj.remove("provider_error");
        }

        json.to_string()
    }

    /// Check if retry_after time has passed.
    pub fn is_retry_eligible(&self) -> bool {
        if self.resume_attempts >= Self::max_resume_attempts() {
            return false;
        }
        if !self.auto_resumable {
            return false;
        }
        match &self.retry_after {
            Some(retry_after_str) => {
                chrono::DateTime::parse_from_rfc3339(retry_after_str)
                    .map(|dt| chrono::Utc::now() >= dt)
                    .unwrap_or(true) // If can't parse, allow retry
            }
            None => true, // No retry_after means retry immediately
        }
    }
}

/// Unified pause reason metadata stored under `task.metadata.pause_reason`.
///
/// Distinguishes user-initiated pauses from provider-error pauses so the
/// frontend can render appropriate UI and reconciliation can skip user-paused tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PauseReason {
    /// User clicked pause (global or per-task)
    UserInitiated {
        previous_status: String,
        paused_at: String,
        /// "global" for pause_execution, "task" for per-task pause
        scope: String,
    },
    /// Provider/API error caused automatic pause
    ProviderError {
        category: ProviderErrorCategory,
        message: String,
        retry_after: Option<String>,
        previous_status: String,
        paused_at: String,
        auto_resumable: bool,
        #[serde(default)]
        resume_attempts: u32,
    },
}

impl PauseReason {
    /// Metadata key used in task.metadata JSON
    const KEY: &'static str = "pause_reason";

    /// Read pause_reason from task metadata JSON string.
    /// Also checks legacy `provider_error` key for backward compatibility.
    pub fn from_task_metadata(metadata: Option<&str>) -> Option<Self> {
        let json: serde_json::Value = serde_json::from_str(metadata?).ok()?;

        // Try new key first
        if let Some(val) = json.get(Self::KEY) {
            if let Ok(reason) = serde_json::from_value::<Self>(val.clone()) {
                return Some(reason);
            }
        }

        // Backward compat: read old provider_error key and convert
        if let Some(val) = json.get("provider_error") {
            if let Ok(old) = serde_json::from_value::<ProviderErrorMetadata>(val.clone()) {
                return Some(Self::ProviderError {
                    category: old.category,
                    message: old.message,
                    retry_after: old.retry_after,
                    previous_status: old.previous_status,
                    paused_at: old.paused_at,
                    auto_resumable: old.auto_resumable,
                    resume_attempts: old.resume_attempts,
                });
            }
        }

        None
    }

    /// Write pause_reason into task metadata JSON string.
    pub fn write_to_task_metadata(&self, existing_metadata: Option<&str>) -> String {
        let mut json: serde_json::Value = existing_metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                Self::KEY.to_string(),
                serde_json::to_value(self).unwrap_or_default(),
            );
        }

        json.to_string()
    }

    /// Remove pause_reason (and legacy provider_error) from task metadata.
    pub fn clear_from_task_metadata(existing_metadata: Option<&str>) -> String {
        let mut json: serde_json::Value = existing_metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(obj) = json.as_object_mut() {
            obj.remove(Self::KEY);
            obj.remove("provider_error"); // clean up legacy key
        }

        json.to_string()
    }

    /// Whether this is a provider error variant.
    pub fn is_provider_error(&self) -> bool {
        matches!(self, Self::ProviderError { .. })
    }

    /// The status the task was in before being paused.
    pub fn previous_status(&self) -> &str {
        match self {
            Self::UserInitiated {
                previous_status, ..
            } => previous_status,
            Self::ProviderError {
                previous_status, ..
            } => previous_status,
        }
    }
}

/// Typed error for stream processing failures.
///
/// Replaces `Result<StreamOutcome, String>` with structured variants that enable
/// precise error handling decisions (retryability, session clearing, task transitions).
#[derive(Debug, Clone)]
pub enum StreamError {
    /// No stdout output received within the line-read timeout.
    Timeout {
        context_type: ChatContextType,
        elapsed_secs: u64,
    },
    /// Stdout traffic received but no parseable stream events within the parse-stall timeout.
    ParseStall {
        context_type: ChatContextType,
        elapsed_secs: u64,
        lines_seen: usize,
        lines_parsed: usize,
    },
    /// Agent process exited with non-zero status and no meaningful output.
    AgentExit {
        exit_code: Option<i32>,
        stderr: String,
    },
    /// A local command or MCP tool failed without proving the agent process crashed.
    LocalToolFailed { message: String },
    /// Backend-owned validation rejected completion or a terminal local tool
    /// failure represents validation evidence rather than an agent crash.
    ValidationFailed { message: String },
    /// Session ID referenced in conversation not found on the Claude side.
    SessionNotFound { session_id: String },
    /// Failed to spawn the agent CLI process.
    ProcessSpawnFailed { command: String, error: String },
    /// Codex exited without a meaningful response. Terminal details are retained
    /// so progress notices cannot mask the actual empty completion.
    NoOutput {
        context_type: ChatContextType,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
        stderr: String,
    },
    /// Agent run was cancelled (e.g., user-initiated stop or prune engine).
    /// `turns_finalized` tracks how many interactive turns completed before cancellation.
    /// When > 0, the agent completed normally and the cancellation path should still
    /// transition the task (e.g., Executing → PendingReview).
    /// `completion_tool_called` indicates the agent called `execution_complete` (or equivalent
    /// completion MCP tool) before the stream was cancelled, meaning the work is done and the
    /// handler should route to the success path rather than the cancelled path.
    Cancelled {
        turns_finalized: usize,
        completion_tool_called: bool,
    },
    /// Provider/API error that is potentially recoverable (rate limits, server errors, etc.).
    /// Task should be paused rather than failed, and auto-resumed when conditions improve.
    ProviderError {
        category: ProviderErrorCategory,
        message: String,
        /// ISO 8601 timestamp when the provider limit resets
        retry_after: Option<String>,
    },
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout {
                context_type,
                elapsed_secs,
            } => write!(
                f,
                "Agent timed out: no output for {}s (context={})",
                elapsed_secs, context_type
            ),
            Self::ParseStall {
                context_type,
                elapsed_secs,
                lines_seen,
                lines_parsed,
            } => write!(
                f,
                "Agent stream stalled: {}s without parseable events (context={}, lines_seen={}, lines_parsed={})",
                elapsed_secs, context_type, lines_seen, lines_parsed
            ),
            Self::AgentExit { exit_code, stderr } => {
                if stderr.is_empty() {
                    write!(
                        f,
                        "Agent exited with non-zero status (code={:?})",
                        exit_code
                    )
                } else {
                    write!(f, "Agent failed: {}", summarize_agent_exit_stderr(stderr))
                }
            }
            Self::LocalToolFailed { message } => write!(f, "Local tool failed: {}", message),
            Self::ValidationFailed { message } => write!(f, "Validation failed: {}", message),
            Self::SessionNotFound { session_id } => {
                write!(f, "No conversation found with session ID {}", session_id)
            }
            Self::ProcessSpawnFailed { command, error } => {
                write!(f, "Failed to spawn agent ({}): {}", command, error)
            }
            Self::NoOutput {
                context_type,
                exit_code,
                exit_signal,
                stderr,
            } => {
                write!(
                    f,
                    "Codex exited without a response (context={context_type}, code={exit_code:?}, signal={exit_signal:?})"
                )?;
                if !stderr.trim().is_empty() {
                    write!(f, "; diagnostics: {}", truncate_agent_error(stderr.trim()))?;
                }
                Ok(())
            }
            Self::Cancelled {
                completion_tool_called,
                ..
            } => write!(
                f,
                "Agent run was cancelled (completion_tool_called={})",
                completion_tool_called
            ),
            Self::ProviderError {
                category, message, ..
            } => write!(f, "Provider error ({}): {}", category, message),
        }
    }
}

impl std::error::Error for StreamError {}

impl StreamError {
    /// Whether this error type is potentially retryable.
    ///
    /// Timeout and ParseStall may succeed on retry (transient stalls).
    /// SessionNotFound is retryable via session recovery.
    /// AgentExit may be retryable depending on the exit code.
    /// ProviderError is retryable after the retry_after period.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::ParseStall { .. }
                | Self::SessionNotFound { .. }
                | Self::AgentExit { .. }
                | Self::ProviderError { .. }
        )
    }

    /// Whether this error requires clearing the stored Claude session ID.
    ///
    /// SessionNotFound means the session is stale and must be cleared.
    /// Timeout/ParseStall are idle-based stalls — the session itself is still
    /// valid and can be resumed, so clearing it would destroy continuity.
    /// ProviderError does NOT require session clear — session is still valid.
    pub fn requires_session_clear(&self) -> bool {
        matches!(self, Self::SessionNotFound { .. })
    }

    /// The suggested task status to transition to after this error.
    ///
    /// Returns `None` for non-task contexts or when no transition is appropriate.
    /// `Failed` is the default for most errors; `Cancelled` for user-initiated stops.
    /// `Paused` for recoverable provider errors (rate limits, server errors, etc.).
    pub fn suggested_task_status(&self) -> Option<InternalStatus> {
        match self {
            Self::Cancelled {
                completion_tool_called: _,
                ..
            } => Some(InternalStatus::Cancelled),
            Self::ProviderError { .. } => Some(InternalStatus::Paused),
            Self::Timeout { .. }
            | Self::ParseStall { .. }
            | Self::AgentExit { .. }
            | Self::LocalToolFailed { .. }
            | Self::ValidationFailed { .. }
            | Self::SessionNotFound { .. }
            | Self::ProcessSpawnFailed { .. }
            | Self::NoOutput { .. } => Some(InternalStatus::Failed),
        }
    }

    /// Whether this is a provider/API error that should pause rather than fail.
    pub fn is_provider_error(&self) -> bool {
        matches!(self, Self::ProviderError { .. })
    }

    /// Build ProviderErrorMetadata for storing in task metadata.
    /// Only valid for ProviderError variants.
    pub fn provider_error_metadata(
        &self,
        previous_status: InternalStatus,
    ) -> Option<ProviderErrorMetadata> {
        match self {
            Self::ProviderError {
                category,
                message,
                retry_after,
            } => Some(ProviderErrorMetadata {
                category: category.clone(),
                message: message.clone(),
                retry_after: retry_after.clone(),
                previous_status: previous_status.to_string(),
                paused_at: chrono::Utc::now().to_rfc3339(),
                auto_resumable: true,
                resume_attempts: 0,
            }),
            _ => None,
        }
    }

    /// Map this stream error to an [`ExecutionFailureSource`] for recovery classification.
    ///
    /// Used by `handle_stream_error()` to populate `ExecutionRecoveryMetadata` alongside
    /// the existing flat metadata writes.
    pub fn to_execution_failure_source(&self) -> crate::domain::entities::ExecutionFailureSource {
        use crate::domain::entities::ExecutionFailureSource;
        match self {
            Self::Timeout { .. } => ExecutionFailureSource::TransientTimeout,
            Self::ParseStall { .. } => ExecutionFailureSource::ParseStall,
            Self::AgentExit { .. } => ExecutionFailureSource::AgentCrash,
            Self::LocalToolFailed { .. } => ExecutionFailureSource::LocalToolFailed,
            Self::ValidationFailed { .. } => ExecutionFailureSource::ValidationFailed,
            _ => ExecutionFailureSource::Unknown,
        }
    }
}

pub const VALIDATION_FAILED_ERROR_CODE: &str = "validation_failed";

/// Classify an error string from agent stderr/result as a provider error if applicable.
///
/// Detects patterns like:
/// - `429 {"error":{"code":"1308","message":"Usage limit reached..."}}`
/// - `Rate limit exceeded`
/// - `overloaded_error`
/// - `API_TIMEOUT_MS`
/// - HTTP status codes 401, 403, 429, 500, 502, 503, 504
pub fn classify_provider_error(error_text: &str) -> Option<StreamError> {
    let lower = error_text.to_lowercase();

    // Claude Code subscription exhaustion banner delivered as assistant text.
    if is_claude_usage_limit_banner(&lower) || lower.contains(CLAUDE_EXTRA_USAGE_PREFIX) {
        let retry_after = parse_retry_after_from_message(error_text);
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::RateLimit,
            message: truncate_error_message(error_text),
            retry_after,
        });
    }

    // 429 rate limit (z.ai style: "429 {"error":{"code":"1308","message":"Usage limit..."}}")
    if lower.contains("429") && (lower.contains("usage limit") || lower.contains("rate limit")) {
        let retry_after = parse_retry_after_from_message(error_text);
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::RateLimit,
            message: truncate_error_message(error_text),
            retry_after,
        });
    }

    // Generic rate limit patterns
    if lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
    {
        let retry_after = parse_retry_after_from_message(error_text);
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::RateLimit,
            message: truncate_error_message(error_text),
            retry_after,
        });
    }

    // Claude overloaded
    if lower.contains("overloaded_error") || lower.contains("overloaded") {
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::Overloaded,
            message: truncate_error_message(error_text),
            retry_after: None,
        });
    }

    // Auth errors
    if lower.contains("401") && (lower.contains("unauthorized") || lower.contains("invalid"))
        || lower.contains("403") && lower.contains("forbidden")
        || lower.contains("invalid api key")
        || lower.contains("invalid_api_key")
    {
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::AuthError,
            message: truncate_error_message(error_text),
            retry_after: None,
        });
    }

    // Server errors (5xx)
    for code in ["500", "502", "503", "504"] {
        if lower.contains(code)
            && (lower.contains("internal server error")
                || lower.contains("bad gateway")
                || lower.contains("service unavailable")
                || lower.contains("gateway timeout")
                || lower.contains("server error"))
        {
            return Some(StreamError::ProviderError {
                category: ProviderErrorCategory::ServerError,
                message: truncate_error_message(error_text),
                retry_after: None,
            });
        }
    }

    // Network errors
    if lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("dns resolution failed")
        || lower.contains("network is unreachable")
        || (lower.contains("api_timeout_ms") && lower.contains("try increasing"))
    {
        return Some(StreamError::ProviderError {
            category: ProviderErrorCategory::NetworkError,
            message: truncate_error_message(error_text),
            retry_after: None,
        });
    }

    None
}

/// Classify provider errors from parsed assistant text.
///
/// Assistant content is model/transcript output, so generic strings like
/// `rate_limit` are not trustworthy provider-runtime evidence. Keep only the
/// Claude subscription exhaustion banners that are known to arrive as assistant
/// content on the success path.
#[doc(hidden)]
pub fn classify_provider_error_from_assistant_content(error_text: &str) -> Option<StreamError> {
    let lower = error_text.to_lowercase();
    if is_claude_usage_limit_banner(&lower) || lower.contains(CLAUDE_EXTRA_USAGE_PREFIX) {
        classify_provider_error(error_text)
    } else {
        None
    }
}

/// Classify the terminal error for a Codex JSONL stream.
///
/// Codex `command_execution` and MCP tool errors can contain arbitrary local
/// repository output, so only runtime-level Codex errors are eligible for
/// provider backpressure classification.
#[doc(hidden)]
pub fn classify_codex_stream_failure(
    runtime_errors: &[String],
    local_tool_errors: &[String],
    exit_code: Option<i32>,
    completed_successfully: bool,
) -> Option<StreamError> {
    let runtime_errors = runtime_errors
        .iter()
        .map(String::as_str)
        .filter(|message| !message.trim().is_empty())
        .filter(|message| !is_agent_progress_noise(message))
        .collect::<Vec<_>>();
    for message in &runtime_errors {
        if let Some(provider_error) = classify_provider_error(message) {
            return Some(provider_error);
        }
    }

    if runtime_errors.len() > 1 {
        let runtime_message = runtime_errors.join("; ");
        if let Some(provider_error) = classify_provider_error(&runtime_message) {
            return Some(provider_error);
        }
    }

    // A command or MCP call can fail while the agent is still actively repairing
    // the task. Once Codex completed the turn (or RalphX accepted a completion
    // signal), those earlier failures are diagnostics, not a failed agent run.
    if completed_successfully {
        return None;
    }

    let local_error_message = local_tool_errors
        .iter()
        .map(String::as_str)
        .filter(|message| !message.trim().is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    if local_tool_errors.iter().any(|message| {
        message
            .to_lowercase()
            .contains(VALIDATION_FAILED_ERROR_CODE)
    }) {
        return Some(StreamError::ValidationFailed {
            message: bounded_diagnostic(&local_error_message),
        });
    }

    if !runtime_errors.is_empty() {
        let error_message = runtime_errors
            .iter()
            .copied()
            .chain((!local_error_message.is_empty()).then_some(local_error_message.as_str()))
            .collect::<Vec<_>>()
            .join("; ");
        return Some(StreamError::AgentExit {
            exit_code,
            stderr: bounded_diagnostic(&error_message),
        });
    }

    if local_error_message.is_empty() {
        return None;
    }

    // No runtime error, no completion signal: the stream itself ended before the
    // turn finished. Local tool diagnostics collected earlier in the turn are
    // supporting evidence, not the terminal cause — a mid-turn `rg` exit code must
    // not be reported as the reason the run failed. State the terminal fact first
    // and keep only a bounded excerpt of the diagnostics.
    Some(StreamError::LocalToolFailed {
        message: format!(
            "{STREAM_ENDED_WITHOUT_COMPLETION}; local tool diagnostics from this turn: {}",
            bounded_diagnostic(&local_error_message)
        ),
    })
}

/// Terminal fact reported when a Codex stream stops without a completion signal
/// and without a runtime error of its own.
pub(super) const STREAM_ENDED_WITHOUT_COMPLETION: &str =
    "Codex stream ended without a completion signal";

/// Bound an accumulated diagnostic blob so terminal error records stay readable.
/// Keeps head and tail because the terminal detail is usually last, and tool
/// transcripts can reach six figures of bytes.
fn bounded_diagnostic(message: &str) -> String {
    const MAX_DIAGNOSTIC_BYTES: usize = 4_000;
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return message.to_string();
    }

    let half = MAX_DIAGNOSTIC_BYTES / 2;
    let head = truncate_str(message, half);
    let mut tail_start = message.len().saturating_sub(half);
    while tail_start < message.len() && !message.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let tail = &message[tail_start..];
    let elided = message.len() - head.len() - tail.len();
    format!("{head}\n... {elided} bytes elided ...\n{tail}")
}

fn summarize_agent_exit_stderr(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let raw_line_count = trimmed
        .split(['\n', '\r'])
        .filter(|line| !line.trim().is_empty())
        .count();
    let lines = normalized_error_lines(trimmed);
    let removed_progress_noise = raw_line_count > lines.len();
    if !removed_progress_noise && trimmed.len() <= 500 && lines.len() <= 8 {
        return trimmed.to_string();
    }

    let mut ranked = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let score = agent_exit_line_score(line);
            (score > 0).then_some((index, score))
        })
        .collect::<Vec<_>>();

    if ranked.is_empty() {
        return truncate_agent_error(trimmed);
    }

    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));

    let mut selected = ranked
        .into_iter()
        .take(6)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    selected.sort_unstable();

    let summary = selected
        .into_iter()
        .map(|index| lines[index].as_str())
        .collect::<Vec<_>>()
        .join("; ");

    truncate_agent_error(&summary)
}

fn normalized_error_lines(stderr: &str) -> Vec<String> {
    stderr
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !is_agent_progress_noise(line))
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn meaningful_agent_exit_stderr(stderr: &str) -> String {
    normalized_error_lines(stderr).join("; ")
}

fn is_agent_progress_noise(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("reading additional input from stdin")
        || lower.starts_with("compiling ")
        || lower.starts_with("building [")
        || lower.starts_with("finished `")
        || lower.starts_with("running ")
        || lower.starts_with("pass [")
        || lower.starts_with("start at ")
        || lower.starts_with("duration ")
        || lower.starts_with("run  v")
        || lower.starts_with("note: run with `rust_backtrace")
}

fn agent_exit_line_score(line: &str) -> i32 {
    let lower = line.to_lowercase();

    if lower.contains("no space left on device") {
        return 120;
    }
    if lower.contains("permission denied")
        || lower.contains("invalid ignored mode")
        || lower.contains("no such file or directory")
    {
        return 110;
    }
    if lower.starts_with("caused by:") || lower.contains("failed to write") {
        return 100;
    }
    if lower.contains("test files") && lower.contains("failed") {
        return 95;
    }
    if lower.contains("tests") && lower.contains("failed") {
        return 90;
    }
    if lower.starts_with("fail ")
        || lower.starts_with("failures:")
        || lower.contains("test result: failed")
    {
        return 85;
    }
    if lower.contains("assertion `left == right` failed") || lower.contains("panicked at") {
        return 80;
    }
    if lower.starts_with("error:")
        || lower.starts_with("fatal:")
        || lower.contains("assertionerror")
        || lower.contains("failed:")
    {
        return 70;
    }
    if lower.contains("received:") || lower.contains("expected") {
        return 40;
    }

    0
}

fn truncate_agent_error(message: &str) -> String {
    const MAX_AGENT_ERROR_BYTES: usize = 1_200;
    if message.len() > MAX_AGENT_ERROR_BYTES {
        format!("{}...", truncate_str(message, MAX_AGENT_ERROR_BYTES))
    } else {
        message.to_string()
    }
}

/// Return true when stderr indicates the agent terminated because the user
/// cancelled an MCP tool call rather than because the assistant produced a
/// user-visible failure that should be serialized into the transcript.
pub fn is_nonfatal_mcp_tool_cancellation(error_text: &str) -> bool {
    let lower = error_text.to_lowercase();
    lower.contains("mcp tool call")
        && (lower.contains("user cancelled") || lower.contains("user canceled"))
}

/// Parse a retry-after timestamp from error messages.
/// Looks for patterns like "will reset at 2026-02-15 14:15:20"
#[doc(hidden)]
pub fn parse_retry_after_from_message(error_text: &str) -> Option<String> {
    // Pattern: "reset at YYYY-MM-DD HH:MM:SS"
    if let Some(idx) = error_text.find("reset at ") {
        let after = &error_text[idx + "reset at ".len()..];
        // Try to grab "YYYY-MM-DD HH:MM:SS" (19 chars)
        if let Some(candidate) = after.get(..19) {
            // Validate it looks like a datetime
            if candidate.chars().nth(4) == Some('-') && candidate.chars().nth(10) == Some(' ') {
                // Convert to RFC3339; positions 0..10 and 11..19 are ASCII-verified above
                if let (Some(date_part), Some(time_part)) =
                    (candidate.get(..10), candidate.get(11..))
                {
                    let rfc3339 = format!("{}T{}+00:00", date_part, time_part);
                    if chrono::DateTime::parse_from_rfc3339(&rfc3339).is_ok() {
                        return Some(rfc3339);
                    }
                }
            }
        }
    }

    parse_claude_reset_banner(error_text)
}

fn parse_claude_reset_banner(error_text: &str) -> Option<String> {
    let lower = error_text.to_lowercase();
    let resets_idx = lower.find("resets ")?;
    let after = error_text.get(resets_idx + "resets ".len()..)?.trim();
    let tz_start = after.find('(')?;
    let tz_end = after[tz_start..].find(')')?;
    let time_part = after.get(..tz_start)?.trim().to_lowercase();
    let tz_name = after.get(tz_start + 1..tz_start + tz_end)?.trim();
    let timezone: Tz = tz_name.parse().ok()?;

    let (clock, meridiem) = if let Some(clock) = time_part.strip_suffix("am") {
        (clock.trim(), "am")
    } else if let Some(clock) = time_part.strip_suffix("pm") {
        (clock.trim(), "pm")
    } else {
        return None;
    };

    let (hour_12, minute) = if let Some((hour, minute)) = clock.split_once(':') {
        (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?)
    } else {
        (clock.parse::<u32>().ok()?, 0)
    };
    if !(1..=12).contains(&hour_12) || minute > 59 {
        return None;
    }

    let hour_24 = match (hour_12, meridiem) {
        (12, "am") => 0,
        (12, "pm") => 12,
        (hour, "pm") => hour + 12,
        (hour, "am") => hour,
        _ => return None,
    };

    let now = Utc::now().with_timezone(&timezone);
    let today = now.date_naive();
    let candidate_today = resolve_tz_local_datetime(timezone, today, hour_24, minute)?;
    let candidate = if candidate_today <= now {
        let tomorrow = today.checked_add_signed(Duration::days(1))?;
        resolve_tz_local_datetime(timezone, tomorrow, hour_24, minute)?
    } else {
        candidate_today
    };

    Some(candidate.with_timezone(&Utc).to_rfc3339())
}

fn resolve_tz_local_datetime(
    timezone: Tz,
    date: chrono::NaiveDate,
    hour: u32,
    minute: u32,
) -> Option<chrono::DateTime<Tz>> {
    match timezone.with_ymd_and_hms(date.year(), date.month(), date.day(), hour, minute, 0) {
        LocalResult::Single(dt) => Some(dt),
        LocalResult::Ambiguous(early, late) => Some(early.min(late)),
        LocalResult::None => None,
    }
}

/// Truncate error message to reasonable length for storage.
#[doc(hidden)]
pub fn truncate_error_message(msg: &str) -> String {
    if msg.len() > 500 {
        format!("{}...", truncate_str(msg, 500))
    } else {
        msg.to_string()
    }
}

/// Classifies agent error strings into structured AppError types
///
/// # Arguments
/// * `error_message` - The error string from the agent
/// * `conversation_id` - The conversation where the error occurred
/// * `stored_session_id` - Optional stored session ID from conversation
///
/// # Returns
/// * `AppError::StaleSession` - If error indicates stale Claude session
/// * `AppError::Agent` - For all other agent errors
pub fn classify_agent_error(
    error_message: &str,
    conversation_id: &ChatConversationId,
    stored_session_id: Option<&str>,
) -> AppError {
    if error_message.contains(STALE_SESSION_ERROR) {
        if let Some(session_id) = stored_session_id {
            return AppError::StaleSession {
                session_id: session_id.to_string(),
                conversation_id: conversation_id.as_str().to_string(),
            };
        }
    }
    AppError::Agent(error_message.to_string())
}

#[cfg(test)]
#[path = "chat_service_errors_summary_tests.rs"]
mod summary_tests;

#[cfg(test)]
#[path = "chat_service_errors_tests.rs"]
mod tests;
