use super::agent_workspace_pr_autofix_attempt::*;
use crate::domain::entities::{
    AgentRun, AgentRunActionKind, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage,
    ChatConversationId, InterruptedConversation,
};
use crate::domain::repositories::AgentRunRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryAgentRunRepository;
use async_trait::async_trait;

struct FailingAuthorityReadRepository;

#[async_trait]
impl AgentRunRepository for FailingAuthorityReadRepository {
    async fn create(&self, _run: AgentRun) -> AppResult<AgentRun> {
        unreachable!("authority read test does not create runs")
    }

    async fn get_by_id(&self, _id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        Err(AppError::Infrastructure(
            "authority repository unavailable".to_string(),
        ))
    }

    async fn get_latest_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        unreachable!("authority read test only reads the caller")
    }

    async fn get_active_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        unreachable!("authority read test only reads the caller")
    }

    async fn get_by_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        unreachable!("authority read test only reads the caller")
    }

    async fn update_status(&self, _id: &AgentRunId, _status: AgentRunStatus) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn update_usage(&self, _id: &AgentRunId, _usage: &AgentRunUsage) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn update_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn complete(&self, _id: &AgentRunId) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn complete_if_prune_cancelled(&self, _id: &AgentRunId) -> AppResult<bool> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn fail(&self, _id: &AgentRunId, _error_message: &str) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn cancel_with_reason(&self, _id: &AgentRunId, _reason: &str) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn delete(&self, _id: &AgentRunId) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn delete_by_conversation(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn count_by_status(
        &self,
        _conversation_id: &ChatConversationId,
        _status: AgentRunStatus,
    ) -> AppResult<u32> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn cancel_running_started_before(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32> {
        unreachable!("authority read test does not mutate runs")
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        unreachable!("authority read test does not inspect interrupted runs")
    }
}

async fn create_attempt(
    repo: &MemoryAgentRunRepository,
    conversation_id: ChatConversationId,
    fingerprint: &str,
    status: AgentRunStatus,
) -> AgentRunId {
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("42".to_string());
    run.action_target_id = Some(fingerprint.to_string());
    run.status = status;
    repo.create(run).await.unwrap().id
}

async fn create_attempt_at(
    repo: &MemoryAgentRunRepository,
    conversation_id: ChatConversationId,
    fingerprint: &str,
    status: AgentRunStatus,
    started_at: chrono::DateTime<chrono::Utc>,
) -> AgentRunId {
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("42".to_string());
    run.action_target_id = Some(fingerprint.to_string());
    run.status = status;
    run.started_at = started_at;
    repo.create(run).await.unwrap().id
}

#[tokio::test]
async fn unchanged_fingerprint_allows_only_one_failed_retry() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let fingerprint = "github_pr_autofix:42:head:issue";

    assert_eq!(
        load_pr_autofix_attempt_decision(&repo, &conversation_id, 42, fingerprint, false,)
            .await
            .unwrap(),
        PrAutofixAttemptDecision::StartFirst
    );
    create_attempt(&repo, conversation_id, fingerprint, AgentRunStatus::Failed).await;
    assert_eq!(
        load_pr_autofix_attempt_decision(&repo, &conversation_id, 42, fingerprint, false,)
            .await
            .unwrap(),
        PrAutofixAttemptDecision::StartRetry
    );
    create_attempt(&repo, conversation_id, fingerprint, AgentRunStatus::Failed).await;
    assert_eq!(
        load_pr_autofix_attempt_decision(&repo, &conversation_id, 42, fingerprint, false,)
            .await
            .unwrap(),
        PrAutofixAttemptDecision::RetryExhausted
    );
}

#[tokio::test]
async fn terminal_nonfailed_attempts_and_legacy_events_require_manual_action() {
    for status in [AgentRunStatus::Completed, AgentRunStatus::Cancelled] {
        let repo = MemoryAgentRunRepository::new();
        let conversation_id = ChatConversationId::new();
        create_attempt(&repo, conversation_id, "same", status).await;
        assert!(
            !load_pr_autofix_attempt_decision(&repo, &conversation_id, 42, "same", false,)
                .await
                .unwrap()
                .allows_start()
        );
    }

    let repo = MemoryAgentRunRepository::new();
    assert_eq!(
        load_pr_autofix_attempt_decision(&repo, &ChatConversationId::new(), 42, "legacy", true,)
            .await
            .unwrap(),
        PrAutofixAttemptDecision::LegacyUnbound
    );
}

#[tokio::test]
async fn unrelated_active_run_does_not_own_the_pr_autofix_attempt() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let mut unrelated = AgentRun::new(conversation_id);
    unrelated.action_kind = Some(AgentRunActionKind::VerifyPlan);
    unrelated.action_context_id = Some("plan-session".to_string());
    unrelated.action_target_id = Some("plan-version".to_string());
    repo.create(unrelated).await.unwrap();

    assert_eq!(
        load_pr_autofix_attempt_decision(
            &repo,
            &conversation_id,
            42,
            "github_pr_autofix:42:head:issue",
            false,
        )
        .await
        .unwrap(),
        PrAutofixAttemptDecision::StartFirst
    );
}

#[test]
fn coverage_regression_manual_decisions_explain_why_autofix_cannot_start() {
    let cases = [
        (
            PrAutofixAttemptDecision::RetryExhausted,
            "retry budget is exhausted",
        ),
        (
            PrAutofixAttemptDecision::CompletedUnresolved,
            "completed but the same issue is still unresolved",
        ),
        (
            PrAutofixAttemptDecision::Cancelled,
            "was cancelled while the same issue remains unresolved",
        ),
        (
            PrAutofixAttemptDecision::LegacyUnbound,
            "legacy PR autofix event has no exact fixer attempt",
        ),
    ];

    for (decision, expected) in cases {
        assert!(!decision.allows_start());
        assert!(decision.manual_summary().unwrap().contains(expected));
    }
    assert_eq!(PrAutofixAttemptDecision::StartFirst.manual_summary(), None);
}

#[tokio::test]
async fn coverage_regression_running_exact_attempt_blocks_duplicate_dispatch() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let fingerprint = "github_pr_autofix:42:head:running";
    create_attempt(&repo, conversation_id, fingerprint, AgentRunStatus::Running).await;

    let decision =
        load_pr_autofix_attempt_decision(&repo, &conversation_id, 42, fingerprint, false)
            .await
            .unwrap();

    assert_eq!(decision, PrAutofixAttemptDecision::Active);
    assert!(!decision.allows_start());
}

#[tokio::test]
async fn completion_authority_accepts_only_the_current_running_exact_attempt() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let fingerprint = "github_pr_autofix:42:head:current";
    let caller_id =
        create_attempt(&repo, conversation_id, fingerprint, AgentRunStatus::Running).await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Current
    );
}

#[tokio::test]
async fn completion_authority_treats_stale_caller_with_running_replacement_as_superseded() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let fingerprint = "github_pr_autofix:42:head:replacement";
    let stale_id = create_attempt(
        &repo,
        conversation_id.clone(),
        fingerprint,
        AgentRunStatus::Failed,
    )
    .await;
    create_attempt(
        &repo,
        conversation_id.clone(),
        fingerprint,
        AgentRunStatus::Running,
    )
    .await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&stale_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Superseded
    );
}

#[tokio::test]
async fn completion_authority_requires_the_caller_to_be_the_latest_exact_pr_attempt() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let fingerprint = "github_pr_autofix:42:head:active-replacement";
    let now = chrono::Utc::now();
    let caller_id = create_attempt_at(
        &repo,
        conversation_id.clone(),
        fingerprint,
        AgentRunStatus::Running,
        now - chrono::Duration::seconds(2),
    )
    .await;
    create_attempt_at(
        &repo,
        conversation_id.clone(),
        fingerprint,
        AgentRunStatus::Completed,
        now - chrono::Duration::seconds(1),
    )
    .await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Invalid
    );

    create_attempt_at(
        &repo,
        conversation_id.clone(),
        fingerprint,
        AgentRunStatus::Running,
        now,
    )
    .await;
    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Superseded
    );
}

#[tokio::test]
async fn completion_authority_supersedes_a_running_caller_for_an_older_fingerprint() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let now = chrono::Utc::now();
    let caller_id = create_attempt_at(
        &repo,
        conversation_id.clone(),
        "github_pr_autofix:42:head:old-issue",
        AgentRunStatus::Running,
        now - chrono::Duration::seconds(1),
    )
    .await;
    create_attempt_at(
        &repo,
        conversation_id.clone(),
        "github_pr_autofix:42:head:new-issue",
        AgentRunStatus::Running,
        now,
    )
    .await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Superseded
    );
}

#[tokio::test]
async fn completion_authority_treats_settled_exact_caller_as_already_completed() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let caller_id = create_attempt(
        &repo,
        conversation_id.clone(),
        "github_pr_autofix:42:head:settled",
        AgentRunStatus::Completed,
    )
    .await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::AlreadyCompleted
    );
}

#[tokio::test]
async fn completion_authority_fails_closed_for_missing_corrupt_or_wrong_owner_ids() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let caller_id = create_attempt(
        &repo,
        conversation_id.clone(),
        "github_pr_autofix:42:head:owner",
        AgentRunStatus::Running,
    )
    .await;

    for (caller_id, expected_pr) in [
        (None, 42),
        (Some("not-a-run-id".to_string()), 42),
        (Some(caller_id.to_string()), 43),
    ] {
        let authority = load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            expected_pr,
            caller_id.as_deref(),
        )
        .await
        .unwrap();
        assert_eq!(authority, PrAutofixCompletionAuthority::Invalid);
    }

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &ChatConversationId::new(),
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Invalid
    );
}

#[tokio::test]
async fn completion_authority_fails_closed_for_malformed_pr_autofix_fingerprint() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let caller_id = create_attempt(
        &repo,
        conversation_id.clone(),
        "malformed-fingerprint",
        AgentRunStatus::Running,
    )
    .await;

    assert_eq!(
        load_pr_autofix_completion_authority(
            &repo,
            &conversation_id,
            42,
            Some(&caller_id.to_string()),
        )
        .await
        .unwrap(),
        PrAutofixCompletionAuthority::Invalid
    );
}

#[tokio::test]
async fn completion_authority_propagates_repository_read_failures() {
    let repo = FailingAuthorityReadRepository;
    let conversation_id = ChatConversationId::new();
    let caller_id = AgentRun::new(conversation_id.clone()).id;

    let error = load_pr_autofix_completion_authority(
        &repo,
        &conversation_id,
        42,
        Some(&caller_id.to_string()),
    )
    .await
    .expect_err("authority must not treat a repository read failure as invalid or current");

    assert!(error
        .to_string()
        .contains("authority repository unavailable"));
}
