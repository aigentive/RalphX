use super::*;
use crate::infrastructure::memory::MemoryChatConversationRepository;

fn make_repo() -> Arc<dyn ChatConversationRepository> {
    Arc::new(MemoryChatConversationRepository::new())
}

// ── TaskExecution always creates a new conversation ──────────────────────

#[tokio::test]
async fn task_execution_creates_new_conversation_even_when_prior_exists() {
    let repo = make_repo();
    let task_id = "task-abc-123";

    // First call creates a conversation
    let (first, _) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();

    // Second call must create a NEW row, not return the existing one
    let (second, _) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();

    assert_ne!(
        first.id, second.id,
        "Expected a fresh conversation each time"
    );
    assert_eq!(second.context_type, ChatContextType::TaskExecution);
    assert_eq!(second.context_id, task_id);
}

// ── parent_conversation_id is set correctly on re-execution ──────────────

#[tokio::test]
async fn task_execution_second_run_has_parent_conversation_id() {
    let repo = make_repo();
    let task_id = "task-xyz-456";

    // First run — no parent yet
    let (first, _) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();
    assert!(
        first.parent_conversation_id.is_none(),
        "First run must have no parent"
    );

    // Second run — should point to first run
    let (second, _) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();
    assert_eq!(
        second.parent_conversation_id.as_deref(),
        Some(first.id.as_str().as_str()),
        "Second run must reference first run's conversation id"
    );
}

// ── Old conversations remain visible via list_conversations ──────────────

#[tokio::test]
async fn old_task_execution_conversations_remain_accessible() {
    let repo = make_repo();
    let task_id = "task-old-999";

    // Create two runs
    get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();
    get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();

    let all = list_conversations(repo, ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();

    assert_eq!(all.len(), 2, "Both execution conversations must be listed");
}

// ── Non-TaskExecution contexts reuse existing conversation ───────────────

#[tokio::test]
async fn non_task_execution_reuses_existing_conversation() {
    let repo = make_repo();
    let task_id = "task-review-111";

    let (first, _) = get_or_create_conversation(repo.clone(), ChatContextType::Task, task_id)
        .await
        .unwrap();

    let (second, _) = get_or_create_conversation(repo.clone(), ChatContextType::Task, task_id)
        .await
        .unwrap();

    assert_eq!(
        first.id, second.id,
        "Non-TaskExecution must reuse existing conversation"
    );
}

// ── is_new bool: true on first call, false on second ─────────────────────

#[tokio::test]
async fn is_new_true_on_first_call_false_on_second_for_reuse_context() {
    let repo = make_repo();
    let task_id = "task-isnew-222";

    // First call: conversation is created → is_new must be true
    let (_, is_new_first) = get_or_create_conversation(repo.clone(), ChatContextType::Task, task_id)
        .await
        .unwrap();
    assert!(is_new_first, "First call must return is_new=true");

    // Second call: conversation is reused → is_new must be false
    let (_, is_new_second) = get_or_create_conversation(repo.clone(), ChatContextType::Task, task_id)
        .await
        .unwrap();
    assert!(!is_new_second, "Second call must return is_new=false for reused conversation");
}

#[tokio::test]
async fn task_execution_always_returns_is_new_true() {
    let repo = make_repo();
    let task_id = "task-exec-isnew-333";

    // TaskExecution always creates fresh conversations — both calls must return is_new=true
    let (_, is_new_first) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();
    assert!(is_new_first, "First TaskExecution call must return is_new=true");

    let (_, is_new_second) = get_or_create_conversation(repo.clone(), ChatContextType::TaskExecution, task_id)
        .await
        .unwrap();
    assert!(is_new_second, "Second TaskExecution call must also return is_new=true (force_fresh)");
}
