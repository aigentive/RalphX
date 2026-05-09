// Tauri commands for Workflow CRUD operations
// Thin layer that delegates to WorkflowRepository

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{
    ColumnBehavior, InternalStatus, WorkflowColumn, WorkflowDefaults, WorkflowId, WorkflowSchema,
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
            is_default: workflow.is_default,
            worker_profile: workflow.defaults.worker_profile,
            reviewer_profile: workflow.defaults.reviewer_profile,
        }
    }
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
    // Parse columns
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

    // Set defaults
    workflow.defaults = WorkflowDefaults {
        worker_profile: input.worker_profile,
        reviewer_profile: input.reviewer_profile,
    };

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

    // Apply updates
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

    // Check and create RalphX default workflow
    let default_id = WorkflowId::from_string("ralphx-default");
    if state
        .workflow_repo
        .get_by_id(&default_id)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        state
            .workflow_repo
            .create(WorkflowSchema::default_ralphx())
            .await
            .map_err(|e| e.to_string())?;
        created += 1;
    }

    // Check and create Jira workflow
    let jira_id = WorkflowId::from_string("jira-compatible");
    if state
        .workflow_repo
        .get_by_id(&jira_id)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        state
            .workflow_repo
            .create(WorkflowSchema::jira_compatible())
            .await
            .map_err(|e| e.to_string())?;
        created += 1;
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
    Ok(vec![
        WorkflowResponse::from(WorkflowSchema::default_ralphx()),
        WorkflowResponse::from(WorkflowSchema::jira_compatible()),
    ])
}
