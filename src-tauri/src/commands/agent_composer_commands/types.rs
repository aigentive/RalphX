use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAgentComposerEntriesInput {
    pub project_id: String,
    pub conversation_id: Option<String>,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentComposerEntryResponse {
    pub path: String,
    pub kind: String,
    pub parent_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAgentComposerEntriesResponse {
    pub entries: Vec<AgentComposerEntryResponse>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAgentComposerPlanReferencesInput {
    pub project_id: String,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentComposerPlanReferenceResponse {
    pub session_id: String,
    pub artifact_id: String,
    pub title: Option<String>,
    pub status: String,
    pub artifact_version: u32,
    pub updated_at: String,
    pub approved_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAgentComposerPlanReferencesResponse {
    pub plans: Vec<AgentComposerPlanReferenceResponse>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentComposerSkillsInput {
    pub project_id: String,
    pub conversation_id: Option<String>,
    pub provider_harness: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentComposerSkillResponse {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub source: String,
    pub provider_harness: Option<String>,
    pub scope: Option<String>,
    pub invocation_kind: String,
    pub invocation_value: String,
    pub enabled: bool,
    pub source_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentComposerSkillsResponse {
    pub skills: Vec<AgentComposerSkillResponse>,
}
