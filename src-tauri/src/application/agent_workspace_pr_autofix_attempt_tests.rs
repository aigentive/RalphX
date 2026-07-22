use super::agent_workspace_pr_autofix_attempt::*;
use crate::domain::entities::{AgentRun, AgentRunActionKind, AgentRunStatus, ChatConversationId};
use crate::domain::repositories::AgentRunRepository;
use crate::infrastructure::memory::MemoryAgentRunRepository;

async fn create_attempt(
    repo: &MemoryAgentRunRepository,
    conversation_id: ChatConversationId,
    fingerprint: &str,
    status: AgentRunStatus,
) {
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::PrAutofix);
    run.action_context_id = Some("42".to_string());
    run.action_target_id = Some(fingerprint.to_string());
    run.status = status;
    repo.create(run).await.unwrap();
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
