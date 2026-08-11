// State machine types - supporting structs for state machine operations
// Includes Blocker tracking and QA failure details

use serde::{Deserialize, Serialize};

/// Represents a task that blocks another task from proceeding.
///
/// Tasks can be blocked by other tasks (dependencies) or by
/// waiting for human input. Once all blockers are resolved,
/// the task can proceed automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    /// The ID of the blocking task or a special identifier for human input
    pub id: String,

    /// Whether this blocker has been resolved
    pub resolved: bool,
}

impl Blocker {
    const FRESHNESS_BLOCKED_PREFIX: &'static str = "FRESHNESS_BLOCKED|";
    /// Creates a new unresolved blocker with the given ID
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            resolved: false,
        }
    }

    /// Creates a new blocker for human input requirement
    pub fn human_input(reason: impl Into<String>) -> Self {
        Self {
            id: format!("human:{}", reason.into()),
            resolved: false,
        }
    }

    /// Returns true if this blocker is for human input
    pub fn is_human_input(&self) -> bool {
        self.id.starts_with("human:")
    }

    /// Returns true when an optional stored blocker reason represents human input.
    pub fn is_human_input_reason(reason: Option<&str>) -> bool {
        reason.is_some_and(|reason| Self::new(reason).is_human_input())
    }

    /// Returns true when a stored blocker reason is the terminal branch-freshness sentinel.
    pub fn is_freshness_blocked_reason(reason: Option<&str>) -> bool {
        reason.is_some_and(|reason| reason.starts_with(Self::FRESHNESS_BLOCKED_PREFIX))
    }

    /// Resolves this blocker
    pub fn resolve(&mut self) {
        self.resolved = true;
    }

    /// Returns a new blocker with resolved = true
    pub fn as_resolved(&self) -> Self {
        Self {
            id: self.id.clone(),
            resolved: true,
        }
    }
}

impl Default for Blocker {
    fn default() -> Self {
        Self {
            id: String::new(),
            resolved: false,
        }
    }
}

/// Represents a single QA test failure with details.
///
/// Used in QaFailedData to track which tests failed and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaFailure {
    /// The name or identifier of the failed test
    pub test_name: String,

    /// The error message or failure reason
    pub error: String,

    /// Optional screenshot path for visual verification failures
    pub screenshot: Option<String>,

    /// Optional expected vs actual values for assertion failures
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl QaFailure {
    /// Creates a new QA failure with just the test name and error
    pub fn new(test_name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            test_name: test_name.into(),
            error: error.into(),
            screenshot: None,
            expected: None,
            actual: None,
        }
    }

    /// Creates a QA failure with assertion details
    pub fn assertion_failure(
        test_name: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        let expected_str = expected.into();
        let actual_str = actual.into();
        Self {
            test_name: test_name.into(),
            error: format!("Expected '{}' but got '{}'", expected_str, actual_str),
            screenshot: None,
            expected: Some(expected_str),
            actual: Some(actual_str),
        }
    }

    /// Creates a QA failure with a screenshot path
    pub fn visual_failure(
        test_name: impl Into<String>,
        error: impl Into<String>,
        screenshot: impl Into<String>,
    ) -> Self {
        Self {
            test_name: test_name.into(),
            error: error.into(),
            screenshot: Some(screenshot.into()),
            expected: None,
            actual: None,
        }
    }

    /// Adds a screenshot path to this failure
    pub fn with_screenshot(mut self, path: impl Into<String>) -> Self {
        self.screenshot = Some(path.into());
        self
    }
}

impl Default for QaFailure {
    fn default() -> Self {
        Self {
            test_name: String::new(),
            error: String::new(),
            screenshot: None,
            expected: None,
            actual: None,
        }
    }
}

// ============================================================================
// State-Local Data Structs
// ============================================================================

/// Data stored with the QaFailed state.
///
/// When a task enters the qa_failed state, this struct tracks
/// the specific test failures that caused it. This allows the
/// UI to display failure details and helps with retry logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QaFailedData {
    /// List of QA test failures
    pub failures: Vec<QaFailure>,

    /// Number of retry attempts made
    pub retry_count: u32,

    /// Whether user has been notified of this failure
    pub notified: bool,
}

impl QaFailedData {
    /// Creates a new QaFailedData with the given failures
    pub fn new(failures: Vec<QaFailure>) -> Self {
        Self {
            failures,
            retry_count: 0,
            notified: false,
        }
    }

    /// Creates QaFailedData from a single failure
    pub fn single(failure: QaFailure) -> Self {
        Self::new(vec![failure])
    }

    /// Returns true if there are any failures
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }

    /// Returns the number of failures
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// Adds a failure to the list
    pub fn add_failure(&mut self, failure: QaFailure) {
        self.failures.push(failure);
    }

    /// Increments the retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    /// Marks as notified
    pub fn mark_notified(&mut self) {
        self.notified = true;
    }

    /// Returns the first failure message, if any
    pub fn first_error(&self) -> Option<&str> {
        self.failures.first().map(|f| f.error.as_str())
    }
}

/// Data stored with the Failed state.
///
/// When a task enters the failed state due to an unrecoverable
/// error during execution, this struct stores the error details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FailedData {
    /// The error message that caused the failure
    pub error: String,

    /// Optional stack trace or additional context
    pub details: Option<String>,

    /// Whether this failure was caused by a timeout
    pub is_timeout: bool,

    /// Whether user has been notified of this failure
    pub notified: bool,

    /// Number of execution attempts before reaching Failed state.
    /// Populated from `auto_retry_count_executing` metadata at transition time.
    /// Display-only — no policy enforcement.
    #[serde(default)]
    pub attempt_count: u32,
}

impl FailedData {
    /// Creates a new FailedData with the given error message
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: None,
            is_timeout: false,
            notified: false,
            attempt_count: 0,
        }
    }

    /// Creates a timeout failure
    pub fn timeout(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: None,
            is_timeout: true,
            notified: false,
            attempt_count: 0,
        }
    }

    /// Adds details to the failure
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Sets the attempt count (number of execution attempts before failure)
    pub fn with_attempt_count(mut self, count: u32) -> Self {
        self.attempt_count = count;
        self
    }

    /// Marks as notified
    pub fn mark_notified(&mut self) {
        self.notified = true;
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
