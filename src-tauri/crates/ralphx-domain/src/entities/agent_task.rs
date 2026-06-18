use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use uuid::Uuid;

use super::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentTaskListId(pub String);

impl AgentTaskListId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AgentTaskListId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentTaskListId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentTaskId(pub String);

impl AgentTaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AgentTaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskState {
    Open,
    Active,
    Done,
    Dropped,
}

impl AgentTaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Active => "active",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    pub fn is_resolved(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

impl fmt::Display for AgentTaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for AgentTaskState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "dropped" => Ok(Self::Dropped),
            _ => Err(format!("Invalid agent task state: {s}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskScope {
    pub project_id: Option<ProjectId>,
    pub scope_type: String,
    pub scope_id: String,
    pub actor_agent: Option<String>,
}

impl AgentTaskScope {
    pub fn new(scope_type: impl Into<String>, scope_id: impl Into<String>) -> Self {
        Self {
            project_id: None,
            scope_type: scope_type.into(),
            scope_id: scope_id.into(),
            actor_agent: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskList {
    pub id: AgentTaskListId,
    pub project_id: Option<ProjectId>,
    pub scope_type: String,
    pub scope_id: String,
    pub list_sequence: i64,
    pub name: Option<String>,
    pub created_by_agent: Option<String>,
    pub next_task_number: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskListSummary {
    pub list_id: AgentTaskListId,
    pub list_sequence: i64,
    pub task_count: i64,
    pub open_count: i64,
    pub active_count: i64,
    pub done_count: i64,
    pub dropped_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskCreate {
    pub title: String,
    pub details: String,
    pub active_label: Option<String>,
    pub owner_agent: Option<String>,
    pub metadata: Option<Value>,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentTaskPatch {
    pub title: Option<String>,
    pub details: Option<String>,
    pub active_label: Option<Option<String>>,
    pub owner_agent: Option<Option<String>>,
    pub state: Option<AgentTaskState>,
    pub metadata_patch: Option<Value>,
    pub add_blocked_by: Vec<String>,
    pub add_blocks: Vec<String>,
    pub remove_blocked_by: Vec<String>,
    pub remove_blocks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskStateChange {
    pub from: AgentTaskState,
    pub to: AgentTaskState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskSummary {
    pub task_id: AgentTaskId,
    pub task_number: i64,
    pub title: String,
    pub state: AgentTaskState,
    pub owner_agent: Option<String>,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub availability: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskDetail {
    pub task_id: AgentTaskId,
    pub task_number: i64,
    pub title: String,
    pub details: String,
    pub active_label: Option<String>,
    pub owner_agent: Option<String>,
    pub state: AgentTaskState,
    pub metadata: Option<Value>,
    pub blocked_by: Vec<String>,
    pub unresolved_blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl AgentTaskDetail {
    pub fn availability(&self) -> &'static str {
        if self.unresolved_blocked_by.is_empty() {
            "ready"
        } else {
            "blocked"
        }
    }
}

pub fn merge_agent_task_metadata(existing: Option<Value>, patch: Value) -> Option<Value> {
    let Some(patch_object) = patch.as_object() else {
        return Some(patch);
    };

    let mut merged = existing
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    for (key, value) in patch_object {
        if value.is_null() {
            merged.remove(key);
        } else {
            merged.insert(key.clone(), value.clone());
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(Value::Object(merged))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskMutationResult {
    pub task: AgentTaskDetail,
    pub changed_fields: Vec<String>,
    pub state_change: Option<AgentTaskStateChange>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn identifiers_display_and_expose_inner_value() {
        let list_id = AgentTaskListId::from_string("list-1");
        let task_id = AgentTaskId::from_string("task-1");

        assert_eq!(list_id.as_str(), "list-1");
        assert_eq!(task_id.as_str(), "task-1");
        assert_eq!(list_id.to_string(), "list-1");
        assert_eq!(task_id.to_string(), "task-1");
        assert!(!AgentTaskListId::new().as_str().is_empty());
        assert!(!AgentTaskId::new().as_str().is_empty());
    }

    #[test]
    fn task_state_serializes_and_parses_expected_values() {
        assert_eq!(AgentTaskState::Open.as_str(), "open");
        assert_eq!(AgentTaskState::Active.to_string(), "active");
        assert_eq!(
            "done".parse::<AgentTaskState>().unwrap(),
            AgentTaskState::Done
        );
        assert_eq!(
            "dropped".parse::<AgentTaskState>().unwrap(),
            AgentTaskState::Dropped
        );
        assert!(AgentTaskState::Done.is_resolved());
        assert!(AgentTaskState::Dropped.is_resolved());
        assert!(!AgentTaskState::Open.is_resolved());
        assert!("invalid".parse::<AgentTaskState>().is_err());
    }

    #[test]
    fn scope_constructor_sets_minimal_defaults() {
        let scope = AgentTaskScope::new("conversation", "conv-1");

        assert_eq!(scope.scope_type, "conversation");
        assert_eq!(scope.scope_id, "conv-1");
        assert!(scope.project_id.is_none());
        assert!(scope.actor_agent.is_none());
    }

    #[test]
    fn detail_availability_reflects_unresolved_blockers() {
        let now = Utc::now();
        let mut detail = AgentTaskDetail {
            task_id: AgentTaskId::from_string("task-1"),
            task_number: 1,
            title: "Task".to_string(),
            details: "Details".to_string(),
            active_label: None,
            owner_agent: None,
            state: AgentTaskState::Open,
            metadata: None,
            blocked_by: vec!["2".to_string()],
            unresolved_blocked_by: vec!["2".to_string()],
            blocks: Vec::new(),
            version: 1,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        assert_eq!(detail.availability(), "blocked");
        detail.unresolved_blocked_by.clear();
        assert_eq!(detail.availability(), "ready");
    }

    #[test]
    fn metadata_merge_handles_replacements_removals_and_empty_results() {
        assert_eq!(
            merge_agent_task_metadata(Some(json!({"old": true})), json!("replace")),
            Some(json!("replace"))
        );
        assert_eq!(
            merge_agent_task_metadata(
                Some(json!({"priority": "high", "old": true})),
                json!({"old": null, "lane": "test"})
            ),
            Some(json!({"priority": "high", "lane": "test"}))
        );
        assert_eq!(
            merge_agent_task_metadata(Some(json!({"old": true})), json!({"old": null})),
            None
        );
        assert_eq!(
            merge_agent_task_metadata(None, json!({"created": true})),
            Some(json!({"created": true}))
        );
    }
}
