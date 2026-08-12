//! ClickUp workspace/task records and the outbound ClickUp API port.
//!
//! The HTTP client that implements the port lives in `infrastructure`; the
//! orchestration service stays in `application`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Auth context for ClickUp REST calls.
///
/// ClickUp v1 uses a single Personal API token sent verbatim in the
/// `Authorization` header (no `Bearer` prefix, no OAuth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpAuthContext {
    pub api_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpWorkspace {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpSpace {
    pub id: String,
    pub name: String,
    pub private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpFolder {
    pub id: String,
    pub name: String,
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpList {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpStatus {
    pub id: Option<String>,
    pub status: String,
    /// ClickUp's raw `status.type` (`open`/`custom`/`done`/`closed`).
    pub status_type: String,
    /// RalphX ticketing category derived from `status_type`.
    pub category: String,
    pub color: Option<String>,
    pub orderindex: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpTag {
    pub name: String,
    pub tag_bg: Option<String>,
    pub tag_fg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpUser {
    pub id: i64,
    pub username: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpComment {
    pub id: String,
    pub body: String,
    pub author_id: Option<i64>,
    pub author_name: Option<String>,
    pub created_at: Option<String>,
    pub attachments: Vec<ClickUpAttachment>,
    pub replies: Vec<ClickUpComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpAttachment {
    pub id: Option<String>,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpTaskSummary {
    pub id: String,
    pub custom_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
    pub status_name: Option<String>,
    pub status_type: Option<String>,
    pub status_category: Option<String>,
    pub status_color: Option<String>,
    pub assignees: Vec<String>,
    pub assignee_ids: Vec<i64>,
    pub watchers: Vec<ClickUpUser>,
    pub tags: Vec<String>,
    pub sprint_names: Vec<String>,
    pub location_ids: Vec<String>,
    pub location_folder_ids: Vec<String>,
    pub location_space_ids: Vec<String>,
    pub space_id: Option<String>,
    pub folder_id: Option<String>,
    pub list_id: Option<String>,
    pub list_name: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpTaskContent {
    pub id: String,
    pub custom_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
    pub description: String,
    pub status_name: Option<String>,
    pub status_type: Option<String>,
    pub status_category: Option<String>,
    pub creator: Option<String>,
    pub assignees: Vec<String>,
    pub watchers: Vec<ClickUpUser>,
    pub tags: Vec<String>,
    pub comments: Vec<ClickUpComment>,
    pub attachments: Vec<ClickUpAttachment>,
    pub updated_at: Option<String>,
    pub space_id: Option<String>,
    pub list_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClickUpTaskListOptions {
    pub query: Option<String>,
    pub limit: Option<usize>,
    pub assignee_ids: Vec<i64>,
}

#[async_trait]
pub trait ClickUpApiClient: Send + Sync {
    async fn validate(&self, auth: &ClickUpAuthContext) -> Result<(), String>;

    async fn list_workspaces(
        &self,
        auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String>;

    async fn list_spaces(
        &self,
        _auth: &ClickUpAuthContext,
        _team_id: &str,
    ) -> Result<Vec<ClickUpSpace>, String> {
        Err("ClickUp spaces are not available for this client".to_string())
    }

    async fn list_folders(
        &self,
        _auth: &ClickUpAuthContext,
        _space_id: &str,
    ) -> Result<Vec<ClickUpFolder>, String> {
        Err("ClickUp folders are not available for this client".to_string())
    }

    async fn list_folder_lists(
        &self,
        _auth: &ClickUpAuthContext,
        _folder_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        Err("ClickUp folder lists are not available for this client".to_string())
    }

    async fn list_folderless_lists(
        &self,
        _auth: &ClickUpAuthContext,
        _space_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        Err("ClickUp folderless lists are not available for this client".to_string())
    }

    async fn list_tasks(
        &self,
        _auth: &ClickUpAuthContext,
        _team_id: &str,
        _space_ids: &[String],
        _options: ClickUpTaskListOptions,
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        Err("ClickUp tasks are not available for this client".to_string())
    }

    async fn list_tasks_for_list(
        &self,
        _auth: &ClickUpAuthContext,
        _list_id: &str,
        _options: ClickUpTaskListOptions,
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        Err("ClickUp list tasks are not available for this client".to_string())
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        Err("ClickUp task lookup is not available for this client".to_string())
    }

    async fn fetch_task_by_custom_id(
        &self,
        _auth: &ClickUpAuthContext,
        _team_id: &str,
        _task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        Err("ClickUp custom task id lookup is not available for this client".to_string())
    }

    async fn list_statuses(
        &self,
        _auth: &ClickUpAuthContext,
        _space_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        Err("ClickUp statuses are not available for this client".to_string())
    }

    async fn list_folder_statuses(
        &self,
        _auth: &ClickUpAuthContext,
        _folder_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        Err("ClickUp folder statuses are not available for this client".to_string())
    }

    async fn list_list_statuses(
        &self,
        _auth: &ClickUpAuthContext,
        _list_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        Err("ClickUp list statuses are not available for this client".to_string())
    }

    async fn current_user(&self, _auth: &ClickUpAuthContext) -> Result<ClickUpUser, String> {
        Err("ClickUp current-user lookup is not available for this client".to_string())
    }

    async fn update_task_status(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
        _status_name: &str,
    ) -> Result<(), String> {
        Err("ClickUp task status updates are not available for this client".to_string())
    }

    async fn assign_task_to_current_user(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
    ) -> Result<ClickUpUser, String> {
        Err("ClickUp task assignment is not available for this client".to_string())
    }

    async fn clear_task_assignee(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
    ) -> Result<(), String> {
        Err("ClickUp task assignee clearing is not available for this client".to_string())
    }

    async fn create_comment(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
        _body_markdown: &str,
    ) -> Result<ClickUpComment, String> {
        Err("ClickUp comments are not available for this client".to_string())
    }

    async fn set_task_tags(
        &self,
        _auth: &ClickUpAuthContext,
        _task_id: &str,
        _tags: Vec<String>,
    ) -> Result<(), String> {
        Err("ClickUp tag updates are not available for this client".to_string())
    }
}
