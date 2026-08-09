use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::agent_workspace_publish_recovery::is_blocked_and_not_auto_retryable;
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, agent_workspace_response_with_pr_supervision_for_state,
    AgentConversationResponse, AgentConversationWorkspaceResponse,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentRunStatus, ChatContextType, ChatConversation, ChatConversationId, DelegationPark, Project,
    ProjectId, TeamMemberStatus, TeamRunBindingStatus, TeamRunTriggerKind,
};

const DEFAULT_LIMIT_PER_GROUP: u32 = 6;
/// Queued wake batches are only sampled as an activity signal; the sidebar
/// needs presence plus a stable fingerprint, not the full queue.
const SIDEBAR_WAKE_BATCH_SCAN_LIMIT: u32 = 16;
const MAX_LIMIT_PER_GROUP: u32 = 100;
const STALE_AFTER_DAYS: i64 = 7;
const STANDALONE_AUTOMATION_GROUP_KEY: &str = "__standalone__";
const STANDALONE_AUTOMATION_GROUP_LABEL: &str = "Standalone";
/// Pseudo project-group key/label for projectless (Standalone context)
/// conversations. Distinct from `STANDALONE_AUTOMATION_GROUP_KEY`, which is an
/// unrelated automation-grouping bucket for "not part of any automation run."
const NO_PROJECT_GROUP_KEY: &str = "__no_project__";
const NO_PROJECT_GROUP_LABEL: &str = "No project";
/// Upper bound on standalone-conversation rows fetched for sidebar enumeration
/// per request; matches other groups' effectively-unbounded fetch (they are
/// bounded by DB volume for a project, not paginated at the repo layer) while
/// still capping a self-keyed, cross-project query.
const NO_PROJECT_ENUMERATION_LIMIT: u32 = 500;

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
    pub priority_conversation_ids: Option<Vec<String>>,
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
    pub attention_lane: String,
    pub parked_delegate_count: usize,
    pub is_muted: bool,
    pub action_verb: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarGroupBy {
    Project,
    Publication,
    Automation,
    Inbox,
}

impl SidebarGroupBy {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("project") => Ok(Self::Project),
            Some("publication") | Some("publication_state") => Ok(Self::Publication),
            Some("automation") => Ok(Self::Automation),
            Some("inbox") => Ok(Self::Inbox),
            Some(value) => Err(format!("invalid sidebar group_by: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SidebarAttentionLane {
    Needs,
    Working,
    Stale,
    Done,
}

impl SidebarAttentionLane {
    const ALL: [Self; 4] = [Self::Needs, Self::Working, Self::Stale, Self::Done];

    fn key(self) -> &'static str {
        match self {
            Self::Needs => "needs",
            Self::Working => "working",
            Self::Stale => "stale",
            Self::Done => "done",
        }
    }

    fn group_label(self) -> &'static str {
        match self {
            Self::Needs => "Needs you",
            Self::Working => "Working",
            Self::Stale => "Stale",
            Self::Done => "Done",
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
pub(crate) enum SidebarPublicationState {
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
    conversation_id: ChatConversationId,
    project_id: String,
    automation_id: Option<String>,
    sort_at: DateTime<Utc>,
    is_pinned: bool,
    is_priority: bool,
    conversation: AgentConversationResponse,
    workspace: Option<AgentConversationWorkspaceResponse>,
    ref_kind: &'static str,
    ref_label: String,
    publication_state: SidebarPublicationState,
    attention_lane: SidebarAttentionLane,
    parked_delegate_count: usize,
    attention_state_fingerprint: String,
    is_muted: bool,
    action_verb: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedTeamActivity {
    pub(crate) is_working: bool,
    pub(crate) fingerprint: String,
}

#[tauri::command]
pub async fn list_agent_sidebar_conversations(
    input: AgentSidebarConversationsInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    list_agent_sidebar_conversations_for_app_state_impl(
        input,
        state.inner(),
        execution_state.inner(),
    )
    .await
}

#[doc(hidden)]
pub async fn list_agent_sidebar_conversations_for_app_state(
    input: AgentSidebarConversationsInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    list_agent_sidebar_conversations_for_app_state_impl(input, state, execution_state).await
}

async fn list_agent_sidebar_conversations_for_app_state_impl(
    input: AgentSidebarConversationsInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
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
    let priority_conversation_ids: HashSet<String> =
        normalize_string_set(input.priority_conversation_ids.as_deref().unwrap_or(&[]))
            .into_iter()
            .collect();
    let managed_team_activity_by_conversation =
        managed_team_activity_by_conversation(state).await?;
    let parked_delegate_counts_by_conversation =
        armed_parked_delegate_counts_by_conversation(state).await?;

    let mut project_labels: Vec<(String, String)> = Vec::new();
    let mut automation_labels: HashMap<String, String> = HashMap::new();
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

        if group_by == SidebarGroupBy::Automation {
            let automations = state
                .automation_repo
                .list_by_project(&project_id)
                .await
                .map_err(|e| e.to_string())?;
            for automation in automations {
                automation_labels.insert(
                    automation.id.as_str().to_string(),
                    automation_label_from_name(automation.id.as_str(), &automation.name),
                );
            }
        }

        let workspaces = state
            .agent_conversation_workspace_repo
            .get_by_project_id(&project_id)
            .await
            .map_err(|e| e.to_string())?;
        let mut workspace_by_conversation_id = HashMap::new();
        for workspace in workspaces {
            let conversation_id = workspace.conversation_id;
            let response = agent_workspace_response_with_pr_supervision_for_state(
                state,
                execution_state,
                workspace,
            )
            .await?;
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
            let workspace = workspace_by_conversation_id.remove(&conversation.id);
            if conversation.automation_run_id.is_some() {
                continue;
            }
            if conversation.parent_conversation_id.is_some() && workspace.is_none() {
                continue;
            }
            if archived_only && !conversation.is_archived() {
                continue;
            }
            if !matches_search(&conversation, search.as_deref()) {
                continue;
            }

            let latest_run = state
                .agent_run_repo
                .get_latest_for_conversation(&conversation.id)
                .await
                .map_err(|e| e.to_string())?;
            let latest_run_status = latest_run.as_ref().map(|run| run.status);
            let blocked_exhausted_repair = state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation.id)
                .await
                .map_err(|e| e.to_string())?
                .as_ref()
                .is_some_and(is_blocked_and_not_auto_retryable);
            let publication_state =
                publication_state_for_workspace(workspace.as_ref(), latest_run_status);
            if !selected_state_set.contains(&publication_state) {
                continue;
            }

            let (ref_kind, ref_label) =
                conversation_ref_display(workspace.as_ref(), default_ref_label.as_str());
            let parked_delegate_count = parked_delegate_counts_by_conversation
                .get(&conversation.id)
                .copied()
                .unwrap_or_default();
            let attention_lane = attention_lane_for_row_with_armed_park(
                conversation.is_archived(),
                publication_state,
                latest_run_status,
                workspace.as_ref(),
                blocked_exhausted_repair,
                conversation
                    .last_message_at
                    .unwrap_or(conversation.updated_at),
                managed_team_activity_by_conversation.get(&conversation.id),
                parked_delegate_counts_by_conversation.contains_key(&conversation.id),
            );
            let attention_state_fingerprint = attention_state_fingerprint(
                conversation.is_archived(),
                publication_state,
                latest_run.as_ref().map(|run| run.id.to_string()).as_deref(),
                latest_run_status,
                normalized_supervision_status(workspace.as_ref()).as_deref(),
                conversation.last_message_at,
                managed_team_activity_by_conversation
                    .get(&conversation.id)
                    .map(|activity| activity.fingerprint.as_str()),
            );
            let action_verb = action_verb_for_row(
                publication_state,
                latest_run_status,
                workspace.as_ref(),
                ref_kind,
            );
            let sort_at = conversation
                .last_message_at
                .unwrap_or(conversation.updated_at);
            let is_pinned = pinned_conversation_ids.contains(&conversation.id.as_str());
            let is_priority = priority_conversation_ids.contains(&conversation.id.as_str());
            // Captured before the response shadows `conversation`: the response
            // carries a plain `String` id, and mute lookups are keyed by the
            // typed conversation id.
            let conversation_id = conversation.id;
            let automation_id = conversation
                .automation_id
                .as_ref()
                .map(|automation_id| automation_id.as_str().to_string());
            let conversation = agent_conversation_response_for_state(state, conversation).await?;
            rows.push(SidebarConversationRow {
                conversation_id,
                project_id: project_id_string.clone(),
                automation_id,
                sort_at,
                is_pinned,
                is_priority,
                conversation,
                workspace,
                ref_kind,
                ref_label,
                publication_state,
                attention_lane,
                parked_delegate_count,
                attention_state_fingerprint,
                is_muted: false,
                action_verb,
            });
        }
    }

    // Standalone (projectless) conversations enumerate independently of
    // `project_ids`: they are self-keyed (context_id == conversation.id), so
    // there is no shared context_id to loop per-id like the Project branch
    // above. Always fetched (visibility of existing rows is not flag-gated —
    // only creation is). The pseudo "No project" group is added to
    // `project_labels` (used only when group_by == Project) ONLY when at
    // least one row actually qualifies — unlike the explicitly requested
    // `project_ids`, callers never ask for this group by id, so it must be
    // data-driven (mirrors automation_groups, which only emits buckets that
    // have rows) rather than always-present like the requested project groups.
    let standalone_default_ref_label = default_ref_label(None);
    let standalone_conversations = state
        .chat_conversation_repo
        .list_by_context_type(
            ChatContextType::Standalone,
            include_archived,
            NO_PROJECT_ENUMERATION_LIMIT,
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut has_no_project_rows = false;
    for conversation in standalone_conversations {
        if archived_only && !conversation.is_archived() {
            continue;
        }
        if !matches_search(&conversation, search.as_deref()) {
            continue;
        }

        let latest_run = state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .map_err(|e| e.to_string())?;
        let latest_run_status = latest_run.as_ref().map(|run| run.status);
        // Standalone (chat-only in this phase) never creates an
        // AgentConversationWorkspace, so there is no per-conversation
        // workspace lookup here (unlike the per-project loop above).
        let publication_state = publication_state_for_workspace(None, latest_run_status);
        if !selected_state_set.contains(&publication_state) {
            continue;
        }

        let (ref_kind, ref_label) =
            conversation_ref_display(None, standalone_default_ref_label.as_str());
        let parked_delegate_count = parked_delegate_counts_by_conversation
            .get(&conversation.id)
            .copied()
            .unwrap_or_default();
        let attention_lane = attention_lane_for_row_with_armed_park(
            conversation.is_archived(),
            publication_state,
            latest_run_status,
            None,
            false,
            conversation
                .last_message_at
                .unwrap_or(conversation.updated_at),
            managed_team_activity_by_conversation.get(&conversation.id),
            parked_delegate_counts_by_conversation.contains_key(&conversation.id),
        );
        let attention_state_fingerprint = attention_state_fingerprint(
            conversation.is_archived(),
            publication_state,
            latest_run.as_ref().map(|run| run.id.to_string()).as_deref(),
            latest_run_status,
            None,
            conversation.last_message_at,
            managed_team_activity_by_conversation
                .get(&conversation.id)
                .map(|activity| activity.fingerprint.as_str()),
        );
        let action_verb = action_verb_for_row(publication_state, latest_run_status, None, ref_kind);
        let sort_at = conversation
            .last_message_at
            .unwrap_or(conversation.updated_at);
        let is_pinned = pinned_conversation_ids.contains(&conversation.id.as_str());
        let is_priority = priority_conversation_ids.contains(&conversation.id.as_str());
        let conversation_id = conversation.id;
        let conversation = agent_conversation_response_for_state(state, conversation).await?;
        has_no_project_rows = true;
        rows.push(SidebarConversationRow {
            conversation_id,
            project_id: NO_PROJECT_GROUP_KEY.to_string(),
            automation_id: None,
            sort_at,
            is_pinned,
            is_priority,
            conversation,
            workspace: None,
            ref_kind,
            ref_label,
            publication_state,
            attention_lane,
            parked_delegate_count,
            attention_state_fingerprint,
            is_muted: false,
            action_verb,
        });
    }
    if has_no_project_rows {
        project_labels.push((
            NO_PROJECT_GROUP_KEY.to_string(),
            NO_PROJECT_GROUP_LABEL.to_string(),
        ));
    }

    apply_current_mutes(&mut rows, state).await?;

    rows.sort_by(|left, right| {
        right
            .is_pinned
            .cmp(&left.is_pinned)
            .then_with(|| right.is_priority.cmp(&left.is_priority))
            .then_with(|| compare_sidebar_rows(left, right, row_sort))
    });

    let offsets = input.offsets.unwrap_or_default();
    let groups = match group_by {
        SidebarGroupBy::Publication => publication_groups(rows, selected_states, limit, &offsets),
        SidebarGroupBy::Project => project_groups(rows, project_labels, row_sort, limit, &offsets),
        SidebarGroupBy::Automation => {
            automation_groups(rows, automation_labels, row_sort, limit, &offsets)
        }
        SidebarGroupBy::Inbox => inbox_groups(rows, limit, &offsets),
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

pub(crate) fn publication_state_for_workspace(
    workspace: Option<&AgentConversationWorkspaceResponse>,
    latest_run_status: Option<AgentRunStatus>,
) -> SidebarPublicationState {
    let Some(workspace) = workspace else {
        return publication_state_for_missing_workspace(latest_run_status);
    };

    publication_state_from_publication_statuses(
        workspace.publication_pr_status.as_deref(),
        workspace.publication_push_status.as_deref(),
    )
}

fn normalize_status(status: &str) -> String {
    status.trim().to_lowercase()
}

fn publication_state_from_publication_statuses(
    pr_status: Option<&str>,
    push_status: Option<&str>,
) -> SidebarPublicationState {
    let pr_status = pr_status.map(normalize_status);
    let push_status = push_status.map(normalize_status);

    match (pr_status.as_deref(), push_status.as_deref()) {
        (Some("merged"), _) => SidebarPublicationState::Merged,
        (Some("closed"), _) => SidebarPublicationState::Closed,
        (_, Some("needs_agent")) => SidebarPublicationState::Uncommitted,
        (_, Some("pending" | "failed" | "description_failed")) => SidebarPublicationState::Unpushed,
        (Some("draft"), _) => SidebarPublicationState::Draft,
        _ => SidebarPublicationState::Active,
    }
}

fn publication_state_for_missing_workspace(
    latest_run_status: Option<AgentRunStatus>,
) -> SidebarPublicationState {
    if matches!(
        latest_run_status,
        Some(AgentRunStatus::Failed | AgentRunStatus::Cancelled)
    ) {
        return SidebarPublicationState::Closed;
    }

    SidebarPublicationState::Active
}

fn is_in_flight_run_status(latest_run_status: Option<AgentRunStatus>) -> bool {
    matches!(latest_run_status, Some(AgentRunStatus::Running))
}

pub(crate) fn normalized_supervision_status(
    workspace: Option<&AgentConversationWorkspaceResponse>,
) -> Option<String> {
    normalized_supervision_status_value(
        workspace.and_then(|workspace| workspace.pr_supervision_status.as_deref()),
    )
}

fn normalized_supervision_status_value(status: Option<&str>) -> Option<String> {
    status.map(normalize_status)
}

/// Stable snapshot of the fields that determine whether a muted conversation still needs attention.
pub(crate) fn attention_state_fingerprint(
    is_archived: bool,
    publication_state: SidebarPublicationState,
    latest_run_id: Option<&str>,
    latest_run_status: Option<AgentRunStatus>,
    supervision_status: Option<&str>,
    last_message_at: Option<DateTime<Utc>>,
    managed_team_activity: Option<&str>,
) -> String {
    [
        format!("archived={is_archived}"),
        format!("publication={}", publication_state.key()),
        format!("run_id={}", latest_run_id.unwrap_or("<none>")),
        format!(
            "run_status={}",
            latest_run_status.map_or("<none>".to_string(), |status| format!("{status:?}"))
        ),
        format!("supervision={}", supervision_status.unwrap_or("<none>")),
        format!("managed_team={}", managed_team_activity.unwrap_or("<none>")),
        format!(
            "last_message_at={}",
            last_message_at.map_or("<none>".to_string(), |at| at.to_rfc3339())
        ),
    ]
    .join("\u{1f}")
}

async fn apply_current_mutes(
    rows: &mut [SidebarConversationRow],
    state: &AppState,
) -> Result<(), String> {
    let conversation_ids: Vec<ChatConversationId> =
        rows.iter().map(|row| row.conversation_id).collect();
    let mute_fingerprints: HashMap<ChatConversationId, String> = state
        .agent_conversation_mute_repo
        .list_by_conversation_ids(&conversation_ids)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|mute| (mute.conversation_id, mute.state_fingerprint))
        .collect();

    for row in rows {
        row.is_muted = mute_fingerprints
            .get(&row.conversation_id)
            .is_some_and(|fingerprint| fingerprint == &row.attention_state_fingerprint);
        if row.is_muted && row.attention_lane == SidebarAttentionLane::Needs {
            row.attention_lane = SidebarAttentionLane::Stale;
        }
    }
    Ok(())
}

#[cfg(test)]
fn attention_lane_for_row(
    is_archived: bool,
    publication_state: SidebarPublicationState,
    latest_run_status: Option<AgentRunStatus>,
    workspace: Option<&AgentConversationWorkspaceResponse>,
    blocked_exhausted_repair: bool,
    last_activity_at: DateTime<Utc>,
    managed_team_activity: Option<&ManagedTeamActivity>,
) -> SidebarAttentionLane {
    attention_lane_for_row_with_armed_park(
        is_archived,
        publication_state,
        latest_run_status,
        workspace,
        blocked_exhausted_repair,
        last_activity_at,
        managed_team_activity,
        false,
    )
}

fn attention_lane_for_row_with_armed_park(
    is_archived: bool,
    publication_state: SidebarPublicationState,
    latest_run_status: Option<AgentRunStatus>,
    workspace: Option<&AgentConversationWorkspaceResponse>,
    blocked_exhausted_repair: bool,
    last_activity_at: DateTime<Utc>,
    managed_team_activity: Option<&ManagedTeamActivity>,
    has_armed_delegation_park: bool,
) -> SidebarAttentionLane {
    if is_archived
        || matches!(
            publication_state,
            SidebarPublicationState::Merged | SidebarPublicationState::Closed
        )
    {
        return SidebarAttentionLane::Done;
    }

    if blocked_exhausted_repair {
        return SidebarAttentionLane::Needs;
    }

    let supervision_status = normalized_supervision_status(workspace);
    if is_in_flight_run_status(latest_run_status)
        || managed_team_activity.is_some_and(|activity| activity.is_working)
        || has_armed_delegation_park
        || matches!(
            supervision_status.as_deref(),
            Some("fixing" | "publishing" | "waiting" | "waiting_for_checks" | "monitoring")
        )
    {
        return SidebarAttentionLane::Working;
    }

    if last_activity_at < Utc::now() - chrono::Duration::days(STALE_AFTER_DAYS) {
        return SidebarAttentionLane::Stale;
    }

    SidebarAttentionLane::Needs
}

async fn armed_parked_delegate_counts_by_conversation(
    state: &AppState,
) -> Result<HashMap<ChatConversationId, usize>, String> {
    state
        .delegation_park_repo
        .list_armed()
        .await
        .map(parked_delegate_counts_by_conversation)
        .map_err(|error| {
            tracing::warn!(error = %error, "failed to load armed delegation parks for sidebar");
            error.to_string()
        })
}

fn parked_delegate_counts_by_conversation(
    parks: Vec<DelegationPark>,
) -> HashMap<ChatConversationId, usize> {
    let mut counts_by_conversation = HashMap::new();
    for park in parks {
        let unsettled_count = park
            .jobs
            .iter()
            .filter(|job| job.settled_status.is_none())
            .count();
        *counts_by_conversation
            .entry(park.parent_conversation_id)
            .or_default() += unsettled_count;
    }
    counts_by_conversation
}

async fn managed_team_activity_by_conversation(
    state: &AppState,
) -> Result<HashMap<ChatConversationId, ManagedTeamActivity>, String> {
    let team_repo = state.managed_team.team_repo();
    let mut activity_by_conversation = HashMap::new();

    // TeamRepository has no bulk roster/binding projection. Load open sessions
    // once, then one roster and binding list per open Team rather than per
    // sidebar row; failures are propagated so a live Team cannot become idle
    // merely because its activity read failed.
    for session in team_repo
        .list_open_sessions()
        .await
        .map_err(|error| error.to_string())?
    {
        let activity = managed_team_activity_for_session(state, &session.id).await?;
        activity_by_conversation.insert(session.coordinator_conversation_id, activity);
    }
    Ok(activity_by_conversation)
}

/// Activity projection for one open Team. The mute command must produce the
/// SAME fingerprint as the sidebar read path or a saved mute never matches.
pub(crate) async fn managed_team_activity_for_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<Option<ManagedTeamActivity>, String> {
    let session = state
        .managed_team
        .team_repo()
        .get_open_session_for_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    match session {
        Some(session) => Ok(Some(
            managed_team_activity_for_session(state, &session.id).await?,
        )),
        None => Ok(None),
    }
}

async fn managed_team_activity_for_session(
    state: &AppState,
    team_id: &crate::domain::entities::TeamSessionId,
) -> Result<ManagedTeamActivity, String> {
    let team_repo = state.managed_team.team_repo();
    let binding_repo = state.managed_team.run_binding_repo();
    let wake_batch_repo = state.managed_team.wake_batch_repo();

    let mut member_states = team_repo
        .list_members(team_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|member| {
            (
                member.id.as_str().to_string(),
                member.generation,
                member.status,
            )
        })
        .collect::<Vec<_>>();
    member_states.sort_by(|left, right| left.0.cmp(&right.0));

    let mut bindings = binding_repo
        .list_for_team(team_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|binding| {
            (
                binding.id.as_str().to_string(),
                binding.trigger_kind,
                binding.status,
            )
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.0.cmp(&right.0));

    let member_working = member_states.iter().any(|(_, _, status)| {
        matches!(
            status,
            TeamMemberStatus::Provisioning | TeamMemberStatus::Working | TeamMemberStatus::Stopping
        )
    });
    let wake_working = bindings.iter().any(|(_, trigger, status)| {
        *trigger == TeamRunTriggerKind::WakeBatch
            && matches!(
                status,
                TeamRunBindingStatus::Planned
                    | TeamRunBindingStatus::Launching
                    | TeamRunBindingStatus::Running
            )
    });
    // Unclaimed queued wake batches have no run binding yet; a queued wake
    // means a coordinator turn is pending, which is Working, not Needs.
    let mut queued_wake_ids = wake_batch_repo
        .list_queued_for_team(team_id, SIDEBAR_WAKE_BATCH_SCAN_LIMIT)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|batch| batch.id.0)
        .collect::<Vec<_>>();
    queued_wake_ids.sort();
    let wake_queued = !queued_wake_ids.is_empty();

    let fingerprint = format!(
        "members={member_states:?};wake_bindings={bindings:?};queued_wakes={queued_wake_ids:?}",
    );
    Ok(ManagedTeamActivity {
        is_working: member_working || wake_working || wake_queued,
        fingerprint,
    })
}

fn action_verb_for_row(
    publication_state: SidebarPublicationState,
    latest_run_status: Option<AgentRunStatus>,
    workspace: Option<&AgentConversationWorkspaceResponse>,
    ref_kind: &str,
) -> String {
    match publication_state {
        SidebarPublicationState::Merged => return "Merged".to_string(),
        SidebarPublicationState::Closed => return "Closed".to_string(),
        _ => {}
    }

    if is_in_flight_run_status(latest_run_status) {
        return "Running".to_string();
    }

    let supervision_status = normalized_supervision_status(workspace);
    match supervision_status.as_deref() {
        Some("fixing" | "publishing") => return "Fixing".to_string(),
        Some("waiting" | "waiting_for_checks") => return "Waiting for checks".to_string(),
        Some("monitoring")
            if workspace.and_then(|workspace| workspace.pr_auto_merge_current) == Some(true) =>
        {
            return "Auto-merging".to_string();
        }
        Some("blocked") => return "Unblock".to_string(),
        _ => {}
    }

    match publication_state {
        SidebarPublicationState::Uncommitted => "Commit changes",
        SidebarPublicationState::Unpushed => "Push changes",
        SidebarPublicationState::Draft => "Publish",
        SidebarPublicationState::Active if ref_kind == "pull_request" => "Review",
        _ => "Continue",
    }
    .to_string()
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

fn inbox_groups(
    rows: Vec<SidebarConversationRow>,
    limit: u32,
    offsets: &HashMap<String, u32>,
) -> Vec<AgentSidebarConversationGroupResponse> {
    let mut rows_by_lane: HashMap<SidebarAttentionLane, Vec<SidebarConversationRow>> =
        SidebarAttentionLane::ALL
            .iter()
            .copied()
            .map(|lane| (lane, Vec::new()))
            .collect();

    for row in rows {
        rows_by_lane
            .entry(row.attention_lane)
            .or_default()
            .push(row);
    }

    SidebarAttentionLane::ALL
        .into_iter()
        .map(|lane| {
            let key = lane.key().to_string();
            build_group(
                key,
                lane.group_label().to_string(),
                rows_by_lane.remove(&lane).unwrap_or_default(),
                offsets.get(lane.key()).copied().unwrap_or(0),
                limit,
            )
        })
        .collect()
}

fn project_groups(
    rows: Vec<SidebarConversationRow>,
    project_labels: Vec<(String, String)>,
    sort: SidebarRowSort,
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

    let mut ordered_labels = project_labels;
    if sort == SidebarRowSort::Latest {
        let latest_by_project: HashMap<&str, DateTime<Utc>> = rows_by_project
            .iter()
            .filter_map(|(pid, group_rows)| {
                group_rows
                    .iter()
                    .map(|row| row.sort_at)
                    .max()
                    .map(|ts| (pid.as_str(), ts))
            })
            .collect();
        ordered_labels.sort_by(|(a_id, _), (b_id, _)| {
            let a_ts = latest_by_project.get(a_id.as_str());
            let b_ts = latest_by_project.get(b_id.as_str());
            match (b_ts, a_ts) {
                (Some(b), Some(a)) => b.cmp(a),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
    }

    ordered_labels
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

fn automation_groups(
    rows: Vec<SidebarConversationRow>,
    automation_labels: HashMap<String, String>,
    sort: SidebarRowSort,
    limit: u32,
    offsets: &HashMap<String, u32>,
) -> Vec<AgentSidebarConversationGroupResponse> {
    let mut rows_by_group: HashMap<String, Vec<SidebarConversationRow>> = HashMap::new();
    for row in rows {
        let key = row
            .automation_id
            .clone()
            .unwrap_or_else(|| STANDALONE_AUTOMATION_GROUP_KEY.to_string());
        rows_by_group.entry(key).or_default().push(row);
    }

    let mut groups: Vec<(String, String, DateTime<Utc>, Vec<SidebarConversationRow>)> =
        rows_by_group
            .into_iter()
            .filter_map(|(key, rows)| {
                let latest = rows.iter().map(|row| row.sort_at).max()?;
                let label = automation_label_for_group(&key, &automation_labels);
                Some((key, label, latest, rows))
            })
            .collect();

    groups.sort_by(|left, right| match sort {
        SidebarRowSort::Latest => right
            .2
            .cmp(&left.2)
            .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
            .then_with(|| left.0.cmp(&right.0)),
        SidebarRowSort::Az => left
            .1
            .to_lowercase()
            .cmp(&right.1.to_lowercase())
            .then_with(|| left.0.cmp(&right.0)),
        SidebarRowSort::Za => right
            .1
            .to_lowercase()
            .cmp(&left.1.to_lowercase())
            .then_with(|| left.0.cmp(&right.0)),
    });

    groups
        .into_iter()
        .map(|(key, label, _, rows)| {
            let offset = offsets.get(&key).copied().unwrap_or(0);
            build_group(key, label, rows, offset, limit)
        })
        .collect()
}

fn automation_label_for_group(key: &str, automation_labels: &HashMap<String, String>) -> String {
    if key == STANDALONE_AUTOMATION_GROUP_KEY {
        return STANDALONE_AUTOMATION_GROUP_LABEL.to_string();
    }

    automation_labels
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback_automation_label(key))
}

fn automation_label_from_name(id: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        fallback_automation_label(id)
    } else {
        name.to_string()
    }
}

fn fallback_automation_label(id: &str) -> String {
    format!("Automation {id}")
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
        let publication_label =
            publication_label_for_workspace_response(row.workspace.as_ref(), row.publication_state);
        Self {
            conversation: row.conversation,
            workspace: row.workspace,
            ref_kind: row.ref_kind.to_string(),
            ref_label: row.ref_label,
            publication_state: row.publication_state.key().to_string(),
            publication_label,
            attention_lane: row.attention_lane.key().to_string(),
            parked_delegate_count: row.parked_delegate_count,
            is_muted: row.is_muted,
            action_verb: row.action_verb,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BulkPublicationStateResponse {
    pub publication_state: String,
    pub publication_label: Option<String>,
}

#[tauri::command]
pub async fn get_bulk_workspace_publication_states(
    conversation_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<HashMap<String, BulkPublicationStateResponse>, String> {
    get_bulk_workspace_publication_states_inner(&conversation_ids, state.inner())
        .await
        .map_err(|e| e.to_string())
}

async fn get_bulk_workspace_publication_states_inner(
    conversation_ids: &[String],
    state: &AppState,
) -> Result<HashMap<String, BulkPublicationStateResponse>, crate::error::AppError> {
    let workspace_repo = &state.agent_conversation_workspace_repo;
    let mut result = HashMap::with_capacity(conversation_ids.len());

    for id in conversation_ids {
        let conv_id = ChatConversationId::from_string(id);
        let workspace = workspace_repo.get_by_conversation_id(&conv_id).await?;
        let latest_run_status = state
            .agent_run_repo
            .get_latest_for_conversation(&conv_id)
            .await?
            .map(|run| run.status);
        let pub_state = publication_state_from_domain(workspace.as_ref(), latest_run_status);
        result.insert(
            id.clone(),
            BulkPublicationStateResponse {
                publication_state: pub_state.key().to_string(),
                publication_label: publication_label_for_domain(workspace.as_ref(), pub_state),
            },
        );
    }

    Ok(result)
}

fn publication_state_from_domain(
    workspace: Option<&crate::domain::entities::AgentConversationWorkspace>,
    latest_run_status: Option<AgentRunStatus>,
) -> SidebarPublicationState {
    let Some(workspace) = workspace else {
        return publication_state_for_missing_workspace(latest_run_status);
    };

    publication_state_from_publication_statuses(
        workspace.publication_pr_status.as_deref(),
        workspace.publication_push_status.as_deref(),
    )
}

fn publication_label_for_workspace_response(
    workspace: Option<&AgentConversationWorkspaceResponse>,
    state: SidebarPublicationState,
) -> Option<String> {
    if matches!(
        state,
        SidebarPublicationState::Active
            | SidebarPublicationState::Uncommitted
            | SidebarPublicationState::Unpushed
    ) {
        if let Some(label) = supervision_publication_label(
            workspace.and_then(|workspace| workspace.pr_supervision_status.as_deref()),
            workspace.and_then(|workspace| workspace.pr_auto_merge_current),
        ) {
            return Some(label.to_string());
        }
    }

    state.publication_label().map(str::to_string)
}

fn publication_label_for_domain(
    workspace: Option<&crate::domain::entities::AgentConversationWorkspace>,
    state: SidebarPublicationState,
) -> Option<String> {
    if matches!(
        state,
        SidebarPublicationState::Active
            | SidebarPublicationState::Uncommitted
            | SidebarPublicationState::Unpushed
    ) {
        if let Some(label) = supervision_publication_label(
            workspace.and_then(|workspace| workspace.pr_supervision_status.as_deref()),
            workspace.and_then(|workspace| workspace.pr_auto_merge_current),
        ) {
            return Some(label.to_string());
        }
    }

    state.publication_label().map(str::to_string)
}

fn supervision_publication_label(
    status: Option<&str>,
    auto_merge_current: Option<bool>,
) -> Option<&'static str> {
    match status.map(normalize_status).as_deref() {
        Some("fixing" | "publishing") => Some("fixing"),
        Some("blocked") => Some("blocked"),
        Some("waiting" | "waiting_for_checks") => Some("waiting"),
        Some("monitoring") if auto_merge_current == Some(true) => Some("auto-merge"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "agent_sidebar_commands_tests.rs"]
mod agent_sidebar_commands_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationMute, AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentRun, Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode,
        AutomationRunId, AutomationStatus, ChatConversation, IdeationAnalysisBaseRefKind, Project,
    };

    async fn list_agent_sidebar_conversations_for_app_state(
        input: AgentSidebarConversationsInput,
        state: &AppState,
    ) -> Result<AgentSidebarConversationGroupsResponse, String> {
        let execution_state = Arc::new(ExecutionState::new());
        super::list_agent_sidebar_conversations_for_app_state(input, state, &execution_state).await
    }

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
            priority_conversation_ids: None,
        }
    }

    async fn create_project(state: &AppState, name: &str) -> Project {
        let mut project = Project::new(name.to_string(), format!("/tmp/{name}"));
        project.base_branch = Some("develop".to_string());
        state.project_repo.create(project).await.unwrap()
    }

    async fn create_automation(
        state: &AppState,
        project_id: &ProjectId,
        id: &str,
        name: &str,
    ) -> Automation {
        let now = Utc::now();
        let automation = Automation {
            id: AutomationId::from_string(id),
            project_id: project_id.clone(),
            name: name.to_string(),
            status: AutomationStatus::Active,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: "Keep improving the project".to_string(),
            setup_conversation_id: None,
            provider_harness: "claude".to_string(),
            model_id: "sonnet".to_string(),
            logical_effort: None,
            run_mode: "edit".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: String::new(),
            base_display_name: None,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: "merged_base".to_string(),
            completion_signal: "pr_merged".to_string(),
            max_runs: 25,
            max_consecutive_failures: 3,
            first_run_prompt: Some("Run the next slice".to_string()),
            setup_analysis_summary: None,
            spec_artifact_id: None,
            authoring_state_json: None,
            plan_approval_mode: AutomationPlanApprovalMode::Manual,
            pr_merge_mode: AutomationPrMergeMode::Manual,
            plan_deep_verification: false,
            created_at: now,
            updated_at: now,
        };
        state.automation_repo.create(automation).await.unwrap()
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

    async fn create_standalone_conversation(
        state: &AppState,
        title: &str,
        created_at: DateTime<Utc>,
    ) -> ChatConversation {
        let mut conversation = ChatConversation::new_standalone();
        conversation.title = Some(title.to_string());
        conversation.created_at = created_at;
        conversation.updated_at = created_at;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap()
    }

    #[test]
    fn blocked_exhausted_repair_escalates_row_to_needs_lane() {
        let now = Utc::now();
        assert_eq!(
            attention_lane_for_row(
                false,
                SidebarPublicationState::Active,
                Some(AgentRunStatus::Running),
                None,
                true,
                now,
                None,
            ),
            SidebarAttentionLane::Needs
        );
        assert_eq!(
            attention_lane_for_row(
                false,
                SidebarPublicationState::Active,
                Some(AgentRunStatus::Running),
                None,
                false,
                now,
                None,
            ),
            SidebarAttentionLane::Working
        );
        assert_eq!(
            attention_lane_for_row(
                true,
                SidebarPublicationState::Merged,
                None,
                None,
                true,
                now,
                None
            ),
            SidebarAttentionLane::Done
        );
    }

    #[tokio::test]
    async fn latest_sort_uses_last_message_or_updated_activity_not_creation_time() {
        let state = AppState::new_test();
        let project = create_project(&state, "latest-activity-sort").await;
        let now = Utc::now();

        let mut created_most_recently = ChatConversation::new_project(project.id.clone());
        created_most_recently.title = Some("Stale activity".to_string());
        created_most_recently.created_at = now;
        created_most_recently.updated_at = now - chrono::Duration::minutes(30);
        created_most_recently.last_message_at = Some(now - chrono::Duration::minutes(30));
        let created_most_recently = state
            .chat_conversation_repo
            .create(created_most_recently)
            .await
            .unwrap();

        let mut fallback_to_updated = ChatConversation::new_project(project.id.clone());
        fallback_to_updated.title = Some("Updated activity".to_string());
        fallback_to_updated.created_at = now - chrono::Duration::minutes(1);
        fallback_to_updated.updated_at = now - chrono::Duration::minutes(10);
        let fallback_to_updated = state
            .chat_conversation_repo
            .create(fallback_to_updated)
            .await
            .unwrap();

        let mut latest_message = ChatConversation::new_project(project.id.clone());
        latest_message.title = Some("Latest message".to_string());
        latest_message.created_at = now - chrono::Duration::minutes(2);
        latest_message.updated_at = now - chrono::Duration::minutes(20);
        latest_message.last_message_at = Some(now);
        let latest_message = state
            .chat_conversation_repo
            .create(latest_message)
            .await
            .unwrap();

        let response =
            list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
                .await
                .expect("sidebar conversations should load");
        let conversation_ids = response
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| row.conversation.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            conversation_ids,
            vec![
                latest_message.id.as_str(),
                fallback_to_updated.id.as_str(),
                created_most_recently.id.as_str(),
            ]
        );
    }

    async fn create_automation_conversation(
        state: &AppState,
        project_id: &ProjectId,
        title: &str,
        created_at: DateTime<Utc>,
        automation_id: AutomationId,
        automation_run_id: Option<AutomationRunId>,
    ) -> ChatConversation {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some(title.to_string());
        conversation.created_at = created_at;
        conversation.updated_at = created_at;
        conversation.automation_id = Some(automation_id);
        conversation.automation_run_id = automation_run_id;
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

    async fn create_run_with_status(
        state: &AppState,
        conversation: &ChatConversation,
        status: AgentRunStatus,
    ) {
        let mut run = AgentRun::new(conversation.id);
        run.status = status;
        state.agent_run_repo.create(run).await.unwrap();
    }

    async fn set_workspace_supervision(
        state: &AppState,
        conversation: &ChatConversation,
        status: &str,
        auto_merge_current: Option<bool>,
    ) {
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await
            .unwrap()
            .unwrap();
        workspace.pr_supervision_status = Some(status.to_string());
        workspace.pr_auto_merge_current = auto_merge_current;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
    }

    #[test]
    fn attention_state_fingerprint_changes_for_every_attention_component() {
        let now = Utc::now();
        let baseline = attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            None,
        );
        assert_eq!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                true,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Draft,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-b"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Running),
                Some("blocked"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("fixing"),
                Some(now),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now + chrono::Duration::seconds(1)),
                None,
            )
        );
        assert_ne!(
            baseline,
            attention_state_fingerprint(
                false,
                SidebarPublicationState::Active,
                Some("run-a"),
                Some(AgentRunStatus::Completed),
                Some("blocked"),
                Some(now),
                Some("members=[worker:Working]"),
            )
        );
    }

    #[test]
    fn team_activity_marks_an_otherwise_idle_row_as_working() {
        let team_activity = ManagedTeamActivity {
            is_working: true,
            fingerprint: "members=[worker:Working]".to_string(),
        };

        assert_eq!(
            attention_lane_for_row(
                false,
                SidebarPublicationState::Active,
                None,
                None,
                false,
                Utc::now(),
                Some(&team_activity),
            ),
            SidebarAttentionLane::Working,
        );
        assert_eq!(
            attention_lane_for_row(
                false,
                SidebarPublicationState::Active,
                None,
                None,
                false,
                Utc::now(),
                None,
            ),
            SidebarAttentionLane::Needs,
        );
        let idle_team = ManagedTeamActivity {
            is_working: false,
            fingerprint: "members=[worker:Idle]".to_string(),
        };
        assert_eq!(
            attention_lane_for_row(
                false,
                SidebarPublicationState::Active,
                None,
                None,
                false,
                Utc::now(),
                Some(&idle_team),
            ),
            SidebarAttentionLane::Needs,
        );
    }

    #[tokio::test]
    async fn muted_needs_row_moves_to_stale_without_affecting_other_rows() {
        let state = AppState::new_test();
        let project = create_project(&state, "muted-needs").await;
        let muted = create_conversation(&state, &project.id, "Muted", Utc::now()).await;
        let other = create_conversation(&state, &project.id, "Other", Utc::now()).await;
        let fingerprint = attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            None,
            None,
            None,
            None,
            None,
        );
        state
            .agent_conversation_mute_repo
            .set_muted(AgentConversationMute {
                conversation_id: muted.id,
                muted_at: Utc::now(),
                state_fingerprint: fingerprint,
            })
            .await
            .unwrap();

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("inbox".to_string());
        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();
        let needs = response
            .groups
            .iter()
            .find(|group| group.key == "needs")
            .unwrap();
        let stale = response
            .groups
            .iter()
            .find(|group| group.key == "stale")
            .unwrap();
        assert_eq!(needs.total, 1);
        assert_eq!(needs.rows[0].conversation.id, other.id.as_str());
        assert!(!needs.rows[0].is_muted);
        assert_eq!(stale.total, 1);
        assert_eq!(stale.rows[0].conversation.id, muted.id.as_str());
        assert!(stale.rows[0].is_muted);
    }

    async fn inbox_row_for(
        state: &AppState,
        project_id: &ProjectId,
        conversation_id: &ChatConversationId,
    ) -> (String, bool) {
        let mut input = sidebar_input(project_id);
        input.group_by = Some("inbox".to_string());
        let response = list_agent_sidebar_conversations_for_app_state(input, state)
            .await
            .unwrap();
        response
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .rows
                    .iter()
                    .map(move |row| (group.key.clone(), row.conversation.id.clone(), row.is_muted))
            })
            .find(|(_, id, _)| *id == conversation_id.as_str())
            .map(|(lane, _, is_muted)| (lane, is_muted))
            .expect("conversation should appear in some lane")
    }

    async fn mute_via_command(state: &AppState, conversation_id: &ChatConversationId) {
        let execution_state = Arc::new(ExecutionState::new());
        crate::commands::agent_conversation_mute_commands::set_agent_conversation_muted_for_app_state(
            crate::commands::agent_conversation_mute_commands::SetAgentConversationMutedInput {
                conversation_id: conversation_id.as_str().to_string(),
                muted: true,
            },
            state,
            &execution_state,
        )
        .await
        .expect("mute should persist");
    }

    /// The write path and the read path must fingerprint identically. If they
    /// ever diverge, a freshly muted row still reads as unmuted and the whole
    /// feature silently does nothing.
    #[tokio::test]
    async fn muting_through_the_command_is_visible_to_the_sidebar_immediately() {
        let state = AppState::new_test();
        let project = create_project(&state, "mute-roundtrip").await;
        let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
        create_workspace(
            &state,
            &conversation,
            &project.id,
            Some(7),
            Some("open"),
            None,
        )
        .await;
        create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;

        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("needs".to_string(), false)
        );

        mute_via_command(&state, &conversation.id).await;

        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("stale".to_string(), true)
        );
    }

    #[tokio::test]
    async fn a_new_run_ends_the_mute_and_returns_the_row_to_needs() {
        let state = AppState::new_test();
        let project = create_project(&state, "mute-new-run").await;
        let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
        create_workspace(&state, &conversation, &project.id, None, Some("open"), None).await;
        create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;
        mute_via_command(&state, &conversation.id).await;
        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("stale".to_string(), true)
        );

        // Same terminal status, brand-new run: the run id alone must end the
        // mute, otherwise a rerun of the same shape stays silenced forever.
        create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;

        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("needs".to_string(), false)
        );
    }

    #[tokio::test]
    async fn a_publication_change_ends_the_mute() {
        let state = AppState::new_test();
        let project = create_project(&state, "mute-publication").await;
        let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
        create_workspace(
            &state,
            &conversation,
            &project.id,
            Some(3),
            Some("open"),
            None,
        )
        .await;
        mute_via_command(&state, &conversation.id).await;
        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("stale".to_string(), true)
        );

        create_workspace(
            &state,
            &conversation,
            &project.id,
            Some(3),
            Some("open"),
            Some("pending"),
        )
        .await;

        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("needs".to_string(), false)
        );
    }

    #[tokio::test]
    async fn a_newer_message_ends_the_mute() {
        let state = AppState::new_test();
        let project = create_project(&state, "mute-message").await;
        let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
        create_workspace(&state, &conversation, &project.id, None, Some("open"), None).await;
        mute_via_command(&state, &conversation.id).await;
        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("stale".to_string(), true)
        );

        state
            .chat_conversation_repo
            .update_message_stats(&conversation.id, 1, Utc::now())
            .await
            .unwrap();

        assert_eq!(
            inbox_row_for(&state, &project.id, &conversation.id).await,
            ("needs".to_string(), false)
        );
    }

    #[tokio::test]
    async fn muting_never_moves_a_working_or_done_row_out_of_its_lane() {
        let state = AppState::new_test();
        let project = create_project(&state, "mute-other-lanes").await;

        let working = create_conversation(&state, &project.id, "Working", Utc::now()).await;
        create_workspace(&state, &working, &project.id, None, Some("open"), None).await;
        create_run_with_status(&state, &working, AgentRunStatus::Running).await;

        let done = create_conversation(&state, &project.id, "Done", Utc::now()).await;
        create_workspace(&state, &done, &project.id, Some(9), Some("merged"), None).await;

        mute_via_command(&state, &working.id).await;
        mute_via_command(&state, &done.id).await;

        assert_eq!(
            inbox_row_for(&state, &project.id, &working.id).await,
            ("working".to_string(), true)
        );
        assert_eq!(
            inbox_row_for(&state, &project.id, &done.id).await,
            ("done".to_string(), true)
        );
    }

    #[test]
    fn sidebar_group_by_parse_accepts_known_modes_and_rejects_unknown_modes() {
        assert_eq!(
            SidebarGroupBy::parse(Some("automation")).unwrap(),
            SidebarGroupBy::Automation
        );
        assert_eq!(
            SidebarGroupBy::parse(Some("inbox")).unwrap(),
            SidebarGroupBy::Inbox
        );
        assert_eq!(
            SidebarGroupBy::parse(Some("definitely-not-valid")).unwrap_err(),
            "invalid sidebar group_by: definitely-not-valid"
        );
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
    async fn sidebar_excludes_parent_owned_child_conversations() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let parent = create_conversation(&state, &project.id, "Parent work", Utc::now()).await;
        create_workspace(
            &state,
            &parent,
            &project.id,
            None,
            Some("open"),
            Some("published"),
        )
        .await;

        let mut child = ChatConversation::new_project(project.id.clone());
        child.title = Some("Review workspace changes".to_string());
        child.parent_conversation_id = Some(parent.id.as_str().to_string());
        state
            .chat_conversation_repo
            .create(child)
            .await
            .expect("child conversation should be created");

        let response =
            list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
                .await
                .expect("sidebar conversations should load");

        let rows = &response.groups[0].rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].conversation.id, parent.id.as_str());
        assert_eq!(rows[0].conversation.title.as_deref(), Some("Parent work"));
    }

    #[tokio::test]
    async fn sidebar_includes_child_conversations_with_owned_workspaces() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let parent = create_conversation(&state, &project.id, "Parent work", now).await;
        create_workspace(
            &state,
            &parent,
            &project.id,
            Some(525),
            Some("merged"),
            Some("published"),
        )
        .await;

        let mut child = ChatConversation::new_project(project.id.clone());
        child.title = Some("Investigate follow-up".to_string());
        child.parent_conversation_id = Some(parent.id.as_str().to_string());
        child.created_at = now + chrono::Duration::minutes(1);
        child.updated_at = child.created_at;
        let child = state
            .chat_conversation_repo
            .create(child)
            .await
            .expect("child conversation should be created");
        create_workspace(&state, &child, &project.id, None, None, None).await;

        let response =
            list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
                .await
                .expect("sidebar conversations should load");

        let conversation_ids = response
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| row.conversation.id.clone())
            .collect::<Vec<_>>();
        assert!(
            conversation_ids.contains(&child.id.as_str()),
            "child conversations with their own workspace should be listed"
        );
    }

    #[tokio::test]
    async fn sidebar_shows_automation_setup_and_hides_run_conversations() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let automation_id = AutomationId::from_string("automation-1");

        let mut setup = ChatConversation::new_project(project.id.clone());
        setup.title = Some("Automation setup".to_string());
        setup.automation_id = Some(automation_id.clone());
        let setup = state
            .chat_conversation_repo
            .create(setup)
            .await
            .expect("setup conversation should be created");

        let mut run = ChatConversation::new_project(project.id.clone());
        run.title = Some("Automation run 1".to_string());
        run.automation_id = Some(automation_id);
        run.automation_run_id = Some(AutomationRunId::from_string("run-1"));
        let run = state
            .chat_conversation_repo
            .create(run)
            .await
            .expect("run conversation should be created");

        let response =
            list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
                .await
                .expect("sidebar conversations should load");

        let conversation_ids = response
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| row.conversation.id.clone())
            .collect::<Vec<_>>();
        assert!(conversation_ids.contains(&setup.id.as_str().to_string()));
        assert!(!conversation_ids.contains(&run.id.as_str().to_string()));
    }

    #[tokio::test]
    async fn automation_grouping_returns_named_and_standalone_groups_without_run_conversations() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let automation = create_automation(
            &state,
            &project.id,
            "automation-setup-owner",
            "Release Train",
        )
        .await;

        let setup = create_automation_conversation(
            &state,
            &project.id,
            "Automation setup",
            now,
            automation.id.clone(),
            None,
        )
        .await;

        let standalone = create_conversation(
            &state,
            &project.id,
            "Standalone task",
            now - chrono::Duration::minutes(5),
        )
        .await;

        let run = create_automation_conversation(
            &state,
            &project.id,
            "Automation run",
            now + chrono::Duration::minutes(5),
            automation.id.clone(),
            Some(AutomationRunId::from_string("run-1")),
        )
        .await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("automation".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .expect("automation grouping should load");

        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].key, automation.id.as_str());
        assert_eq!(response.groups[0].label, "Release Train");
        assert_eq!(response.groups[0].total, 1);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            setup.id.as_str()
        );
        assert_eq!(response.groups[1].key, "__standalone__");
        assert_eq!(response.groups[1].label, "Standalone");
        assert_eq!(response.groups[1].total, 1);
        assert_eq!(
            response.groups[1].rows[0].conversation.id,
            standalone.id.as_str()
        );
        let visible_ids = response
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| row.conversation.id.clone())
            .collect::<Vec<_>>();
        assert!(!visible_ids.contains(&run.id.as_str().to_string()));
    }

    #[tokio::test]
    async fn automation_grouping_sorts_by_fallback_label_and_paginates_visible_rows() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let fallback =
            create_automation(&state, &project.id, "automation-fallback-id", "   ").await;
        create_automation(&state, &project.id, "automation-zed", "Zed Automation").await;

        let _alpha = create_automation_conversation(
            &state,
            &project.id,
            "Alpha visible",
            now - chrono::Duration::minutes(2),
            fallback.id.clone(),
            None,
        )
        .await;

        let beta = create_automation_conversation(
            &state,
            &project.id,
            "Beta visible",
            now - chrono::Duration::minutes(1),
            fallback.id.clone(),
            None,
        )
        .await;

        let merged = create_automation_conversation(
            &state,
            &project.id,
            "Merged hidden",
            now,
            fallback.id.clone(),
            None,
        )
        .await;
        create_workspace(
            &state,
            &merged,
            &project.id,
            Some(55),
            Some("merged"),
            Some("published"),
        )
        .await;

        let zed = create_automation_conversation(
            &state,
            &project.id,
            "Zed visible",
            now,
            AutomationId::from_string("automation-zed"),
            None,
        )
        .await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("automation".to_string());
        input.sort = Some("az".to_string());
        input.publication_states = Some(vec!["active".to_string()]);
        input.limit_per_group = Some(1);
        input.offsets = Some(HashMap::from([("automation-fallback-id".to_string(), 1)]));

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .expect("automation grouping should load");

        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].key, "automation-fallback-id");
        assert_eq!(
            response.groups[0].label,
            "Automation automation-fallback-id"
        );
        assert_eq!(response.groups[0].total, 2);
        assert_eq!(response.groups[0].offset, 1);
        assert!(!response.groups[0].has_more);
        assert_eq!(response.groups[0].rows[0].conversation.id, beta.id.as_str());
        assert_eq!(response.groups[1].key, "automation-zed");
        assert_eq!(response.groups[1].label, "Zed Automation");
        assert_eq!(response.groups[1].total, 1);
        assert_eq!(response.groups[1].rows[0].conversation.id, zed.id.as_str());
    }

    #[tokio::test]
    async fn publication_grouping_keeps_failed_unpublished_workspace_active() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let stopped = create_conversation(&state, &project.id, "Stopped work", now).await;
        create_workspace(&state, &stopped, &project.id, None, None, None).await;
        create_run_with_status(&state, &stopped, AgentRunStatus::Failed).await;

        let mut input = sidebar_input(&project.id);
        input.publication_states = Some(vec!["active".to_string(), "closed".to_string()]);

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].key, "active");
        assert_eq!(response.groups[0].total, 1);
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            stopped.id.as_str()
        );
        assert_eq!(response.groups[0].rows[0].publication_state, "active");
        assert!(response.groups[0].rows[0].publication_label.is_none());
        assert_eq!(response.groups[1].key, "closed");
        assert_eq!(response.groups[1].total, 0);
    }

    #[tokio::test]
    async fn bulk_publication_states_keep_cancelled_unpublished_workspace_active() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let conversation =
            create_conversation(&state, &project.id, "Cancelled work", Utc::now()).await;
        create_workspace(&state, &conversation, &project.id, None, None, None).await;
        create_run_with_status(&state, &conversation, AgentRunStatus::Cancelled).await;

        let response = get_bulk_workspace_publication_states_inner(
            &[conversation.id.as_str().to_string()],
            &state,
        )
        .await
        .unwrap();
        let conversation_id = conversation.id.as_str();

        assert_eq!(
            response
                .get(&conversation_id)
                .map(|row| row.publication_state.as_str()),
            Some("active")
        );
        assert_eq!(
            response
                .get(&conversation_id)
                .and_then(|row| row.publication_label.as_deref()),
            None
        );
    }

    #[tokio::test]
    async fn publication_grouping_surfaces_pr_supervision_attention_labels() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let fixing = create_conversation(&state, &project.id, "Fixing PR", now).await;
        create_workspace(
            &state,
            &fixing,
            &project.id,
            Some(77),
            Some("open"),
            Some("needs_agent"),
        )
        .await;
        set_workspace_supervision(&state, &fixing, "fixing", Some(false)).await;

        let monitored = create_conversation(
            &state,
            &project.id,
            "Auto merge ready",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(
            &state,
            &monitored,
            &project.id,
            Some(78),
            Some("open"),
            Some("pushed"),
        )
        .await;
        set_workspace_supervision(&state, &monitored, "monitoring", Some(true)).await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("project".to_string());
        input.publication_states = Some(vec!["active".to_string(), "uncommitted".to_string()]);

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        let rows = &response.groups[0].rows;
        let fixing_row = rows
            .iter()
            .find(|row| row.conversation.id == fixing.id.as_str())
            .unwrap();
        assert_eq!(fixing_row.publication_state, "uncommitted");
        assert_eq!(fixing_row.publication_label.as_deref(), Some("fixing"));

        let monitored_row = rows
            .iter()
            .find(|row| row.conversation.id == monitored.id.as_str())
            .unwrap();
        assert_eq!(monitored_row.publication_state, "active");
        assert_eq!(
            monitored_row.publication_label.as_deref(),
            Some("auto-merge")
        );
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
    async fn inbox_grouping_emits_all_lanes_in_fixed_order_including_empty_ones() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("inbox".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 4);
        assert_eq!(
            response
                .groups
                .iter()
                .map(|group| (group.key.as_str(), group.label.as_str(), group.total))
                .collect::<Vec<_>>(),
            vec![
                ("needs", "Needs you", 0),
                ("working", "Working", 0),
                ("stale", "Stale", 0),
                ("done", "Done", 0),
            ]
        );
    }

    #[tokio::test]
    async fn inbox_grouping_derives_attention_lanes_and_action_verbs() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();

        let merged = create_conversation(&state, &project.id, "Merged", now).await;
        create_workspace(&state, &merged, &project.id, Some(1), Some("merged"), None).await;

        let running = create_conversation(&state, &project.id, "Running", now).await;
        create_workspace(&state, &running, &project.id, None, Some("open"), None).await;
        create_run_with_status(&state, &running, AgentRunStatus::Running).await;

        let fixing = create_conversation(&state, &project.id, "Fixing", now).await;
        create_workspace(&state, &fixing, &project.id, Some(2), Some("open"), None).await;
        set_workspace_supervision(&state, &fixing, "fixing", Some(false)).await;

        let blocked = create_conversation(&state, &project.id, "Blocked", now).await;
        create_workspace(&state, &blocked, &project.id, Some(3), Some("open"), None).await;
        set_workspace_supervision(&state, &blocked, "blocked", Some(false)).await;

        let stale = create_conversation(
            &state,
            &project.id,
            "Stale",
            now - chrono::Duration::days(STALE_AFTER_DAYS + 1),
        )
        .await;
        create_workspace(&state, &stale, &project.id, None, Some("open"), None).await;

        let fresh = create_conversation(&state, &project.id, "Fresh", now).await;
        create_workspace(&state, &fresh, &project.id, None, Some("open"), None).await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("inbox".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();
        let rows = response
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .map(|row| (row.conversation.id.clone(), row))
            .collect::<HashMap<_, _>>();

        assert_eq!(rows[&merged.id.as_str()].attention_lane, "done");
        assert_eq!(rows[&merged.id.as_str()].action_verb, "Merged");
        assert_eq!(rows[&running.id.as_str()].attention_lane, "working");
        assert_eq!(rows[&running.id.as_str()].action_verb, "Running");
        assert_eq!(rows[&fixing.id.as_str()].attention_lane, "working");
        assert_eq!(rows[&fixing.id.as_str()].action_verb, "Fixing");
        assert_ne!(rows[&blocked.id.as_str()].attention_lane, "working");
        assert_eq!(rows[&blocked.id.as_str()].action_verb, "Unblock");
        assert_eq!(rows[&stale.id.as_str()].attention_lane, "stale");
        assert_eq!(rows[&fresh.id.as_str()].attention_lane, "needs");
        assert_eq!(rows[&fresh.id.as_str()].action_verb, "Continue");
    }

    #[tokio::test]
    async fn inbox_grouping_paginates_each_lane_independently_and_pins_first() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();

        let newest_needs = create_conversation(&state, &project.id, "Newest needs", now).await;
        create_workspace(&state, &newest_needs, &project.id, None, Some("open"), None).await;
        let pinned_needs = create_conversation(
            &state,
            &project.id,
            "Pinned needs",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(&state, &pinned_needs, &project.id, None, Some("open"), None).await;
        let newest_done = create_conversation(&state, &project.id, "Newest done", now).await;
        create_workspace(
            &state,
            &newest_done,
            &project.id,
            Some(1),
            Some("merged"),
            None,
        )
        .await;
        let older_done = create_conversation(
            &state,
            &project.id,
            "Older done",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(
            &state,
            &older_done,
            &project.id,
            Some(2),
            Some("merged"),
            None,
        )
        .await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("inbox".to_string());
        input.limit_per_group = Some(1);
        input.pinned_conversation_ids = Some(vec![pinned_needs.id.as_str().to_string()]);
        input.offsets = Some(HashMap::from([
            ("needs".to_string(), 0),
            ("done".to_string(), 1),
        ]));

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();
        let needs = response
            .groups
            .iter()
            .find(|group| group.key == "needs")
            .unwrap();
        let done = response
            .groups
            .iter()
            .find(|group| group.key == "done")
            .unwrap();

        assert_eq!(needs.total, 2);
        assert_eq!(needs.rows[0].conversation.id, pinned_needs.id.as_str());
        assert_eq!(done.total, 2);
        assert_eq!(done.offset, 1);
        assert_eq!(done.rows[0].conversation.id, older_done.id.as_str());
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
    async fn bulk_publication_states_returns_active_for_no_workspace() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();
        let conv = create_conversation(&state, &project.id, "No workspace", now).await;
        let conv_id = conv.id.as_str();

        let result =
            get_bulk_workspace_publication_states_inner(std::slice::from_ref(&conv_id), &state)
                .await
                .unwrap();

        assert_eq!(result.len(), 1);
        let entry = result.get(&conv_id).unwrap();
        assert_eq!(entry.publication_state, "active");
        assert!(entry.publication_label.is_none());
    }

    #[tokio::test]
    async fn bulk_publication_states_returns_correct_states_for_various_workspaces() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha").await;
        let now = Utc::now();

        let merged_conv = create_conversation(&state, &project.id, "Merged", now).await;
        create_workspace(
            &state,
            &merged_conv,
            &project.id,
            Some(10),
            Some("merged"),
            None,
        )
        .await;
        let merged_id = merged_conv.id.as_str();

        let draft_conv = create_conversation(
            &state,
            &project.id,
            "Draft",
            now - chrono::Duration::minutes(1),
        )
        .await;
        create_workspace(
            &state,
            &draft_conv,
            &project.id,
            Some(11),
            Some("draft"),
            None,
        )
        .await;
        let draft_id = draft_conv.id.as_str();

        let uncommitted_conv = create_conversation(
            &state,
            &project.id,
            "Uncommitted",
            now - chrono::Duration::minutes(2),
        )
        .await;
        create_workspace(
            &state,
            &uncommitted_conv,
            &project.id,
            None,
            None,
            Some("needs_agent"),
        )
        .await;
        let uncommitted_id = uncommitted_conv.id.as_str();

        let unpushed_conv = create_conversation(
            &state,
            &project.id,
            "Unpushed",
            now - chrono::Duration::minutes(3),
        )
        .await;
        create_workspace(
            &state,
            &unpushed_conv,
            &project.id,
            None,
            None,
            Some("pending"),
        )
        .await;
        let unpushed_id = unpushed_conv.id.as_str();

        let closed_conv = create_conversation(
            &state,
            &project.id,
            "Closed",
            now - chrono::Duration::minutes(4),
        )
        .await;
        create_workspace(
            &state,
            &closed_conv,
            &project.id,
            Some(12),
            Some("closed"),
            None,
        )
        .await;
        let closed_id = closed_conv.id.as_str();

        let ids: Vec<String> = vec![
            merged_id.clone(),
            draft_id.clone(),
            uncommitted_id.clone(),
            unpushed_id.clone(),
            closed_id.clone(),
        ];

        let result = get_bulk_workspace_publication_states_inner(&ids, &state)
            .await
            .unwrap();

        assert_eq!(result.len(), 5);
        assert_eq!(result.get(&merged_id).unwrap().publication_state, "merged");
        assert_eq!(
            result.get(&merged_id).unwrap().publication_label.as_deref(),
            Some("merged")
        );
        assert_eq!(result.get(&draft_id).unwrap().publication_state, "draft");
        assert_eq!(
            result.get(&uncommitted_id).unwrap().publication_state,
            "uncommitted"
        );
        assert_eq!(
            result.get(&unpushed_id).unwrap().publication_state,
            "unpushed"
        );
        assert_eq!(result.get(&closed_id).unwrap().publication_state, "closed");
    }

    #[tokio::test]
    async fn bulk_publication_states_returns_empty_for_empty_input() {
        let state = AppState::new_test();

        let result = get_bulk_workspace_publication_states_inner(&[], &state)
            .await
            .unwrap();

        assert!(result.is_empty());
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
        let beta_conversation = create_conversation(
            &state,
            &beta.id,
            "Beta work",
            now - chrono::Duration::seconds(1),
        )
        .await;
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

    #[tokio::test]
    async fn project_grouping_adds_no_project_group_for_standalone_conversations() {
        let state = AppState::new_test();
        let alpha = create_project(&state, "alpha-standalone").await;
        let now = Utc::now();

        let project_conversation = create_conversation(&state, &alpha.id, "Alpha work", now).await;
        let standalone_conversation = create_standalone_conversation(
            &state,
            "Standalone chat",
            now - chrono::Duration::minutes(1),
        )
        .await;

        let mut input = sidebar_input(&alpha.id);
        input.group_by = Some("project".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(
            response.groups.len(),
            2,
            "the requested project group plus a data-driven 'No project' group"
        );
        assert_eq!(response.groups[0].key, alpha.id.as_str());
        assert_eq!(
            response.groups[0].rows[0].conversation.id,
            project_conversation.id.as_str()
        );

        let no_project_group = &response.groups[1];
        assert_eq!(no_project_group.key, "__no_project__");
        assert_eq!(no_project_group.label, "No project");
        assert_eq!(no_project_group.total, 1);
        assert_eq!(
            no_project_group.rows[0].conversation.id,
            standalone_conversation.id.as_str()
        );
        assert_eq!(
            no_project_group.rows[0].conversation.context_type,
            "standalone"
        );
        assert!(no_project_group.rows[0].workspace.is_none());
    }

    #[tokio::test]
    async fn project_grouping_omits_no_project_group_when_no_standalone_conversations_exist() {
        // Regression guard for the OTHER direction: unlike explicitly requested
        // project_ids (which always get a group even when empty), the "No
        // project" group must be entirely absent when there are zero
        // standalone conversations — it is data-driven, not
        // always-present, so callers with no standalone rows don't render an
        // empty phantom group.
        let state = AppState::new_test();
        let alpha = create_project(&state, "alpha-no-standalone").await;
        let now = Utc::now();
        create_conversation(&state, &alpha.id, "Alpha only", now).await;

        let mut input = sidebar_input(&alpha.id);
        input.group_by = Some("project".to_string());

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].key, alpha.id.as_str());
        assert!(!response
            .groups
            .iter()
            .any(|group| group.key == "__no_project__"));
    }

    #[tokio::test]
    async fn project_grouping_sorts_pinned_rows_before_priority_rows() {
        let state = AppState::new_test();
        let project = create_project(&state, "alpha-priority").await;
        let now = Utc::now();

        let unpinned = create_conversation(&state, &project.id, "Unpinned newest", now).await;
        create_workspace(&state, &unpinned, &project.id, None, Some("open"), None).await;
        let priority = create_conversation(
            &state,
            &project.id,
            "Selected priority",
            now - chrono::Duration::minutes(5),
        )
        .await;
        create_workspace(&state, &priority, &project.id, None, Some("open"), None).await;
        let pinned = create_conversation(
            &state,
            &project.id,
            "Pinned oldest",
            now - chrono::Duration::minutes(10),
        )
        .await;
        create_workspace(&state, &pinned, &project.id, None, Some("open"), None).await;

        let mut input = sidebar_input(&project.id);
        input.group_by = Some("project".to_string());
        input.pinned_conversation_ids = Some(vec![pinned.id.as_str().to_string()]);
        input.priority_conversation_ids = Some(vec![priority.id.as_str().to_string()]);

        let response = list_agent_sidebar_conversations_for_app_state(input, &state)
            .await
            .unwrap();

        let rows = &response.groups[0].rows;
        assert_eq!(rows[0].conversation.id, pinned.id.as_str());
        assert_eq!(rows[1].conversation.id, priority.id.as_str());
        assert_eq!(rows[2].conversation.id, unpinned.id.as_str());
    }

    #[test]
    fn publication_state_for_workspace_no_workspace_failed_run_is_closed() {
        assert_eq!(
            publication_state_for_workspace(None, Some(AgentRunStatus::Failed)),
            SidebarPublicationState::Closed
        );
    }

    #[test]
    fn publication_state_for_workspace_no_workspace_cancelled_run_is_closed() {
        assert_eq!(
            publication_state_for_workspace(None, Some(AgentRunStatus::Cancelled)),
            SidebarPublicationState::Closed
        );
    }

    #[test]
    fn publication_state_for_workspace_no_workspace_running_run_is_active() {
        // A non-terminal latest run (or no run) must not flip an unpublished
        // workspace-less conversation to Closed.
        assert_eq!(
            publication_state_for_workspace(None, Some(AgentRunStatus::Running)),
            SidebarPublicationState::Active
        );
        assert_eq!(
            publication_state_for_workspace(None, None),
            SidebarPublicationState::Active
        );
    }

    #[test]
    fn publication_state_from_domain_no_workspace_failed_run_is_closed() {
        assert_eq!(
            publication_state_from_domain(None, Some(AgentRunStatus::Failed)),
            SidebarPublicationState::Closed
        );
    }

    #[test]
    fn publication_state_from_domain_no_workspace_cancelled_run_is_closed() {
        assert_eq!(
            publication_state_from_domain(None, Some(AgentRunStatus::Cancelled)),
            SidebarPublicationState::Closed
        );
    }

    #[test]
    fn publication_state_from_domain_no_workspace_running_run_is_active() {
        assert_eq!(
            publication_state_from_domain(None, Some(AgentRunStatus::Running)),
            SidebarPublicationState::Active
        );
        assert_eq!(
            publication_state_from_domain(None, None),
            SidebarPublicationState::Active
        );
    }

    #[test]
    fn publication_state_from_domain_active_workspace_failed_run_is_active() {
        let conversation_id = ChatConversationId::from_string("conversation-1".to_string());
        let project_id = ProjectId::from_string("project-1".to_string());
        let workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "ralphx/project/agent-conversation-1".to_string(),
            "/tmp/worktrees/agent-conversation-1".to_string(),
        );

        assert_eq!(
            publication_state_from_domain(Some(&workspace), Some(AgentRunStatus::Failed)),
            SidebarPublicationState::Active
        );
        assert_eq!(
            publication_state_from_domain(Some(&workspace), Some(AgentRunStatus::Cancelled)),
            SidebarPublicationState::Active
        );
    }
}
