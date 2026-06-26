use super::*;
use crate::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings, StoredAgentLaneSettings};
use chrono::Utc;
use std::sync::Arc;

struct MockAgentLaneSettingsRepository {
    rows: Vec<StoredAgentLaneSettings>,
}

impl MockAgentLaneSettingsRepository {
    fn new() -> Self {
        Self { rows: vec![] }
    }

    fn with_rows(rows: Vec<StoredAgentLaneSettings>) -> Self {
        Self { rows }
    }
}

#[async_trait]
impl AgentLaneSettingsRepository for MockAgentLaneSettingsRepository {
    async fn get_global(
        &self,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .iter()
            .find(|row| row.project_id.is_none() && row.lane == lane)
            .cloned())
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .iter()
            .find(|row| row.project_id.as_deref() == Some(project_id) && row.lane == lane)
            .cloned())
    }

    async fn list_global(
        &self,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .iter()
            .filter(|row| row.project_id.is_none())
            .cloned()
            .collect())
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .iter()
            .filter(|row| row.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect())
    }

    async fn upsert_global(
        &self,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        Ok(StoredAgentLaneSettings {
            id: 1,
            project_id: None,
            lane,
            settings: settings.clone(),
            updated_at: Utc::now(),
        })
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        Ok(StoredAgentLaneSettings {
            id: 2,
            project_id: Some(project_id.to_string()),
            lane,
            settings: settings.clone(),
            updated_at: Utc::now(),
        })
    }
}

#[test]
fn test_trait_object_safety() {
    let repo = MockAgentLaneSettingsRepository::new();
    let _: Arc<dyn AgentLaneSettingsRepository> = Arc::new(repo);
}

#[test]
fn test_mock_with_rows() {
    let row = StoredAgentLaneSettings {
        id: 1,
        project_id: Some("project-1".to_string()),
        lane: AgentLane::IdeationPrimary,
        settings: AgentLaneSettings {
            harness: AgentHarnessKind::Codex,
            model: Some("gpt-5.4".to_string()),
            effort: None,
            approval_policy: None,
            sandbox_mode: None,
        },
        updated_at: Utc::now(),
    };
    let repo = MockAgentLaneSettingsRepository::with_rows(vec![row.clone()]);

    assert_eq!(repo.rows.len(), 1);
    assert_eq!(repo.rows[0].lane, AgentLane::IdeationPrimary);
    assert_eq!(repo.rows[0].settings.harness, AgentHarnessKind::Codex);
}
