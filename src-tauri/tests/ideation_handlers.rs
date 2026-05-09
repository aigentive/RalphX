use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::ideation::{
    IdeationSessionStatus, SessionOrigin, SessionPurpose, VerificationStatus,
};
use ralphx_lib::domain::entities::{
    ChatContextType, ChatMessage, IdeationSession, IdeationSessionBuilder, IdeationSessionId,
    ProjectId, VerificationGap, VerificationRoundSnapshot, VerificationRunSnapshot,
};
use ralphx_lib::domain::services::RunningAgentKey;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::http_server::types::{
    UpdateVerificationRequest, VerificationGapRequest, VerificationInfraFailureRequest,
    VerificationQueryParams,
};
use std::sync::Arc;

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

fn default_verification_query() -> Query<VerificationQueryParams> {
    Query(VerificationQueryParams::default())
}

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));

    HttpServerState {
        app_state,
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

async fn get_external_event_types(state: &HttpServerState, project_id: &ProjectId) -> Vec<String> {
    state
        .app_state
        .external_events_repo
        .get_events_after_cursor(&[project_id.as_str().to_string()], 0, 100)
        .await
        .expect("external events query should succeed")
        .into_iter()
        .map(|event| event.event_type)
        .collect()
}

#[tokio::test]
async fn test_get_session_messages_empty_session() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 50,
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.messages.is_empty());
    assert_eq!(response.count, 0);
    assert!(!response.truncated);
    assert_eq!(response.total_available, 0);
}

#[tokio::test]
async fn test_get_session_messages_returns_messages() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    // Create messages
    let msg1 = ChatMessage::user_in_session(session_id.clone(), "Hello");
    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Hi there!");

    state
        .app_state
        .chat_message_repo
        .create(msg1.clone())
        .await
        .unwrap();
    state
        .app_state
        .chat_message_repo
        .create(msg2.clone())
        .await
        .unwrap();

    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 50,
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.messages.len(), 2);
    assert_eq!(response.count, 2);
    assert!(!response.truncated);
    assert_eq!(response.total_available, 2);
}

#[tokio::test]
async fn test_get_session_messages_respects_limit() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    // Create 10 messages
    for i in 0..10 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("Message {}", i));
        state.app_state.chat_message_repo.create(msg).await.unwrap();
    }

    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 5,
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.messages.len(), 5);
    assert_eq!(response.count, 5);
    assert!(response.truncated);
    assert_eq!(response.total_available, 10);
}

#[tokio::test]
async fn test_get_session_messages_caps_at_200() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    // Request limit over 200
    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 500, // Should be capped to 200
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    // Should still succeed (empty in this case)
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_session_messages_default_limit() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    // Create 60 messages
    for i in 0..60 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("Message {}", i));
        state.app_state.chat_message_repo.create(msg).await.unwrap();
    }

    // Use default limit (should be 50)
    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 50, // explicit default
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.messages.len(), 50);
    assert!(response.truncated);
    assert_eq!(response.total_available, 60);
}

#[tokio::test]
async fn test_get_session_messages_returns_chronological_order() {
    let state = setup_test_state().await;
    let session_id = IdeationSessionId::new();

    // Create messages in order
    let msg1 = ChatMessage::user_in_session(session_id.clone(), "First");
    let msg2 = ChatMessage::user_in_session(session_id.clone(), "Second");
    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Third");

    // Small delays to ensure different timestamps
    state
        .app_state
        .chat_message_repo
        .create(msg1)
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    state
        .app_state
        .chat_message_repo
        .create(msg2)
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    state
        .app_state
        .chat_message_repo
        .create(msg3)
        .await
        .unwrap();

    let result = get_session_messages(
        State(state),
        Json(GetSessionMessagesRequest {
            session_id: session_id.as_str().to_string(),
            limit: 50,
            offset: 0,
            include_tool_calls: false,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    // get_recent_by_session returns messages in chronological order (oldest to newest)
    // after selecting the most recent N messages
    assert_eq!(response.messages[0].content, "First");
    assert_eq!(response.messages[1].content, "Second");
    assert_eq!(response.messages[2].content, "Third");
}

// ─────────────────────────────────────────────────────────────────────────────
// get_plan_verification handler tests
// ─────────────────────────────────────────────────────────────────────────────

fn make_gap(severity: &str, category: &str, description: &str) -> VerificationGap {
    VerificationGap {
        severity: severity.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        why_it_matters: None,
        source: None,
    }
}

fn make_round(fingerprints: Vec<&str>, gap_score: u32) -> VerificationRoundSnapshot {
    VerificationRoundSnapshot {
        round: 0,
        gap_score,
        fingerprints: fingerprints
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
        gaps: Vec::new(),
        parse_failed: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn make_snapshot(
    generation: i32,
    status: VerificationStatus,
    in_progress: bool,
    current_round: u32,
    max_rounds: u32,
    current_gaps: Vec<VerificationGap>,
    rounds: Vec<VerificationRoundSnapshot>,
    convergence_reason: Option<&str>,
    best_round_index: Option<u32>,
) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status,
        in_progress,
        current_round,
        max_rounds,
        best_round_index,
        convergence_reason: convergence_reason.map(|value| value.to_string()),
        current_gaps,
        rounds: rounds
            .into_iter()
            .enumerate()
            .map(|(index, round)| VerificationRoundSnapshot {
                round: if round.round == 0 {
                    (index + 1) as u32
                } else {
                    round.round
                },
                ..round
            })
            .collect(),
    }
}

/// Happy path: session with 3 gaps and 2 rounds → response includes
/// current_gaps (3 items) and rounds (2 items with correct scores/counts).
#[tokio::test]
async fn test_get_plan_verification_happy_path_gaps_and_rounds() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    let session_id = session.id.clone();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &VerificationRunSnapshot {
                generation: 0,
                status: VerificationStatus::NeedsRevision,
                in_progress: false,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![
                    VerificationGap {
                        severity: "critical".to_string(),
                        category: "architecture".to_string(),
                        description: "Missing auth layer".to_string(),
                        why_it_matters: Some("Security risk".to_string()),
                        source: None,
                    },
                    VerificationGap {
                        severity: "high".to_string(),
                        category: "performance".to_string(),
                        description: "No caching strategy".to_string(),
                        why_it_matters: None,
                        source: None,
                    },
                    VerificationGap {
                        severity: "medium".to_string(),
                        category: "testing".to_string(),
                        description: "No unit tests".to_string(),
                        why_it_matters: None,
                        source: None,
                    },
                ],
                rounds: vec![
                    VerificationRoundSnapshot {
                        round: 1,
                        gap_score: 13,
                        fingerprints: vec!["fp-a".to_string(), "fp-b".to_string()],
                        gaps: vec![
                            VerificationGap {
                                severity: "critical".to_string(),
                                category: "architecture".to_string(),
                                description: "Missing auth layer".to_string(),
                                why_it_matters: None,
                                source: None,
                            },
                            VerificationGap {
                                severity: "high".to_string(),
                                category: "performance".to_string(),
                                description: "No caching strategy".to_string(),
                                why_it_matters: None,
                                source: None,
                            },
                        ],
                        parse_failed: false,
                    },
                    VerificationRoundSnapshot {
                        round: 2,
                        gap_score: 10,
                        fingerprints: vec![
                            "fp-a".to_string(),
                            "fp-b".to_string(),
                            "fp-c".to_string(),
                        ],
                        gaps: vec![
                            VerificationGap {
                                severity: "critical".to_string(),
                                category: "architecture".to_string(),
                                description: "Missing auth layer".to_string(),
                                why_it_matters: None,
                                source: None,
                            },
                            VerificationGap {
                                severity: "high".to_string(),
                                category: "performance".to_string(),
                                description: "No caching strategy".to_string(),
                                why_it_matters: None,
                                source: None,
                            },
                            VerificationGap {
                                severity: "medium".to_string(),
                                category: "testing".to_string(),
                                description: "No unit tests".to_string(),
                                why_it_matters: None,
                                source: None,
                            },
                        ],
                        parse_failed: false,
                    },
                ],
            },
        )
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let response = result.unwrap().0;

    // current_gaps: 3 items with correct fields
    assert_eq!(response.current_gaps.len(), 3, "expected 3 current_gaps");
    let critical = &response.current_gaps[0];
    assert_eq!(critical.severity, "critical");
    assert_eq!(critical.category, "architecture");
    assert_eq!(critical.description, "Missing auth layer");
    assert_eq!(critical.why_it_matters.as_deref(), Some("Security risk"));
    let high = &response.current_gaps[1];
    assert_eq!(high.severity, "high");
    assert!(high.why_it_matters.is_none());

    // rounds: 2 items with 1-based round numbers and correct gap_counts
    assert_eq!(response.rounds.len(), 2, "expected 2 rounds");
    let r1 = &response.rounds[0];
    assert_eq!(r1.round, 1);
    assert_eq!(r1.gap_score, 13);
    assert_eq!(r1.gap_count, 2); // fingerprints.len()
    let r2 = &response.rounds[1];
    assert_eq!(r2.round, 2);
    assert_eq!(r2.gap_score, 10);
    assert_eq!(r2.gap_count, 3);

    assert_eq!(response.round_details.len(), 2, "expected 2 round details");
    let d1 = &response.round_details[0];
    assert_eq!(d1.round, 1);
    assert_eq!(d1.gaps.len(), 2);
    assert_eq!(d1.gaps[0].description, "Missing auth layer");
    let d2 = &response.round_details[1];
    assert_eq!(d2.round, 2);
    assert_eq!(d2.gaps.len(), 3);
    assert_eq!(d2.gaps[2].description, "No unit tests");
}

/// No native snapshot stored yet → current_gaps: [] and round_details: [].
#[tokio::test]
async fn test_get_plan_verification_without_native_snapshot_returns_empty_vecs() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    let session_id = session.id.clone();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Explicitly set NULL metadata
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(
        response.current_gaps.is_empty(),
        "current_gaps must be empty for NULL metadata"
    );
    assert!(
        response.rounds.is_empty(),
        "rounds must be empty for NULL metadata"
    );
    assert!(
        response.round_details.is_empty(),
        "round_details must be empty for NULL metadata"
    );
    assert!(response.gap_score.is_none());
}

/// Reviewing state with no native snapshot must not panic and should return empty arrays.
#[tokio::test]
async fn test_get_plan_verification_reviewing_without_snapshot_no_panic() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    let session_id = session.id.clone();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await;

    assert!(
        result.is_ok(),
        "missing snapshot must not panic the handler"
    );
    let response = result.unwrap().0;
    assert!(
        response.current_gaps.is_empty(),
        "current_gaps should be empty without a native snapshot"
    );
    assert!(
        response.rounds.is_empty(),
        "rounds should be empty without a native snapshot"
    );
}

#[tokio::test]
async fn test_get_plan_verification_hides_blank_orphaned_active_generation() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .verification_generation(20)
        .build();
    let session_id = session.id.clone();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                20,
                VerificationStatus::Reviewing,
                true,
                0,
                5,
                vec![],
                vec![],
                None,
                None,
            ),
        )
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(session_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_status(&child_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed");

    assert_eq!(result.0.status, "unverified");
    assert!(
        !result.0.in_progress,
        "blank orphaned active generation must not block fresh verification"
    );
    assert!(
        result.0.run_history.iter().all(|run| run.generation != 20),
        "blank orphaned active generation should be filtered out of run history"
    );
    let child_info = result
        .0
        .verification_child
        .expect("latest archived child should still be exposed for continuity");
    assert!(child_info.latest_child_archived);
    assert!(child_info.active_child_session_id.is_none());
}

/// Rounds cap test: session with 15 rounds → last 10 returned in chronological order
/// (rounds 6-15, i.e. 1-based indices 6..=15 from the original vec).
#[tokio::test]
async fn test_get_plan_verification_rounds_capped_at_10() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    let session_id = session.id.clone();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Build 15 rounds with distinct gap_scores (1..=15) so we can verify ordering
    let rounds: Vec<VerificationRoundSnapshot> =
        (1u32..=15).map(|i| make_round(vec!["fp-x"], i)).collect();
    let snapshot = make_snapshot(
        0,
        VerificationStatus::NeedsRevision,
        false,
        15,
        15,
        vec![],
        rounds,
        None,
        None,
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id, &snapshot)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;

    // Only 10 rounds returned
    assert_eq!(response.rounds.len(), 10, "cap must limit to 10 rounds");

    // First returned round is round 6 (oldest of the last 10)
    assert_eq!(
        response.rounds[0].round, 6,
        "first returned round should be round 6"
    );
    assert_eq!(
        response.rounds[0].gap_score, 6,
        "gap_score should match round index"
    );

    // Last returned round is round 15
    assert_eq!(
        response.rounds[9].round, 15,
        "last returned round should be round 15"
    );
    assert_eq!(response.rounds[9].gap_score, 15);

    // Verify chronological order throughout
    for (i, r) in response.rounds.iter().enumerate() {
        assert_eq!(
            r.round,
            (i + 6) as u32,
            "round at index {} should be {}",
            i,
            i + 6
        );
    }
}

/// Round-trip integration test: write gaps via POST /verification (post_verification_status),
/// then read via GET /verification (get_plan_verification), and verify current_gaps contains
/// the same data with correct field names.
#[tokio::test]
async fn test_get_plan_verification_round_trip_post_then_get() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    let session_id = session.id.clone();
    let session_id_str = session_id.as_str().to_string();

    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // POST: write gaps via post_verification_status handler
    let post_result = post_verification_status(
        State(state.clone()),
        Path(session_id_str.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: false,
            round: Some(1),
            gaps: Some(vec![
                VerificationGapRequest {
                    severity: "critical".to_string(),
                    category: "security".to_string(),
                    description: "No authentication".to_string(),
                    why_it_matters: Some("Users can access any data".to_string()),
                    source: None,
                },
                VerificationGapRequest {
                    severity: "high".to_string(),
                    category: "scalability".to_string(),
                    description: "No horizontal scaling plan".to_string(),
                    why_it_matters: None,
                    source: None,
                },
            ]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        post_result.is_ok(),
        "POST should succeed: {:?}",
        post_result.err()
    );

    // GET: read back via get_plan_verification handler
    let get_result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        default_verification_query(),
    )
    .await;

    assert!(
        get_result.is_ok(),
        "GET should succeed: {:?}",
        get_result.err()
    );
    let response = get_result.unwrap().0;

    // current_gaps should contain the same 2 gaps written via POST
    assert_eq!(
        response.current_gaps.len(),
        2,
        "round-trip: expected 2 current_gaps"
    );

    let g0 = &response.current_gaps[0];
    assert_eq!(g0.severity, "critical");
    assert_eq!(g0.category, "security");
    assert_eq!(g0.description, "No authentication");
    assert_eq!(
        g0.why_it_matters.as_deref(),
        Some("Users can access any data")
    );

    let g1 = &response.current_gaps[1];
    assert_eq!(g1.severity, "high");
    assert_eq!(g1.category, "scalability");
    assert!(g1.why_it_matters.is_none());

    // POST handler creates a round entry; GET should reflect it
    assert_eq!(
        response.rounds.len(),
        1,
        "round-trip: 1 round should be present"
    );
    assert_eq!(response.rounds[0].round, 1);
    assert_eq!(response.rounds[0].gap_count, 2); // 2 fingerprints (one per gap)
}

#[tokio::test]
async fn test_post_verification_status_uses_snapshot_truth_when_summary_is_stale() {
    let state = setup_test_state().await;
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(4)
        .build();
    let session_id = session.id.clone();
    let session_id_str = session_id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &VerificationRunSnapshot {
                generation: 4,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 1,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![],
            },
        )
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id_str),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "Missing regression".to_string(),
                why_it_matters: Some("runtime behavior is unguarded".to_string()),
                source: Some("completeness".to_string()),
            }]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(4),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "active-generation snapshot must drive transition validation: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "needs_revision");
    assert!(!response.in_progress);
    assert_eq!(response.verification_generation, 4);
}

// ── Condition 6 tests: reviewing with gaps → needs_revision auto-transition ──

/// Condition 6 test 1: reviewing + critical gaps → overridden to needs_revision
#[tokio::test]
async fn test_condition6_reviewing_critical_gaps_overrides_to_needs_revision() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "Missing auth entirely".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "needs_revision",
        "critical gaps → needs_revision"
    );
    // Rule A: in_progress is preserved from the caller (true) — loop is still active (no convergence_reason)
    assert!(
        resp.in_progress,
        "in_progress preserved: non-terminal, caller sent true"
    );
}

/// Condition 6 test 2: reviewing + medium-only gaps → overridden to needs_revision (any severity)
#[tokio::test]
async fn test_condition6_reviewing_medium_gaps_overrides_to_needs_revision() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "medium".to_string(),
                category: "performance".to_string(),
                description: "No caching layer defined".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "needs_revision",
        "medium gaps → needs_revision (any severity)"
    );
    // Rule A: in_progress is preserved from the caller (true) — loop is still active (no convergence_reason)
    assert!(
        resp.in_progress,
        "in_progress preserved: non-terminal, caller sent true"
    );
}

/// Condition 6 test 3: reviewing + gaps + max_rounds convergence → verified (convergence wins)
#[tokio::test]
async fn test_condition6_convergence_takes_priority_over_reviewing_with_gaps() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // max_rounds=1, round=1 → condition 3 fires first (max_rounds) → Verified
    // condition 6 then sees Verified (not Reviewing) and does not fire
    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "scalability".to_string(),
                description: "No horizontal scaling plan".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(1),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "convergence (max_rounds) takes priority over condition 6"
    );
}

/// Condition 6 test 4: reviewing + no gaps → status stays reviewing (condition 6 does not fire)
#[tokio::test]
async fn test_condition6_reviewing_no_gaps_stays_reviewing() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]), // explicitly empty — no gaps
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(resp.status, "reviewing", "no gaps → status stays reviewing");
}

/// Condition 6 test 5: reviewing + in_progress=false already + gaps → still overridden to needs_revision
#[tokio::test]
async fn test_condition6_reviewing_in_progress_false_with_gaps_overrides_to_needs_revision() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: false, // already false — condition 6 still fires on status
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "low".to_string(),
                category: "documentation".to_string(),
                description: "API docs incomplete".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "needs_revision",
        "condition 6 fires regardless of requested in_progress value"
    );
    assert!(!resp.in_progress, "in_progress remains false");
}

/// Rule A: reviewing + gaps + in_progress=true + no convergence_reason → in_progress preserved as true.
///
/// The verifier loop is still active mid-round — verification_in_progress must remain 1 in DB.
/// The split-brain bug was that condition 6 used to force effective_in_progress=false here,
/// making the UI think verification had stopped even while the verifier was still running.
#[tokio::test]
async fn test_rule_a_non_terminal_preserves_in_progress_true() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id_str),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "security".to_string(),
                description: "Auth token not validated on write paths".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "needs_revision",
        "gaps → condition 6 overrides to needs_revision"
    );
    // Rule A: in_progress is preserved (not forced to false) — no convergence_reason means loop is active
    assert!(
        resp.in_progress,
        "Rule A: in_progress must be preserved as true (non-terminal)"
    );

    // Verify DB: verification_in_progress = 1
    let saved = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert!(
        saved.verification_in_progress,
        "DB: verification_in_progress must be 1 — loop is still active"
    );
}

/// Rule A must also clear stale terminal convergence metadata when a live reviewing round
/// continues after an earlier infra/runtime failure marker was left behind.
#[tokio::test]
async fn test_rule_a_clears_stale_convergence_reason_on_live_round_update() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let stale_snapshot = make_snapshot(
        0,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
        vec![],
        vec![],
        Some("agent_error"),
        None,
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &stale_snapshot)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id_str),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "completeness".to_string(),
                description: "Missing migration registration".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(resp.status, "needs_revision");
    assert!(
        resp.in_progress,
        "stale convergence_reason must not force a live round terminal"
    );
    assert!(
        resp.convergence_reason.is_none(),
        "stale convergence_reason must be cleared on live round continuation"
    );

    let saved = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert!(saved.verification_in_progress);
    let snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, saved.verification_generation)
        .await
        .unwrap()
        .unwrap();
    assert!(
        snapshot.convergence_reason.is_none(),
        "stored convergence_reason must be cleared for the continued round"
    );
}

/// Rule B: convergence_reason present → terminal guard forces in_progress=false.
///
/// Covers auto-convergence paths (conditions 1–4) that set convergence_reason without
/// explicitly resetting effective_in_progress. Uses max_rounds server-side trigger.
#[tokio::test]
async fn test_rule_b_terminal_guard_max_rounds_forces_in_progress_false() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // max_rounds=3, round=3 → server-side condition 3 fires: new_status=Verified, convergence_reason="max_rounds"
    // Terminal guard then fires (convergence_reason.is_some()) → effective_in_progress=false
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id_str),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true, // caller sends true — terminal guard must override to false
            round: Some(3),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "scalability".to_string(),
                description: "No horizontal scaling plan".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(3),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(resp.status, "verified", "max_rounds convergence → verified");
    assert_eq!(
        resp.convergence_reason.as_deref(),
        Some("max_rounds"),
        "convergence_reason set"
    );
    // Rule B: terminal guard forces in_progress=false even though caller sent true
    assert!(
        !resp.in_progress,
        "Rule B: terminal guard must force in_progress=false on convergence"
    );

    // Verify DB: verification_in_progress = 0
    let saved = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !saved.verification_in_progress,
        "DB: verification_in_progress must be 0 after max_rounds convergence"
    );
}

/// Rule B: explicit verified + convergence_reason=zero_blocking → in_progress forced to false.
///
/// Covers the orchestrator path where it directly sends status=verified with a convergence_reason.
/// The terminal guard must set verification_in_progress=0 in DB.
#[tokio::test]
async fn test_rule_b_terminal_guard_zero_blocking_verified_forces_in_progress_false() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Put session in NeedsRevision (simulating prior reviewing→needs_revision cycle)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::NeedsRevision, true)
        .await
        .unwrap();

    // Orchestrator sends: status=verified + convergence_reason=zero_blocking + in_progress=true
    // Terminal guard must override in_progress to false (matches!(new_status, Verified))
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id_str),
        Json(UpdateVerificationRequest {
            status: "verified".to_string(),
            in_progress: true, // caller sends true — terminal guard must override to false
            round: None,
            gaps: None,
            convergence_reason: Some("zero_blocking".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(resp.status, "verified");
    assert_eq!(resp.convergence_reason.as_deref(), Some("zero_blocking"));
    // Rule B: terminal guard forces in_progress=false (new_status == Verified)
    assert!(
        !resp.in_progress,
        "Rule B: terminal guard must force in_progress=false for verified status"
    );

    // Verify DB: verification_in_progress = 0
    let saved = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !saved.verification_in_progress,
        "DB: verification_in_progress must be 0 after zero_blocking convergence"
    );
}

// ── needs_revision → verified transition tests ──

/// needs_revision → verified succeeds when convergence_reason is provided.
///
/// The orchestrator calls this path when adversarial convergence is met
/// (e.g., 0 critical gaps after N rounds) and directly requests verified status.
#[tokio::test]
async fn test_needs_revision_to_verified_with_convergence_reason() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Put session in NeedsRevision state (simulating prior reviewing→needs_revision cycle)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "verified".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: Some(
                "No critical gaps after 5 rounds of adversarial review".to_string(),
            ),
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("needs_revision → verified with convergence_reason must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "convergence_reason present → verified"
    );
    assert!(
        !resp.in_progress,
        "in_progress must be false after verification"
    );
}

/// needs_revision → verified is rejected (422) when convergence_reason is absent.
///
/// Without a convergence_reason, the orchestrator cannot skip further review rounds.
#[tokio::test]
async fn test_needs_revision_to_verified_without_convergence_reason() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "verified".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "needs_revision → verified without convergence_reason must fail"
    );
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "must return 422 when convergence_reason is absent"
    );
}

// ── Auto-verifier integration tests ──
// These tests verify the server-side behavior that the auto-verifier agent relies on.

/// Zombie protection: generation mismatch → 409 CONFLICT
///
/// When a stale auto-verifier agent sends `in_progress=true` with an outdated
/// generation counter (e.g., because the verification was reset and a new run started),
/// the server must reject it with 409 CONFLICT to prevent two agents from running
/// simultaneously and corrupting state.
#[tokio::test]
async fn test_zombie_generation_mismatch() {
    let state = setup_test_state().await;

    // Create a session with generation=5 (simulates a reset that incremented the counter)
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(5)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Stale agent sends generation=999 → must be rejected with 409
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: Some(999),
        }),
    )
    .await;

    assert!(result.is_err(), "stale generation must be rejected");
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "must return 409 CONFLICT for generation mismatch"
    );

    // Correct generation (5) → must succeed
    let result_ok = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: Some(5),
        }),
    )
    .await;

    assert!(
        result_ok.is_ok(),
        "correct generation must succeed: {:?}",
        result_ok.err()
    );
}

/// Zombie protection: no generation provided → guard does not fire
///
/// When `generation` is None (not provided by the agent), the guard is skipped
/// and the call proceeds normally regardless of the stored generation value.
#[tokio::test]
async fn test_zombie_guard_skipped_when_no_generation_provided() {
    let state = setup_test_state().await;

    // Session with generation=7
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(7)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // No generation → guard not triggered → must succeed even though stored generation is 7
    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None, // no generation = no guard check
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "missing generation must not trigger the guard: {:?}",
        result.err()
    );
}

/// Zombie protection: terminal call (in_progress=false) with stale generation → 409 CONFLICT
///
/// Terminal calls (verified, needs_revision, skipped) must also be guarded.
/// A zombie agent that finished after a reset must not overwrite the new agent's terminal status.
#[tokio::test]
async fn test_zombie_terminal_call_stale_generation_rejected() {
    let state = setup_test_state().await;

    // Session at Reviewing with generation=3 (simulates a reset mid-verification)
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(3)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Zombie agent sends terminal status (in_progress=false) with stale generation=1 → must be rejected
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(2),
            gaps: None,
            convergence_reason: Some("max_rounds".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: Some(1), // Stale — reset incremented to 3
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "terminal call with stale generation must be rejected"
    );
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "must return 409 CONFLICT for stale generation on terminal call"
    );
}

/// Zombie protection: terminal call (in_progress=false) with correct generation → success
///
/// A legitimate agent finishing its round loop must be able to write terminal status
/// when it provides the correct current generation.
#[tokio::test]
async fn test_zombie_terminal_call_correct_generation_succeeds() {
    let state = setup_test_state().await;

    // Session at Reviewing with generation=3
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(3)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Correct generation → terminal call must succeed
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(2),
            gaps: None,
            convergence_reason: Some("max_rounds".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: Some(3), // Correct generation
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "terminal call with correct generation must succeed: {:?}",
        result.err()
    );
}

/// Zombie protection: terminal call without generation parameter → guard does not fire
///
/// Backward compatibility: callers that omit generation entirely are not affected
/// by the guard, regardless of whether the call is in_progress=true or false.
#[tokio::test]
async fn test_zombie_terminal_call_no_generation_no_guard() {
    let state = setup_test_state().await;

    // Session at Reviewing with generation=5
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(5)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // No generation parameter → guard skipped, terminal call proceeds normally
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(1),
            gaps: None,
            convergence_reason: Some("max_rounds".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: None, // No generation = no guard check
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "terminal call without generation must not trigger the guard: {:?}",
        result.err()
    );
}

/// Empty round guard: round 1 with 0 gaps does NOT trigger convergence.
///
/// A critic that finds 0 gaps in round 1 may simply be broken or confused.
/// The server requires at least round 2 before accepting zero_blocking convergence.
/// After round 1 with 0 gaps, the status should remain reviewing (condition 6 doesn't
/// fire because there are no gaps), not verified.
#[tokio::test]
async fn test_single_round_zero_gaps_does_not_converge() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Round 1 with 0 gaps — zero_blocking_converged would be true,
    // but the round guard (round >= 2) prevents convergence.
    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]), // explicitly empty — no gaps found
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    // Round 1 + 0 gaps: condition 6 doesn't fire (no gaps), auto-converge blocked (round < 2)
    // So status stays "reviewing" with in_progress=true
    assert_eq!(
        resp.status, "reviewing",
        "round 1 + 0 gaps must NOT trigger convergence — round guard requires round >= 2"
    );
    assert!(
        resp.convergence_reason.is_none(),
        "no convergence_reason expected for round 1"
    );
}

/// Iterative convergence: gaps clear across rounds → server auto-detects zero_blocking.
///
/// Simulates the real verification loop where the critic finds no blocking gaps
/// after the plan is revised. The server auto-detects convergence when 0 critical AND
/// 0 high AND 0 medium (zero_blocking, AD3) AND round >= 2.
///
/// Flow:
/// - Pre-state: session in Reviewing with metadata showing 1 critical + 2 high from round 1
/// - Round 2: agent sends needs_revision (Reviewing → NeedsRevision), gaps=[] (all cleared)
///   → zero_blocking_converged = (0==0 && 0==0 && 0==0) = true, round=2 >= 2 → Verified
///
/// The server detects convergence automatically without the agent providing convergence_reason.
#[tokio::test]
async fn test_iterative_convergence_decreasing_gaps() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Set up round 1 state: session is Reviewing, metadata.current_gaps has 1 critical + 2 high
    // (simulates what the agent stored after the first critic pass)
    let prior_gaps = vec![
        make_gap("critical", "security", "No authentication layer"),
        make_gap("high", "scalability", "No caching strategy"),
        make_gap("high", "reliability", "No retry mechanism"),
    ];
    let prior_rounds = vec![make_round(
        vec![
            "no-authentication-layer",
            "no-caching-strategy",
            "no-retry-mechanism",
        ],
        50,
    )];
    let round1_snapshot = make_snapshot(
        0,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
        prior_gaps,
        prior_rounds,
        None,
        None,
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &round1_snapshot)
        .await
        .unwrap();

    // Round 2: agent sends needs_revision with 0 critical + 0 high + 0 medium (all cleared)
    // Server computes: zero_blocking_converged = true, round=2 >= 2 → Verified
    let round2 = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]), // all blocking gaps resolved
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 2 must succeed");

    let resp2 = round2.0;
    assert_eq!(
        resp2.status, "verified",
        "0 critical + 0 high + 0 medium at round 2 → server auto-converges to verified"
    );
    assert_eq!(
        resp2.convergence_reason.as_deref(),
        Some("zero_blocking"),
        "convergence_reason must be 'zero_blocking'"
    );
}

/// Jaccard convergence: 2-pair requirement — only 1 matching pair does not converge.
///
/// The server requires jaccard >= 0.8 for BOTH of:
///   - (new_round, prev_round) pair
///   - (prev_round, prev_prev_round) pair
///
/// This test verifies that the 2-pair requirement is enforced: if only the most recent
/// consecutive pair matches, convergence is not triggered.
///
/// Flow:
/// - Round 1: [gap_a, gap_b] → needs_revision, rounds=[fp1]
/// - Round 2: [gap_c, gap_d] (different) → needs_revision, rounds=[fp1, fp2]
/// - Round 3: [gap_c, gap_d] (same as round 2) → jaccard(fp3,fp2)=1.0 BUT jaccard(fp2,fp1)<1.0
///   → 2-pair requirement not met → needs_revision (no convergence)
#[tokio::test]
async fn test_jaccard_convergence_same_fingerprints() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Shared gap set — same gaps submitted in each round to produce identical fingerprints
    let make_gaps = || {
        vec![
            VerificationGapRequest {
                severity: "high".to_string(),
                category: "scalability".to_string(),
                description: "No horizontal scaling plan".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGapRequest {
                severity: "medium".to_string(),
                category: "documentation".to_string(),
                description: "API docs are incomplete".to_string(),
                why_it_matters: None,
                source: None,
            },
        ]
    };

    // Round 1: Unverified → reviewing + gaps → condition 6 → needs_revision, rounds=[fp1]
    let round1 = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(make_gaps()),
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 1 must succeed");

    assert_eq!(
        round1.0.status, "needs_revision",
        "round 1 → needs_revision (condition 6)"
    );

    // Read current snapshot (persisted by round 1) so we can reset status to Reviewing
    // while keeping the rounds/fingerprints intact.
    let after_r1 = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    let mut r1_snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, after_r1.verification_generation)
        .await
        .unwrap()
        .unwrap();
    r1_snapshot.status = VerificationStatus::Reviewing;
    r1_snapshot.in_progress = true;

    // Reset status to Reviewing (keeps round 1 fingerprints in the native snapshot)
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &r1_snapshot)
        .await
        .unwrap();

    // Round 2: Reviewing → needs_revision + different gaps (with critical), round=2
    // Before push: metadata.rounds=[fp1] (len==1) → "need 2 consecutive" → not yet converged.
    // Using critical gaps to prevent zero_blocking from firing (critical_count > 0).
    // Using different gaps from round 1 so jaccard(fp2, fp1) < 1.0.
    let make_gaps_round2 = || {
        vec![
            VerificationGapRequest {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "No authentication mechanism specified".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGapRequest {
                severity: "high".to_string(),
                category: "reliability".to_string(),
                description: "No retry mechanism defined".to_string(),
                why_it_matters: None,
                source: None,
            },
        ]
    };

    let round2 = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(make_gaps_round2()),
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 2 must succeed");

    // critical_count=1 → zero_blocking=false. Jaccard: len==1 before push → not yet. → needs_revision
    assert_eq!(
        round2.0.status, "needs_revision",
        "round 2 → still needs_revision (jaccard needs 2 pairs)"
    );

    // Reset status to Reviewing again (keeps 2-round metadata)
    let after_r2 = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    let mut r2_snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, after_r2.verification_generation)
        .await
        .unwrap()
        .unwrap();
    r2_snapshot.status = VerificationStatus::Reviewing;
    r2_snapshot.in_progress = true;

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &r2_snapshot)
        .await
        .unwrap();

    // Round 3: same gaps as round 2 → jaccard(fp3=fp2, fp2)=1.0 BUT jaccard(fp2, fp1)<1.0
    // fp1 = fingerprints of [high_scale, medium_docs]
    // fp2 = fingerprints of [crit_auth, high_retry]
    // fp3 = fp2 (same gaps)
    // jaccard(fp3, fp2) = 1.0 ≥ 0.8 ✓ (same gaps)
    // jaccard(fp2, fp1) = 0.0 (completely different descriptions) < 0.8 ✗
    // → 2-pair requirement NOT met → no convergence
    let round3 = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(3),
            gaps: Some(make_gaps_round2()), // same as round 2 → fp3 == fp2
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 3 must succeed");

    // critical_count=1 → zero_blocking=false. Jaccard fires but only 1 of 2 pairs qualifies.
    // → needs_revision (no convergence)
    assert_eq!(
        round3.0.status, "needs_revision",
        "round 3: jaccard(fp3,fp2)=1.0 but jaccard(fp2,fp1)=0.0 → 2-pair requirement not met"
    );
    assert!(
        round3.0.convergence_reason.is_none(),
        "no convergence when only 1 of 2 consecutive pairs is above the Jaccard threshold"
    );
}

/// Jaccard convergence triggered: all 3 consecutive rounds have identical critical gaps.
///
/// When a critic keeps finding the same gaps unchanged for 3 rounds,
/// the server detects that the plan has converged (can't be improved further).
///
/// Uses critical gaps to prevent zero_blocking from triggering first.
/// Status is reset to Reviewing between rounds using direct repo calls
/// (simulating the agent's needs_revision → reviewing → needs_revision cycle).
#[tokio::test]
async fn test_jaccard_convergence_triggered_three_identical_rounds() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Gaps with 1 critical → zero_blocking can never fire (critical_count > 0)
    // Same gaps each round → fingerprints identical → Jaccard = 1.0 for all pairs
    let stable_gaps = || {
        vec![
            VerificationGapRequest {
                severity: "critical".to_string(),
                category: "architecture".to_string(),
                description: "Plan has no rollback strategy".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGapRequest {
                severity: "high".to_string(),
                category: "operations".to_string(),
                description: "No deployment runbook provided".to_string(),
                why_it_matters: None,
                source: None,
            },
        ]
    };

    // Round 1: Unverified → reviewing + critical gaps → condition 6 → needs_revision, rounds=[fp1]
    let round1 = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(stable_gaps()),
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 1 must succeed");

    assert_eq!(
        round1.0.status, "needs_revision",
        "round 1 → needs_revision (condition 6)"
    );

    // Reset to Reviewing, preserving round 1's metadata (fingerprints)
    let after_r1 = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    let mut after_r1_snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, after_r1.verification_generation)
        .await
        .unwrap()
        .unwrap();
    after_r1_snapshot.status = VerificationStatus::Reviewing;
    after_r1_snapshot.in_progress = true;
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &after_r1_snapshot)
        .await
        .unwrap();

    // Round 2: Reviewing → needs_revision + same gaps, round=2
    // Before push: rounds=[fp1] (len==1) → "need 2 consecutive" → not yet. After: rounds=[fp1, fp2=fp1]
    // zero_blocking: critical_count=1 > 0 → false. → needs_revision stays.
    let round2 = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(stable_gaps()),
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 2 must succeed");

    assert_eq!(
        round2.0.status, "needs_revision",
        "round 2 → needs_revision (1 pair is not enough for jaccard)"
    );

    // Reset to Reviewing again, preserving round 1+2 metadata
    let after_r2 = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    let mut after_r2_snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, after_r2.verification_generation)
        .await
        .unwrap()
        .unwrap();
    after_r2_snapshot.status = VerificationStatus::Reviewing;
    after_r2_snapshot.in_progress = true;
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &after_r2_snapshot)
        .await
        .unwrap();

    // Round 3: Reviewing → needs_revision + same gaps, round=3
    // Before push: rounds=[fp1, fp2=fp1] (len==2) → jaccard check fires
    // jaccard(new_fp, fp2) = jaccard(fp1, fp1) = 1.0 ≥ 0.8 ✓
    // jaccard(fp2, fp1) = jaccard(fp1, fp1) = 1.0 ≥ 0.8 ✓
    // → jaccard_converged = true → Verified
    let round3 = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(3),
            gaps: Some(stable_gaps()),
            convergence_reason: None,
            max_rounds: Some(10),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 3 must succeed");

    let resp3 = round3.0;
    assert_eq!(
        resp3.status, "verified",
        "3 identical rounds → jaccard_converged (all pairs Jaccard=1.0 ≥ 0.8)"
    );
    assert_eq!(
        resp3.convergence_reason.as_deref(),
        Some("jaccard_converged"),
        "convergence_reason must be 'jaccard_converged'"
    );
}

/// Max rounds exit: reaching max_rounds forces convergence to verified.
///
/// The server auto-terminates the verification loop when `current_round >= max_rounds`.
/// This prevents infinite loops when the plan has stubborn unresolved gaps.
#[tokio::test]
async fn test_max_rounds_exit_behavior() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Round 3 = max_rounds=3: gaps present but max_rounds fires → verified
    // (condition 3 takes priority over condition 6)
    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(3),
            gaps: Some(vec![VerificationGapRequest {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "Unresolved authentication gap".to_string(),
                why_it_matters: Some("Users remain vulnerable".to_string()),
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(3),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "current_round >= max_rounds → server forces convergence to verified"
    );
    assert_eq!(
        resp.convergence_reason.as_deref(),
        Some("max_rounds"),
        "convergence_reason must be 'max_rounds'"
    );
}

// ── Re-verification transition tests ──
// Tests covering Verified → Reviewing, Skipped → Reviewing, metadata reset,
// zombie protection after re-verify, and regression for existing transitions.

/// Re-verify: Verified → Reviewing returns 200.
///
/// A ralphx-plan-verifier agent must be able to restart verification on a session
/// that was previously verified. The Verified → Reviewing transition must
/// be allowed and return 200 with status="reviewing".
#[tokio::test]
async fn test_reverify_verified_to_reviewing_returns_200() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "Verified → Reviewing must return 200: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(
        resp.status, "reviewing",
        "status must be reviewing after transition"
    );
    assert!(resp.in_progress, "in_progress must be true");
}

/// Re-verify: Skipped → Reviewing returns 200.
///
/// A user who previously skipped verification must be able to start it.
/// Skipped → Reviewing must be allowed.
#[tokio::test]
async fn test_reverify_skipped_to_reviewing_returns_200() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Skipped)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "Skipped → Reviewing must return 200: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(resp.status, "reviewing");
    assert!(resp.in_progress);
}

/// Re-verify clears ALL stale metadata and passes condition 6.
///
/// When transitioning from Verified → Reviewing, the handler must:
/// 1. Clear all stale fields: current_gaps, rounds, convergence_reason,
///    best_round_index, current_round, parse_failures.
/// 2. Increment verification_generation (N → N+1).
/// 3. Return response with new generation N+1 (not stale N).
/// 4. Allow a subsequent needs_revision call with generation=N+1.
///
/// The initial reviewing call sends NO gaps, which is safe from condition 6
/// (condition 6 only fires when reviewing + gaps present).
#[tokio::test]
async fn test_reverify_clears_all_stale_metadata() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(3)
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Set stale metadata from a prior verification run
    let stale_snapshot = make_snapshot(
        3,
        VerificationStatus::Verified,
        false,
        3,
        5,
        vec![make_gap("high", "security", "No auth")],
        vec![
            make_round(vec!["fp-auth", "fp-scale"], 12),
            make_round(vec!["fp-auth"], 8),
            make_round(vec!["fp-auth"], 8),
        ],
        Some("max_rounds"),
        Some(2),
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Verified, false)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &stale_snapshot)
        .await
        .unwrap();

    // Trigger re-verify with no gaps — safe from condition 6
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(3),
        }),
    )
    .await
    .expect("re-verify from Verified must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "reviewing",
        "status must be reviewing after re-verify"
    );
    assert!(resp.in_progress, "in_progress must be true");
    assert_eq!(
        resp.verification_generation, 4,
        "response must use new generation N+1=4, not stale N=3"
    );
    assert!(
        resp.current_gaps.is_empty(),
        "stale current_gaps must be cleared in response"
    );
    assert!(
        resp.rounds.is_empty(),
        "stale rounds must be cleared in response"
    );
    assert!(
        resp.convergence_reason.is_none(),
        "stale convergence_reason must be cleared"
    );

    // Verify DB state directly
    let updated = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.verification_status, VerificationStatus::Reviewing);
    assert!(updated.verification_in_progress);
    assert_eq!(
        updated.verification_generation, 4,
        "generation must be 4 in DB"
    );

    // Legacy metadata stays cleared; native reverify state is carried by the new generation snapshot.
    assert_eq!(updated.verification_current_round, None);
    assert_eq!(updated.verification_max_rounds, None);
    assert_eq!(updated.verification_gap_count, 0);
    assert_eq!(updated.verification_gap_score, None);
    assert_eq!(updated.verification_convergence_reason, None);

    // Confirm next valid call succeeds with new generation — do NOT call reviewing→reviewing (422)
    let next = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "New gap found in fresh review".to_string(),
                why_it_matters: None,
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(4), // New generation after reset
        }),
    )
    .await;

    assert!(
        next.is_ok(),
        "needs_revision with new generation=4 must succeed after metadata reset: {:?}",
        next.err()
    );
    let next_resp = next.unwrap().0;
    assert_eq!(next_resp.status, "needs_revision");
    assert_eq!(next_resp.current_gaps.len(), 1, "new gap must be present");
}

/// Skipped → NeedsRevision is rejected (422).
///
/// The only allowed transition from Skipped is Skipped → Reviewing.
#[tokio::test]
async fn test_skipped_to_needs_revision_blocked() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Skipped)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Skipped → NeedsRevision must be rejected");
    let (status, _) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "must return 422 for invalid Skipped → NeedsRevision"
    );
}

/// Skipped → Verified is rejected (422).
///
/// The only allowed transition from Skipped is Skipped → Reviewing.
#[tokio::test]
async fn test_skipped_to_verified_blocked() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Skipped)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "verified".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: Some("zero_blocking".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Skipped → Verified must be rejected");
    let (status, _) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "must return 422 for invalid Skipped → Verified"
    );
}

/// Zombie protection after re-verify: stale-generation agent is rejected with 409.
///
/// When reset_and_begin_reverify increments the generation from N to N+1,
/// a stale agent still sending generation=N must receive 409 CONFLICT.
/// The new agent with generation=N+1 must succeed.
#[tokio::test]
async fn test_reverify_zombie_agent_rejected_after_generation_increment() {
    let state = setup_test_state().await;

    // Session at Verified with generation=5
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(5)
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Re-verify: generation increments from 5 → 6
    let reverify = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(5),
        }),
    )
    .await
    .expect("re-verify must succeed and increment generation to 6");

    assert_eq!(
        reverify.0.verification_generation, 6,
        "generation must be 6 after reset"
    );

    // Zombie agent sends needs_revision with old generation=5 → must be rejected with 409
    let zombie = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(5), // Stale — old generation
        }),
    )
    .await;

    assert!(
        zombie.is_err(),
        "zombie agent with stale generation=5 must be rejected"
    );
    let (status, body) = zombie.unwrap_err();
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "must return 409 CONFLICT for zombie agent after re-verify"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("Call get_plan_verification on the parent session"),
        "generation mismatch error should tell the agent how to recover"
    );

    // Fresh agent with correct generation=6 → must succeed
    let fresh = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(6),
        }),
    )
    .await;

    assert!(
        fresh.is_ok(),
        "fresh agent with generation=6 must succeed: {:?}",
        fresh.err()
    );
}

/// ImportedVerified → Reviewing triggers metadata reset.
///
/// ImportedVerified sessions may carry stale gaps/rounds from the imported state.
/// Transitioning to Reviewing must clear all stale metadata (same as Verified → Reviewing).
#[tokio::test]
async fn test_imported_verified_to_reviewing_triggers_metadata_reset() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(2)
        .verification_status(VerificationStatus::ImportedVerified)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let stale_snapshot = make_snapshot(
        2,
        VerificationStatus::ImportedVerified,
        false,
        2,
        3,
        vec![make_gap("medium", "docs", "Missing docs")],
        vec![make_round(vec!["fp-imported"], 5)],
        Some("zero_blocking"),
        Some(0),
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::ImportedVerified, false)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &stale_snapshot)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(2),
        }),
    )
    .await
    .expect("ImportedVerified → Reviewing must succeed");

    let resp = result.0;
    assert_eq!(resp.status, "reviewing");
    assert!(resp.in_progress);
    assert_eq!(
        resp.verification_generation, 3,
        "generation must be incremented to 2+1=3"
    );
    assert!(
        resp.current_gaps.is_empty(),
        "imported stale gaps must be cleared"
    );
    assert!(
        resp.rounds.is_empty(),
        "imported stale rounds must be cleared"
    );
    assert!(
        resp.convergence_reason.is_none(),
        "imported convergence_reason must be cleared"
    );

    let updated = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.verification_generation, 3);

    assert_eq!(updated.verification_current_round, None);
    assert_eq!(updated.verification_max_rounds, None);
    assert_eq!(updated.verification_gap_count, 0);
    assert_eq!(updated.verification_gap_score, None);
    assert_eq!(updated.verification_convergence_reason, None);
}

/// Regression: Verified → Skipped still allowed after new re-verify arms.
///
/// The new Verified → Reviewing match arm must not shadow the existing
/// catch-all that allows any status → Skipped.
#[tokio::test]
async fn test_verified_to_skipped_still_allowed() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "skipped".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "Verified → Skipped must still be allowed: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(resp.status, "skipped");
}

/// Full re-verify flow: reach Verified, re-verify, new gaps replace old gaps.
///
/// End-to-end test simulating the ralphx-plan-verifier agent's second run:
/// 1. Session is at Verified with stale gaps from prior run.
/// 2. Re-verify transition clears metadata and increments generation.
/// 3. First round of new verification (needs_revision with new gaps) succeeds.
/// 4. New gaps are present in the response; old gaps are gone.
#[tokio::test]
async fn test_full_reverify_flow_new_gaps_replace_old() {
    let state = setup_test_state().await;

    // Session at Verified with generation=1 and stale gaps
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(1)
        .verification_status(VerificationStatus::Verified)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let old_snapshot = make_snapshot(
        1,
        VerificationStatus::Verified,
        false,
        1,
        1,
        vec![make_gap("high", "old", "Old outdated gap")],
        vec![make_round(vec!["old-gap-fp"], 5)],
        Some("max_rounds"),
        Some(0),
    );

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Verified, false)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &old_snapshot)
        .await
        .unwrap();

    // Re-verify: Verified → Reviewing (clears metadata, increments gen 1 → 2)
    let reverify = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(1),
        }),
    )
    .await
    .expect("re-verify must succeed");

    assert_eq!(reverify.0.verification_generation, 2);
    assert!(
        reverify.0.current_gaps.is_empty(),
        "old gaps must be cleared after re-verify"
    );

    // Round 1 with fresh gaps using new generation=2
    let round1 = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "Completely new security gap found in fresh review".to_string(),
                why_it_matters: Some("Fresh analysis found new vulnerabilities".to_string()),
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: Some(2),
        }),
    )
    .await
    .expect("round 1 with new generation must succeed");

    let r1 = round1.0;
    assert_eq!(r1.status, "needs_revision", "critical gap → needs_revision");
    assert_eq!(r1.current_gaps.len(), 1, "exactly 1 new gap");
    assert_eq!(
        r1.current_gaps[0].description, "Completely new security gap found in fresh review",
        "new gap description must be present"
    );
    assert_eq!(r1.current_gaps[0].severity, "critical");

    // DB: new gaps present, old gaps gone
    let final_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    let snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, final_session.verification_generation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.current_gaps[0].description, "Completely new security gap found in fresh review",
        "DB must contain new gap"
    );
    assert_ne!(
        snapshot.current_gaps[0].description, "Old outdated gap",
        "old gap must not be present in DB"
    );
}

// ============================================================================
// External origin guard tests
// ============================================================================

/// External session + status=skipped → post_verification_status must return 403.
///
/// Proof Obligation 1: external agent attempts to skip verification via the internal HTTP handler.
/// The guard at the top of post_verification_status checks session.origin == External before
/// processing any transition logic.
#[tokio::test]
async fn test_update_verification_rejects_skip_for_external_origin() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .origin(SessionOrigin::External)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "skipped".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(result.is_err(), "external session must reject skip status");
    let (status, body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "external skip must return 403 FORBIDDEN"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("Use status='reviewing' for in-progress rounds"),
        "external skip error should provide repair guidance"
    );
}

/// External session + revert_and_skip → must return 403.
///
/// Proof Obligation 2: external agent attempts to use the revert_and_skip endpoint.
/// The origin guard fires before any artifact lookup, so no artifact needs to exist.
#[tokio::test]
async fn test_revert_and_skip_blocks_external_origin() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .origin(SessionOrigin::External)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = revert_and_skip(
        State(state),
        Path(session_id),
        Json(RevertAndSkipRequest {
            plan_version_to_restore: "non-existent-artifact-id".to_string(),
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "external session must reject revert_and_skip"
    );
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "external revert_and_skip must return 403 FORBIDDEN"
    );
}

// ============================================================================
// Auto-propose integration tests (PDM-24)
// ============================================================================

/// Auto-propose fires for external sessions that reach zero_blocking convergence.
///
/// Proof Obligation 3 (partial): session.origin == External && convergence_reason == zero_blocking
/// → auto_propose_for_external() sends message to the orchestrator agent.
///
/// Flow:
/// - Create external session, pre-register running agent (Gate 2 → queues message)
/// - Round 1: Reviewing + gaps stored in metadata
/// - Round 2: needs_revision + 0 gaps → server auto-converges (zero_blocking)
/// - Assert: message_queue contains <auto-propose> for the session's ideation context
#[tokio::test]
async fn test_auto_propose_fires_for_external_zero_blocking() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .origin(SessionOrigin::External)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Pre-register a running agent under "ideation" key so send_message queues instead of spawning.
    // auto_propose_for_external calls send_message(ChatContextType::Ideation, session_id, ...)
    // which uses RunningAgentKey::new("ideation", session_id) for Gate 2.
    let agent_key = RunningAgentKey::new("ideation", &session_id);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            std::process::id(),
            "test-conv-ap".to_string(),
            "test-run-ap".to_string(),
            None,
            None,
        )
        .await;

    // Clone the message_queue Arc before state is moved into the handler
    let message_queue = Arc::clone(&state.app_state.message_queue);

    // Set up round 1 state: session in Reviewing with 2 blocking gaps
    let prior_gaps = vec![
        make_gap("high", "security", "No authentication layer"),
        make_gap("medium", "testing", "No unit tests"),
    ];
    let prior_rounds = vec![make_round(
        vec!["no-authentication-layer", "no-unit-tests"],
        30,
    )];
    let round1_snapshot = make_snapshot(
        0,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
        prior_gaps,
        prior_rounds,
        None,
        None,
    );
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &round1_snapshot)
        .await
        .unwrap();

    // Round 2: 0 gaps → server auto-detects zero_blocking, overrides to Verified,
    // then calls auto_propose_for_external (external + zero_blocking guard passes).
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]), // all blocking gaps cleared
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 2 must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "0 gaps at round 2 → server auto-converges to verified"
    );
    assert_eq!(
        resp.convergence_reason.as_deref(),
        Some("zero_blocking"),
        "convergence_reason must be 'zero_blocking'"
    );

    // The detached auto-propose task should enqueue the trigger message and complete the
    // external activity handoff shortly after the handler returns.
    let mut queued_contents: Vec<String> = Vec::new();
    let mut final_phase = None;
    for _ in 0..200 {
        let queued = message_queue.get_queued(ChatContextType::Ideation, &session_id);
        queued_contents = queued.iter().map(|m| m.content.clone()).collect();
        let refreshed = state
            .app_state
            .ideation_session_repo
            .get_by_id(&session_id_obj)
            .await
            .expect("session reload should succeed")
            .expect("session should still exist");
        final_phase = refreshed.external_activity_phase.clone();
        if queued_contents
            .iter()
            .any(|content| content.contains("<auto-propose>"))
            && final_phase.as_deref() == Some("ready")
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }
    assert!(
        queued_contents
            .iter()
            .any(|content| content.contains("<auto-propose>")),
        "queued message must contain <auto-propose> tag; got: {:?}",
        queued_contents
    );
    assert_eq!(
        final_phase.as_deref(),
        Some("ready"),
        "external auto-propose must restore activity phase to ready after delivery"
    );
}

/// External zero_blocking convergence with a live verification child must still emit verified
/// side effects before the child is stopped/archived.
#[tokio::test]
async fn test_external_zero_blocking_verified_side_effects_survive_child_shutdown() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .origin(SessionOrigin::External)
        .build();
    let parent_id = parent.id.clone();
    let parent_id_str = parent_id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Keep the parent on the queued-message path so auto-propose never spawns a real agent.
    let parent_key = RunningAgentKey::new("ideation", &parent_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            parent_key,
            std::process::id(),
            "test-conv-parent".to_string(),
            "test-run-parent".to_string(),
            None,
            None,
        )
        .await;

    let child = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .origin(SessionOrigin::External)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    let child_id_str = child_id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Register the verifier child as running so terminal verification will actively stop it.
    let child_key = RunningAgentKey::new("ideation", &child_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            child_key,
            66666,
            "test-conv-child".to_string(),
            "test-run-child".to_string(),
            None,
            None,
        )
        .await;

    let prior_gaps = vec![
        make_gap("high", "security", "No authentication layer"),
        make_gap("medium", "testing", "No unit tests"),
    ];
    let prior_rounds = vec![make_round(
        vec!["no-authentication-layer", "no-unit-tests"],
        30,
    )];
    let round1_snapshot = make_snapshot(
        0,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
        prior_gaps,
        prior_rounds,
        None,
        None,
    );
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&parent_id, &round1_snapshot)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(parent_id_str.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("terminal zero_blocking update must succeed");

    assert_eq!(result.0.status, "verified");
    assert_eq!(
        result.0.convergence_reason.as_deref(),
        Some("zero_blocking")
    );

    let mut queued_auto_propose = false;
    let mut child_archived = false;
    let mut verified_event_seen = false;
    let mut final_phase = None;
    for _ in 0..200 {
        queued_auto_propose = state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, &parent_id_str)
            .iter()
            .any(|message| message.content.contains("<auto-propose>"));
        child_archived = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .expect("child reload should succeed")
            .expect("child should exist")
            .status
            == IdeationSessionStatus::Archived;
        final_phase = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .expect("parent reload should succeed")
            .expect("parent should exist")
            .external_activity_phase;
        verified_event_seen = get_external_event_types(&state, &project_id)
            .await
            .iter()
            .any(|event_type| event_type == "ideation:verified");

        if queued_auto_propose
            && child_archived
            && verified_event_seen
            && final_phase.as_deref() == Some("ready")
        {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }

    assert!(
        queued_auto_propose,
        "parent must receive queued <auto-propose> message"
    );
    assert!(
        child_archived,
        "verification child must be archived after terminal verification"
    );
    assert!(
        verified_event_seen,
        "ideation:verified event must be emitted"
    );
    assert_eq!(
        final_phase.as_deref(),
        Some("ready"),
        "external auto-propose path must restore activity phase to ready"
    );
}

/// Auto-propose is skipped for internal (non-external) sessions even if zero_blocking fires.
///
/// Proof Obligation 3: internal sessions are NOT affected by auto-propose.
/// The guard inside auto_propose_for_external checks session.origin == External and returns
/// early for Internal sessions. The call site also has the origin check, so auto_propose
/// is never invoked for internal sessions.
///
/// Flow: same verification round sequence as above, but session origin = Internal (default).
/// Assert: message_queue is empty after convergence.
#[tokio::test]
async fn test_auto_propose_skipped_for_internal_session() {
    let state = setup_test_state().await;

    // Default origin = Internal (no .origin() call needed)
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(1)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Clone message_queue before state is moved
    let message_queue = Arc::clone(&state.app_state.message_queue);

    // Round 1: Reviewing + gaps
    let prior_gaps = vec![
        make_gap("high", "security", "No authentication layer"),
        make_gap("medium", "testing", "No unit tests"),
    ];
    let prior_rounds = vec![make_round(
        vec!["no-authentication-layer", "no-unit-tests"],
        30,
    )];
    let round1_snapshot = make_snapshot(
        1,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
        prior_gaps,
        prior_rounds,
        None,
        None,
    );
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &round1_snapshot)
        .await
        .unwrap();

    // Round 2: zero_blocking convergence on internal session
    let result = post_verification_status(
        State(state),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round 2 must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "internal session must still converge to verified on zero_blocking"
    );
    assert_eq!(
        resp.convergence_reason.as_deref(),
        Some("zero_blocking"),
        "convergence_reason must be 'zero_blocking'"
    );

    // No auto-propose message should be queued for internal sessions
    let queued = message_queue.get_queued(ChatContextType::Ideation, &session_id);
    assert!(
        queued.is_empty(),
        "no auto-propose must be queued for internal sessions; got: {:?}",
        queued.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
}

/// Auto-propose is skipped when convergence reason is max_rounds (not zero_blocking).
///
/// Proof Obligation 4: only zero_blocking triggers auto-propose.
/// max_rounds and jaccard_converged may have unresolved gaps, so they are excluded.
/// The call site guard checks convergence_reason == Some("zero_blocking") explicitly.
///
/// Flow: external session reaches Verified via max_rounds (gaps still present).
/// Assert: message_queue is empty after convergence.
#[tokio::test]
async fn test_auto_propose_skipped_for_non_zero_blocking() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .origin(SessionOrigin::External)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Clone message_queue before state is moved
    let message_queue = Arc::clone(&state.app_state.message_queue);

    // Round 3 = max_rounds=3: critical gap still present → server forces convergence via max_rounds
    // (not zero_blocking). auto_propose call site guard fails on convergence_reason check.
    let result = post_verification_status(
        State(state),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(3),
            gaps: Some(vec![VerificationGapRequest {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "Unresolved authentication gap".to_string(),
                why_it_matters: Some("Users remain vulnerable".to_string()),
                source: None,
            }]),
            convergence_reason: None,
            max_rounds: Some(3), // round == max_rounds → max_rounds convergence
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed");

    let resp = result.0;
    assert_eq!(
        resp.status, "verified",
        "current_round >= max_rounds → verified via max_rounds"
    );
    assert_eq!(
        resp.convergence_reason.as_deref(),
        Some("max_rounds"),
        "convergence_reason must be 'max_rounds'"
    );

    // No auto-propose for max_rounds convergence even on external sessions
    let queued = message_queue.get_queued(ChatContextType::Ideation, &session_id);
    assert!(
        queued.is_empty(),
        "no auto-propose must be queued for max_rounds convergence; got: {:?}",
        queued.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_post_verification_status_remaps_verification_child_session_id_to_parent() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(child_id.as_str().to_string()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("handler must succeed via parent remap");

    assert_eq!(
        result.0.session_id,
        parent_id.as_str(),
        "verification updates routed through child id must return the canonical parent id"
    );
    assert_eq!(result.0.status, "reviewing");
    assert!(result.0.in_progress);

    let refreshed_parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .expect("parent must still exist");
    assert_eq!(
        refreshed_parent.verification_status,
        VerificationStatus::Reviewing
    );
    assert!(
        refreshed_parent.verification_in_progress,
        "parent session must receive the verification state update"
    );
}

#[tokio::test]
async fn test_get_plan_verification_remaps_verification_child_session_id_to_parent() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(child_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed via parent remap");

    assert_eq!(
        result.0.session_id,
        parent_id.as_str(),
        "verification reads routed through child id must return the canonical parent id"
    );
    assert_eq!(result.0.status, "reviewing");
    assert!(result.0.in_progress);
    assert_eq!(
        result
            .0
            .verification_child
            .as_ref()
            .map(|info| info.latest_child_session_id.as_str()),
        Some(child_id.as_str()),
        "parent continuity block must still point at the verification child"
    );
}

#[tokio::test]
async fn test_get_plan_verification_uses_native_round_store_when_metadata_is_missing() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(1)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id_obj = session.id.clone();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let round_update = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "critical".to_string(),
                category: "completeness".to_string(),
                description: "Migration not registered".to_string(),
                why_it_matters: Some("Backfill will never execute".to_string()),
                source: Some("completeness".to_string()),
            }]),
            convergence_reason: Some("escalated_to_parent".to_string()),
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await
    .expect("round update must succeed");

    assert_eq!(round_update.0.status, "needs_revision");
    assert_eq!(round_update.0.current_gaps.len(), 1);
    assert_eq!(round_update.0.round_details.len(), 1);

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();

    let queried = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id),
        default_verification_query(),
    )
    .await
    .expect("query must succeed")
    .0;

    assert_eq!(queried.status, "needs_revision");
    assert_eq!(
        queried.current_gaps.len(),
        1,
        "native store must preserve current gaps"
    );
    assert_eq!(
        queried.rounds.len(),
        1,
        "native store must preserve round summaries"
    );
    assert_eq!(
        queried.round_details.len(),
        1,
        "native store must preserve round lineage"
    );
    assert_eq!(queried.round_details[0].gap_count, 1);
    assert_eq!(
        queried.round_details[0].gaps[0].description,
        "Migration not registered"
    );
}

#[tokio::test]
async fn test_get_plan_verification_exposes_native_run_history_and_selected_generation() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(20)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                18,
                VerificationStatus::NeedsRevision,
                false,
                2,
                5,
                vec![make_gap("high", "testing", "Missing migration regression")],
                vec![
                    make_round(vec!["fp-a", "fp-b"], 8),
                    make_round(vec!["fp-a"], 3),
                ],
                Some("escalated_to_parent"),
                Some(1),
            ),
        )
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                20,
                VerificationStatus::Reviewing,
                true,
                0,
                5,
                vec![],
                vec![],
                None,
                None,
            ),
        )
        .await
        .unwrap();

    let current = get_plan_verification(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        default_verification_query(),
    )
    .await
    .expect("current generation query must succeed")
    .0;

    assert_eq!(current.status, "reviewing");
    assert!(current.in_progress);
    assert_eq!(current.verification_generation, 20);
    assert_eq!(current.selected_generation, 20);
    assert_eq!(
        current
            .run_history
            .iter()
            .map(|entry| entry.generation)
            .collect::<Vec<_>>(),
        vec![20, 18]
    );
    assert_eq!(current.run_history[0].status, "reviewing");
    assert_eq!(current.run_history[1].status, "needs_revision");
    assert_eq!(current.run_history[1].round_count, 2);
    assert_eq!(current.run_history[1].gap_count, 1);

    let historical = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        Query(VerificationQueryParams {
            generation: Some(18),
        }),
    )
    .await
    .expect("historical generation query must succeed")
    .0;

    assert_eq!(historical.verification_generation, 20);
    assert_eq!(historical.selected_generation, 18);
    assert_eq!(historical.status, "needs_revision");
    assert!(!historical.in_progress);
    assert_eq!(historical.current_round, Some(2));
    assert_eq!(historical.round_details.len(), 2);
    assert_eq!(historical.current_gaps.len(), 1);
    assert_eq!(
        historical.current_gaps[0].description,
        "Missing migration regression"
    );
}

#[tokio::test]
async fn test_get_plan_verification_filters_stale_blank_historical_generation_from_run_history() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(21)
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                18,
                VerificationStatus::NeedsRevision,
                false,
                2,
                5,
                vec![make_gap("high", "testing", "Missing migration regression")],
                vec![
                    make_round(vec!["fp-a", "fp-b"], 8),
                    make_round(vec!["fp-a"], 3),
                ],
                Some("escalated_to_parent"),
                Some(1),
            ),
        )
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                20,
                VerificationStatus::Reviewing,
                true,
                0,
                5,
                vec![],
                vec![],
                None,
                None,
            ),
        )
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                21,
                VerificationStatus::Reviewing,
                true,
                0,
                5,
                vec![],
                vec![],
                None,
                None,
            ),
        )
        .await
        .unwrap();

    let active_child = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .parent_session_id(session_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    state
        .app_state
        .ideation_session_repo
        .create(active_child)
        .await
        .unwrap();

    let current = get_plan_verification(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        default_verification_query(),
    )
    .await
    .expect("current generation query must succeed")
    .0;

    assert_eq!(current.verification_generation, 21);
    assert_eq!(current.selected_generation, 21);
    assert_eq!(
        current
            .run_history
            .iter()
            .map(|entry| entry.generation)
            .collect::<Vec<_>>(),
        vec![21, 18],
        "historical blank in-progress generations must be filtered out of run history",
    );

    let historical = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        Query(VerificationQueryParams {
            generation: Some(18),
        }),
    )
    .await
    .expect("historical generation query must succeed")
    .0;

    assert_eq!(historical.verification_generation, 21);
    assert_eq!(historical.selected_generation, 18);
    assert_eq!(
        historical
            .run_history
            .iter()
            .map(|entry| entry.generation)
            .collect::<Vec<_>>(),
        vec![21, 18],
        "historical blank in-progress generations must stay hidden even when viewing an older run",
    );
}

#[tokio::test]
async fn test_get_plan_verification_defaults_to_latest_displayable_generation() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let session = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .verification_generation(2)
        .verification_status(VerificationStatus::Unverified)
        .build();
    let session_id = session.id.clone();
    let session_id_str = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                1,
                VerificationStatus::Verified,
                false,
                4,
                5,
                vec![make_gap("medium", "completeness", "Historical gap")],
                vec![make_round(vec!["fp-a"], 1)],
                Some("max_rounds"),
                Some(0),
            ),
        )
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &make_snapshot(
                2,
                VerificationStatus::Unverified,
                false,
                0,
                0,
                vec![],
                vec![],
                None,
                None,
            ),
        )
        .await
        .unwrap();

    let historical_child = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .parent_session_id(session_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .title("Auto-verification (gen 1)")
        .status(IdeationSessionStatus::Archived)
        .build();
    let historical_child_id = historical_child.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(historical_child)
        .await
        .unwrap();

    let blank_child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(session_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .title("Auto-verification (gen 2)")
        .status(IdeationSessionStatus::Archived)
        .build();
    state
        .app_state
        .ideation_session_repo
        .create(blank_child)
        .await
        .unwrap();

    let current = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        default_verification_query(),
    )
    .await
    .expect("default query must fall back to the latest displayable verification")
    .0;

    assert_eq!(current.verification_generation, 2);
    assert_eq!(current.selected_generation, 1);
    assert_eq!(current.status, "verified");
    assert_eq!(current.current_round, Some(4));
    assert_eq!(current.round_details.len(), 1);
    assert_eq!(
        current
            .run_history
            .iter()
            .map(|entry| entry.generation)
            .collect::<Vec<_>>(),
        vec![1],
        "blank generation 2 should not be exposed as a selectable run"
    );
    let child = current
        .verification_child
        .expect("selected historical generation should expose its verifier child");
    assert_eq!(child.latest_child_session_id, historical_child_id);
    assert!(child.latest_child_archived);
    assert_eq!(child.active_child_session_id, None);
}

#[tokio::test]
async fn test_mark_verification_infra_failure_remaps_child_and_resets_parent_to_unverified() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(4)
        .build();
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &ralphx_lib::domain::entities::VerificationRunSnapshot {
                generation: 4,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: Some(1),
                convergence_reason: None,
                current_gaps: vec![ralphx_lib::domain::entities::VerificationGap {
                    severity: "critical".to_string(),
                    category: "runtime".to_string(),
                    description: "stale gap that must not survive infra failure".to_string(),
                    why_it_matters: None,
                    source: Some("completeness".to_string()),
                }],
                rounds: vec![ralphx_lib::domain::entities::VerificationRoundSnapshot {
                    round: 1,
                    gap_score: 10,
                    fingerprints: vec!["gap-a".to_string()],
                    gaps: vec![ralphx_lib::domain::entities::VerificationGap {
                        severity: "critical".to_string(),
                        category: "runtime".to_string(),
                        description: "stale gap that must not survive infra failure".to_string(),
                        why_it_matters: None,
                        source: Some("completeness".to_string()),
                    }],
                    parse_failed: false,
                }],
            },
        )
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    let result = mark_verification_infra_failure(
        State(state.clone()),
        Path(child_id.as_str().to_string()),
        Json(VerificationInfraFailureRequest {
            generation: Some(4),
            convergence_reason: Some("agent_error".to_string()),
            round: Some(2),
            max_rounds: Some(5),
        }),
    )
    .await
    .expect("handler must succeed via parent remap");

    assert_eq!(result.0.session_id, parent_id.as_str());
    assert_eq!(result.0.status, "unverified");
    assert!(!result.0.in_progress);
    assert_eq!(result.0.convergence_reason.as_deref(), Some("agent_error"));
    assert!(
        result.0.current_gaps.is_empty(),
        "infra failure must not leave authoritative blocking gaps behind"
    );
    assert_eq!(
        result.0.verification_generation, 5,
        "infra failure must increment generation to fence zombie verifiers"
    );

    let refreshed_parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .expect("parent must still exist");
    assert_eq!(
        refreshed_parent.verification_status,
        VerificationStatus::Unverified
    );
    assert!(
        !refreshed_parent.verification_in_progress,
        "infra failure must clear in-progress on the parent"
    );
    assert_eq!(refreshed_parent.verification_generation, 5);
    let stored_snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&parent_id, 4)
        .await
        .unwrap()
        .expect("infra failure must persist the native run snapshot for the failed generation");
    assert_eq!(stored_snapshot.status, VerificationStatus::Unverified);
    assert!(!stored_snapshot.in_progress);
    assert!(
        stored_snapshot.current_gaps.is_empty(),
        "native snapshot must also clear authoritative current gaps on infra failure"
    );
    assert_eq!(stored_snapshot.rounds.len(), 1);
}

#[tokio::test]
async fn test_mark_verification_infra_failure_rejects_stale_generation() {
    let state = setup_test_state().await;
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(7)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = mark_verification_infra_failure(
        State(state.clone()),
        Path(session_id.as_str().to_string()),
        Json(VerificationInfraFailureRequest {
            generation: Some(6),
            convergence_reason: Some("agent_error".to_string()),
            round: Some(1),
            max_rounds: Some(5),
        }),
    )
    .await;

    let error = result.expect_err("stale generation must be rejected");
    assert_eq!(error.0, StatusCode::CONFLICT);
    let body = error.1 .0.to_string();
    assert!(
        body.contains("Generation mismatch"),
        "error body must explain the zombie-generation rejection"
    );
}

#[tokio::test]
async fn test_mark_verification_infra_failure_uses_snapshot_truth_when_summary_is_stale() {
    let state = setup_test_state().await;
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_generation(9)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &VerificationRunSnapshot {
                generation: 9,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![],
            },
        )
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let result = mark_verification_infra_failure(
        State(state.clone()),
        Path(session_id.as_str().to_string()),
        Json(VerificationInfraFailureRequest {
            generation: Some(9),
            convergence_reason: Some("agent_error".to_string()),
            round: Some(2),
            max_rounds: Some(5),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "active-generation snapshot must allow infra-failure cleanup even if summary is stale: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "unverified");
    assert!(!response.in_progress);
    assert_eq!(response.verification_generation, 10);
}

// ── verification_child continuity tests ──────────────────────────────────────

/// No verification child → verification_child is None
#[tokio::test]
async fn test_get_plan_verification_no_child_returns_null() {
    let state = setup_test_state().await;
    let session = IdeationSession::new(ProjectId::new());
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed");

    assert!(
        result.0.verification_child.is_none(),
        "no verification child → verification_child must be None"
    );
}

/// Parent with active (non-archived) child and in_progress=true →
/// active_child_session_id is populated.
#[tokio::test]
async fn test_get_plan_verification_active_child_populates_active_id() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    // Create parent session
    let parent = IdeationSession::new(project_id.clone());
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Create active verification child
    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id_str = child.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Set parent verification state: in_progress=true
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(parent_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed");

    let child_info = result
        .0
        .verification_child
        .expect("active child must produce verification_child block");

    assert_eq!(
        child_info.active_child_session_id.as_deref(),
        Some(child_id_str.as_str()),
        "in_progress=true + non-archived child → active_child_session_id must be set"
    );
    assert_eq!(child_info.latest_child_session_id, child_id_str);
    assert!(
        !child_info.latest_child_archived,
        "active child must not be archived"
    );
    assert_eq!(child_info.agent_state, "idle", "no registry entry → idle");
}

/// Parent with archived child → latest_child_archived=true, active_child_session_id=None
#[tokio::test]
async fn test_get_plan_verification_archived_child_no_active_id() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id_str = child.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(child.clone())
        .await
        .unwrap();

    // Archive the child
    state
        .app_state
        .ideation_session_repo
        .update_status(&child.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    // in_progress=false (verification done)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Verified, false)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(parent_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed");

    let child_info = result
        .0
        .verification_child
        .expect("archived child must still produce verification_child block");

    assert_eq!(child_info.latest_child_session_id, child_id_str);
    assert!(
        child_info.latest_child_archived,
        "archived child must set latest_child_archived=true"
    );
    assert!(
        child_info.active_child_session_id.is_none(),
        "archived child → active_child_session_id must be None"
    );
}

/// Last orchestrator message is surfaced in last_assistant_message
#[tokio::test]
async fn test_get_plan_verification_child_last_orchestrator_message() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Seed an orchestrator message in the child session
    let msg =
        ChatMessage::orchestrator_in_session(child_id.clone(), "Verification round 1 complete.");
    state.app_state.chat_message_repo.create(msg).await.unwrap();

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification(
        State(state),
        unrestricted_scope(),
        Path(parent_id.as_str().to_string()),
        default_verification_query(),
    )
    .await
    .expect("handler must succeed");

    let child_info = result
        .0
        .verification_child
        .expect("child with message must produce verification_child block");

    assert_eq!(
        child_info.last_assistant_message.as_deref(),
        Some("Verification round 1 complete."),
        "last orchestrator message must be surfaced"
    );
    assert!(
        child_info.last_assistant_message_at.is_some(),
        "last_assistant_message_at must be populated when message exists"
    );
}

// ── PDM-335 regression tests: report_verification_round idempotency ──────────

/// PDM-335 regression 1: parent already in Reviewing → report_verification_round with
/// status=reviewing succeeds (HTTP 200) and round data persists.
///
/// Before the fix, `(Reviewing, Reviewing)` had no match arm → 422 rejection.
/// After the fix, the arm is added and the call is idempotent.
#[tokio::test]
async fn test_reviewing_parent_report_round_succeeds() {
    let state = setup_test_state().await;

    // Create a session already in Reviewing state (simulates mid-verification)
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    let session_id_obj = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Call report_verification_round (status=reviewing, in_progress=true) — must succeed
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "Reviewing → Reviewing must succeed (idempotent in-progress path): {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(resp.status, "reviewing");
    assert!(resp.in_progress);

    // Round data must persist
    let refreshed = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .expect("session must still exist");
    assert_eq!(
        refreshed.verification_status,
        VerificationStatus::Reviewing,
        "session must remain in Reviewing after idempotent round report"
    );
    assert!(
        refreshed.verification_in_progress,
        "verification must remain in_progress after idempotent round report"
    );
}

#[tokio::test]
async fn test_reviewing_round_start_without_gaps_persists_current_round_in_native_snapshot() {
    let state = setup_test_state().await;

    let mut session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    session.verification_in_progress = true;
    let session_id = session.id.as_str().to_string();
    let session_id_obj = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(3),
            gaps: None,
            convergence_reason: None,
            max_rounds: Some(5),
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "round-start persistence without gaps must succeed: {:?}",
        result.err()
    );
    let response = result.unwrap().0;
    assert_eq!(response.status, "reviewing");
    assert!(response.in_progress);
    assert_eq!(response.current_round, Some(3));
    assert_eq!(response.max_rounds, Some(5));

    let snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, 0)
        .await
        .unwrap()
        .expect("native verification snapshot must exist");
    assert_eq!(snapshot.status, VerificationStatus::Reviewing);
    assert!(snapshot.in_progress);
    assert_eq!(snapshot.current_round, 3);
    assert_eq!(snapshot.max_rounds, 5);
    assert!(
        snapshot.rounds.is_empty(),
        "starting a round without reported gaps must not fabricate a completed round snapshot"
    );
}

/// PDM-335 regression 2: two consecutive report_verification_round calls while parent
/// stays in Reviewing both succeed (idempotent repeated rounds).
#[tokio::test]
async fn test_repeated_reviewing_reports_idempotent() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // First call — round 1 report
    let first = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        first.is_ok(),
        "First Reviewing → Reviewing call must succeed: {:?}",
        first.err()
    );
    assert_eq!(first.unwrap().0.status, "reviewing");

    // Second call — round 2 report while parent remains in Reviewing
    let second = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        second.is_ok(),
        "Second consecutive Reviewing → Reviewing call must also succeed (idempotent): {:?}",
        second.err()
    );
    let resp2 = second.unwrap().0;
    assert_eq!(
        resp2.status, "reviewing",
        "session must remain in Reviewing after two round reports"
    );
    assert!(resp2.in_progress, "in_progress must remain true");
}

/// PDM-335 regression 2b: retrying the same round replaces the existing round snapshot
/// instead of persisting a duplicate round number.
#[tokio::test]
async fn test_same_round_report_retry_replaces_existing_snapshot() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let session_id = session.id.as_str().to_string();
    let session_id_obj = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let first = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "First round gap".to_string(),
                why_it_matters: Some("Needs coverage".to_string()),
                source: Some("completeness".to_string()),
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;
    assert!(
        first.is_ok(),
        "initial round report must succeed: {:?}",
        first.err()
    );

    let retry = post_verification_status(
        State(state.clone()),
        Path(session_id.clone()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: Some(vec![VerificationGapRequest {
                severity: "medium".to_string(),
                category: "testing".to_string(),
                description: "Updated round gap".to_string(),
                why_it_matters: Some("Retry payload should replace".to_string()),
                source: Some("feasibility".to_string()),
            }]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        retry.is_ok(),
        "same-round retry must replace the persisted round snapshot instead of failing: {:?}",
        retry.err()
    );

    let snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id_obj, 0)
        .await
        .unwrap()
        .expect("native verification snapshot must exist");

    assert_eq!(
        snapshot.rounds.len(),
        1,
        "same-round retries must not persist duplicate round numbers"
    );
    assert_eq!(snapshot.rounds[0].round, 1);
    assert_eq!(snapshot.rounds[0].gap_score, 1);
    assert_eq!(snapshot.rounds[0].gaps[0].description, "Updated round gap");
    assert_eq!(snapshot.current_gaps[0].description, "Updated round gap");
}

/// PDM-335 regression 3: generation mismatch still returns 409 CONFLICT.
///
/// The new idempotent arm must not bypass the generation guard — zombie protection
/// continues to work for in-progress round reports.
#[tokio::test]
async fn test_generation_mismatch_still_fails() {
    let state = setup_test_state().await;

    // Session in Reviewing with generation=3
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(3)
        .build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Send stale generation=1 → must return 409 CONFLICT even with Reviewing → Reviewing arm
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: Some(1), // Stale — current is 3
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "stale generation must still be rejected with 409"
    );
    let (status, _body) = result.unwrap_err();
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "generation mismatch must return 409 CONFLICT regardless of Reviewing → Reviewing arm"
    );
}

/// PDM-335 regression 4: terminal complete_plan_verification (in_progress=false) still works.
///
/// The idempotency fix must not affect terminal transitions: verified/needs_revision/skipped
/// still apply and complete the verification loop correctly.
#[tokio::test]
async fn test_terminal_complete_still_works() {
    let state = setup_test_state().await;

    // Session in Reviewing with generation=1
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::Reviewing)
        .verification_generation(1)
        .build();
    let session_id = session.id.as_str().to_string();
    let session_id_obj = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Terminal call: in_progress=false, status=needs_revision, convergence_reason provided.
    // max_rounds is None to avoid triggering the max_rounds convergence condition
    // (which would auto-promote needs_revision → verified when round == max_rounds).
    let result = post_verification_status(
        State(state.clone()),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(3),
            gaps: None,
            convergence_reason: Some("max_rounds".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: Some(1),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "terminal complete_plan_verification must still succeed: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(
        resp.status, "needs_revision",
        "terminal call must transition to needs_revision"
    );
    assert!(
        !resp.in_progress,
        "terminal call must set in_progress=false"
    );

    let refreshed = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .expect("session must still exist");
    assert_eq!(
        refreshed.verification_status,
        VerificationStatus::NeedsRevision,
        "session must be NeedsRevision after terminal completion"
    );
    assert!(
        !refreshed.verification_in_progress,
        "verification must not be in_progress after terminal completion"
    );
}

/// Terminal completion must be idempotent when the backend already reached needs_revision.
#[tokio::test]
async fn test_terminal_needs_revision_completion_is_idempotent() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .verification_status(VerificationStatus::NeedsRevision)
        .verification_generation(9)
        .build();
    let session_id = session.id.as_str().to_string();
    let session_id_obj = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = post_verification_status(
        State(state.clone()),
        Path(session_id),
        Json(UpdateVerificationRequest {
            status: "needs_revision".to_string(),
            in_progress: false,
            round: Some(1),
            gaps: None,
            convergence_reason: Some("escalated_to_parent".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: Some(9),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "terminal needs_revision -> needs_revision should be idempotent: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(resp.status, "needs_revision");
    assert!(!resp.in_progress);

    let refreshed = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed.verification_status,
        VerificationStatus::NeedsRevision
    );
    assert!(!refreshed.verification_in_progress);
}

/// PDM-335 regression 5: passing a child verification session ID is correctly remapped
/// to the parent, and the round report succeeds even when parent is already in Reviewing.
///
/// Combines the child-session remap with the new idempotent Reviewing → Reviewing path.
#[tokio::test]
async fn test_child_session_remap_still_works() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();

    // Parent session in Reviewing state
    let parent = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .verification_status(VerificationStatus::Reviewing)
        .build();
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Child verification session linked to the parent
    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Call with child session ID — backend must remap to parent and succeed
    // Parent is already in Reviewing, so this exercises the new idempotent arm.
    let result = post_verification_status(
        State(state.clone()),
        Path(child_id.as_str().to_string()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(2),
            gaps: Some(vec![]),
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "child session remap + Reviewing → Reviewing must succeed: {:?}",
        result.err()
    );
    let resp = result.unwrap().0;
    assert_eq!(
        resp.session_id,
        parent_id.as_str(),
        "response must carry the canonical parent session_id after remap"
    );
    assert_eq!(resp.status, "reviewing");
    assert!(resp.in_progress);

    // Parent session must reflect the update
    let refreshed_parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .expect("parent must still exist");
    assert_eq!(
        refreshed_parent.verification_status,
        VerificationStatus::Reviewing,
        "parent session must stay in Reviewing after child-remapped round report"
    );
    assert!(
        refreshed_parent.verification_in_progress,
        "parent session must be in_progress after child-remapped round report"
    );
}
