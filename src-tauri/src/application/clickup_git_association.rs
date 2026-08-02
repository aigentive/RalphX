use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::application::clickup_integration_service::{
    ClickUpIntegrationService, ClickUpTaskContent,
};
use crate::application::external_issue_link_service::{
    ExternalIssueLinkService, TicketConversationLinkInput,
};
use crate::application::git_service::GitService;
use crate::domain::integrations::{ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus};
use crate::domain::services::{GithubServiceTrait, PrSearchResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpTaskIdentity {
    pub id: String,
    pub custom_id: Option<String>,
    pub url: Option<String>,
}

impl ClickUpTaskIdentity {
    pub fn new(id: impl Into<String>, custom_id: Option<String>, url: Option<String>) -> Self {
        Self {
            id: id.into().trim().to_string(),
            custom_id: custom_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            url: url
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    pub fn preferred_token(&self) -> String {
        preferred_clickup_task_token(&self.id, self.custom_id.as_deref())
    }

    pub fn aliases(&self) -> Vec<String> {
        let preferred_token = self.preferred_token();
        let mut aliases = Vec::new();
        let mut seen = HashSet::new();
        for alias in [
            Some(self.id.as_str()),
            self.custom_id.as_deref(),
            Some(preferred_token.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            let alias = alias.trim();
            if !alias.is_empty() && seen.insert(alias.to_ascii_lowercase()) {
                aliases.push(alias.to_string());
            }
        }
        aliases
    }
}

pub fn preferred_clickup_task_token(id: &str, custom_id: Option<&str>) -> String {
    if let Some(custom_id) = custom_id.map(str::trim).filter(|value| !value.is_empty()) {
        return custom_id.to_string();
    }
    let id = id.trim();
    if id.to_ascii_uppercase().starts_with("CU-") {
        id.to_string()
    } else {
        format!("CU-{id}")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClickUpGitEvidence {
    pub branch: String,
    pub title: String,
    pub body: Option<String>,
    pub commit_subjects: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickUpGitEvidenceSource {
    Branch,
    PullRequestTitle,
    PullRequestBody,
    CommitSubject,
}

impl ClickUpGitEvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::PullRequestTitle => "pr_title",
            Self::PullRequestBody => "pr_body",
            Self::CommitSubject => "commit_subject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpGitEvidenceMatch {
    pub source: ClickUpGitEvidenceSource,
    pub matched_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpTaskCandidate {
    pub lookup_key: String,
    pub matched_token: String,
    pub source: ClickUpGitEvidenceSource,
}

const MAX_CLICKUP_TASK_CANDIDATES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpPrAssociationInput {
    pub conversation_id: String,
    pub project_id: String,
    pub evidence: ClickUpGitEvidence,
    pub pr_number: i64,
    pub pr_url: Option<String>,
    pub pr_status: String,
    pub head_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickUpPrAssociationOutcome {
    NoCandidate,
    NoValidatedCandidate,
    PendingValidation { errors: Vec<String> },
    Ambiguous { task_ids: Vec<String> },
    Linked { task_id: String, link_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUpTicketStartCandidate {
    pub branch_name: String,
    pub pull_request: Option<PrSearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickUpTicketStartResolution {
    NoMatch,
    Unique(Box<ClickUpTicketStartCandidate>),
    Ambiguous { branch_names: Vec<String> },
}

const CLICKUP_START_PR_SEARCH_LIMIT: usize = 20;

pub async fn resolve_clickup_ticket_start(
    task: &ClickUpTaskIdentity,
    repo_path: &Path,
    github: Option<&dyn GithubServiceTrait>,
) -> Result<ClickUpTicketStartResolution, String> {
    let mut pull_requests = Vec::new();
    if let Some(github) = github {
        for alias in task.aliases() {
            let results = github
                .search_pull_requests(repo_path, Some(&alias), CLICKUP_START_PR_SEARCH_LIMIT)
                .await
                .map_err(|error| error.to_string())?;
            for pull_request in results {
                if !pull_request.is_open()
                    || pull_request.is_cross_repository
                    || pull_requests
                        .iter()
                        .any(|existing: &PrSearchResult| existing.number == pull_request.number)
                {
                    continue;
                }
                let evidence = ClickUpGitEvidence {
                    branch: pull_request.head_ref_name.clone(),
                    title: pull_request.title.clone(),
                    ..Default::default()
                };
                if matching_clickup_evidence(task, &evidence).is_some() {
                    pull_requests.push(pull_request);
                }
            }
        }
    }

    let branches = GitService::list_branches(repo_path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(select_clickup_ticket_start_candidate(
        task,
        pull_requests,
        branches,
    ))
}

pub fn select_clickup_ticket_start_candidate(
    task: &ClickUpTaskIdentity,
    mut pull_requests: Vec<PrSearchResult>,
    branches: Vec<String>,
) -> ClickUpTicketStartResolution {
    pull_requests.retain(|pull_request| {
        pull_request.is_open()
            && !pull_request.is_cross_repository
            && matching_clickup_evidence(
                task,
                &ClickUpGitEvidence {
                    branch: pull_request.head_ref_name.clone(),
                    title: pull_request.title.clone(),
                    ..Default::default()
                },
            )
            .is_some()
    });
    pull_requests.sort_by(|left, right| left.number.cmp(&right.number));
    pull_requests.dedup_by_key(|pull_request| pull_request.number);
    if pull_requests.len() == 1 {
        let pull_request = pull_requests.pop().expect("one pull request candidate");
        return ClickUpTicketStartResolution::Unique(Box::new(ClickUpTicketStartCandidate {
            branch_name: pull_request.head_ref_name.clone(),
            pull_request: Some(pull_request),
        }));
    }
    if pull_requests.len() > 1 {
        return ClickUpTicketStartResolution::Ambiguous {
            branch_names: sorted_unique_branch_names(
                pull_requests
                    .into_iter()
                    .map(|pull_request| pull_request.head_ref_name),
            ),
        };
    }

    let branch_names = sorted_unique_branch_names(branches.into_iter().filter(|branch| {
        matching_clickup_evidence(
            task,
            &ClickUpGitEvidence {
                branch: branch.clone(),
                ..Default::default()
            },
        )
        .is_some()
    }));
    match branch_names.as_slice() {
        [] => ClickUpTicketStartResolution::NoMatch,
        [branch_name] => {
            ClickUpTicketStartResolution::Unique(Box::new(ClickUpTicketStartCandidate {
                branch_name: branch_name.clone(),
                pull_request: None,
            }))
        }
        _ => ClickUpTicketStartResolution::Ambiguous { branch_names },
    }
}

fn sorted_unique_branch_names(branches: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut branches = branches
        .into_iter()
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty())
        .collect::<Vec<_>>();
    branches.sort_by_key(|branch| branch.to_ascii_lowercase());
    branches.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    branches
}

pub async fn reconcile_clickup_pr_to_conversation(
    clickup: &ClickUpIntegrationService,
    links: &ExternalIssueLinkService,
    input: ClickUpPrAssociationInput,
) -> Result<ClickUpPrAssociationOutcome, String> {
    let candidates = clickup_task_candidates(&input.evidence);
    if candidates.is_empty() {
        return Ok(ClickUpPrAssociationOutcome::NoCandidate);
    }
    if candidates.len() > MAX_CLICKUP_TASK_CANDIDATES {
        return Ok(ClickUpPrAssociationOutcome::Ambiguous {
            task_ids: candidates
                .into_iter()
                .map(|candidate| candidate.lookup_key)
                .collect(),
        });
    }

    // PR title and PR body are discovery-only evidence: RalphX's own title
    // normalizer writes ticket prefixes into workspace PR titles (a title-based
    // link would validate the normalizer's own output), and PR bodies can
    // mention tickets as documentation examples. Only workspace-owned signals
    // (branch name, branch-authored commit subjects) may authorize a
    // ticket↔conversation link.
    let link_evidence = ClickUpGitEvidence {
        branch: input.evidence.branch.clone(),
        title: String::new(),
        body: None,
        commit_subjects: input.evidence.commit_subjects.clone(),
    };

    let mut validated = Vec::new();
    let mut retryable_errors = Vec::new();
    for candidate in candidates {
        match clickup.fetch_task(&candidate.lookup_key).await {
            Ok(task) => {
                let identity = clickup_identity_from_task(&task);
                if let Some(matched) = matching_clickup_evidence(&identity, &link_evidence) {
                    if !validated.iter().any(
                        |(existing, _): &(ClickUpTaskContent, ClickUpGitEvidenceMatch)| {
                            existing.id.eq_ignore_ascii_case(&task.id)
                        },
                    ) {
                        validated.push((task, matched));
                    }
                }
            }
            Err(error) if clickup_lookup_error_is_retryable(&error) => {
                retryable_errors.push(format!("{}: {error}", candidate.lookup_key));
            }
            Err(_) => {}
        }
    }

    if !retryable_errors.is_empty() {
        return Ok(ClickUpPrAssociationOutcome::PendingValidation {
            errors: retryable_errors,
        });
    }
    if validated.len() > 1 {
        let mut task_ids = validated
            .into_iter()
            .map(|(task, _)| task.id)
            .collect::<Vec<_>>();
        task_ids.sort();
        return Ok(ClickUpPrAssociationOutcome::Ambiguous { task_ids });
    }
    let Some((task, matched)) = validated.pop() else {
        return Ok(ClickUpPrAssociationOutcome::NoValidatedCandidate);
    };

    let metadata_json = serde_json::json!({
        "source": matched.source.as_str(),
        "matched_token": matched.matched_token,
        "branch": input.evidence.branch,
        "pr_number": input.pr_number,
        "pr_url": input.pr_url,
        "pr_status": input.pr_status,
        "title": task.name,
        "validated_at": Utc::now().to_rfc3339(),
    })
    .to_string();
    let link = links
        .upsert_ticket_conversation_link(TicketConversationLinkInput {
            provider: "clickup".to_string(),
            external_kind: "clickup".to_string(),
            external_id: task.id.clone(),
            external_key: task.custom_id.clone(),
            external_url: task.url.clone(),
            conversation_id: input.conversation_id.clone(),
            project_id: input.project_id,
            local_sha: input.head_sha.clone(),
            local_state: Some(input.pr_status.clone()),
            metadata_json: Some(metadata_json.clone()),
        })
        .await
        .map_err(|error| error.to_string())?;
    links
        .upsert_sync_record(ExternalIssueSyncRecordUpsert {
            link_id: link.id.clone(),
            sync_kind: "clickup_git_association".to_string(),
            idempotency_key: format!(
                "clickup:git-association:{}:pr:{}",
                input.conversation_id, input.pr_number
            ),
            local_sha: input.head_sha,
            local_state: Some(input.pr_status),
            external_version: task.updated_at,
            status: ExternalIssueSyncStatus::Succeeded,
            error_message: None,
            metadata_json: Some(metadata_json),
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(ClickUpPrAssociationOutcome::Linked {
        task_id: task.id,
        link_id: link.id,
    })
}

pub fn clickup_identity_from_task(task: &ClickUpTaskContent) -> ClickUpTaskIdentity {
    ClickUpTaskIdentity::new(&task.id, task.custom_id.clone(), task.url.clone())
}

fn clickup_lookup_error_is_retryable(error: &str) -> bool {
    !error.to_ascii_lowercase().contains("http 404")
}

pub fn matching_clickup_evidence(
    task: &ClickUpTaskIdentity,
    evidence: &ClickUpGitEvidence,
) -> Option<ClickUpGitEvidenceMatch> {
    let aliases = task.aliases();
    let fields = [
        (
            ClickUpGitEvidenceSource::Branch,
            Some(evidence.branch.as_str()),
        ),
        (
            ClickUpGitEvidenceSource::PullRequestTitle,
            Some(evidence.title.as_str()),
        ),
        (
            ClickUpGitEvidenceSource::PullRequestBody,
            evidence.body.as_deref(),
        ),
    ];
    for (source, value) in fields {
        let Some(value) = value else {
            continue;
        };
        if let Some(alias) = aliases
            .iter()
            .find(|alias| contains_token_boundary(value, alias))
        {
            return Some(ClickUpGitEvidenceMatch {
                source,
                matched_token: alias.clone(),
            });
        }
    }
    for subject in &evidence.commit_subjects {
        if let Some(alias) = aliases
            .iter()
            .find(|alias| contains_token_boundary(subject, alias))
        {
            return Some(ClickUpGitEvidenceMatch {
                source: ClickUpGitEvidenceSource::CommitSubject,
                matched_token: alias.clone(),
            });
        }
    }
    None
}

pub fn clickup_task_candidates(evidence: &ClickUpGitEvidence) -> Vec<ClickUpTaskCandidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for (source, value) in [
        (ClickUpGitEvidenceSource::Branch, evidence.branch.as_str()),
        (
            ClickUpGitEvidenceSource::PullRequestTitle,
            evidence.title.as_str(),
        ),
    ] {
        collect_candidates(value, source, &mut seen, &mut candidates);
    }
    if let Some(body) = evidence.body.as_deref() {
        collect_candidates(
            body,
            ClickUpGitEvidenceSource::PullRequestBody,
            &mut seen,
            &mut candidates,
        );
    }
    for subject in &evidence.commit_subjects {
        collect_candidates(
            subject,
            ClickUpGitEvidenceSource::CommitSubject,
            &mut seen,
            &mut candidates,
        );
    }
    candidates
}

fn collect_candidates(
    value: &str,
    source: ClickUpGitEvidenceSource,
    seen: &mut HashSet<String>,
    candidates: &mut Vec<ClickUpTaskCandidate>,
) {
    for token in evidence_tokens(value) {
        let Some((lookup_key, matched_token)) = clickup_lookup_from_token(token) else {
            continue;
        };
        if seen.insert(lookup_key.to_ascii_lowercase()) {
            candidates.push(ClickUpTaskCandidate {
                lookup_key,
                matched_token,
                source,
            });
        }
    }
}

fn clickup_lookup_from_token(token: &str) -> Option<(String, String)> {
    let token = token.trim_matches('-');
    if token.len() < 3 {
        return None;
    }
    if token
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("CU-"))
    {
        let id = token[3..].split('-').next()?.trim();
        return valid_opaque_clickup_id(id).then(|| (id.to_string(), format!("CU-{id}")));
    }

    let mut parts = token.split('-');
    let prefix = parts.next()?;
    let suffix = parts.next()?;
    let is_custom_id = !prefix.is_empty()
        && prefix.chars().all(|ch| ch.is_ascii_uppercase())
        && prefix.chars().any(|ch| ch.is_ascii_alphabetic())
        && !suffix.is_empty()
        && suffix.chars().all(|ch| ch.is_ascii_digit());
    is_custom_id.then(|| {
        let matched = format!("{prefix}-{suffix}");
        (matched.clone(), matched)
    })
}

fn valid_opaque_clickup_id(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn evidence_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .map(|token| token.trim_matches('-'))
        .filter(|token| !token.is_empty())
}

fn contains_token_boundary(value: &str, token: &str) -> bool {
    let value = value.as_bytes();
    let token = token.as_bytes();
    if token.is_empty() || token.len() > value.len() {
        return false;
    }
    value
        .windows(token.len())
        .enumerate()
        .any(|(index, window)| {
            window.eq_ignore_ascii_case(token)
                && (index == 0 || !value[index - 1].is_ascii_alphanumeric())
                && (index + token.len() == value.len()
                    || !value[index + token.len()].is_ascii_alphanumeric())
        })
}

#[cfg(test)]
#[path = "clickup_git_association_tests.rs"]
mod tests;
