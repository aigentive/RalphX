use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::agent_workspace_publish_recovery::is_blocked_and_not_auto_retryable;
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_workspace_response_for_state, agent_workspace_response_without_repair_recovery_for_state,
    AgentConversationResponse, AgentConversationWorkspaceResponse,
};
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
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    list_agent_sidebar_conversations_for_app_state(input, state.inner()).await
}

/// Hydrate every requested project's workspace responses through the FULL hydrator, which — by
/// design — schedules host-side PR-supervision recovery as a side effect of a workspace load.
/// This is the LOCAL/host path: a local inbox load is allowed to nudge recovery.
///
/// The remote facade must NOT do this; see [`hydrate_sidebar_workspaces_read_only`]. The two
/// hydrators are named (never passed as a function pointer) so the authority call graph can
/// PROVE which one each entry point reaches — a pointer would collapse both to the same
/// token-only, edge-free node and the detector could no longer distinguish them. The local
/// wrapper reaches `agent_workspace_response_for_state`, which arms and resolves the git CLI
/// through the recovery scheduler; the remote wrapper reaches only the recovery-free hydrator.
async fn hydrate_sidebar_workspaces_with_recovery(
    state: &AppState,
    project_ids: &[String],
) -> Result<HashMap<ChatConversationId, AgentConversationWorkspaceResponse>, String> {
    let mut workspace_responses = HashMap::new();
    for project_id_string in normalize_project_ids(project_ids.to_vec()) {
        let project_id = ProjectId::from_string(project_id_string);
        let workspaces = state
            .agent_conversation_workspace_repo
            .get_by_project_id(&project_id)
            .await
            .map_err(|e| e.to_string())?;
        for workspace in workspaces {
            let conversation_id = workspace.conversation_id;
            // Sidebar rows need persisted publication metadata, not active-conversation
            // recovery, repair-spend, or mode-lock hydration. Plan-linked workspaces keep
            // the richer projection because their publication state is owned by PlanBranch.
            let response = if workspace.linked_plan_branch_id.is_some() {
                agent_workspace_response_for_state(state, workspace).await?
            } else {
                AgentConversationWorkspaceResponse::from(workspace)
            };
            workspace_responses.insert(conversation_id, response);
        }
    }
    Ok(workspace_responses)
}

/// Recovery-free twin of [`hydrate_sidebar_workspaces_with_recovery`] for the remote facade.
///
/// Delegates to `agent_workspace_response_without_repair_recovery_for_state`, which returns the
/// SAME persisted workspace projection but does NOT schedule PR-supervision recovery. A paired
/// device reading its Agents inbox must not trigger host-owned background recovery work, and —
/// because the recovery scheduler is what reaches the git CLI resolver — routing the remote
/// read through this hydrator is also what keeps the remote closure clear of detector (c)'s
/// process-launch floor. Proven by
/// `remote_server::capability_ledger_tests::remote_agent_sidebar_read_carries_no_spawn_authority`.
async fn hydrate_sidebar_workspaces_read_only(
    state: &AppState,
    project_ids: &[String],
) -> Result<HashMap<ChatConversationId, AgentConversationWorkspaceResponse>, String> {
    let mut workspace_responses = HashMap::new();
    for project_id_string in normalize_project_ids(project_ids.to_vec()) {
        let project_id = ProjectId::from_string(project_id_string);
        let workspaces = state
            .agent_conversation_workspace_repo
            .get_by_project_id(&project_id)
            .await
            .map_err(|e| e.to_string())?;
        for workspace in workspaces {
            let conversation_id = workspace.conversation_id;
            // Same plan-linked rule as the local twin, resolved through the recovery-free
            // hydrator so the remote closure stays clear of the git CLI resolver.
            let response = if workspace.linked_plan_branch_id.is_some() {
                agent_workspace_response_without_repair_recovery_for_state(state, workspace).await?
            } else {
                AgentConversationWorkspaceResponse::from(workspace)
            };
            workspace_responses.insert(conversation_id, response);
        }
    }
    Ok(workspace_responses)
}

#[doc(hidden)]
pub async fn list_agent_sidebar_conversations_for_app_state(
    input: AgentSidebarConversationsInput,
    state: &AppState,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    let workspace_responses =
        hydrate_sidebar_workspaces_with_recovery(state, &input.project_ids).await?;
    list_agent_sidebar_conversation_groups_from_hydrated(input, state, workspace_responses).await
}

/// Spawn-free `_for_app_state` seam for the remote facade: identical grouping, recovery-free
/// workspace hydration. The facade twin
/// (`commands::remote_transcript_commands::list_remote_agent_sidebar_conversations`) blanks
/// `worktree_path` on top of this; the grouping logic itself is shared verbatim with the local
/// path via [`list_agent_sidebar_conversation_groups_from_hydrated`], so the two transports
/// cannot drift.
pub(crate) async fn list_agent_sidebar_conversations_read_only_for_app_state(
    input: AgentSidebarConversationsInput,
    state: &AppState,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    let workspace_responses =
        hydrate_sidebar_workspaces_read_only(state, &input.project_ids).await?;
    list_agent_sidebar_conversation_groups_from_hydrated(input, state, workspace_responses).await
}

/// Shared grouping over PRE-HYDRATED workspace responses. Holds no hydrator call of its own, so
/// its authority profile is inherited entirely from whichever caller built `workspace_responses`
/// — the seam that lets the local path arm and the remote path stay clean without forking this
/// grouping assembler.
async fn list_agent_sidebar_conversation_groups_from_hydrated(
    input: AgentSidebarConversationsInput,
    state: &AppState,
    mut workspace_responses: HashMap<ChatConversationId, AgentConversationWorkspaceResponse>,
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
            let workspace = workspace_responses.remove(&conversation.id);
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
            let repair_attempt = state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&conversation.id)
                .await
                .map_err(|e| e.to_string())?;
            let blocked_exhausted_repair = repair_attempt
                .as_ref()
                .is_some_and(is_blocked_and_not_auto_retryable);
            let held_repair = repair_attempt
                .as_ref()
                .is_some_and(|attempt| attempt.operation_snapshot().hold_reason.is_some());
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
                held_repair,
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
            // Runtime/persona attribution is hydrated by the active conversation surface.
            // Doing it for every sidebar candidate turns a small summary query into a full
            // transcript scan and makes one old transcript read failure hide all rows.
            let conversation = AgentConversationResponse::from(conversation);
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
        let conversation = AgentConversationResponse::from(conversation);
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
    held_repair: bool,
    last_activity_at: DateTime<Utc>,
    managed_team_activity: Option<&ManagedTeamActivity>,
) -> SidebarAttentionLane {
    attention_lane_for_row_with_armed_park(
        is_archived,
        publication_state,
        latest_run_status,
        workspace,
        blocked_exhausted_repair,
        held_repair,
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
    held_repair: bool,
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

    if blocked_exhausted_repair || held_repair {
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
        Some("held" | "paused") => Some("paused"),
        Some("waiting" | "waiting_for_checks") => Some("waiting"),
        Some("monitoring") if auto_merge_current == Some(true) => Some("auto-merge"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "agent_sidebar_commands_tests.rs"]
mod agent_sidebar_commands_tests;

#[cfg(test)]
#[path = "agent_sidebar_commands_lane_tests.rs"]
mod tests;
