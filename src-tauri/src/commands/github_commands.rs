// GitHub commands — read-only visibility surface over the locally-authenticated
// `gh` CLI. RalphX stores no GitHub token (Decision 1): connection status is a
// live reflection of `gh auth status` only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::pull_request_detail::types::PullRequestDetail;
use crate::application::pull_request_detail::{
    load_pull_request_detail, PullRequestDetailDeps, PullRequestDetailRequest,
};
use crate::application::{AppState, GitService};
use crate::domain::entities::{AgentConversationWorkspace, ChatContextType, ProjectId};
use crate::domain::services::github_service::{
    GithubConnectionStatus, PrBranchMatch, PrSearchResult, PrStatus,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

/// Tauri DTO for GitHub connection status (camelCase for the frontend).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConnectionStatusResponse {
    pub gh_installed: bool,
    pub authenticated: bool,
    pub host: Option<String>,
    pub account: Option<String>,
}

/// Tauri input for `get_github_branch_overview`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGithubBranchOverviewInput {
    pub project_id: String,
}

/// One branch/PR row for the GitHub branch overview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubBranchOverviewItem {
    pub branch_name: String,
    pub is_current: bool,
    pub pr_number: Option<i64>,
    pub pr_title: Option<String>,
    pub pr_url: Option<String>,
    pub pr_status: Option<String>,
    pub pr_is_draft: bool,
    pub pr_updated_at: Option<String>,
    pub pr_author_login: Option<String>,
    pub pr_base_ref_name: Option<String>,
    pub rx_conversation_count: usize,
    pub rx_conversations: Vec<GithubBranchRxConversation>,
    pub ticket_count: usize,
    pub ticket_links: Vec<GithubBranchTicketLink>,
    pub ticket_labels: Vec<String>,
}

/// A RalphX conversation attached to a branch through its workspace row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubBranchRxConversation {
    pub conversation_id: String,
    pub title: Option<String>,
}

/// A provider ticket attached to a branch through a linked RalphX conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubBranchTicketLink {
    pub provider: String,
    pub label: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Read-only branch overview for the Ticketing GitHub surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubBranchOverviewResponse {
    pub current_branch: Option<String>,
    pub branches: Vec<GithubBranchOverviewItem>,
    pub sources_unavailable: Vec<String>,
}

#[derive(Debug, Clone)]
struct BranchPrSummary {
    number: i64,
    title: Option<String>,
    url: Option<String>,
    status: Option<String>,
    is_draft: bool,
    updated_at: Option<String>,
    author_login: Option<String>,
    base_ref_name: Option<String>,
}

impl From<GithubConnectionStatus> for GithubConnectionStatusResponse {
    fn from(status: GithubConnectionStatus) -> Self {
        Self {
            gh_installed: status.gh_installed,
            authenticated: status.authenticated,
            host: status.host,
            account: status.account,
        }
    }
}

/// Report whether `gh` is installed and authenticated, plus the active host/account.
///
/// Never panics or returns an `Err`: a missing GitHub service, an absent/unauthenticated
/// `gh`, or any underlying failure all collapse to a typed "unavailable" status so the
/// UI can render distinct not-installed / not-authenticated / connected states.
#[tauri::command]
pub async fn get_github_connection_status(
    state: State<'_, AppState>,
) -> Result<GithubConnectionStatusResponse, String> {
    let Some(service) = state.github_service.as_ref() else {
        return Ok(GithubConnectionStatus::unavailable().into());
    };

    let status = service
        .fetch_github_connection_status()
        .await
        .unwrap_or_else(|_| GithubConnectionStatus::unavailable());

    Ok(status.into())
}

/// Return all local/remote branches for the project, joined to open GitHub PRs,
/// RalphX workspaces, and ticket links attached through those workspaces.
///
/// Git branch enumeration is required and returns `Err` if the project repo is
/// invalid. GitHub PR search is best-effort: unauthenticated/missing `gh` still
/// renders branch/RX/ticket state with `sourcesUnavailable=["githubPullRequests"]`.
#[tauri::command]
pub async fn get_github_branch_overview(
    input: GetGithubBranchOverviewInput,
    state: State<'_, AppState>,
) -> Result<GithubBranchOverviewResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let working_dir = get_project_working_directory(&project_id, state.inner()).await?;
    let branch_names = GitService::list_branches(&working_dir)
        .await
        .map_err(|error| error.to_string())?;

    let mut sources_unavailable = Vec::new();
    let current_branch = match GitService::get_current_branch(&working_dir).await {
        Ok(branch) if !branch.trim().is_empty() && branch != "HEAD" => Some(branch),
        Ok(_) => None,
        Err(_) => {
            sources_unavailable.push("currentBranch".to_string());
            None
        }
    };

    let (pull_requests, latest_pr_matches) = match state.github_service.as_ref() {
        Some(github_service) => match github_service
            .search_pull_requests(&working_dir, None, 50)
            .await
        {
            Ok(results) => {
                let open_pr_branches: BTreeSet<String> = results
                    .iter()
                    .map(|pr| pr.head_ref_name.trim().to_string())
                    .filter(|branch| !branch.is_empty())
                    .collect();
                let latest_matches = latest_pull_request_matches_for_branches(
                    github_service.as_ref(),
                    &working_dir,
                    &branch_names,
                    &open_pr_branches,
                    &mut sources_unavailable,
                )
                .await;
                (results, latest_matches)
            }
            Err(_) => {
                sources_unavailable.push("githubPullRequests".to_string());
                (Vec::new(), Vec::new())
            }
        },
        None => {
            sources_unavailable.push("githubPullRequests".to_string());
            (Vec::new(), Vec::new())
        }
    };

    let workspaces = state
        .agent_conversation_workspace_repo
        .get_by_project_id(&project_id)
        .await
        .map_err(|error| error.to_string())?;
    let ticket_links_by_branch =
        branch_ticket_links(state.inner(), &project_id, &workspaces).await?;
    let conversation_titles_by_id = project_conversation_titles(state.inner(), &project_id).await?;

    Ok(build_branch_overview_response(
        branch_names,
        current_branch,
        pull_requests,
        latest_pr_matches,
        workspaces,
        conversation_titles_by_id,
        ticket_links_by_branch,
        sources_unavailable,
    ))
}

/// Tauri input for `get_pull_request_detail`. Provide either `prNumber` or
/// `branch` (number takes precedence when both are present).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPullRequestDetailInput {
    pub project_id: String,
    pub pr_number: Option<i64>,
    pub branch: Option<String>,
}

/// Return the full PR-detail graph (description + checks/review + comments +
/// live review thread + attached RalphX conversations) for a `(project, pr)` or
/// `(project, branch)` selector.
///
/// Never panics or returns `Err`: every failure collapses into a typed
/// `state` on the payload (no-PR / gh-unauthenticated / repo-unresolvable /
/// fetch-timeout / rate-limited), so the UI renders distinct empty/error
/// surfaces. Repo is resolved from the project's `working_directory` (Decision 5).
#[tauri::command]
pub async fn get_pull_request_detail(
    input: GetPullRequestDetailInput,
    state: State<'_, AppState>,
) -> Result<PullRequestDetail, String> {
    let deps = PullRequestDetailDeps {
        github_service: state.github_service.clone(),
        project_repo: Arc::clone(&state.project_repo),
        workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
        plan_branch_repo: Arc::clone(&state.plan_branch_repo),
    };
    let request = PullRequestDetailRequest {
        project_id: ProjectId::from_string(input.project_id),
        pr_number: input.pr_number,
        branch: input.branch,
    };

    Ok(load_pull_request_detail(deps, request).await)
}

async fn get_project_working_directory(
    project_id: &ProjectId,
    state: &AppState,
) -> Result<PathBuf, String> {
    let project = state
        .project_repo
        .get_by_id(project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;

    let working_dir = validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "project working directory",
    )
    .map_err(|e| e.to_string())?;
    if !working_dir.is_dir() {
        return Err(format!(
            "Project working directory does not exist: {}",
            working_dir.display()
        ));
    }

    Ok(working_dir)
}

async fn branch_ticket_links(
    state: &AppState,
    project_id: &ProjectId,
    workspaces: &[AgentConversationWorkspace],
) -> Result<BTreeMap<String, Vec<GithubBranchTicketLink>>, String> {
    let mut by_branch: BTreeMap<String, Vec<GithubBranchTicketLink>> = BTreeMap::new();
    for workspace in workspaces {
        let branch_name = workspace.branch_name.trim();
        if branch_name.is_empty() {
            continue;
        }

        if let Some(link) = state
            .agent_conversation_jira_issue_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .filter(|link| link.project_id == *project_id)
        {
            insert_branch_ticket_link(
                &mut by_branch,
                branch_name,
                GithubBranchTicketLink {
                    provider: "jira".to_string(),
                    label: link.issue_key,
                    title: link.title,
                    url: link.issue_url,
                },
            );
        }

        if let Some(link) = state
            .agent_conversation_linear_issue_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .filter(|link| link.project_id == *project_id)
        {
            let issue_label = link.issue_key.unwrap_or(link.issue_id);
            insert_branch_ticket_link(
                &mut by_branch,
                branch_name,
                GithubBranchTicketLink {
                    provider: "linear".to_string(),
                    label: issue_label,
                    title: link.title,
                    url: link.issue_url,
                },
            );
        }
    }
    Ok(by_branch)
}

async fn latest_pull_request_matches_for_branches(
    github_service: &dyn crate::domain::services::github_service::GithubServiceTrait,
    working_dir: &Path,
    branch_names: &[String],
    open_pr_branches: &BTreeSet<String>,
    sources_unavailable: &mut Vec<String>,
) -> Vec<PrBranchMatch> {
    let mut matches = Vec::new();
    let mut status_lookup_failed = false;
    for branch_name in branch_names
        .iter()
        .map(|branch| branch.trim())
        .filter(|branch| !branch.is_empty())
        .filter(|branch| *branch != "main" && *branch != "master")
        .filter(|branch| !open_pr_branches.contains(*branch))
        .take(50)
    {
        match github_service
            .find_latest_pr_by_head_branch(working_dir, branch_name)
            .await
        {
            Ok(Some(pr_match)) if pr_match.head_ref_name == branch_name => matches.push(pr_match),
            Ok(Some(_)) => {}
            Ok(None) => {}
            Err(_) => status_lookup_failed = true,
        }
    }
    if status_lookup_failed {
        sources_unavailable.push("githubPullRequestStatus".to_string());
    }
    matches
}

fn insert_branch_ticket_link(
    by_branch: &mut BTreeMap<String, Vec<GithubBranchTicketLink>>,
    branch_name: &str,
    link: GithubBranchTicketLink,
) {
    let links = by_branch.entry(branch_name.to_string()).or_default();
    if !links.contains(&link) {
        links.push(link);
    }
}

async fn project_conversation_titles(
    state: &AppState,
    project_id: &ProjectId,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let conversations = state
        .chat_conversation_repo
        .get_by_context_filtered(ChatContextType::Project, project_id.as_str(), true)
        .await
        .map_err(|error| error.to_string())?;
    Ok(conversations
        .into_iter()
        .map(|conversation| {
            (
                conversation.id.to_string(),
                conversation.title.filter(|title| !title.trim().is_empty()),
            )
        })
        .collect())
}

fn build_branch_overview_response(
    branch_names: Vec<String>,
    current_branch: Option<String>,
    pull_requests: Vec<PrSearchResult>,
    latest_pr_matches: Vec<PrBranchMatch>,
    workspaces: Vec<AgentConversationWorkspace>,
    conversation_titles_by_id: BTreeMap<String, Option<String>>,
    ticket_links_by_branch: BTreeMap<String, Vec<GithubBranchTicketLink>>,
    sources_unavailable: Vec<String>,
) -> GithubBranchOverviewResponse {
    let mut all_branch_names: BTreeSet<String> = branch_names
        .into_iter()
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty())
        .collect();

    let mut pr_by_branch: BTreeMap<String, BranchPrSummary> = BTreeMap::new();
    for pr in pull_requests {
        let branch = pr.head_ref_name.trim();
        if branch.is_empty() {
            continue;
        }
        all_branch_names.insert(branch.to_string());
        pr_by_branch.insert(
            branch.to_string(),
            BranchPrSummary {
                number: pr.number,
                title: Some(pr.title),
                url: Some(pr.url),
                status: Some(if pr.is_draft { "draft" } else { "open" }.to_string()),
                is_draft: pr.is_draft,
                updated_at: pr.updated_at,
                author_login: pr.author_login,
                base_ref_name: Some(pr.base_ref_name),
            },
        );
    }
    for pr_match in latest_pr_matches {
        let branch = pr_match.head_ref_name.trim();
        if branch.is_empty() {
            continue;
        }
        all_branch_names.insert(branch.to_string());
        pr_by_branch
            .entry(branch.to_string())
            .or_insert_with(|| branch_match_pr_summary(pr_match));
    }

    let mut rx_conversations_by_branch: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rx_links_by_branch: BTreeMap<String, Vec<GithubBranchRxConversation>> = BTreeMap::new();
    for workspace in workspaces {
        let branch = workspace.branch_name.trim();
        if branch.is_empty() {
            continue;
        }
        all_branch_names.insert(branch.to_string());
        let conversation_id = workspace.conversation_id.to_string();
        rx_conversations_by_branch
            .entry(branch.to_string())
            .or_default()
            .insert(conversation_id.clone());
        rx_links_by_branch
            .entry(branch.to_string())
            .or_default()
            .push(GithubBranchRxConversation {
                title: conversation_titles_by_id
                    .get(&conversation_id)
                    .cloned()
                    .flatten(),
                conversation_id,
            });

        if let Some(number) = workspace.publication_pr_number {
            pr_by_branch
                .entry(branch.to_string())
                .or_insert_with(|| BranchPrSummary {
                    number,
                    title: None,
                    url: workspace.publication_pr_url,
                    status: workspace.publication_pr_status,
                    is_draft: false,
                    updated_at: Some(workspace.updated_at.to_rfc3339()),
                    author_login: None,
                    base_ref_name: Some(workspace.base_ref),
                });
        }
    }

    for branch in ticket_links_by_branch.keys() {
        all_branch_names.insert(branch.clone());
    }

    let mut branches: Vec<GithubBranchOverviewItem> = all_branch_names
        .into_iter()
        .map(|branch_name| {
            let pr = pr_by_branch.get(&branch_name);
            let ticket_links = ticket_links_by_branch
                .get(&branch_name)
                .cloned()
                .unwrap_or_default();
            let ticket_links = merge_ticket_links_with_branch_fallback(ticket_links, &branch_name);
            let ticket_labels: Vec<String> = ticket_links
                .iter()
                .map(|link| format!("{} {}", provider_ticket_label(&link.provider), link.label))
                .collect();
            let rx_conversations = rx_links_by_branch
                .get(&branch_name)
                .cloned()
                .unwrap_or_default();
            GithubBranchOverviewItem {
                is_current: current_branch.as_deref() == Some(branch_name.as_str()),
                rx_conversation_count: rx_conversations_by_branch
                    .get(&branch_name)
                    .map(BTreeSet::len)
                    .unwrap_or(0),
                rx_conversations,
                ticket_count: ticket_links.len(),
                ticket_links,
                ticket_labels,
                pr_number: pr.map(|summary| summary.number),
                pr_title: pr.and_then(|summary| summary.title.clone()),
                pr_url: pr.and_then(|summary| summary.url.clone()),
                pr_status: pr.and_then(|summary| summary.status.clone()),
                pr_is_draft: pr.is_some_and(|summary| summary.is_draft),
                pr_updated_at: pr.and_then(|summary| summary.updated_at.clone()),
                pr_author_login: pr.and_then(|summary| summary.author_login.clone()),
                pr_base_ref_name: pr.and_then(|summary| summary.base_ref_name.clone()),
                branch_name,
            }
        })
        .collect();

    branches.sort_by(|left, right| {
        right
            .is_current
            .cmp(&left.is_current)
            .then_with(|| right.pr_number.is_some().cmp(&left.pr_number.is_some()))
            .then_with(|| right.rx_conversation_count.cmp(&left.rx_conversation_count))
            .then_with(|| right.ticket_count.cmp(&left.ticket_count))
            .then_with(|| left.branch_name.cmp(&right.branch_name))
    });

    GithubBranchOverviewResponse {
        current_branch,
        branches,
        sources_unavailable,
    }
}

fn branch_match_pr_summary(pr_match: PrBranchMatch) -> BranchPrSummary {
    BranchPrSummary {
        number: pr_match.number,
        title: None,
        url: Some(pr_match.url),
        status: Some(publication_status_label(&pr_match.status, pr_match.is_draft).to_string()),
        is_draft: pr_match.is_draft,
        updated_at: pr_match.updated_at,
        author_login: None,
        base_ref_name: None,
    }
}

fn publication_status_label(status: &PrStatus, is_draft: bool) -> &'static str {
    match status {
        PrStatus::Open if is_draft => "draft",
        PrStatus::Open => "open",
        PrStatus::Closed => "closed",
        PrStatus::Merged { .. } => "merged",
    }
}

fn provider_ticket_label(provider: &str) -> &'static str {
    match provider {
        "jira" => "Jira",
        "linear" => "Linear",
        "clickup" => "ClickUp",
        _ => "Ticket",
    }
}

fn merge_ticket_links_with_branch_fallback(
    mut links: Vec<GithubBranchTicketLink>,
    branch_name: &str,
) -> Vec<GithubBranchTicketLink> {
    if let Some(fallback) = infer_ticket_link_from_branch_name(branch_name) {
        if !links.iter().any(|link| {
            link.provider == fallback.provider && link.label.eq_ignore_ascii_case(&fallback.label)
        }) {
            links.push(fallback);
        }
    }
    links
}

fn infer_ticket_link_from_branch_name(branch_name: &str) -> Option<GithubBranchTicketLink> {
    let suffix = branch_name.strip_prefix("ralphx/ticket/")?;
    let (provider, label) = suffix.split_once('-')?;
    if !matches!(provider, "jira" | "linear" | "clickup") || label.trim().is_empty() {
        return None;
    }
    Some(GithubBranchTicketLink {
        provider: provider.to_string(),
        label: label.to_string(),
        title: None,
        url: None,
    })
}
