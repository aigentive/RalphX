use super::{AgentTaskAssignmentId, TeamMemberId, TeamSessionId, TeamWorkClassification};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamWorkspaceReservationId(pub String);
impl TeamWorkspaceReservationId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl Default for TeamWorkspaceReservationId {
    fn default() -> Self {
        Self::new()
    }
}
pub fn normalize_team_writable_path(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("team writable paths must be contained normalized relative paths".to_string());
    }
    Ok(path)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamWorkspaceReservation {
    pub id: TeamWorkspaceReservationId,
    pub team_id: TeamSessionId,
    pub team_member_id: TeamMemberId,
    pub assignment_id: Option<AgentTaskAssignmentId>,
    pub team_member_generation: i64,
    pub writable_paths: Vec<String>,
    pub generated_outputs: Vec<String>,
    pub resource_locks: Vec<String>,
    pub work_classification: TeamWorkClassification,
    pub attempt_number: i64,
    pub acquired_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
}
impl TeamWorkspaceReservation {
    pub fn validate(&self) -> Result<(), String> {
        if self.attempt_number < 1
            || !matches!(
                self.work_classification,
                TeamWorkClassification::ReadOnly
                    | TeamWorkClassification::Write
                    | TeamWorkClassification::Validator
            )
        {
            return Err("invalid team workspace reservation classification or attempt".to_string());
        }
        for path in self
            .writable_paths
            .iter()
            .chain(self.generated_outputs.iter())
        {
            normalize_team_writable_path(path)?;
        }
        if self
            .resource_locks
            .iter()
            .any(|lock| lock.trim().is_empty())
        {
            return Err("team resource locks must be named".to_string());
        }
        Ok(())
    }
    pub fn may_release(&self, generation: i64, attempt: i64) -> bool {
        self.team_member_generation == generation
            && self.attempt_number == attempt
            && self.released_at.is_none()
    }
}
