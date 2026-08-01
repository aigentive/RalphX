use super::{
    clickup_task_candidates, matching_clickup_evidence, preferred_clickup_task_token,
    reconcile_clickup_pr_to_conversation, select_clickup_ticket_start_candidate,
    ClickUpGitEvidence, ClickUpGitEvidenceSource, ClickUpPrAssociationInput,
    ClickUpPrAssociationOutcome, ClickUpTaskIdentity, ClickUpTicketStartResolution,
};
use crate::application::clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpIntegrationService, ClickUpTaskContent,
    ClickUpWorkspace,
};
use crate::application::external_issue_link_service::ExternalIssueLinkService;
use crate::domain::integrations::{
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::PrSearchResult;
use crate::domain::services::SecretStore;
use crate::infrastructure::memory::{
    MemoryClickUpIntegrationSettingsRepository, MemoryExternalIssueLinkRepository,
    MemorySecretStore,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

fn identity(id: &str, custom_id: Option<&str>) -> ClickUpTaskIdentity {
    ClickUpTaskIdentity::new(
        id,
        custom_id.map(str::to_string),
        Some(format!("https://app.clickup.com/t/{id}")),
    )
}

fn evidence(branch: &str, title: &str, body: Option<&str>, commits: &[&str]) -> ClickUpGitEvidence {
    ClickUpGitEvidence {
        branch: branch.to_string(),
        title: title.to_string(),
        body: body.map(str::to_string),
        commit_subjects: commits.iter().map(|value| (*value).to_string()).collect(),
    }
}

fn pull_request(number: i64, branch: &str, title: &str) -> PrSearchResult {
    PrSearchResult {
        number,
        title: title.to_string(),
        url: format!("https://github.com/owner/repo/pull/{number}"),
        head_ref_name: branch.to_string(),
        head_ref_oid: Some(format!("sha-{number}")),
        base_ref_name: "main".to_string(),
        is_draft: false,
        state: Some("OPEN".to_string()),
        merged_at: None,
        updated_at: None,
        author_login: None,
        assignee_logins: Vec::new(),
        review_decision: None,
        latest_review_author_logins: Vec::new(),
        review_request_logins: Vec::new(),
        is_cross_repository: false,
    }
}

#[derive(Default)]
struct PartialFailureClickUpClient;

#[async_trait]
impl ClickUpApiClient for PartialFailureClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(Vec::new())
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        if task_id.eq_ignore_ascii_case("DEV-42") {
            return Ok(clickup_task("8689abc", "DEV-42"));
        }
        Err("ClickUp transport unavailable".to_string())
    }

    async fn fetch_task_by_custom_id(
        &self,
        auth: &ClickUpAuthContext,
        _team_id: &str,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.fetch_task(auth, task_id).await
    }
}

#[derive(Default)]
struct StaticClickUpClient {
    tasks_by_lookup: HashMap<String, ClickUpTaskContent>,
}

#[async_trait]
impl ClickUpApiClient for StaticClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(Vec::new())
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.tasks_by_lookup
            .get(&task_id.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| "HTTP 404: task not found".to_string())
    }

    async fn fetch_task_by_custom_id(
        &self,
        auth: &ClickUpAuthContext,
        _team_id: &str,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.fetch_task(auth, task_id).await
    }
}

fn clickup_task(id: &str, custom_id: &str) -> ClickUpTaskContent {
    ClickUpTaskContent {
        id: id.to_string(),
        custom_id: Some(custom_id.to_string()),
        name: "Validated task".to_string(),
        url: Some(format!("https://app.clickup.com/t/{id}")),
        description: String::new(),
        status_name: None,
        status_type: None,
        status_category: None,
        creator: None,
        assignees: Vec::new(),
        watchers: Vec::new(),
        tags: Vec::new(),
        comments: Vec::new(),
        attachments: Vec::new(),
        updated_at: None,
        space_id: None,
        list_name: None,
    }
}

async fn static_clickup_service(tasks: Vec<ClickUpTaskContent>) -> ClickUpIntegrationService {
    let mut tasks_by_lookup = HashMap::new();
    for task in tasks {
        tasks_by_lookup.insert(task.id.to_ascii_lowercase(), task.clone());
        if let Some(custom_id) = task.custom_id.as_deref() {
            tasks_by_lookup.insert(custom_id.to_ascii_lowercase(), task.clone());
        }
    }
    let settings = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret("clickup-test-token", "pk_test")
        .await
        .unwrap();
    settings
        .upsert(&ClickUpIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("clickup-test-token".to_string()),
            workspace_id: Some("workspace-1".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            task_search_available: true,
            ..Default::default()
        })
        .await
        .unwrap();
    ClickUpIntegrationService::new(
        settings,
        secrets,
        Arc::new(StaticClickUpClient { tasks_by_lookup }),
    )
}

async fn partial_failure_clickup_service() -> ClickUpIntegrationService {
    let settings = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret("clickup-test-token", "pk_test")
        .await
        .unwrap();
    settings
        .upsert(&ClickUpIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("clickup-test-token".to_string()),
            workspace_id: Some("workspace-1".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            task_search_available: true,
            ..Default::default()
        })
        .await
        .unwrap();
    ClickUpIntegrationService::new(settings, secrets, Arc::new(PartialFailureClickUpClient))
}

#[test]
fn preferred_token_uses_custom_id_or_stable_opaque_fallback() {
    assert_eq!(
        preferred_clickup_task_token("8689abc", Some("DEV-42")),
        "DEV-42"
    );
    assert_eq!(preferred_clickup_task_token("8689abc", None), "CU-8689abc");
    assert_eq!(
        preferred_clickup_task_token("CU-8689abc", None),
        "CU-8689abc"
    );
}

#[test]
fn validated_aliases_match_at_token_boundaries_in_evidence_order() {
    let task = identity("8689abc", Some("DEV-42"));
    let branch_match = matching_clickup_evidence(
        &task,
        &evidence(
            "feature/dev-42-fix",
            "DEV-42 appears in the title too",
            None,
            &[],
        ),
    )
    .expect("branch should match first");
    assert_eq!(branch_match.source, ClickUpGitEvidenceSource::Branch);
    assert_eq!(branch_match.matched_token, "DEV-42");

    let commit_match = matching_clickup_evidence(
        &task,
        &evidence(
            "feature/unrelated",
            "No ticket",
            None,
            &["Fix DEV-42 safely"],
        ),
    )
    .expect("commit subject should match");
    assert_eq!(commit_match.source, ClickUpGitEvidenceSource::CommitSubject);
}

#[test]
fn aliases_do_not_match_arbitrary_substrings() {
    let task = identity("8689abc", Some("DEV-42"));
    assert!(matching_clickup_evidence(
        &task,
        &evidence(
            "feature/xdev-420-fix",
            "PREDEV-42POST",
            Some("DEV-420 is a different task"),
            &["Avoid DEV-421 too"],
        ),
    )
    .is_none());
}

#[test]
fn candidate_extraction_deduplicates_same_task_across_fields() {
    let candidates = clickup_task_candidates(&evidence(
        "feature/CU-8689abc-fix",
        "CU-8689abc: Fix it",
        Some("Tracks CU-8689abc"),
        &["CU-8689abc first pass"],
    ));
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].lookup_key, "8689abc");
    assert_eq!(candidates[0].matched_token, "CU-8689abc");
    assert_eq!(candidates[0].source, ClickUpGitEvidenceSource::Branch);
}

#[test]
fn candidate_extraction_retains_distinct_task_candidates_for_fail_closed_reconciliation() {
    let candidates = clickup_task_candidates(&evidence(
        "feature/DEV-42-fix",
        "Also touches OPS-7",
        None,
        &[],
    ));
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].lookup_key, "DEV-42");
    assert_eq!(candidates[1].lookup_key, "OPS-7");
}

#[test]
fn ticket_start_prefers_unique_pull_request_over_branch_candidates() {
    let resolution = select_clickup_ticket_start_candidate(
        &identity("8689abc", Some("DEV-42")),
        vec![pull_request(42, "feature/DEV-42", "DEV-42: fix")],
        vec!["feature/DEV-42-old".to_string()],
    );

    let ClickUpTicketStartResolution::Unique(candidate) = resolution else {
        panic!("expected unique candidate");
    };
    assert_eq!(candidate.branch_name, "feature/DEV-42");
    assert_eq!(candidate.pull_request.map(|pr| pr.number), Some(42));
}

#[test]
fn ticket_start_fails_closed_for_multiple_pull_requests_or_branches() {
    let task = identity("8689abc", Some("DEV-42"));
    assert!(matches!(
        select_clickup_ticket_start_candidate(
            &task,
            vec![
                pull_request(41, "feature/DEV-42-a", "DEV-42 a"),
                pull_request(42, "feature/DEV-42-b", "DEV-42 b"),
            ],
            Vec::new(),
        ),
        ClickUpTicketStartResolution::Ambiguous { .. }
    ));
    assert!(matches!(
        select_clickup_ticket_start_candidate(
            &task,
            Vec::new(),
            vec![
                "feature/DEV-42-a".to_string(),
                "feature/DEV-42-b".to_string(),
            ],
        ),
        ClickUpTicketStartResolution::Ambiguous { .. }
    ));
}

#[test]
fn ticket_start_filters_cross_repo_duplicate_and_unmatched_prs_before_branch_fallback() {
    let resolution = select_clickup_ticket_start_candidate(
        &identity("8689abc", Some("DEV-42")),
        vec![
            pull_request(42, "feature/unrelated", "No ticket here"),
            PrSearchResult {
                is_cross_repository: true,
                ..pull_request(43, "feature/DEV-42-cross", "DEV-42 fork")
            },
        ],
        vec!["feature/DEV-42-branch".to_string()],
    );

    let ClickUpTicketStartResolution::Unique(candidate) = resolution else {
        panic!("expected branch fallback after PR filtering");
    };
    assert_eq!(candidate.branch_name, "feature/DEV-42-branch");
    assert!(candidate.pull_request.is_none());
}

#[test]
fn ticket_start_deduplicates_branch_names_case_insensitively() {
    let resolution = select_clickup_ticket_start_candidate(
        &identity("8689abc", Some("DEV-42")),
        Vec::new(),
        vec![
            " feature/DEV-42 ".to_string(),
            "FEATURE/dev-42".to_string(),
            String::new(),
        ],
    );

    let ClickUpTicketStartResolution::Unique(candidate) = resolution else {
        panic!("expected case-insensitive duplicate branches to collapse");
    };
    assert_eq!(candidate.branch_name, "feature/DEV-42");
}

#[test]
fn candidate_extraction_covers_body_commits_opaque_ids_and_invalid_tokens() {
    let candidates = clickup_task_candidates(&evidence(
        "--CU-ab12--",
        "not-a-ticket DEV-abc and cu-",
        Some("Body references OPS-7"),
        &["Commit mentions CU-xyz9-extra and ab-12"],
    ));

    let lookup_keys = candidates
        .iter()
        .map(|candidate| candidate.lookup_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(lookup_keys, vec!["ab12", "OPS-7", "xyz9"]);
    assert_eq!(candidates[0].matched_token, "CU-ab12");
    assert_eq!(
        candidates[1].source,
        ClickUpGitEvidenceSource::PullRequestBody
    );
    assert_eq!(
        candidates[2].source,
        ClickUpGitEvidenceSource::CommitSubject
    );
}

#[tokio::test]
async fn pr_reconciliation_does_not_link_when_another_candidate_cannot_be_validated() {
    let clickup = partial_failure_clickup_service().await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));
    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-1".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence("feature/DEV-42-fix", "Also touches OPS-7", None, &[]),
            pr_number: 42,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should remain non-fatal");

    assert!(matches!(
        outcome,
        ClickUpPrAssociationOutcome::PendingValidation { .. }
    ));
    assert!(links
        .list_ticket_links_for_conversation("conversation-1")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pr_reconciliation_reports_no_candidate_without_persisting_links() {
    let clickup = static_clickup_service(vec![clickup_task("8689abc", "DEV-42")]).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-no-candidate".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence("feature/no-ticket", "No ticket", None, &[]),
            pr_number: 50,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(outcome, ClickUpPrAssociationOutcome::NoCandidate);
    assert!(links
        .list_ticket_links_for_conversation("conversation-no-candidate")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pr_reconciliation_reports_no_validated_candidate_for_not_found_task() {
    let clickup = static_clickup_service(Vec::new()).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-missing".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence("feature/DEV-42", "DEV-42: fix", None, &[]),
            pr_number: 51,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(outcome, ClickUpPrAssociationOutcome::NoValidatedCandidate);
    assert!(links
        .list_ticket_links_for_conversation("conversation-missing")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pr_reconciliation_fails_closed_for_ambiguous_validated_tasks() {
    let clickup = static_clickup_service(vec![
        clickup_task("8689abc", "DEV-42"),
        clickup_task("99xyz", "OPS-7"),
    ])
    .await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-ambiguous".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence(
                "feature/DEV-42",
                "No title ticket",
                None,
                &["OPS-7 follow-up"],
            ),
            pr_number: 52,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        ClickUpPrAssociationOutcome::Ambiguous {
            task_ids: vec!["8689abc".to_string(), "99xyz".to_string()]
        }
    );
    assert!(links
        .list_ticket_links_for_conversation("conversation-ambiguous")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pr_reconciliation_persists_link_and_sync_record_for_single_validated_task() {
    let clickup = static_clickup_service(vec![clickup_task("8689abc", "DEV-42")]).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-linked".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence(
                "feature/DEV-42-fix",
                "No title ticket",
                Some("Body links DEV-42 for context"),
                &[],
            ),
            pr_number: 53,
            pr_url: Some("https://github.com/owner/repo/pull/53".to_string()),
            pr_status: "merged".to_string(),
            head_sha: Some("abc123".to_string()),
        },
    )
    .await
    .expect("reconciliation should succeed");

    let ClickUpPrAssociationOutcome::Linked { task_id, link_id } = outcome else {
        panic!("expected link outcome");
    };
    assert_eq!(task_id, "8689abc");

    let ticket_links = links
        .list_ticket_links_for_conversation("conversation-linked")
        .await
        .unwrap();
    assert_eq!(ticket_links.len(), 1);
    assert_eq!(ticket_links[0].id, link_id);
    assert_eq!(ticket_links[0].external_id, "8689abc");
    assert_eq!(ticket_links[0].external_key.as_deref(), Some("DEV-42"));
    assert_eq!(ticket_links[0].local_sha.as_deref(), Some("abc123"));
    assert_eq!(ticket_links[0].local_state.as_deref(), Some("merged"));
    assert!(ticket_links[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("\"source\":\"branch\""));

    let sync_records = links.list_sync_records_for_link(&link_id).await.unwrap();
    assert_eq!(sync_records.len(), 1);
    assert_eq!(sync_records[0].sync_kind, "clickup_git_association");
    assert_eq!(sync_records[0].local_sha.as_deref(), Some("abc123"));
    assert_eq!(sync_records[0].local_state.as_deref(), Some("merged"));
}

// Regression: a ticket token appearing only in the PR body (e.g. a
// documentation example like `@clickup:DEV-42`) must not create a
// ticket↔conversation link, even when the referenced task exists.
#[tokio::test]
async fn pr_reconciliation_does_not_link_from_body_only_ticket_mentions() {
    let clickup = static_clickup_service(vec![clickup_task("8689abc", "DEV-42")]).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-body-only".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence(
                "ralphx/ralphx/agent-1234abcd",
                "No title ticket",
                Some("Docs example: use @clickup:DEV-42 to reference a ticket"),
                &[],
            ),
            pr_number: 60,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(outcome, ClickUpPrAssociationOutcome::NoValidatedCandidate);
    assert!(links
        .list_ticket_links_for_conversation("conversation-body-only")
        .await
        .unwrap()
        .is_empty());
}

// Regression: a ticket prefix appearing only in the PR title must not create
// or re-validate a link. RalphX's own title normalizer writes that prefix, so
// accepting it as evidence lets the normalizer's output validate the very link
// that produced it (self-reinforcing contamination loop).
#[tokio::test]
async fn pr_reconciliation_does_not_link_from_title_only_ticket_prefix() {
    let clickup = static_clickup_service(vec![clickup_task("8689abc", "DEV-42")]).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-title-only".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence(
                "ralphx/ralphx/agent-1234abcd",
                "DEV-42: Normalizer-written title",
                None,
                &[],
            ),
            pr_number: 61,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(outcome, ClickUpPrAssociationOutcome::NoValidatedCandidate);
    assert!(links
        .list_ticket_links_for_conversation("conversation-title-only")
        .await
        .unwrap()
        .is_empty());
}

// Branch-authored commit subjects remain valid link evidence.
#[tokio::test]
async fn pr_reconciliation_links_from_commit_subject_evidence() {
    let clickup = static_clickup_service(vec![clickup_task("8689abc", "DEV-42")]).await;
    let links = ExternalIssueLinkService::new(Arc::new(MemoryExternalIssueLinkRepository::new()));

    let outcome = reconcile_clickup_pr_to_conversation(
        &clickup,
        &links,
        ClickUpPrAssociationInput {
            conversation_id: "conversation-commit-subject".to_string(),
            project_id: "project-1".to_string(),
            evidence: evidence(
                "ralphx/ralphx/agent-1234abcd",
                "No title ticket",
                None,
                &["DEV-42: implement the fix"],
            ),
            pr_number: 62,
            pr_url: None,
            pr_status: "open".to_string(),
            head_sha: None,
        },
    )
    .await
    .expect("reconciliation should succeed");

    let ClickUpPrAssociationOutcome::Linked { task_id, link_id } = outcome else {
        panic!("expected link outcome");
    };
    assert_eq!(task_id, "8689abc");
    let ticket_links = links
        .list_ticket_links_for_conversation("conversation-commit-subject")
        .await
        .unwrap();
    assert_eq!(ticket_links.len(), 1);
    assert_eq!(ticket_links[0].id, link_id);
    assert!(ticket_links[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("\"source\":\"commit_subject\""));
}
