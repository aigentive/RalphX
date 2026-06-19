// Tauri commands for Workflow CRUD operations
// Thin layer that delegates to WorkflowRepository

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{
    ColumnBehavior, ExternalSyncConfig, InternalStatus, WorkflowColumn, WorkflowDefaults,
    WorkflowId, WorkflowSchema,
};

/// Input for creating a new column
#[derive(Debug, Deserialize)]
pub struct WorkflowColumnInput {
    pub id: String,
    pub name: String,
    pub maps_to: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub skip_review: Option<bool>,
    pub auto_advance: Option<bool>,
    pub agent_profile: Option<String>,
}

impl WorkflowColumnInput {
    #[doc(hidden)]
    pub fn to_column(&self) -> Result<WorkflowColumn, String> {
        let maps_to: InternalStatus = self
            .maps_to
            .parse()
            .map_err(|_| format!("Invalid internal status: {}", self.maps_to))?;

        let mut column = WorkflowColumn::new(&self.id, &self.name, maps_to);

        if let Some(ref color) = self.color {
            column = column.with_color(color);
        }
        if let Some(ref icon) = self.icon {
            column = column.with_icon(icon);
        }

        // Add behavior if any behavior options are set
        if self.skip_review.is_some() || self.auto_advance.is_some() || self.agent_profile.is_some()
        {
            let mut behavior = ColumnBehavior::new();
            if let Some(skip) = self.skip_review {
                behavior = behavior.with_skip_review(skip);
            }
            if let Some(advance) = self.auto_advance {
                behavior = behavior.with_auto_advance(advance);
            }
            if let Some(ref profile) = self.agent_profile {
                behavior = behavior.with_agent_profile(profile);
            }
            column = column.with_behavior(behavior);
        }

        Ok(column)
    }
}

/// Input for creating a new workflow
#[derive(Debug, Deserialize)]
pub struct CreateWorkflowInput {
    pub name: String,
    pub description: Option<String>,
    pub columns: Vec<WorkflowColumnInput>,
    pub is_default: Option<bool>,
    pub worker_profile: Option<String>,
    pub reviewer_profile: Option<String>,
    pub external_sync: Option<ExternalSyncConfig>,
}

/// Input for updating a workflow
#[derive(Debug, Deserialize)]
pub struct UpdateWorkflowInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub columns: Option<Vec<WorkflowColumnInput>>,
    pub is_default: Option<bool>,
    pub worker_profile: Option<String>,
    pub reviewer_profile: Option<String>,
    pub external_sync: Option<ExternalSyncConfig>,
}

/// Response wrapper for state group within a column
#[derive(Debug, Serialize)]
pub struct StateGroupResponse {
    pub id: String,
    pub label: String,
    pub statuses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_drag_from: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_drop_to: Option<bool>,
}

/// Response wrapper for workflow column
#[derive(Debug, Serialize)]
pub struct WorkflowColumnResponse {
    pub id: String,
    pub name: String,
    pub maps_to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_review: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_advance: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<StateGroupResponse>>,
}

impl From<&WorkflowColumn> for WorkflowColumnResponse {
    fn from(col: &WorkflowColumn) -> Self {
        let (skip_review, auto_advance, agent_profile) = match &col.behavior {
            Some(b) => (b.skip_review, b.auto_advance, b.agent_profile.clone()),
            None => (None, None, None),
        };

        let groups = col.groups.as_ref().map(|gs| {
            gs.iter()
                .map(|g| StateGroupResponse {
                    id: g.id.clone(),
                    label: g.label.clone(),
                    statuses: g.statuses.iter().map(|s| s.to_string()).collect(),
                    icon: g.icon.clone(),
                    accent_color: g.accent_color.clone(),
                    can_drag_from: g.can_drag_from,
                    can_drop_to: g.can_drop_to,
                })
                .collect()
        });

        Self {
            id: col.id.clone(),
            name: col.name.clone(),
            maps_to: col.maps_to.to_string(),
            color: col.color.clone(),
            icon: col.icon.clone(),
            skip_review,
            auto_advance,
            agent_profile,
            groups,
        }
    }
}

/// Response wrapper for workflow operations
#[derive(Debug, Serialize)]
pub struct WorkflowResponse {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub columns: Vec<WorkflowColumnResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_sync: Option<ExternalSyncConfig>,
    pub is_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_profile: Option<String>,
}

impl From<WorkflowSchema> for WorkflowResponse {
    fn from(workflow: WorkflowSchema) -> Self {
        let workflow = normalize_builtin_workflow_for_response(workflow);
        Self {
            id: workflow.id.as_str().to_string(),
            name: workflow.name,
            description: workflow.description,
            columns: workflow
                .columns
                .iter()
                .map(WorkflowColumnResponse::from)
                .collect(),
            external_sync: workflow.external_sync,
            is_default: workflow.is_default,
            worker_profile: workflow.defaults.worker_profile,
            reviewer_profile: workflow.defaults.reviewer_profile,
        }
    }
}

fn workflow_from_create_input(input: CreateWorkflowInput) -> Result<WorkflowSchema, String> {
    let columns: Result<Vec<WorkflowColumn>, String> =
        input.columns.iter().map(|c| c.to_column()).collect();
    let columns = columns?;

    let mut workflow = WorkflowSchema::new(&input.name, columns);

    if let Some(ref desc) = input.description {
        workflow = workflow.with_description(desc);
    }

    if input.is_default.unwrap_or(false) {
        workflow = workflow.as_default();
    }
    workflow.external_sync = input.external_sync;

    workflow.defaults = WorkflowDefaults {
        worker_profile: input.worker_profile,
        reviewer_profile: input.reviewer_profile,
    };

    Ok(workflow)
}

fn apply_workflow_update(
    workflow: &mut WorkflowSchema,
    input: UpdateWorkflowInput,
) -> Result<(), String> {
    if let Some(name) = input.name {
        workflow.name = name;
    }
    if let Some(description) = input.description {
        workflow.description = Some(description);
    }
    if let Some(columns_input) = input.columns {
        let columns: Result<Vec<WorkflowColumn>, String> =
            columns_input.iter().map(|c| c.to_column()).collect();
        workflow.columns = columns?;
    }
    if let Some(is_default) = input.is_default {
        workflow.is_default = is_default;
    }
    if let Some(worker_profile) = input.worker_profile {
        workflow.defaults.worker_profile = Some(worker_profile);
    }
    if let Some(reviewer_profile) = input.reviewer_profile {
        workflow.defaults.reviewer_profile = Some(reviewer_profile);
    }
    if let Some(external_sync) = input.external_sync {
        workflow.external_sync = Some(external_sync);
    }

    Ok(())
}

fn normalize_builtin_workflow_for_response(workflow: WorkflowSchema) -> WorkflowSchema {
    if workflow.id.as_str() != "ralphx-default" {
        return workflow;
    }

    let mut current = WorkflowSchema::default_ralphx();
    current.is_default = workflow.is_default;
    current.defaults = workflow.defaults;
    current
}

/// List all workflows
#[tauri::command]
pub async fn get_workflows(state: State<'_, AppState>) -> Result<Vec<WorkflowResponse>, String> {
    state
        .workflow_repo
        .get_all()
        .await
        .map(|workflows| workflows.into_iter().map(WorkflowResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get a single workflow by ID
#[tauri::command]
pub async fn get_workflow(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<WorkflowResponse>, String> {
    let workflow_id = WorkflowId::from_string(id);
    state
        .workflow_repo
        .get_by_id(&workflow_id)
        .await
        .map(|opt| opt.map(WorkflowResponse::from))
        .map_err(|e| e.to_string())
}

/// Create a new workflow
#[tauri::command]
pub async fn create_workflow(
    input: CreateWorkflowInput,
    state: State<'_, AppState>,
) -> Result<WorkflowResponse, String> {
    let workflow = workflow_from_create_input(input)?;

    state
        .workflow_repo
        .create(workflow)
        .await
        .map(WorkflowResponse::from)
        .map_err(|e| e.to_string())
}

/// Update an existing workflow
#[tauri::command]
pub async fn update_workflow(
    id: String,
    input: UpdateWorkflowInput,
    state: State<'_, AppState>,
) -> Result<WorkflowResponse, String> {
    let workflow_id = WorkflowId::from_string(id);

    // Get existing workflow
    let mut workflow = state
        .workflow_repo
        .get_by_id(&workflow_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Workflow not found: {}", workflow_id.as_str()))?;

    apply_workflow_update(&mut workflow, input)?;

    state
        .workflow_repo
        .update(&workflow)
        .await
        .map_err(|e| e.to_string())?;

    Ok(WorkflowResponse::from(workflow))
}

/// Delete a workflow
#[tauri::command]
pub async fn delete_workflow(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let workflow_id = WorkflowId::from_string(id);
    state
        .workflow_repo
        .delete(&workflow_id)
        .await
        .map_err(|e| e.to_string())
}

/// Seed builtin workflows if they don't exist
/// Returns the number of workflows created
#[tauri::command]
pub async fn seed_builtin_workflows(state: State<'_, AppState>) -> Result<usize, String> {
    let mut created = 0;

    for workflow in builtin_workflows() {
        if state
            .workflow_repo
            .get_by_id(&workflow.id)
            .await
            .map_err(|e| e.to_string())?
            .is_none()
        {
            state
                .workflow_repo
                .create(workflow)
                .await
                .map_err(|e| e.to_string())?;
            created += 1;
        }
    }

    Ok(created)
}

/// Set the default workflow
#[tauri::command]
pub async fn set_default_workflow(
    id: String,
    state: State<'_, AppState>,
) -> Result<WorkflowResponse, String> {
    let workflow_id = WorkflowId::from_string(id);

    state
        .workflow_repo
        .set_default(&workflow_id)
        .await
        .map_err(|e| e.to_string())?;

    // Return the updated workflow
    state
        .workflow_repo
        .get_by_id(&workflow_id)
        .await
        .map_err(|e| e.to_string())?
        .map(WorkflowResponse::from)
        .ok_or_else(|| format!("Workflow not found: {}", workflow_id.as_str()))
}

/// Get the columns for the currently active/default workflow
#[tauri::command]
pub async fn get_active_workflow_columns(
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowColumnResponse>, String> {
    let default_workflow = state
        .workflow_repo
        .get_default()
        .await
        .map_err(|e| e.to_string())?;

    match default_workflow {
        Some(workflow) => {
            let workflow = normalize_builtin_workflow_for_response(workflow);
            Ok(workflow
                .columns
                .iter()
                .map(WorkflowColumnResponse::from)
                .collect())
        }
        None => {
            // Return the RalphX default columns if no workflow is set
            let default = WorkflowSchema::default_ralphx();
            Ok(default
                .columns
                .iter()
                .map(WorkflowColumnResponse::from)
                .collect())
        }
    }
}

/// Get the builtin workflow schemas
#[tauri::command]
pub async fn get_builtin_workflows() -> Result<Vec<WorkflowResponse>, String> {
    Ok(builtin_workflows()
        .into_iter()
        .map(WorkflowResponse::from)
        .collect())
}

fn builtin_workflows() -> Vec<WorkflowSchema> {
    vec![
        WorkflowSchema::default_ralphx(),
        WorkflowSchema::jira_compatible(),
        WorkflowSchema::linear_compatible(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        ConflictResolution, ExternalStatusMapping, ExternalSyncConfig, SyncDirection, SyncProvider,
        SyncSettings,
    };

    #[test]
    fn column_input_maps_behavior_and_rejects_unknown_status() {
        let input = WorkflowColumnInput {
            id: "review".to_string(),
            name: "Review".to_string(),
            maps_to: "reviewing".to_string(),
            color: Some("#ff6b35".to_string()),
            icon: Some("check".to_string()),
            skip_review: Some(true),
            auto_advance: Some(false),
            agent_profile: Some("reviewer".to_string()),
        };

        let column = input.to_column().expect("column should parse");

        assert_eq!(column.id, "review");
        assert_eq!(column.name, "Review");
        assert_eq!(column.maps_to, InternalStatus::Reviewing);
        assert_eq!(column.color.as_deref(), Some("#ff6b35"));
        assert_eq!(column.icon.as_deref(), Some("check"));
        let behavior = column.behavior.expect("behavior should be populated");
        assert_eq!(behavior.skip_review, Some(true));
        assert_eq!(behavior.auto_advance, Some(false));
        assert_eq!(behavior.agent_profile.as_deref(), Some("reviewer"));

        let invalid = WorkflowColumnInput {
            maps_to: "unknown-status".to_string(),
            ..input
        };
        assert!(invalid
            .to_column()
            .unwrap_err()
            .contains("Invalid internal status"));
    }

    #[test]
    fn workflow_response_preserves_external_sync_and_defaults() {
        let mut workflow = WorkflowSchema::new(
            "Linear Workflow",
            vec![
                WorkflowColumn::new("in_progress", "In Progress", InternalStatus::Executing)
                    .with_behavior(ColumnBehavior::new().with_auto_advance(true)),
            ],
        )
        .as_default();
        workflow.description = Some("Workflow description".to_string());
        workflow.defaults = WorkflowDefaults {
            worker_profile: Some("worker".to_string()),
            reviewer_profile: Some("reviewer".to_string()),
        };
        workflow.external_sync = Some(ExternalSyncConfig {
            provider: SyncProvider::Linear,
            mapping: [(
                "In Progress".to_string(),
                ExternalStatusMapping {
                    external_status: "In Progress".to_string(),
                    internal_status: InternalStatus::Executing,
                    column_id: "in_progress".to_string(),
                },
            )]
            .into_iter()
            .collect(),
            sync: SyncSettings {
                direction: SyncDirection::Bidirectional,
                webhook: Some(true),
            },
            conflict_resolution: ConflictResolution::ExternalWins,
        });

        let response = WorkflowResponse::from(workflow);

        assert_eq!(response.name, "Linear Workflow");
        assert_eq!(
            response.description.as_deref(),
            Some("Workflow description")
        );
        assert!(response.is_default);
        assert_eq!(response.worker_profile.as_deref(), Some("worker"));
        assert_eq!(response.reviewer_profile.as_deref(), Some("reviewer"));
        assert!(response.external_sync.is_some());
        assert_eq!(response.columns.len(), 1);
        assert_eq!(response.columns[0].maps_to, "executing");
        assert_eq!(response.columns[0].auto_advance, Some(true));
    }

    #[test]
    fn create_input_builds_workflow_schema_with_defaults_and_sync() {
        let workflow = workflow_from_create_input(CreateWorkflowInput {
            name: "Custom Workflow".to_string(),
            description: Some("Created from input".to_string()),
            columns: vec![WorkflowColumnInput {
                id: "ready".to_string(),
                name: "Ready".to_string(),
                maps_to: "ready".to_string(),
                color: None,
                icon: None,
                skip_review: None,
                auto_advance: None,
                agent_profile: None,
            }],
            is_default: Some(true),
            worker_profile: Some("builder".to_string()),
            reviewer_profile: Some("reviewer".to_string()),
            external_sync: Some(ExternalSyncConfig {
                provider: SyncProvider::Linear,
                mapping: Default::default(),
                sync: SyncSettings {
                    direction: SyncDirection::Pull,
                    webhook: Some(false),
                },
                conflict_resolution: ConflictResolution::InternalWins,
            }),
        })
        .expect("create input should build workflow");

        assert_eq!(workflow.name, "Custom Workflow");
        assert_eq!(workflow.description.as_deref(), Some("Created from input"));
        assert!(workflow.is_default);
        assert_eq!(workflow.columns.len(), 1);
        assert_eq!(workflow.columns[0].maps_to, InternalStatus::Ready);
        assert_eq!(workflow.defaults.worker_profile.as_deref(), Some("builder"));
        assert_eq!(
            workflow.defaults.reviewer_profile.as_deref(),
            Some("reviewer")
        );
        assert!(workflow.external_sync.is_some());
    }

    #[test]
    fn create_input_rejects_invalid_column_status_before_repository_write() {
        let error = workflow_from_create_input(CreateWorkflowInput {
            name: "Invalid Workflow".to_string(),
            description: None,
            columns: vec![WorkflowColumnInput {
                id: "bad".to_string(),
                name: "Bad".to_string(),
                maps_to: "not-a-status".to_string(),
                color: None,
                icon: None,
                skip_review: None,
                auto_advance: None,
                agent_profile: None,
            }],
            is_default: None,
            worker_profile: None,
            reviewer_profile: None,
            external_sync: None,
        })
        .unwrap_err();

        assert!(error.contains("Invalid internal status"));
    }

    #[test]
    fn update_input_applies_only_present_fields() {
        let mut workflow = WorkflowSchema::new(
            "Original",
            vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
        );
        workflow.defaults.worker_profile = Some("old-worker".to_string());

        apply_workflow_update(
            &mut workflow,
            UpdateWorkflowInput {
                name: Some("Updated".to_string()),
                description: Some("Updated description".to_string()),
                columns: Some(vec![WorkflowColumnInput {
                    id: "done".to_string(),
                    name: "Done".to_string(),
                    maps_to: "merged".to_string(),
                    color: Some("#22c55e".to_string()),
                    icon: Some("check".to_string()),
                    skip_review: Some(true),
                    auto_advance: None,
                    agent_profile: Some("closer".to_string()),
                }]),
                is_default: Some(true),
                worker_profile: Some("new-worker".to_string()),
                reviewer_profile: None,
                external_sync: None,
            },
        )
        .expect("update should apply");

        assert_eq!(workflow.name, "Updated");
        assert_eq!(workflow.description.as_deref(), Some("Updated description"));
        assert!(workflow.is_default);
        assert_eq!(
            workflow.defaults.worker_profile.as_deref(),
            Some("new-worker")
        );
        assert!(workflow.defaults.reviewer_profile.is_none());
        assert_eq!(workflow.columns.len(), 1);
        assert_eq!(workflow.columns[0].id, "done");
        assert_eq!(workflow.columns[0].maps_to, InternalStatus::Merged);
        assert_eq!(workflow.columns[0].color.as_deref(), Some("#22c55e"));
        assert_eq!(
            workflow.columns[0]
                .behavior
                .as_ref()
                .and_then(|behavior| behavior.agent_profile.as_deref()),
            Some("closer")
        );
    }

    #[test]
    fn update_input_rejects_invalid_column_without_mutating_existing_columns() {
        let mut workflow = WorkflowSchema::new(
            "Original",
            vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
        );

        let error = apply_workflow_update(
            &mut workflow,
            UpdateWorkflowInput {
                name: None,
                description: None,
                columns: Some(vec![WorkflowColumnInput {
                    id: "bad".to_string(),
                    name: "Bad".to_string(),
                    maps_to: "unknown".to_string(),
                    color: None,
                    icon: None,
                    skip_review: None,
                    auto_advance: None,
                    agent_profile: None,
                }]),
                is_default: None,
                worker_profile: None,
                reviewer_profile: None,
                external_sync: None,
            },
        )
        .unwrap_err();

        assert!(error.contains("Invalid internal status"));
        assert_eq!(workflow.columns.len(), 1);
        assert_eq!(workflow.columns[0].id, "ready");
    }

    #[test]
    fn builtin_workflows_include_linear_compatible_schema() {
        let responses = builtin_workflows()
            .into_iter()
            .map(WorkflowResponse::from)
            .collect::<Vec<_>>();

        assert!(responses
            .iter()
            .any(|workflow| workflow.id == "linear-compat"));
        assert!(responses
            .iter()
            .any(|workflow| workflow.id == "jira-compat"));
        assert!(responses
            .iter()
            .any(|workflow| workflow.id == "ralphx-default"));
    }

    #[test]
    fn builtin_default_workflow_response_uses_current_default_shape() {
        let mut stale_default = WorkflowSchema::default_ralphx();
        stale_default.columns = vec![WorkflowColumn::new("old", "Old", InternalStatus::Ready)];
        stale_default.defaults = WorkflowDefaults {
            worker_profile: Some("worker".to_string()),
            reviewer_profile: Some("reviewer".to_string()),
        };

        let response = WorkflowResponse::from(stale_default);

        assert_eq!(response.id, "ralphx-default");
        assert!(response.columns.len() > 1);
        assert_eq!(response.worker_profile.as_deref(), Some("worker"));
        assert_eq!(response.reviewer_profile.as_deref(), Some("reviewer"));
    }
}
