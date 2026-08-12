use super::*;
use crate::testing::SqliteTestDb;

fn setup() -> (SqliteTestDb, SqliteQuestionRepository) {
    let db = SqliteTestDb::new("sqlite_question_repo_tests");
    let repo = SqliteQuestionRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn sample_info() -> PendingQuestionInfo {
    PendingQuestionInfo {
        request_id: "req-1".to_string(),
        session_id: "session-1".to_string(),
        question: "Which database?".to_string(),
        header: Some("Database Selection".to_string()),
        options: vec![
            QuestionOption {
                value: "pg".to_string(),
                label: "PostgreSQL".to_string(),
                description: Some("Relational".to_string()),
            },
            QuestionOption {
                value: "sqlite".to_string(),
                label: "SQLite".to_string(),
                description: None,
            },
        ],
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
    let (_db, repo) = setup();
    let info = sample_info();

    repo.create_pending(&info).await.unwrap();

    let pending = repo.get_pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "req-1");
    assert_eq!(pending[0].session_id, "session-1");
    assert_eq!(pending[0].question, "Which database?");
    assert_eq!(pending[0].header, Some("Database Selection".to_string()));
    assert_eq!(pending[0].options.len(), 2);
    assert_eq!(pending[0].options[0].value, "pg");
    assert_eq!(pending[0].options[1].label, "SQLite");
    assert!(!pending[0].multi_select);
    assert!(pending[0].allow_skip);
    assert_eq!(pending[0].batch_index, None);
    assert_eq!(pending[0].batch_total, None);
}

#[tokio::test]
async fn test_skip_and_batch_metadata_round_trip() {
    let (_db, repo) = setup();
    let info = PendingQuestionInfo {
        request_id: "req-batch".to_string(),
        session_id: "session-1".to_string(),
        question: "Any deadline constraints?".to_string(),
        header: Some("Planning interview".to_string()),
        options: vec![],
        multi_select: false,
        allow_skip: false,
        batch_index: Some(2),
        batch_total: Some(3),
        metadata: Some(serde_json::json!({
            "kind": "plan_mode_proposal",
            "conversation_id": "conversation-1",
        })),
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    repo.create_pending(&info).await.unwrap();

    let found = repo.get_by_request_id("req-batch").await.unwrap().unwrap();
    assert!(!found.allow_skip);
    assert_eq!(found.batch_index, Some(2));
    assert_eq!(found.batch_total, Some(3));
    assert_eq!(
        found
            .metadata
            .as_ref()
            .and_then(|value| value.get("kind"))
            .and_then(|value| value.as_str()),
        Some("plan_mode_proposal"),
    );

    let answer = QuestionAnswer {
        selected_options: vec![],
        text: None,
        skipped: true,
    };
    assert!(repo.resolve("req-batch", &answer).await.unwrap());

    let stored = repo
        .get_resolved_answer("req-batch")
        .await
        .unwrap()
        .unwrap();
    assert!(stored.skipped);
    assert!(stored.selected_options.is_empty());
    assert!(stored.text.is_none());
}

#[tokio::test]
async fn test_get_by_request_id() {
    let (_db, repo) = setup();
    repo.create_pending(&sample_info()).await.unwrap();

    let found = repo.get_by_request_id("req-1").await.unwrap();
    assert!(found.is_some());
    let q = found.unwrap();
    assert_eq!(q.question, "Which database?");
    assert_eq!(q.options.len(), 2);

    let not_found = repo.get_by_request_id("nonexistent").await.unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_resolve() {
    let (_db, repo) = setup();
    repo.create_pending(&sample_info()).await.unwrap();

    let answer = QuestionAnswer {
        selected_options: vec!["pg".to_string()],
        text: None,
        skipped: false,
    };
    let resolved = repo.resolve("req-1", &answer).await.unwrap();
    assert!(resolved);

    // After resolving, no longer in pending
    let pending = repo.get_pending().await.unwrap();
    assert!(pending.is_empty());

    // But still retrievable by id
    let found = repo.get_by_request_id("req-1").await.unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_resolve_nonexistent() {
    let (_db, repo) = setup();
    let answer = QuestionAnswer {
        selected_options: vec![],
        text: None,
        skipped: false,
    };
    let resolved = repo.resolve("nope", &answer).await.unwrap();
    assert!(!resolved);
}

#[tokio::test]
async fn test_expire_all_pending() {
    let (_db, repo) = setup();

    for i in 0..3 {
        let info = PendingQuestionInfo {
            request_id: format!("req-{}", i),
            session_id: "session-1".to_string(),
            question: format!("Q{}", i),
            header: None,
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: None,
            created_at: "2026-07-10T00:00:00+00:00".to_string(),
        };
        repo.create_pending(&info).await.unwrap();
    }

    // Resolve one so it's not pending
    let answer = QuestionAnswer {
        selected_options: vec![],
        text: Some("done".to_string()),
        skipped: false,
    };
    repo.resolve("req-0", &answer).await.unwrap();

    let expired = repo.expire_all_pending().await.unwrap();
    assert_eq!(expired, 2);

    let pending = repo.get_pending().await.unwrap();
    assert_eq!(pending.len(), 2);
    let request_ids: Vec<_> = pending.iter().map(|p| p.request_id.as_str()).collect();
    assert!(request_ids.contains(&"req-1"));
    assert!(request_ids.contains(&"req-2"));
}

#[tokio::test]
async fn test_remove() {
    let (_db, repo) = setup();
    repo.create_pending(&sample_info()).await.unwrap();

    let removed = repo.remove("req-1").await.unwrap();
    assert!(removed);

    let found = repo.get_by_request_id("req-1").await.unwrap();
    assert!(found.is_none());

    let removed_again = repo.remove("req-1").await.unwrap();
    assert!(!removed_again);
}


#[tokio::test]
async fn test_expired_wait_remains_resolvable() {
    let (_db, repo) = setup();
    repo.create_pending(&sample_info()).await.unwrap();

    repo.expire_by_request_id("req-1").await.unwrap();

    let pending = repo.get_pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "req-1");

    let answer = QuestionAnswer {
        selected_options: vec![],
        text: Some("Use SQLite".to_string()),
        skipped: false,
    };
    assert!(repo.resolve("req-1", &answer).await.unwrap());
    assert!(repo.get_pending().await.unwrap().is_empty());

    let stored = repo.get_resolved_answer("req-1").await.unwrap().unwrap();
    assert_eq!(stored.text, Some("Use SQLite".to_string()));
}

#[tokio::test]
async fn test_multi_select_round_trip() {
    let (_db, repo) = setup();
    let info = PendingQuestionInfo {
        request_id: "req-multi".to_string(),
        session_id: "session-1".to_string(),
        question: "Select all that apply".to_string(),
        header: None,
        options: vec![
            QuestionOption {
                value: "a".to_string(),
                label: "A".to_string(),
                description: None,
            },
            QuestionOption {
                value: "b".to_string(),
                label: "B".to_string(),
                description: None,
            },
        ],
        multi_select: true,
        allow_skip: true,
        batch_index: None,
        batch_total: None,
        metadata: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    repo.create_pending(&info).await.unwrap();

    let found = repo.get_by_request_id("req-multi").await.unwrap().unwrap();
    assert!(found.multi_select);
}
