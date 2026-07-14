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
