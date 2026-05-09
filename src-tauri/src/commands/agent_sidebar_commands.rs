use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_workspace_response_for_state, AgentConversationResponse,
    AgentConversationWorkspaceResponse,
};
use crate::domain::entities::{ChatContextType, ChatConversation, Project, ProjectId};

const DEFAULT_LIMIT_PER_GROUP: u32 = 6;
const MAX_LIMIT_PER_GROUP: u32 = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSidebarConversationsInput {
    pub project_ids: Vec<String>,
    pub include_archived: Option<bool>,
    pub archived_only: Option<bool>,
    pub search: Option<String>,
    pub publication_states: Option<Vec<String>>,
    pub group_by: Option<String>,
    pub sort: Option<String>,
    pub limit_per_group: Option<u32>,
    pub offsets: Option<HashMap<String, u32>>,
    pub pinned_conversation_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct AgentSidebarConversationGroupsResponse {
    pub groups: Vec<AgentSidebarConversationGroupResponse>,
}

#[derive(Debug, Serialize)]
pub struct AgentSidebarConversationGroupResponse {
    pub key: String,
    pub label: String,
    pub total: i64,
    pub offset: u32,
    pub limit: u32,
    pub has_more: bool,
    pub rows: Vec<AgentSidebarConversationRowResponse>,
}

#[derive(Debug, Serialize)]
pub struct AgentSidebarConversationRowResponse {
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub ref_kind: String,
    pub ref_label: String,
    pub publication_state: String,
    pub publication_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarGroupBy {
    Project,
    Publication,
}

impl SidebarGroupBy {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("project") => Ok(Self::Project),
            Some("publication") | Some("publication_state") => Ok(Self::Publication),
            Some(value) => Err(format!("invalid sidebar group_by: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarRowSort {
    Latest,
    Az,
    Za,
}

impl SidebarRowSort {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("latest") => Ok(Self::Latest),
            Some("az") => Ok(Self::Az),
            Some("za") => Ok(Self::Za),
            Some(value) => Err(format!("invalid sidebar sort: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SidebarPublicationState {
    Active,
    Draft,
    Merged,
    Closed,
    Uncommitted,
    Unpushed,
}

impl SidebarPublicationState {
    const ALL: [Self; 6] = [
        Self::Active,
        Self::Draft,
        Self::Merged,
        Self::Closed,
        Self::Uncommitted,
        Self::Unpushed,
    ];

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "active" => Ok(Self::Active),
            "draft" => Ok(Self::Draft),
            "merged" => Ok(Self::Merged),
            "closed" => Ok(Self::Closed),
            "uncommitted" => Ok(Self::Uncommitted),
            "unpushed" => Ok(Self::Unpushed),
            value => Err(format!("invalid publication state: {value}")),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Uncommitted => "uncommitted",
            Self::Unpushed => "unpushed",
        }
    }

    fn group_label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Uncommitted => "Uncommitted",
            Self::Unpushed => "Unpushed",
        }
    }

    fn publication_label(self) -> Option<&'static str> {
        match self {
            Self::Active => None,
            Self::Draft => Some("draft"),
            Self::Merged => Some("merged"),
            Self::Closed => Some("closed"),
            Self::Uncommitted => Some("uncommitted"),
            Self::Unpushed => Some("unpushed"),
        }
    }
}

struct SidebarConversationRow {
    project_id: String,
    sort_at: DateTime<Utc>,
    is_pinned: bool,
    conversation: ChatConversation,
    workspace: Option<AgentConversationWorkspaceResponse>,
    ref_kind: &'static str,
    ref_label: String,
    publication_state: SidebarPublicationState,
}

#[tauri::command]
pub async fn list_agent_sidebar_conversations(
    input: AgentSidebarConversationsInput,
    state: State<'_, AppState>,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    list_agent_sidebar_conversations_for_app_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn list_agent_sidebar_conversations_for_app_state(
    input: AgentSidebarConversationsInput,
    state: &AppState,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    let group_by = SidebarGroupBy::parse(input.group_by.as_deref())?;
    let row_sort = SidebarRowSort::parse(input.sort.as_deref())?;
    let limit = input
        .limit_per_group
        .unwrap_or(DEFAULT_LIMIT_PER_GROUP)
        .clamp(1, MAX_LIMIT_PER_GROUP);
    let selected_states = normalize_publication_states(input.publication_states.as_deref())?;
    let selected_state_set: HashSet<SidebarPublicationState> =
        selected_states.iter().copied().collect();
    let project_ids = normalize_project_ids(input.project_ids);
    let include_archived =
        input.include_archived.unwrap_or(false) || input.archived_only.unwrap_or(false);
    let archived_only = input.archived_only.unwrap_or(false);
    let search = normalize_search(input.search.as_deref());
    let pinned_conversation_ids: HashSet<String> =
        normalize_string_set(input.pinned_conversation_ids.as_deref().unwrap_or(&[]))
            .into_iter()
            .collect();

    let mut project_labels: Vec<(String, String)> = Vec::new();
    let mut rows = Vec::new();

    for project_id_string in project_ids {
        let project_id = ProjectId::from_string(project_id_string.clone());
        let project = state
            .project_repo
            .get_by_id(&project_id)
            .await
            .map_err(|e| e.to_string())?;
        let project_label = project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| project_id_string.clone());
        let default_ref_label = default_ref_label(project.as_ref());
        project_labels.push((project_id_string.clone(), project_label));

        let workspaces = state
            .agent_conversation_workspace_repo
            .get_by_project_id(&project_id)
            .await
            .map_err(|e| e.to_string())?;
        let mut workspace_by_conversation_id = HashMap::new();
        for workspace in workspaces {
            let conversation_id = workspace.conversation_id;
            let response = agent_workspace_response_for_state(state, workspace).await?;
            workspace_by_conversation_id.insert(conversation_id, response);
        }

        let conversations = state
            .chat_conversation_repo
            .get_by_context_filtered(
                ChatContextType::Project,
                &project_id_string,
                include_archived,
            )
            .await
            .map_err(|e| e.to_string())?;

        for conversation in conversations {
            if archived_only && !conversation.is_archived() {
                continue;
            }
            if !matches_search(&conversation, search.as_deref()) {
                continue;
            }

            let workspace = workspace_by_conversation_id.remove(&conversation.id);
            let publication_state = publication_state_for_workspace(workspace.as_ref());
            if !selected_state_set.contains(&publication_state) {
                continue;
            }

            let (ref_kind, ref_label) =
                conversation_ref_display(workspace.as_ref(), default_ref_label.as_str());
            let sort_at = conversation.created_at;
            let is_pinned = pinned_conversation_ids.contains(&conversation.id.as_str());
            rows.push(SidebarConversationRow {
                project_id: project_id_string.clone(),
                sort_at,
                is_pinned,
                conversation,
                workspace,
                ref_kind,
                ref_label,
                publication_state,
            });
        }
    }

    rows.sort_by(|left, right| {
        right
            .is_pinned
            .cmp(&left.is_pinned)
            .then_with(|| compare_sidebar_rows(left, right, row_sort))
    });

    let offsets = input.offsets.unwrap_or_default();
    let groups = match group_by {
        SidebarGroupBy::Publication => publication_groups(rows, selected_states, limit, &offsets),
        SidebarGroupBy::Project => project_groups(rows, project_labels, limit, &offsets),
    };

    Ok(AgentSidebarConversationGroupsResponse { groups })
}

fn compare_sidebar_rows(
    left: &SidebarConversationRow,
    right: &SidebarConversationRow,
    sort: SidebarRowSort,
) -> std::cmp::Ordering {
    match sort {
        SidebarRowSort::Latest => right.sort_at.cmp(&left.sort_at),
        SidebarRowSort::Az => conversation_sort_title(left)
            .cmp(&conversation_sort_title(right))
            .then_with(|| right.sort_at.cmp(&left.sort_at)),
        SidebarRowSort::Za => conversation_sort_title(right)
            .cmp(&conversation_sort_title(left))
            .then_with(|| right.sort_at.cmp(&left.sort_at)),
    }
}

fn conversation_sort_title(row: &SidebarConversationRow) -> String {
    row.conversation
        .title
        .as_deref()
        .unwrap_or("Untitled agent")
        .to_lowercase()
}

fn normalize_publication_states(
    states: Option<&[String]>,
) -> Result<Vec<SidebarPublicationState>, String> {
    let Some(states) = states else {
        return Ok(SidebarPublicationState::ALL.to_vec());
    };

    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for state in states {
        let state = SidebarPublicationState::parse(state)?;
        if seen.insert(state) {
            normalized.push(state);
        }
    }

    Ok(normalized)
}

fn normalize_project_ids(project_ids: Vec<String>) -> Vec<String> {
    normalize_string_set(&project_ids)
}

fn normalize_string_set(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .filter_map(|project_id| {
            let project_id = project_id.trim().to_string();
            (!project_id.is_empty() && seen.insert(project_id.clone())).then_some(project_id)
        })
        .collect()
}

fn normalize_search(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase())
}

fn matches_search(conversation: &ChatConversation, search: Option<&str>) -> bool {
    search.map_or(true, |term| {
        conversation
            .title
            .as_deref()
            .unwrap_or("Untitled agent")
            .to_lowercase()
            .contains(term)
    })
}

fn default_ref_label(project: Option<&Project>) -> String {
    project
        .and_then(|project| project.base_branch.as_deref())
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .unwrap_or("master")
        .to_string()
}

fn conversation_ref_display(
    workspace: Option<&AgentConversationWorkspaceResponse>,
    default_ref_label: &str,
) -> (&'static str, String) {
    if let Some(pr_number) = workspace.and_then(|workspace| workspace.publication_pr_number) {
        return ("pull_request", format!("PR #{pr_number}"));
    }

    let label = workspace
        .map(|workspace| workspace.base_ref.as_str())
        .filter(|base_ref| !base_ref.trim().is_empty())
        .or_else(|| {
            workspace
                .and_then(|workspace| workspace.base_display_name.as_deref())
                .filter(|display_name| !display_name.trim().is_empty())
        })
        .unwrap_or(default_ref_label);

    ("branch", label.to_string())
}

fn publication_state_for_workspace(
    workspace: Option<&AgentConversationWorkspaceResponse>,
) -> SidebarPublicationState {
    let pr_status = workspace
        .and_then(|workspace| workspace.publication_pr_status.as_deref())
        .map(normalize_status);
    let push_status = workspace
        .and_then(|workspace| workspace.publication_push_status.as_deref())
        .map(normalize_status);

    match (pr_status.as_deref(), push_status.as_deref()) {
        (Some("merged"), _) => SidebarPublicationState::Merged,
        (Some("closed"), _) => SidebarPublicationState::Closed,
        (_, Some("needs_agent")) => SidebarPublicationState::Uncommitted,
        (_, Some("pending" | "failed" | "description_failed")) => SidebarPublicationState::Unpushed,
        (Some("draft"), _) => SidebarPublicationState::Draft,
        _ => SidebarPublicationState::Active,
    }
}

fn normalize_status(status: &str) -> String {
    status.trim().to_lowercase()
}

fn publication_groups(
    rows: Vec<SidebarConversationRow>,
    selected_states: Vec<SidebarPublicationState>,
    limit: u32,
    offsets: &HashMap<String, u32>,
) -> Vec<AgentSidebarConversationGroupResponse> {
    let mut rows_by_state: HashMap<SidebarPublicationState, Vec<SidebarConversationRow>> =
        selected_states
            .iter()
            .copied()
            .map(|state| (state, Vec::new()))
            .collect();

    for row in rows {
        if let Some(group_rows) = rows_by_state.get_mut(&row.publication_state) {
            group_rows.push(row);
        }
    }

    selected_states
        .into_iter()
        .map(|state| {
            let key = state.key().to_string();
            let rows = rows_by_state.remove(&state).unwrap_or_default();
            build_group(
                key,
                state.group_label().to_string(),
                rows,
                offsets.get(state.key()).copied().unwrap_or(0),
                limit,
            )
        })
        .collect()
}

fn project_groups(
    rows: Vec<SidebarConversationRow>,
    project_labels: Vec<(String, String)>,
    limit: u32,
    offsets: &HashMap<String, u32>,
) -> Vec<AgentSidebarConversationGroupResponse> {
    let mut rows_by_project: HashMap<String, Vec<SidebarConversationRow>> = project_labels
        .iter()
        .map(|(project_id, _)| (project_id.clone(), Vec::new()))
        .collect();

    for row in rows {
        if let Some(group_rows) = rows_by_project.get_mut(&row.project_id) {
            group_rows.push(row);
        }
    }

    project_labels
        .into_iter()
        .map(|(project_id, label)| {
            let rows = rows_by_project.remove(&project_id).unwrap_or_default();
            build_group(
                project_id.clone(),
                label,
                rows,
                offsets.get(&project_id).copied().unwrap_or(0),
                limit,
            )
        })
        .collect()
}

fn build_group(
    key: String,
    label: String,
    rows: Vec<SidebarConversationRow>,
    offset: u32,
    limit: u32,
) -> AgentSidebarConversationGroupResponse {
    let total = rows.len() as i64;
    let start = offset as usize;
    let rows = if start >= rows.len() {
        Vec::new()
    } else {
        rows.into_iter()
            .skip(start)
            .take(limit as usize)
            .map(AgentSidebarConversationRowResponse::from)
            .collect()
    };

    AgentSidebarConversationGroupResponse {
        key,
        label,
        total,
        offset,
        limit,
        has_more: i64::from(offset) + (rows.len() as i64) < total,
        rows,
    }
}

impl From<SidebarConversationRow> for AgentSidebarConversationRowResponse {
    fn from(row: SidebarConversationRow) -> Self {
        let publication_label = row
            .publication_state
            .publication_label()
            .map(str::to_string);
        Self {
            conversation: AgentConversationResponse::from(row.conversation),
            workspace: row.workspace,
            ref_kind: row.ref_kind.to_string(),
            ref_label: row.ref_label,
            publication_state: row.publication_state.key().to_string(),
            publication_label,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
        IdeationAnalysisBaseRefKind, Project,
    };

    fn sidebar_input(project_id: &ProjectId) -> AgentSidebarConversationsInput {
        AgentSidebarConversationsInput {
            project_ids: vec![project_id.as_str().to_string()],
            include_archived: None,
            archived_only: None,
            search: None,
            publication_states: None,
            group_by: Some("publication".to_string()),
            sort: None,
            limit_per_group: Some(6),
            offsets: None,
            pinned_conversation_ids: None,
        }
    }

    async fn create_project(state: &AppState, name: &str) -> Project {
        let mut project = Project::new(name.to_string(), format!("/tmp/{name}"));
        project.base_branch = Some("develop".to_string());
        state.project_repo.create(project).await.unwrap()
    }

    async fn create_conversation(
        state: &AppState,
        project_id: &ProjectId,
        title: &str,
        created_at: DateTime<Utc>,
    ) -> ChatConversation {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some(title.to_string());
        conversation.created_at = created_at;
        conversation.updated_at = created_at;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap()
    }

    async fn create_workspace(
        state: &AppState,
        conversation: &ChatConversation,
        project_id: &ProjectId,
        pr_number: Option<i64>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) {
        let mut workspace = AgentConversationWorkspace::new(
            conversation.id,
            project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "develop".to_string(),
            Some("Current branch (develop)".to_string()),
            None,
            format!("agent/{}", conversation.id),
            format!("/tmp/worktrees/{}", conversation.id),
        );
        workspace.publication_pr_number = pr_number;
        workspace.publication_pr_status = pr_status.map(str::to_string);
        workspace.publication_push_status = push_status.map(str::to_string);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn publication_grouping_returns_enriched_filtered_rows() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let merged = create_conversation(&state, &project.id, "Merged work", now).await;
        create_workspace(
            &state,
            &merged,
            &project.id,
            Some(123),
            Some("merged"),
            Some("published"),
        )
        .await;
        let unpushed = create_conversation(
            &state,
            &project.id,
            "Needs push",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(
            &state,
            &unpushed,
            &project.id,
            None,
            Some("open"),
            Some("pending"),
        )
        .await;
        let active = create_conversation(
            &state,
            &project.id,
            "Active work",
            now - chrono::Duration::minutes(2),
        )
        .await;
        create_workspace(
            &state,
            &active,
            &project.id,
            None,
            Some("open"),
            Some("published"),
        )
        .await;

        let mut input = sidebar_input(&project.id);
        input.publication_states = Some(vec!["merged".to_string(), "unpushed".to_string()]);

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].key, "merged");
        assert_eq!(response.groups[0].total, 1);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            merged.id.as_str()
        );
        assert_eq!(response.groups[0].rows[0].ref_kind, "pull_request");
        assert_eq!(response.groups[0].rows[0].ref_label, "PR #123");
        assert_eq!(
            response.groups[0].rows[0].publication_label.as_deref(),
            Some("merged")
        );
        assert_eq!(response.groups[1].key, "unpushed");
        assert_eq!(response.groups[1].total, 1);
        assert_eq!(
            response.groups[1].rows[0].conversation.id,
            unpushed.id.as_str()
        );
        assert_eq!(response.groups[1].rows[0].publication_state, "unpushed");
    }

    #[tokio::test]
    async fn publication_grouping_paginates_each_group_independently() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let newest = create_conversation(&state, &project.id, "Newest merged", now).await;
        create_workspace(&state, &newest, &project.id, Some(11), Some("merged"), None).await;
        let older = create_conversation(
            &state,
            &project.id,
            "Older merged",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(&state, &older, &project.id, Some(10), Some("merged"), None).await;

        let mut input = sidebar_input(&project.id);
        input.publication_states = Some(vec!["merged".to_string()]);
        input.limit_per_group = Some(1);
        input.offsets = Some(HashMap::from([("merged".to_string(), 1)]));

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].total, 2);
        assert_eq!(response.groups[0].offset, 1);
        assert!(!response.groups[0].has_more);
        assert_eq!(response.groups[0].rows.len(), 1);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            older.id.as_str()
        );
    }

    #[tokio::test]
    async fn publication_grouping_sorts_rows_by_requested_title_order() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let zulu = create_conversation(&state, &project.id, "Zulu merged", now).await;
        create_workspace(&state, &zulu, &project.id, Some(12), Some("merged"), None).await;
        let alpha = create_conversation(
            &state,
            &project.id,
            "Alpha merged",
            now - chrono::Duration::minutes(5),
        )
        .await;
        create_workspace(&state, &alpha, &project.id, Some(11), Some("merged"), None).await;

        let mut input = sidebar_input(&project.id);
        input.publication_states = Some(vec!["merged".to_string()]);
        input.sort = Some("az".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].rows.len(), 2);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            alpha.id.as_str()
        );
        assert_eq!(response.groups[0].rows[1].conversation.id, zulu.id.as_str());
    }

    #[tokio::test]
    async fn project_grouping_returns_project_groups_with_pinned_rows_first() {
        let state = AppState::new_test();
        let alpha = create_project(&state, "alpha").await;
        let beta = create_project(&state, "beta").await;
        let now = Utc::now();

        let newest = create_conversation(&state, &alpha.id, "Newest alpha", now).await;
        create_workspace(&state, &newest, &alpha.id, None, Some("open"), None).await;
        let pinned = create_conversation(
            &state,
            &alpha.id,
            "Pinned alpha",
            now - chrono::Duration::minutes(5),
        )
        .await;
        create_workspace(&state, &pinned, &alpha.id, Some(42), Some("open"), None).await;
        let beta_conversation = create_conversation(&state, &beta.id, "Beta work", now).await;
        create_workspace(
            &state,
            &beta_conversation,
            &beta.id,
            None,
            Some("draft"),
            None,
        )
        .await;

        let mut input = sidebar_input(&alpha.id);
        input.project_ids = vec![alpha.id.as_str().to_string(), beta.id.as_str().to_string()];
        input.group_by = Some("project".to_string());
        input.pinned_conversation_ids = Some(vec![pinned.id.as_str().to_string()]);

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].key, alpha.id.as_str());
        assert_eq!(response.groups[0].label, "alpha");
        assert_eq!(response.groups[0].total, 2);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            pinned.id.as_str()
        );
        assert_eq!(response.groups[0].rows[0].ref_label, "PR #42");
        assert_eq!(
            response.groups[0].rows[1].conversation.id,
            newest.id.as_str()
        );
        assert_eq!(response.groups[1].key, beta.id.as_str());
        assert_eq!(response.groups[1].label, "beta");
        assert_eq!(response.groups[1].total, 1);
        assert_eq!(
            response.groups[1].rows[0].conversation.id,
            beta_conversation.id.as_str()
        );
    }
}
