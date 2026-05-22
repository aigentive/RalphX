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
    };
    let cloned = answer.clone();
    assert_eq!(cloned.selected_options, vec!["opt1"]);
    assert_eq!(cloned.text, Some("Custom text".to_string()));
}

#[tokio::test]
async fn test_question_answer_serialization() {
    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string(), "b".to_string()],
        text: None,
    };
    let json = serde_json::to_string(&answer).unwrap();
    assert!(json.contains("\"selected_options\""));

    let deserialized: QuestionAnswer = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.selected_options.len(), 2);
    assert!(deserialized.text.is_none());
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
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"request_id\":\"req-123\""));
    assert!(json.contains("\"session_id\":\"session-456\""));
    assert!(json.contains("\"question\":\"Which approach?\""));
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
    let (resolved, session_id) = state
        .resolve(
            &request_id,
            QuestionAnswer {
                selected_options: vec!["react".to_string()],
                text: None,
            },
        )
        .await;
    assert!(resolved);
    assert_eq!(session_id, Some("session-1".to_string()));

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

    let (resolved, session_id) = state
        .resolve(
            "nonexistent",
            QuestionAnswer {
                selected_options: vec![],
                text: None,
            },
        )
        .await;
    assert!(!resolved);
    assert!(session_id.is_none());
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
        };
        let (resolved, session_id) = state.resolve("req-1", answer).await;
        assert!(resolved);
        assert_eq!(session_id, Some("session-1".to_string()));

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
        let repo = Arc::new(crate::infrastructure::sqlite::SqliteQuestionRepository::new(
            _db.new_connection(),
        ));
        let state = QuestionState::with_repo(
            repo.clone() as Arc<dyn crate::domain::repositories::QuestionRepository>,
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
        assert!(pending.is_empty());

        let found = repo.get_by_request_id("req-exp").await.unwrap();
        assert!(found.is_some());
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
            };
            repo.create_pending(&info).await.unwrap();
        }

        assert_eq!(repo.get_pending().await.unwrap().len(), 3);

        let state = QuestionState::with_repo(repo.clone());
        state.expire_stale_on_startup().await;

        // All stale questions should be expired
        assert!(repo.get_pending().await.unwrap().is_empty());
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

        let (resolved, _) = state
            .resolve(
                "req-imm",
                QuestionAnswer {
                    selected_options: vec!["yes".to_string()],
                    text: None,
                },
            )
            .await;
        assert!(resolved);

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

        let (resolved, _) = state
            .resolve(
                "req-rx",
                QuestionAnswer {
                    selected_options: vec!["opt-a".to_string()],
                    text: Some("hello".to_string()),
                },
            )
            .await;
        assert!(resolved);

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
    async fn test_resolve_removes_from_hashmap_even_when_repo_errors() {
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
            async fn remove(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<bool> {
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

        let (resolved, _) = state
            .resolve(
                "req-fail",
                QuestionAnswer {
                    selected_options: vec![],
                    text: None,
                },
            )
            .await;
        assert!(resolved);

        // HashMap must be empty even though repo.resolve() errored
        let pending_after = state.get_pending_info().await;
        assert!(pending_after.is_empty());
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

        let (resolved, _) = state
            .resolve(
                "req-ans",
                QuestionAnswer {
                    selected_options: vec!["choice-a".to_string()],
                    text: None,
                },
            )
            .await;
        assert!(resolved);

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

        // Should also be expired in repo (removed from pending in MemoryQuestionRepository)
        let repo_pending = repo.get_pending().await.unwrap();
        assert!(repo_pending.is_empty());
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
}
