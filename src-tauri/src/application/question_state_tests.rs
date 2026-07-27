use super::*;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn test_question_state_new() {
    let state = QuestionState::new();
    let pending = state.pending.lock().await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_question_state_default() {
    let state = QuestionState::default();
    let pending = state.pending.lock().await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_question_answer_clone() {
    let answer = QuestionAnswer {
        selected_options: vec!["opt1".to_string()],
        text: Some("Custom text".to_string()),
        skipped: false,
    };
    let cloned = answer.clone();
    assert_eq!(cloned.selected_options, vec!["opt1"]);
    assert_eq!(cloned.text, Some("Custom text".to_string()));
    assert!(!cloned.skipped);
}

#[tokio::test]
async fn test_question_answer_serialization() {
    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string(), "b".to_string()],
        text: None,
        skipped: false,
    };
    let json = serde_json::to_string(&answer).unwrap();
    assert!(json.contains("\"selected_options\""));
    assert!(json.contains("\"skipped\""));

    let deserialized: QuestionAnswer = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.selected_options.len(), 2);
    assert!(deserialized.text.is_none());
    assert!(!deserialized.skipped);
}

#[tokio::test]
async fn test_pending_question_info_serialization() {
    let info = PendingQuestionInfo {
        request_id: "req-123".to_string(),
        session_id: "session-456".to_string(),
        question: "Which approach?".to_string(),
        header: Some("Architecture Decision".to_string()),
        options: vec![QuestionOption {
            value: "a".to_string(),
            label: "Option A".to_string(),
            description: Some("First approach".to_string()),
        }],
        multi_select: false,
        allow_skip: true,
        batch_index: None,
        batch_total: None,
        metadata: Some(serde_json::json!({ "kind": "plan_mode_proposal" })),
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"request_id\":\"req-123\""));
    assert!(json.contains("\"session_id\":\"session-456\""));
    assert!(json.contains("\"question\":\"Which approach?\""));
    assert!(json.contains("\"allow_skip\":true"));
    assert!(json.contains("\"kind\":\"plan_mode_proposal\""));
}

#[tokio::test]
async fn test_pending_question_info_defaults_allow_skip_when_missing() {
    let info: PendingQuestionInfo = serde_json::from_value(serde_json::json!({
        "request_id": "req-default-skip",
        "session_id": "session-1",
        "question": "Proceed?",
        "header": null,
        "options": [],
        "multi_select": false
    }))
    .expect("pending question info deserializes");

    assert!(info.allow_skip);
}

#[tokio::test]
async fn test_register_with_metadata_tracks_skip_and_batch_progress() {
    let state = QuestionState::new();

    state
        .register_with_metadata(
            "req-batch-2".to_string(),
            "session-1".to_string(),
            "Any deadline constraints?".to_string(),
            Some("Planning interview".to_string()),
            vec![],
            false,
            false,
            Some(2),
            Some(3),
            None,
        )
        .await;

    let pending = state.get_pending_info().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "req-batch-2");
    assert!(!pending[0].allow_skip);
    assert_eq!(pending[0].batch_index, Some(2));
    assert_eq!(pending[0].batch_total, Some(3));
}

#[tokio::test]
async fn test_register_and_resolve_question() {
    let state = QuestionState::new();

    let request_id = "test-question-123".to_string();
    let rx = state
        .register(
            request_id.clone(),
            "session-1".to_string(),
            "Which framework?".to_string(),
            None,
            vec![
                QuestionOption {
                    value: "react".to_string(),
                    label: "React".to_string(),
                    description: None,
                },
                QuestionOption {
                    value: "vue".to_string(),
                    label: "Vue".to_string(),
                    description: None,
                },
            ],
            false,
        )
        .await;

    // Verify it's in pending
    {
        let pending = state.pending.lock().await;
        assert!(pending.contains_key(&request_id));
        let question = pending.get(&request_id).unwrap();
        assert_eq!(question.info.question, "Which framework?");
    }

    // Resolve with an answer
    let result = state
        .resolve(
            &request_id,
            QuestionAnswer {
                selected_options: vec!["react".to_string()],
                text: None,
                skipped: false,
            },
        )
        .await;
    assert!(result.resolved);
    assert_eq!(result.session_id, Some("session-1".to_string()));
    assert!(result.delivered_to_waiting_agent);

    // Check the answer was received
    let answer = rx.borrow().clone();
    assert!(answer.is_some());
    let answer = answer.unwrap();
    assert_eq!(answer.selected_options, vec!["react"]);
}

#[tokio::test]
async fn test_get_pending_info() {
    let state = QuestionState::new();

    for i in 0..3 {
        state
            .register(
                format!("request-{}", i),
                "session-1".to_string(),
                format!("Question {}", i),
                None,
                vec![],
                false,
            )
            .await;
    }

    let pending_info = state.get_pending_info().await;
    assert_eq!(pending_info.len(), 3);

    let request_ids: Vec<_> = pending_info.iter().map(|p| p.request_id.as_str()).collect();
    assert!(request_ids.contains(&"request-0"));
    assert!(request_ids.contains(&"request-1"));
    assert!(request_ids.contains(&"request-2"));
}

#[tokio::test]
async fn test_remove_pending_question() {
    let state = QuestionState::new();

    let request_id = "to-remove".to_string();
    state
        .register(
            request_id.clone(),
            "session-1".to_string(),
            "Remove me?".to_string(),
            None,
            vec![],
            false,
        )
        .await;

    {
        let pending = state.pending.lock().await;
        assert!(pending.contains_key(&request_id));
    }

    let removed = state.remove(&request_id).await;
    assert!(removed);

    {
        let pending = state.pending.lock().await;
        assert!(!pending.contains_key(&request_id));
    }

    let removed_again = state.remove(&request_id).await;
    assert!(!removed_again);
}

#[tokio::test]
async fn test_expire_pending_question() {
    let state = QuestionState::new();

    state
        .register(
            "to-expire".to_string(),
            "session-1".to_string(),
            "Expire me?".to_string(),
            None,
            vec![],
            false,
        )
        .await;

    let expired = state.expire("to-expire").await;
    assert!(expired.is_some());
    assert_eq!(expired.unwrap().session_id, "session-1");

    let pending = state.pending.lock().await;
    assert!(!pending.contains_key("to-expire"));
}

#[tokio::test]
async fn test_resolve_nonexistent_question() {
    let state = QuestionState::new();

    let result = state
        .resolve(
            "nonexistent",
            QuestionAnswer {
                selected_options: vec![],
                text: None,
                skipped: false,
            },
        )
        .await;
    assert!(!result.resolved);
    assert!(result.session_id.is_none());
    assert!(!result.delivered_to_waiting_agent);
}

#[tokio::test]
async fn test_has_pending_for_session() {
    let state = QuestionState::new();

    // No pending questions initially
    assert!(!state.has_pending_for_session("session-1").await);
    assert!(!state.has_pending_for_session("session-2").await);

    // Register a question for session-1
    state
        .register(
            "req-1".to_string(),
            "session-1".to_string(),
            "Question 1?".to_string(),
            None,
            vec![],
            false,
        )
        .await;

    // Now session-1 should have a pending question
    assert!(state.has_pending_for_session("session-1").await);
    assert!(!state.has_pending_for_session("session-2").await);

    // Register another question for session-2
    state
        .register(
            "req-2".to_string(),
            "session-2".to_string(),
            "Question 2?".to_string(),
            None,
            vec![],
            false,
        )
        .await;

    // Both should now have pending questions
    assert!(state.has_pending_for_session("session-1").await);
    assert!(state.has_pending_for_session("session-2").await);

    // Remove from session-1
    state.remove("req-1").await;

    // Now only session-2 should have pending
    assert!(!state.has_pending_for_session("session-1").await);
    assert!(state.has_pending_for_session("session-2").await);
}

// --- Tests with repo persistence ---

mod with_repo {
    use super::*;
    use crate::domain::repositories::QuestionRepository;
    use crate::infrastructure::memory::MemoryQuestionRepository;
    use chrono::{Duration, Utc};
    use std::sync::Arc;

    fn make_state_with_repo() -> (QuestionState, Arc<MemoryQuestionRepository>) {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let state = QuestionState::with_repo(repo.clone());
        (state, repo)
    }

    #[tokio::test]
    async fn test_with_repo_constructor() {
        let (state, _repo) = make_state_with_repo();
        assert!(state.repo.is_some());
        let pending = state.pending.lock().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_register_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        state
            .register(
                "req-1".to_string(),
                "session-1".to_string(),
                "Which framework?".to_string(),
                None,
                vec![QuestionOption {
                    value: "react".to_string(),
                    label: "React".to_string(),
                    description: None,
                }],
                false,
            )
            .await;

        // Verify persisted in repo
        let repo_pending = repo.get_pending().await.unwrap();
        assert_eq!(repo_pending.len(), 1);
        assert_eq!(repo_pending[0].request_id, "req-1");
        assert_eq!(repo_pending[0].question, "Which framework?");
    }

    #[tokio::test]
    async fn test_get_pending_info_keeps_live_question_when_repo_create_fails() {
        use crate::error::AppError;

        struct FailingCreateRepo(MemoryQuestionRepository);

        #[async_trait::async_trait]
        impl QuestionRepository for FailingCreateRepo {
            async fn create_pending(
                &self,
                _info: &PendingQuestionInfo,
            ) -> crate::error::AppResult<()> {
                Err(AppError::Database("simulated create failure".to_string()))
            }
            async fn resolve(
                &self,
                request_id: &str,
                answer: &QuestionAnswer,
            ) -> crate::error::AppResult<bool> {
                self.0.resolve(request_id, answer).await
            }
            async fn get_pending(&self) -> crate::error::AppResult<Vec<PendingQuestionInfo>> {
                self.0.get_pending().await
            }
            async fn get_by_request_id(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<PendingQuestionInfo>> {
                self.0.get_by_request_id(request_id).await
            }
            async fn expire_all_pending(&self) -> crate::error::AppResult<u64> {
                self.0.expire_all_pending().await
            }
            async fn expire_by_request_id(&self, request_id: &str) -> crate::error::AppResult<()> {
                self.0.expire_by_request_id(request_id).await
            }
            async fn remove(&self, request_id: &str) -> crate::error::AppResult<bool> {
                self.0.remove(request_id).await
            }
            async fn get_resolved_answer(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<QuestionAnswer>> {
                self.0.get_resolved_answer(request_id).await
            }
        }

        let repo = Arc::new(FailingCreateRepo(MemoryQuestionRepository::new()));
        let state = QuestionState::with_repo(
            repo as Arc<dyn crate::domain::repositories::QuestionRepository>,
        );

        state
            .register(
                "req-live-only".to_string(),
                "session-1".to_string(),
                "Still visible?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let pending = state.get_pending_info().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "req-live-only");
        assert_eq!(pending[0].question, "Still visible?");
    }

    #[tokio::test]
    async fn test_resolve_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        state
            .register(
                "req-1".to_string(),
                "session-1".to_string(),
                "Pick one".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let answer = QuestionAnswer {
            selected_options: vec!["a".to_string()],
            text: None,
            skipped: false,
        };
        let result = state.resolve("req-1", answer).await;
        assert!(result.resolved);
        assert_eq!(result.session_id, Some("session-1".to_string()));
        assert!(result.delivered_to_waiting_agent);

        // After resolve, HashMap should be empty (immediate removal)
        let pending_info = state.get_pending_info().await;
        assert!(pending_info.is_empty());

        // After resolve, repo should have no pending
        let repo_pending = repo.get_pending().await.unwrap();
        assert!(repo_pending.is_empty());

        // But the record still exists in the repo
        let found = repo.get_by_request_id("req-1").await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_remove_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        state
            .register(
                "req-rm".to_string(),
                "session-1".to_string(),
                "Remove me".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let removed = state.remove("req-rm").await;
        assert!(removed);

        // Repo record should be gone
        let found = repo.get_by_request_id("req-rm").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_expire_persists_to_repo() {
        let _db = SqliteTestDb::new("question-state");
        let repo = Arc::new(
            crate::infrastructure::sqlite::SqliteQuestionRepository::new(_db.new_connection()),
        );
        let state = QuestionState::with_repo(
            repo.clone() as Arc<dyn crate::domain::repositories::QuestionRepository>
        );

        state
            .register(
                "req-exp".to_string(),
                "session-1".to_string(),
                "Expire me".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let expired = state.expire("req-exp").await;
        assert!(expired.is_some());

        let pending = repo.get_pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "req-exp");

        let found = repo.get_by_request_id("req-exp").await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_get_pending_info_rehydrates_wait_expired_question_from_repo() {
        let _db = SqliteTestDb::new("question-state-durable-pending");
        let repo = Arc::new(
            crate::infrastructure::sqlite::SqliteQuestionRepository::new(_db.new_connection()),
        );
        let state = QuestionState::with_repo(
            repo.clone() as Arc<dyn crate::domain::repositories::QuestionRepository>
        );

        state
            .register(
                "req-durable".to_string(),
                "session-1".to_string(),
                "Need clarification?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        state.expire("req-durable").await;

        let pending = state.get_pending_info().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "req-durable");
        assert_eq!(pending[0].question, "Need clarification?");
    }

    #[tokio::test]
    async fn test_resolve_wait_expired_question_without_live_waiter() {
        let _db = SqliteTestDb::new("question-state-durable-resolve");
        let repo = Arc::new(
            crate::infrastructure::sqlite::SqliteQuestionRepository::new(_db.new_connection()),
        );
        let state = QuestionState::with_repo(
            repo.clone() as Arc<dyn crate::domain::repositories::QuestionRepository>
        );

        state
            .register(
                "req-late".to_string(),
                "session-1".to_string(),
                "Need clarification?".to_string(),
                None,
                vec![],
                false,
            )
            .await;
        state.expire("req-late").await;

        let result = state
            .resolve(
                "req-late",
                QuestionAnswer {
                    selected_options: vec![],
                    text: Some("Late answer".to_string()),
                    skipped: false,
                },
            )
            .await;

        assert!(result.resolved);
        assert_eq!(result.session_id, Some("session-1".to_string()));
        assert!(!result.delivered_to_waiting_agent);
        assert!(repo.get_pending().await.unwrap().is_empty());

        let answer = state.get_resolved_answer("req-late").await.unwrap();
        assert_eq!(answer.unwrap().text, Some("Late answer".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_unknown_question_with_repo_returns_unresolved() {
        let (state, _repo) = make_state_with_repo();

        let result = state
            .resolve(
                "req-missing",
                QuestionAnswer {
                    selected_options: vec![],
                    text: Some("late answer".to_string()),
                    skipped: false,
                },
            )
            .await;

        assert!(!result.resolved);
        assert!(result.session_id.is_none());
        assert!(!result.delivered_to_waiting_agent);
    }

    #[tokio::test]
    async fn test_expire_stale_on_startup() {
        let repo = Arc::new(MemoryQuestionRepository::new());

        // Seed repo with pending questions (simulating leftover from previous run)
        for i in 0..3 {
            let info = PendingQuestionInfo {
                request_id: format!("stale-{}", i),
                session_id: "old-session".to_string(),
                question: format!("Stale question {}", i),
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

        assert_eq!(repo.get_pending().await.unwrap().len(), 3);

        let state = QuestionState::with_repo(repo.clone());
        state.expire_stale_on_startup().await;

        // Startup only expires the live wait; the durable question remains user-visible.
        assert_eq!(repo.get_pending().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_expire_stale_noop_without_repo() {
        let state = QuestionState::new();
        // Should not panic when no repo
        state.expire_stale_on_startup().await;
    }

    // --- New tests for immediate removal on resolve ---

    #[tokio::test]
    async fn test_resolve_removes_from_hashmap_immediately() {
        let (state, _repo) = make_state_with_repo();

        state
            .register(
                "req-imm".to_string(),
                "session-1".to_string(),
                "Immediate?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let pending_before = state.get_pending_info().await;
        assert_eq!(pending_before.len(), 1);

        let result = state
            .resolve(
                "req-imm",
                QuestionAnswer {
                    selected_options: vec!["yes".to_string()],
                    text: None,
                    skipped: false,
                },
            )
            .await;
        assert!(result.resolved);
        assert!(result.delivered_to_waiting_agent);

        // HashMap must be empty immediately after resolve
        let pending_after = state.get_pending_info().await;
        assert!(pending_after.is_empty());
    }

    #[tokio::test]
    async fn test_receiver_gets_answer_after_hashmap_removal() {
        let (state, _repo) = make_state_with_repo();

        let rx = state
            .register(
                "req-rx".to_string(),
                "session-1".to_string(),
                "Receive?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let result = state
            .resolve(
                "req-rx",
                QuestionAnswer {
                    selected_options: vec!["opt-a".to_string()],
                    text: Some("hello".to_string()),
                    skipped: false,
                },
            )
            .await;
        assert!(result.resolved);
        assert!(result.delivered_to_waiting_agent);

        // HashMap is now empty, but the Receiver must still have the answer
        let pending_after = state.get_pending_info().await;
        assert!(pending_after.is_empty());

        let answer = rx.borrow().clone();
        assert!(answer.is_some());
        let answer = answer.unwrap();
        assert_eq!(answer.selected_options, vec!["opt-a"]);
        assert_eq!(answer.text, Some("hello".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_keeps_live_waiter_pending_when_durable_commit_fails() {
        use crate::error::AppError;

        // Use a failing repo that always errors on resolve
        struct FailingRepo(MemoryQuestionRepository);

        #[async_trait::async_trait]
        impl QuestionRepository for FailingRepo {
            async fn create_pending(
                &self,
                info: &PendingQuestionInfo,
            ) -> crate::error::AppResult<()> {
                self.0.create_pending(info).await
            }
            async fn resolve(
                &self,
                _request_id: &str,
                _answer: &QuestionAnswer,
            ) -> crate::error::AppResult<bool> {
                Err(AppError::Database("simulated DB failure".to_string()))
            }
            async fn get_pending(&self) -> crate::error::AppResult<Vec<PendingQuestionInfo>> {
                self.0.get_pending().await
            }
            async fn get_by_request_id(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<PendingQuestionInfo>> {
                self.0.get_by_request_id(request_id).await
            }
            async fn expire_all_pending(&self) -> crate::error::AppResult<u64> {
                self.0.expire_all_pending().await
            }
            async fn expire_by_request_id(&self, request_id: &str) -> crate::error::AppResult<()> {
                self.0.expire_by_request_id(request_id).await
            }
            async fn remove(&self, request_id: &str) -> crate::error::AppResult<bool> {
                self.0.remove(request_id).await
            }
            async fn get_resolved_answer(
                &self,
                _request_id: &str,
            ) -> crate::error::AppResult<Option<QuestionAnswer>> {
                Ok(None)
            }
        }

        let repo = Arc::new(FailingRepo(MemoryQuestionRepository::new()));
        let state = QuestionState::with_repo(
            repo as Arc<dyn crate::domain::repositories::QuestionRepository>,
        );

        state
            .register(
                "req-fail".to_string(),
                "session-1".to_string(),
                "Fail repo?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let result = state
            .resolve(
                "req-fail",
                QuestionAnswer {
                    selected_options: vec![],
                    text: None,
                    skipped: false,
                },
            )
            .await;
        assert!(!result.resolved);
        assert!(!result.delivered_to_waiting_agent);

        // The live waiter must remain available for a later durable retry.
        let pending_after = state.pending.lock().await;
        assert!(pending_after.contains_key("req-fail"));
    }

    #[tokio::test]
    async fn test_get_resolved_answer_returns_some_for_resolved() {
        let (state, _repo) = make_state_with_repo();

        state
            .register(
                "req-ans".to_string(),
                "session-1".to_string(),
                "Answer me?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let result = state
            .resolve(
                "req-ans",
                QuestionAnswer {
                    selected_options: vec!["choice-a".to_string()],
                    text: None,
                    skipped: false,
                },
            )
            .await;
        assert!(result.resolved);
        assert!(result.delivered_to_waiting_agent);

        // get_resolved_answer should return Some after resolve persisted to memory repo
        let answer = state.get_resolved_answer("req-ans").await.unwrap();
        assert!(answer.is_some());
        let answer = answer.unwrap();
        assert_eq!(answer.selected_options, vec!["choice-a"]);
    }

    #[tokio::test]
    async fn test_get_resolved_answer_returns_none_for_pending_and_unknown() {
        let (state, _repo) = make_state_with_repo();

        state
            .register(
                "req-pend".to_string(),
                "session-1".to_string(),
                "Still pending?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        // Pending question — not yet resolved → None
        let answer = state.get_resolved_answer("req-pend").await.unwrap();
        assert!(answer.is_none());

        // Unknown request_id → None
        let answer = state.get_resolved_answer("unknown-req").await.unwrap();
        assert!(answer.is_none());
    }

    #[tokio::test]
    async fn test_get_resolved_answer_returns_none_without_repo() {
        let state = QuestionState::new(); // no repo
        let answer = state.get_resolved_answer("any-req").await.unwrap();
        assert!(answer.is_none());
    }

    // --- Sweep stale tests (from plan branch) ---

    #[tokio::test]
    async fn test_sweep_stale_removes_old_questions() {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let state = QuestionState::with_repo(repo.clone());

        // Register a question
        state
            .register(
                "old-req-1".to_string(),
                "session-1".to_string(),
                "Old question?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        // Verify it's in pending
        {
            let pending = state.pending.lock().await;
            assert!(pending.contains_key("old-req-1"));
        }

        // Sweep with zero max_age — everything is older than 0 duration
        state.sweep_stale(std::time::Duration::from_secs(0)).await;

        // Should be removed from in-memory HashMap
        {
            let pending = state.pending.lock().await;
            assert!(!pending.contains_key("old-req-1"));
        }

        // Should remain durable in repo so the UI can still collect a late answer.
        let repo_pending = repo.get_pending().await.unwrap();
        assert_eq!(repo_pending.len(), 1);
        assert_eq!(repo_pending[0].request_id, "old-req-1");
    }

    #[tokio::test]
    async fn test_sweep_stale_keeps_fresh_questions() {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let state = QuestionState::with_repo(repo.clone());

        // Register a question
        state
            .register(
                "fresh-req-1".to_string(),
                "session-1".to_string(),
                "Fresh question?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        // Sweep with a very large max_age — nothing should be swept
        state
            .sweep_stale(std::time::Duration::from_secs(3600))
            .await;

        // Should still be in pending
        {
            let pending = state.pending.lock().await;
            assert!(pending.contains_key("fresh-req-1"));
        }

        // Should still be pending in repo
        let repo_pending = repo.get_pending().await.unwrap();
        assert_eq!(repo_pending.len(), 1);
        assert_eq!(repo_pending[0].request_id, "fresh-req-1");
    }

    #[tokio::test]
    async fn strict_pending_read_merges_durable_and_live_questions_without_duplicates() {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let durable = PendingQuestionInfo {
            request_id: "durable-question".to_string(),
            session_id: "session-1".to_string(),
            question: "Persisted question?".to_string(),
            header: None,
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: None,
            created_at: Utc::now().to_rfc3339(),
        };
        repo.create_pending(&durable).await.unwrap();
        let state = QuestionState::with_repo(repo);
        state
            .register(
                "live-question".to_string(),
                "session-2".to_string(),
                "Live question?".to_string(),
                None,
                vec![],
                false,
            )
            .await;
        state
            .register(
                "durable-question".to_string(),
                "session-1".to_string(),
                "Duplicate live question?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let pending = state.get_pending_info_strict().await.unwrap();
        let request_ids: std::collections::HashSet<_> = pending
            .iter()
            .map(|question| question.request_id.as_str())
            .collect();

        assert_eq!(
            request_ids,
            std::collections::HashSet::from(["durable-question", "live-question"])
        );
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn strict_pending_read_excludes_expired_questions_and_keeps_fresh_entries() {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let expired = PendingQuestionInfo {
            request_id: "expired-question".to_string(),
            session_id: "session-expired".to_string(),
            question: "Expired question?".to_string(),
            header: None,
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: None,
            created_at: (Utc::now() - Duration::minutes(6)).to_rfc3339(),
        };
        let fresh = PendingQuestionInfo {
            request_id: "fresh-question".to_string(),
            session_id: "session-fresh".to_string(),
            question: "Fresh question?".to_string(),
            header: None,
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: None,
            created_at: Utc::now().to_rfc3339(),
        };
        repo.create_pending(&expired).await.unwrap();
        repo.create_pending(&fresh).await.unwrap();

        let request_ids: std::collections::HashSet<_> = QuestionState::with_repo(repo)
            .get_pending_info_strict()
            .await
            .unwrap()
            .into_iter()
            .map(|question| question.request_id)
            .collect();

        assert_eq!(
            request_ids,
            std::collections::HashSet::from([fresh.request_id])
        );
    }

    #[tokio::test]
    async fn strict_pending_read_returns_durable_repository_failure() {
        use crate::error::AppError;

        struct FailingReadRepo(MemoryQuestionRepository);

        #[async_trait::async_trait]
        impl QuestionRepository for FailingReadRepo {
            async fn create_pending(
                &self,
                info: &PendingQuestionInfo,
            ) -> crate::error::AppResult<()> {
                self.0.create_pending(info).await
            }
            async fn resolve(
                &self,
                request_id: &str,
                answer: &QuestionAnswer,
            ) -> crate::error::AppResult<bool> {
                self.0.resolve(request_id, answer).await
            }
            async fn get_pending(&self) -> crate::error::AppResult<Vec<PendingQuestionInfo>> {
                Err(AppError::Database("durable read failed".to_string()))
            }
            async fn get_by_request_id(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<PendingQuestionInfo>> {
                self.0.get_by_request_id(request_id).await
            }
            async fn expire_all_pending(&self) -> crate::error::AppResult<u64> {
                self.0.expire_all_pending().await
            }
            async fn expire_by_request_id(&self, request_id: &str) -> crate::error::AppResult<()> {
                self.0.expire_by_request_id(request_id).await
            }
            async fn remove(&self, request_id: &str) -> crate::error::AppResult<bool> {
                self.0.remove(request_id).await
            }
            async fn get_resolved_answer(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<QuestionAnswer>> {
                self.0.get_resolved_answer(request_id).await
            }
        }

        let repo = Arc::new(FailingReadRepo(MemoryQuestionRepository::new()));
        let state = QuestionState::with_repo(repo);
        state
            .register(
                "live-question".to_string(),
                "session-1".to_string(),
                "Still live?".to_string(),
                None,
                vec![],
                false,
            )
            .await;

        let error = state.get_pending_info_strict().await.unwrap_err();
        assert!(matches!(error, AppError::Database(message) if message == "durable read failed"));
    }

    #[tokio::test]
    async fn claim_pending_reserves_without_waking_the_live_waiter() {
        let (state, _repo) = make_state_with_repo();
        let receiver = state
            .register_with_metadata(
                "claim-without-wake".to_string(),
                "session-1".to_string(),
                "Wait for a durable commit?".to_string(),
                None,
                vec![],
                false,
                true,
                None,
                None,
                Some(serde_json::json!({
                    "kind": "plan_mode_proposal",
                    "conversation_id": "conversation-live",
                    "reason": "live claim metadata",
                })),
            )
            .await;

        let claim = state
            .claim_pending("claim-without-wake")
            .await
            .unwrap()
            .expect("claim the live question");

        let claimed_question = claim.pending_question();
        assert_eq!(claimed_question.request_id, "claim-without-wake");
        assert_eq!(claimed_question.session_id, "session-1");
        assert_eq!(
            claimed_question.metadata,
            Some(serde_json::json!({
                "kind": "plan_mode_proposal",
                "conversation_id": "conversation-live",
                "reason": "live claim metadata",
            }))
        );
        assert!(receiver.borrow().is_none());
        assert!(state.release_claim(claim).await);
        assert!(receiver.borrow().is_none());
    }

    #[tokio::test]
    async fn claim_pending_exposes_metadata_from_the_durable_record() {
        let repo = Arc::new(MemoryQuestionRepository::new());
        let durable_question = PendingQuestionInfo {
            request_id: "claim-durable-metadata".to_string(),
            session_id: "session-durable".to_string(),
            question: "Use the persisted proposal?".to_string(),
            header: None,
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: Some(serde_json::json!({
                "kind": "plan_mode_proposal",
                "conversation_id": "conversation-durable",
                "reason": "durable claim metadata",
            })),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        repo.create_pending(&durable_question).await.unwrap();
        let state = QuestionState::with_repo(repo);

        let claim = state
            .claim_pending("claim-durable-metadata")
            .await
            .unwrap()
            .expect("claim the durable question");

        assert_eq!(
            claim.pending_question().request_id,
            durable_question.request_id
        );
        assert_eq!(
            claim.pending_question().session_id,
            durable_question.session_id
        );
        assert_eq!(claim.pending_question().metadata, durable_question.metadata);
        assert!(state.release_claim(claim).await);
    }

    #[tokio::test]
    async fn commit_claim_persists_before_waking_the_live_waiter() {
        let (state, repo) = make_state_with_repo();
        let receiver = state
            .register(
                "commit-live-durable".to_string(),
                "session-1".to_string(),
                "Commit both states?".to_string(),
                None,
                vec![],
                false,
            )
            .await;
        let claim = state
            .claim_pending("commit-live-durable")
            .await
            .unwrap()
            .expect("claim the live question");
        let answer = QuestionAnswer {
            selected_options: vec!["yes".to_string()],
            text: None,
            skipped: false,
        };

        let result = state.commit_claim(claim, answer).await;

        assert!(result.resolved);
        assert!(result.delivered_to_waiting_agent);
        assert_eq!(
            receiver.borrow().as_ref().unwrap().selected_options,
            vec!["yes"]
        );
        let persisted_answer = repo
            .get_resolved_answer("commit-live-durable")
            .await
            .unwrap()
            .expect("answer is durably persisted before the live waiter wakes");
        assert_eq!(persisted_answer.selected_options, vec!["yes"]);
        assert_eq!(persisted_answer.text, None);
        assert!(!persisted_answer.skipped);
    }

    #[tokio::test]
    async fn only_one_concurrent_claim_can_reserve_a_question() {
        let (state, _repo) = make_state_with_repo();
        state
            .register(
                "concurrent-claim".to_string(),
                "session-1".to_string(),
                "Only one claim?".to_string(),
                None,
                vec![],
                false,
            )
            .await;
        let state = Arc::new(state);

        let (first, second) = tokio::join!(
            state.claim_pending("concurrent-claim"),
            state.claim_pending("concurrent-claim")
        );

        let claims = [first.unwrap(), second.unwrap()];
        assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
        let claim = claims.into_iter().flatten().next().unwrap();
        assert!(state.release_claim(claim).await);
    }

    #[tokio::test]
    async fn failed_durable_commit_keeps_the_live_waiter_unwoken_and_releasable() {
        use crate::error::AppError;

        struct FailingResolveRepo(MemoryQuestionRepository);

        #[async_trait::async_trait]
        impl QuestionRepository for FailingResolveRepo {
            async fn create_pending(
                &self,
                info: &PendingQuestionInfo,
            ) -> crate::error::AppResult<()> {
                self.0.create_pending(info).await
            }

            async fn resolve(
                &self,
                _request_id: &str,
                _answer: &QuestionAnswer,
            ) -> crate::error::AppResult<bool> {
                Err(AppError::Database("durable write failed".to_string()))
            }

            async fn get_pending(&self) -> crate::error::AppResult<Vec<PendingQuestionInfo>> {
                self.0.get_pending().await
            }

            async fn get_by_request_id(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<PendingQuestionInfo>> {
                self.0.get_by_request_id(request_id).await
            }

            async fn expire_all_pending(&self) -> crate::error::AppResult<u64> {
                self.0.expire_all_pending().await
            }

            async fn expire_by_request_id(&self, request_id: &str) -> crate::error::AppResult<()> {
                self.0.expire_by_request_id(request_id).await
            }

            async fn remove(&self, request_id: &str) -> crate::error::AppResult<bool> {
                self.0.remove(request_id).await
            }

            async fn get_resolved_answer(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<QuestionAnswer>> {
                self.0.get_resolved_answer(request_id).await
            }
        }

        let state = QuestionState::with_repo(Arc::new(FailingResolveRepo(
            MemoryQuestionRepository::new(),
        )));
        let receiver = state
            .register(
                "failed-durable-commit".to_string(),
                "session-1".to_string(),
                "Stay pending?".to_string(),
                None,
                vec![],
                false,
            )
            .await;
        let claim = state
            .claim_pending("failed-durable-commit")
            .await
            .unwrap()
            .expect("claim the live question");

        let result = state
            .commit_claim(
                claim,
                QuestionAnswer {
                    selected_options: vec![],
                    text: Some("not yet".to_string()),
                    skipped: false,
                },
            )
            .await;

        assert!(!result.resolved);
        assert!(!result.delivered_to_waiting_agent);
        assert!(receiver.borrow().is_none());
        assert!(state
            .claim_pending("failed-durable-commit")
            .await
            .unwrap()
            .is_some());
    }
}
