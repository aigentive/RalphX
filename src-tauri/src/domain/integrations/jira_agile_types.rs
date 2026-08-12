use serde::{Deserialize, Serialize};

/// A Jira Software board visible to the connected user for a selected project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraBoardSummary {
    pub id: String,
    pub name: String,
    pub board_type: String,
    pub project_key: Option<String>,
}

/// One ordered Jira board column and the workflow statuses mapped into it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraBoardColumn {
    pub name: String,
    pub status_ids: Vec<String>,
}

/// Jira Software board configuration used to mirror provider column grouping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraBoardConfiguration {
    pub id: String,
    pub name: String,
    pub board_type: String,
    pub columns: Vec<JiraBoardColumn>,
}

/// An active Jira sprint. IDs remain provider-stable even when names repeat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraSprintSummary {
    pub id: String,
    pub name: String,
    pub state: String,
    pub board_id: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub complete_date: Option<String>,
    pub goal: Option<String>,
}
