//! Performance test suite for plan selector with 100+ plans
//!
//! Tests verify that:
//! - Ranking algorithm computations complete in < 1ms per plan
//! - Database queries with 150+ sessions complete in < 200ms
//! - Query complexity scales linearly O(n) not O(n²)
//! - Memory usage remains reasonable with large datasets

use chrono::{DateTime, Duration, TimeZone, Utc};
use ralphx_lib::application::plan_ranking::{
    compute_final_score, compute_final_score_with_breakdown,
};
use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    IdeationSession, IdeationSessionId, IdeationSessionStatus, InternalStatus, ProjectId,
    SelectionSource, Task, TaskCategory, TaskId,
};
use std::time::Instant;

// ============================================================================
// Test Fixtures
// ============================================================================

/// Create a test AppState with in-memory repositories
fn create_test_state() -> AppState {
    AppState::new_test()
}

/// Generate a fixed timestamp for deterministic tests
fn base_timestamp() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 2, 1, 12, 0, 0).unwrap()
}

/// Create accepted ideation session with specified properties
async fn create_accepted_session(
    state: &AppState,
    project_id: &ProjectId,
    title: &str,
    converted_at: DateTime<Utc>,
) -> IdeationSessionId {
    let session_id = IdeationSessionId::new();
    let session = IdeationSession {
        id: session_id.clone(),
        project_id: project_id.clone(),
        title: Some(title.to_string()),
        status: IdeationSessionStatus::Accepted,
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: converted_at - Duration::days(1),
        updated_at: converted_at,
        archived_at: None,
        converted_at: Some(converted_at),
        title_source: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: false,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: Default::default(),
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    };

    state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");
    session_id
}

/// Create task with specified properties
async fn create_test_task(
    state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    status: InternalStatus,
) -> TaskId {
    let task_id = TaskId::new();
    let task = Task {
        id: task_id.clone(),
        project_id: project_id.clone(),
        category: TaskCategory::Regular,
        title: "Test Task".to_string(),
        description: Some("Test description".to_string()),
        priority: 50,
        internal_status: status,
        needs_review_point: false,
        source_proposal_id: None,
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        ideation_session_id: Some(session_id.clone()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        started_at: None,
        completed_at: None,
        archived_at: None,
        blocked_reason: None,
        task_branch: None,
        task_branch_base_ref: None,
        task_branch_base_sha: None,
        worktree_path: None,
        merge_commit_sha: None,
        metadata: None,
        execution_plan_id: None,
        merge_pipeline_active: None,
    };

    state
        .task_repo
        .create(task)
        .await
        .expect("Failed to create task");
    task_id
}

/// Record plan selection interaction
async fn record_selection(
    state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    count: u32,
    selected_at: DateTime<Utc>,
    source: SelectionSource,
) {
    for _ in 0..count {
        state
            .plan_selection_stats_repo
            .record_selection(project_id, session_id, source, selected_at)
            .await
            .expect("Failed to record selection");
    }
}

/// Struct to hold candidate data for sorting tests
#[derive(Debug, Clone, serde::Serialize)]
struct PlanCandidate {
    session_id: IdeationSessionId,
    title: String,
    accepted_at: DateTime<Utc>,
    task_total: u32,
    task_incomplete: u32,
    task_active: u32,
    selected_count: u32,
    last_selected_at: Option<DateTime<Utc>>,
    score: f64,
}

/// Create a large test fixture with 150 accepted sessions
async fn create_large_fixture(state: &AppState, project_id: &ProjectId) -> Vec<PlanCandidate> {
    let base = base_timestamp();
    let mut candidates = Vec::new();

    for i in 0..150 {
        // Vary converted_at across 60 days
        let days_ago = (i * 60) / 150;
        let converted_at = base - Duration::days(days_ago as i64);

        let session_id = create_accepted_session(
            state,
            project_id,
            &format!("Plan {}: Test Feature", i + 1),
            converted_at,
        )
        .await;

        // Vary task counts: 0-20 tasks per session
        let task_total = (i % 21) as u32;
        let incomplete_ratio = 0.3 + (i as f64 * 0.7 / 150.0); // 30% to 100%
        let task_incomplete = (task_total as f64 * incomplete_ratio) as u32;
        let task_active = if i % 5 == 0 { 1 } else { 0 }; // 20% have active tasks

        for j in 0..task_total {
            let status = if j < task_active {
                InternalStatus::Executing
            } else if j < task_incomplete {
                InternalStatus::Backlog
            } else {
                InternalStatus::Approved
            };

            create_test_task(state, project_id, &session_id, status).await;
        }

        // Vary selection counts: 0-30 selections
        let selected_count = (i % 31) as u32;
        let last_selected_at = if selected_count > 0 {
            // Last selection varies from 1-42 days ago
            let last_selection_days = 1 + (i % 42);
            let timestamp = base - Duration::days(last_selection_days as i64);

            record_selection(
                state,
                project_id,
                &session_id,
                selected_count,
                timestamp,
                SelectionSource::QuickSwitcher,
            )
            .await;

            Some(timestamp)
        } else {
            None
        };

        // Compute score using ranking algorithm
        let score = compute_final_score(
            selected_count,
            last_selected_at,
            task_active,
            task_incomplete,
            task_total,
            converted_at,
            base,
        );

        candidates.push(PlanCandidate {
            session_id,
            title: format!("Plan {}: Test Feature", i + 1),
            accepted_at: converted_at,
            task_total,
            task_incomplete,
            task_active,
            selected_count,
            last_selected_at,
            score,
        });
    }

    candidates
}

// ============================================================================
// Performance Tests
// ============================================================================

#[tokio::test]
async fn test_ranking_algorithm_performance_150_plans() {
    let base = base_timestamp();

    // Create 150 mock candidate data points
    let mut test_data = Vec::new();
    for i in 0..150 {
        let days_ago = (i * 60) / 150;
        let converted_at = base - Duration::days(days_ago);
        let selected_count = (i % 31) as u32;
        let last_selected_at = if selected_count > 0 {
            Some(base - Duration::days(1 + (i % 42)))
        } else {
            None
        };
        let task_total = (i % 21) as u32;
        let task_incomplete = ((task_total as f64) * 0.6) as u32;
        let task_active = if i % 5 == 0 { 1 } else { 0 };

        test_data.push((
            selected_count,
            last_selected_at,
            task_active,
            task_incomplete,
            task_total,
            converted_at,
        ));
    }

    // Benchmark ranking computation
    let start = Instant::now();
    let mut scores = Vec::new();
    for data in &test_data {
        let score = compute_final_score(data.0, data.1, data.2, data.3, data.4, data.5, base);
        scores.push(score);
    }
    let duration = start.elapsed();

    println!(
        "Computed scores for {} plans in {} µs ({} µs per plan)",
        150,
        duration.as_micros(),
        duration.as_micros() / 150
    );

    // Verify performance target: < 1ms per plan on average
    let micros_per_plan = duration.as_micros() / 150;
    assert!(
        micros_per_plan < 1000,
        "Ranking took {} µs per plan, target is < 1000µs",
        micros_per_plan
    );

    // Verify all scores are in valid range
    for score in scores {
        assert!((0.0..=1.0).contains(&score), "Score should be in [0, 1]");
    }
}

#[tokio::test]
async fn test_database_query_performance_150_sessions() {
    let state = create_test_state();
    let project_id = ProjectId::new();

    // Create 150 sessions with varied properties
    let _candidates = create_large_fixture(&state, &project_id).await;

    // Benchmark querying all sessions
    let start = Instant::now();
    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id)
        .await
        .expect("Query failed");
    let duration = start.elapsed();

    println!(
        "Queried {} sessions in {} ms",
        sessions.len(),
        duration.as_millis()
    );

    // Verify performance target
    assert!(
        duration.as_millis() < 200,
        "Session query took {} ms, target is < 200ms",
        duration.as_millis()
    );

    // Verify we got all sessions
    assert_eq!(sessions.len(), 150, "Should return all 150 sessions");
}

#[tokio::test]
async fn test_query_scales_linearly() {
    let state = create_test_state();
    let project_id = ProjectId::new();

    // Test with different dataset sizes
    let sizes = vec![50, 100, 150];
    let mut times = Vec::new();

    let base = base_timestamp();
    let mut created_count = 0;

    for &size in &sizes {
        // Create additional sessions to reach target size
        while created_count < size {
            let converted_at = base - Duration::days((created_count / 3) as i64);
            create_accepted_session(
                &state,
                &project_id,
                &format!("Plan {}", created_count + 1),
                converted_at,
            )
            .await;
            created_count += 1;
        }

        // Benchmark query
        let start = Instant::now();
        let sessions = state
            .ideation_session_repo
            .get_by_project(&project_id)
            .await
            .expect("Query failed");
        let duration = start.elapsed();

        assert_eq!(sessions.len(), size, "Should return {} sessions", size);
        times.push(duration.as_millis());
        println!("Size {}: {} ms", size, duration.as_millis());
    }

    // Verify roughly linear scaling (allow 2.5x margin for variance)
    if times[0] > 0 {
        let ratio_100_50 = times[1] as f64 / times[0] as f64;
        assert!(
            ratio_100_50 < 3.0,
            "100 vs 50: ratio {} suggests worse than O(n) scaling",
            ratio_100_50
        );
    }

    if times[1] > 0 {
        let ratio_150_100 = times[2] as f64 / times[1] as f64;
        assert!(
            ratio_150_100 < 2.5,
            "150 vs 100: ratio {} suggests worse than O(n) scaling",
            ratio_150_100
        );
    }
}

#[tokio::test]
async fn test_ranking_correctness_at_scale() {
    let state = create_test_state();
    let project_id = ProjectId::new();
    let base = base_timestamp();

    // Create sessions with known ranking order:
    // 1. Recent + high interaction + active tasks (should rank highest)
    let session_high = create_accepted_session(
        &state,
        &project_id,
        "High Priority Plan",
        base - Duration::days(5),
    )
    .await;
    for _ in 0..10 {
        create_test_task(
            &state,
            &project_id,
            &session_high,
            InternalStatus::Executing,
        )
        .await;
    }
    record_selection(
        &state,
        &project_id,
        &session_high,
        20,
        base - Duration::days(2),
        SelectionSource::QuickSwitcher,
    )
    .await;

    // 2. Old + no interaction (should rank lowest)
    let session_low =
        create_accepted_session(&state, &project_id, "Old Plan", base - Duration::days(60)).await;
    create_test_task(&state, &project_id, &session_low, InternalStatus::Approved).await;

    // 3. Middle ground: recent but no interaction
    let session_mid = create_accepted_session(
        &state,
        &project_id,
        "Recent No Interaction",
        base - Duration::days(10),
    )
    .await;
    create_test_task(&state, &project_id, &session_mid, InternalStatus::Backlog).await;

    // Add 147 more sessions to reach 150
    for i in 0..147 {
        create_accepted_session(
            &state,
            &project_id,
            &format!("Filler Plan {}", i),
            base - Duration::days((i / 3) as i64),
        )
        .await;
    }

    // Query all sessions and compute scores
    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id)
        .await
        .expect("Query failed");

    let mut scored_sessions = Vec::new();
    for session in sessions {
        // Get task stats
        let tasks = state
            .task_repo
            .get_by_ideation_session(&session.id)
            .await
            .expect("Failed to get tasks");

        let task_total = tasks.len() as u32;
        let task_incomplete = tasks
            .iter()
            .filter(|t| t.internal_status != InternalStatus::Approved)
            .count() as u32;
        let task_active = tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.internal_status,
                    InternalStatus::Executing
                        | InternalStatus::QaRefining
                        | InternalStatus::QaTesting
                        | InternalStatus::PendingReview
                        | InternalStatus::Reviewing
                )
            })
            .count() as u32;

        // Get selection stats
        let stats = state
            .plan_selection_stats_repo
            .get_stats(&project_id, &session.id)
            .await
            .expect("Failed to get stats");

        let (selected_count, last_selected_at) = if let Some(s) = stats {
            (s.selected_count, s.last_selected_at)
        } else {
            (0, None)
        };

        let score = compute_final_score(
            selected_count,
            last_selected_at,
            task_active,
            task_incomplete,
            task_total,
            session.converted_at.unwrap_or(session.created_at),
            base,
        );

        scored_sessions.push((session.id.clone(), score));
    }

    // Sort by score descending
    scored_sessions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Find positions of our test sessions
    let pos_high = scored_sessions
        .iter()
        .position(|(id, _)| *id == session_high)
        .expect("High priority session should be in results");
    let pos_mid = scored_sessions
        .iter()
        .position(|(id, _)| *id == session_mid)
        .expect("Mid priority session should be in results");
    let pos_low = scored_sessions
        .iter()
        .position(|(id, _)| *id == session_low)
        .expect("Low priority session should be in results");

    println!(
        "Rankings: high={}, mid={}, low={}",
        pos_high, pos_mid, pos_low
    );

    // Verify ranking order
    assert!(
        pos_high < pos_mid,
        "High priority session (pos {}) should rank higher than mid (pos {})",
        pos_high,
        pos_mid
    );
    assert!(
        pos_mid < pos_low,
        "Mid priority session (pos {}) should rank higher than low (pos {})",
        pos_mid,
        pos_low
    );

    // High priority should be in top 20 (allowing for some variance)
    assert!(
        pos_high < 20,
        "High priority session should be in top 20, was at position {}",
        pos_high
    );
}

#[tokio::test]
async fn test_score_breakdown_with_150_plans() {
    let base = base_timestamp();

    // Create 150 test cases
    let mut breakdowns = Vec::new();
    for i in 0..150 {
        let days_ago = (i * 60) / 150;
        let converted_at = base - Duration::days(days_ago);
        let selected_count = (i % 31) as u32;
        let last_selected_at = if selected_count > 0 {
            Some(base - Duration::days(1 + (i % 42)))
        } else {
            None
        };
        let task_total = (i % 21) as u32;
        let task_incomplete = ((task_total as f64) * 0.6) as u32;
        let task_active = if i % 5 == 0 { 1 } else { 0 };

        let breakdown = compute_final_score_with_breakdown(
            selected_count,
            last_selected_at,
            task_active,
            task_incomplete,
            task_total,
            converted_at,
            base,
        );
        breakdowns.push(breakdown);
    }

    // Verify all breakdowns have consistent weights
    for breakdown in &breakdowns {
        // Weights: 45% interaction + 35% activity + 20% recency
        let expected = 0.45 * breakdown.interaction_score
            + 0.35 * breakdown.activity_score
            + 0.20 * breakdown.recency_score;
        assert!(
            (breakdown.final_score - expected).abs() < 0.001,
            "Score breakdown should match weighted sum"
        );
    }
}

#[tokio::test]
async fn test_memory_usage_reasonable() {
    let state = create_test_state();
    let project_id = ProjectId::new();

    // Create 150 sessions with full fixture data
    let candidates = create_large_fixture(&state, &project_id).await;

    // Verify we can serialize candidate data without issues
    let json = serde_json::to_string(&candidates).expect("Should serialize");

    // JSON size should be reasonable (< 1MB for 150 plans)
    let json_size_kb = json.len() / 1024;
    println!("JSON size for 150 candidates: {} KB", json_size_kb);

    assert!(
        json_size_kb < 1024,
        "Serialized candidate list should be < 1MB, got {} KB",
        json_size_kb
    );

    // Verify each candidate has expected fields populated
    for candidate in &candidates {
        assert!(!candidate.session_id.as_str().is_empty());
        assert!(!candidate.title.is_empty());
        assert!(candidate.score >= 0.0 && candidate.score <= 1.0);
        assert!(candidate.task_total < 1000); // Sanity check
    }
}
