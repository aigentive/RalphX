// ExecutionPlan entity - represents one implementation attempt of an ideation session
//
// Each time a plan is re-accepted after session reopen, a new ExecutionPlan is created.
// This ensures unique branch names and clean separation of execution attempts.

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use super::types::{ExecutionPlanId, IdeationSessionId};

/// Status of an execution plan
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPlanStatus {
    Active,
    Superseded,
}

impl ExecutionPlanStatus {
    pub fn to_db_string(&self) -> &'static str {
        match self {
            ExecutionPlanStatus::Active => "active",
            ExecutionPlanStatus::Superseded => "superseded",
        }
    }

    pub fn from_db_string(s: &str) -> Result<Self, ParseExecutionPlanStatusError> {
        match s {
            "active" => Ok(ExecutionPlanStatus::Active),
            "superseded" => Ok(ExecutionPlanStatus::Superseded),
            _ => Err(ParseExecutionPlanStatusError(s.to_string())),
        }
    }
}

impl std::fmt::Display for ExecutionPlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_db_string())
    }
}

/// Error when parsing ExecutionPlanStatus from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseExecutionPlanStatusError(pub String);

impl std::fmt::Display for ParseExecutionPlanStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown execution plan status: '{}'", self.0)
    }
}

impl std::error::Error for ParseExecutionPlanStatusError {}

/// Runtime halt mode of an execution plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPlanHaltMode {
    Running,
    Paused,
    Stopped,
}

impl ExecutionPlanHaltMode {
    pub fn to_db_string(&self) -> &'static str {
        match self {
            ExecutionPlanHaltMode::Running => "running",
            ExecutionPlanHaltMode::Paused => "paused",
            ExecutionPlanHaltMode::Stopped => "stopped",
        }
    }

    pub fn from_db_string(s: &str) -> Result<Self, ParseExecutionPlanHaltModeError> {
        match s {
            "running" => Ok(ExecutionPlanHaltMode::Running),
            "paused" => Ok(ExecutionPlanHaltMode::Paused),
            "stopped" => Ok(ExecutionPlanHaltMode::Stopped),
            _ => Err(ParseExecutionPlanHaltModeError(s.to_string())),
        }
    }

    pub fn permits_scheduling(&self) -> bool {
        matches!(self, ExecutionPlanHaltMode::Running)
    }
}

impl std::fmt::Display for ExecutionPlanHaltMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_db_string())
    }
}

/// Error when parsing ExecutionPlanHaltMode from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseExecutionPlanHaltModeError(pub String);

impl std::fmt::Display for ParseExecutionPlanHaltModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown execution plan halt mode: '{}'", self.0)
    }
}

impl std::error::Error for ParseExecutionPlanHaltModeError {}

/// An execution plan - represents one implementation attempt of an ideation session
///
/// When a plan is re-accepted after session reopen, a new ExecutionPlan is created.
/// The plan_branch and tasks reference this execution_plan_id, ensuring unique
/// branch names and clean separation of execution attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub id: ExecutionPlanId,
    pub session_id: IdeationSessionId,
    pub status: ExecutionPlanStatus,
    pub halt_mode: ExecutionPlanHaltMode,
    pub created_at: DateTime<Utc>,
}

impl ExecutionPlan {
    pub fn new(session_id: IdeationSessionId) -> Self {
        let now = Utc::now();
        Self {
            id: ExecutionPlanId::new(),
            session_id,
            status: ExecutionPlanStatus::Active,
            halt_mode: ExecutionPlanHaltMode::Running,
            created_at: now,
        }
    }

    /// Deserialize an ExecutionPlan from a SQLite row
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let status_str: String = row.get("status")?;
        let status = ExecutionPlanStatus::from_db_string(&status_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let halt_mode_str: String = row
            .get("halt_mode")
            .unwrap_or_else(|_| ExecutionPlanHaltMode::Running.to_db_string().to_string());
        let halt_mode = ExecutionPlanHaltMode::from_db_string(&halt_mode_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(Self {
            id: ExecutionPlanId::from_string(row.get::<_, String>("id")?),
            session_id: IdeationSessionId::from_string(row.get::<_, String>("session_id")?),
            status,
            halt_mode,
            created_at: Self::parse_datetime(row.get::<_, String>("created_at")?),
        })
    }

    fn parse_datetime(s: String) -> DateTime<Utc> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
            return Utc.from_utc_datetime(&dt);
        }
        Utc::now()
    }
}
