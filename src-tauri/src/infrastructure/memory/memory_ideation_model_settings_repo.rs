// Memory-based IdeationModelSettingsRepository implementation for testing
// Uses RwLock for thread-safe storage without a real database

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::ProjectId;
use crate::domain::ideation::{IdeationModelSettings, ModelLevel};
use crate::domain::repositories::IdeationModelSettingsRepository;

/// In-memory implementation of IdeationModelSettingsRepository for testing.
/// Two-field storage mirrors the DB semantics:
/// - `global_row`: the NULL project_id row
/// - `project_rows`: per-project override rows keyed by project_id string
pub struct MemoryIdeationModelSettingsRepository {
    global_row: Arc<RwLock<Option<IdeationModelSettings>>>,
    project_rows: Arc<RwLock<HashMap<String, IdeationModelSettings>>>,
}

impl Default for MemoryIdeationModelSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryIdeationModelSettingsRepository {
    /// Create a new empty in-memory ideation model settings repository
    pub fn new() -> Self {
        Self {
            global_row: Arc::new(RwLock::new(None)),
            project_rows: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl IdeationModelSettingsRepository for MemoryIdeationModelSettingsRepository {
    async fn get_global(
        &self,
    ) -> Result<Option<IdeationModelSettings>, Box<dyn std::error::Error>> {
        let row = self.global_row.read().await;
        Ok(row.clone())
    }

    async fn get_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<IdeationModelSettings>, Box<dyn std::error::Error>> {
        let rows = self.project_rows.read().await;
        Ok(rows.get(project_id).cloned())
    }

    async fn upsert_global(
        &self,
        primary_model: &str,
        verifier_model: &str,
        verifier_subagent_model: &str,
        ideation_subagent_model: &str,
    ) -> Result<IdeationModelSettings, Box<dyn std::error::Error>> {
        let primary = ModelLevel::from_str(primary_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let verifier = ModelLevel::from_str(verifier_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let verifier_subagent = ModelLevel::from_str(verifier_subagent_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let ideation_subagent = ModelLevel::from_str(ideation_subagent_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        let mut row = self.global_row.write().await;
        let id = row.as_ref().map(|r| r.id).unwrap_or(1);
        let settings = IdeationModelSettings {
            id,
            project_id: None,
            primary_model: primary,
            verifier_model: verifier,
            verifier_subagent_model: verifier_subagent,
            ideation_subagent_model: ideation_subagent,
            updated_at: Utc::now(),
        };
        *row = Some(settings.clone());
        Ok(settings)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        primary_model: &str,
        verifier_model: &str,
        verifier_subagent_model: &str,
        ideation_subagent_model: &str,
    ) -> Result<IdeationModelSettings, Box<dyn std::error::Error>> {
        let primary = ModelLevel::from_str(primary_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let verifier = ModelLevel::from_str(verifier_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let verifier_subagent = ModelLevel::from_str(verifier_subagent_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        let ideation_subagent = ModelLevel::from_str(ideation_subagent_model)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        let mut rows = self.project_rows.write().await;
        let id = rows
            .get(project_id)
            .map(|r| r.id)
            .unwrap_or_else(|| rows.len() as i64 + 2);
        let settings = IdeationModelSettings {
            id,
            project_id: Some(ProjectId(project_id.to_string())),
            primary_model: primary,
            verifier_model: verifier,
            verifier_subagent_model: verifier_subagent,
            ideation_subagent_model: ideation_subagent,
            updated_at: Utc::now(),
        };
        rows.insert(project_id.to_string(), settings.clone());
        Ok(settings)
    }
}

#[cfg(test)]
#[path = "memory_ideation_model_settings_repo_tests.rs"]
mod tests;
