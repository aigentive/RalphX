use super::*;
use crate::domain::entities::question_request::QuestionOption;

fn sample_question(request_id: &str) -> PendingQuestionInfo {
    PendingQuestionInfo {
        request_id: request_id.to_string(),
        session_id: "session-1".to_string(),
        question: "Which approach?".to_string(),
        header: None,
        options: vec![QuestionOption {
            value: "a".to_string(),
            label: "Option A".to_string(),
            description: None,
        }],
        multi_select: false,
        allow_skip: true,
        batch_index: None,
        batch_total: None,
        metadata: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    }
}

#[tokio::test]
async fn test_create_and_get_pending() {
    let repo = MemoryQuestionRepository::new();
    repo.create_pending(&sample_question("req-1"))
        .await
        .unwrap();

    let pending = repo.get_pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "req-1");
}

#[tokio::test]
async fn test_get_by_request_id() {
    let repo = MemoryQuestionRepository::new();
    repo.create_pending(&sample_question("req-42"))
        .await
        .unwrap();

    let found = repo.get_by_request_id("req-42").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().question, "Which approach?");

    let not_found = repo.get_by_request_id("nonexistent").await.unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_resolve() {
    let repo = MemoryQuestionRepository::new();
    repo.create_pending(&sample_question("req-1"))
        .await
        .unwrap();

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    assert!(repo.resolve("req-1", &answer).await.unwrap());

    let pending = repo.get_pending().await.unwrap();
    assert!(pending.is_empty());

    // Record still exists
    assert!(repo.get_by_request_id("req-1").await.unwrap().is_some());
}

#[tokio::test]
async fn test_resolve_nonexistent() {
    let repo = MemoryQuestionRepository::new();
    let answer = QuestionAnswer {
        selected_options: vec![],
        text: None,
        skipped: false,
    };
    assert!(!repo.resolve("nope", &answer).await.unwrap());
}

#[tokio::test]
async fn test_expire_all_pending() {
    let repo = MemoryQuestionRepository::new();
    for i in 0..3 {
        repo.create_pending(&sample_question(&format!("req-{i}")))
            .await
            .unwrap();
    }

    // Resolve one
    let answer = QuestionAnswer {
        selected_options: vec![],
        text: Some("done".to_string()),
        skipped: false,
    };
    repo.resolve("req-0", &answer).await.unwrap();

    let expired = repo.expire_all_pending().await.unwrap();
    assert_eq!(expired, 2);
    assert_eq!(repo.get_pending().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_expired_wait_remains_resolvable() {
    let repo = MemoryQuestionRepository::new();
    repo.create_pending(&sample_question("req-1"))
        .await
        .unwrap();

    repo.expire_by_request_id("req-1").await.unwrap();
    assert_eq!(repo.get_pending().await.unwrap().len(), 1);

    let answer = QuestionAnswer {
        selected_options: vec![],
        text: Some("late".to_string()),
        skipped: false,
    };
    assert!(repo.resolve("req-1", &answer).await.unwrap());
    assert!(repo.get_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_remove() {
    let repo = MemoryQuestionRepository::new();
    repo.create_pending(&sample_question("req-rm"))
        .await
        .unwrap();

    assert!(repo.remove("req-rm").await.unwrap());
    assert!(repo.get_by_request_id("req-rm").await.unwrap().is_none());
    assert!(!repo.remove("req-rm").await.unwrap());
}

#[test]
fn test_default_impl() {
    let repo = MemoryQuestionRepository::default();
    let questions = repo.questions.read().unwrap();
    assert!(questions.is_empty());
}
