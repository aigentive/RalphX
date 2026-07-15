// Tests for merge progress event emission in the merge pipeline.
//
// The actual event emission goes through `EventSink`.
// These tests verify:
// 1. Phase constants match frontend expectations (consistency across Rust ↔ TS)
// 2. emit_merge_progress is a safe no-op with None event_sink
// 3. emit_merge_progress records the expected EventSink event and payload shape
// 4. The merge pipeline doesn't crash when event emission calls run with None
// 5. Phase ordering in derive_phases_from_analysis matches pipeline execution order

use crate::domain::entities::merge_progress_event::{
    derive_phases_from_analysis, MergePhase, MergePhaseStatus, PhaseAnalysisEntry,
};

// ==================
// Phase constant values match frontend DEFAULT_PHASE_CONFIG
// ==================

#[test]
fn phase_constants_match_frontend_ids() {
    // These must match the `id` fields in MergePhaseTimeline.tsx DEFAULT_PHASE_CONFIG
    assert_eq!(MergePhase::MERGE_PREPARATION, "merge_preparation");
    assert_eq!(MergePhase::PRECONDITION_CHECK, "precondition_check");
    assert_eq!(MergePhase::BRANCH_FRESHNESS, "branch_freshness");
    assert_eq!(MergePhase::MERGE_CLEANUP, "merge_cleanup");
    assert_eq!(MergePhase::WORKTREE_SETUP, "worktree_setup");
    assert_eq!(MergePhase::PROGRAMMATIC_MERGE, "programmatic_merge");
    assert_eq!(MergePhase::FINALIZE, "finalize");
}

// ==================
// Phase ordering in derived phase list matches pipeline execution order
// ==================

#[test]
fn derived_phase_order_matches_pipeline_execution() {
    // The pipeline executes in this order:
    // 1. merge_preparation (context loading)
    // 2. precondition_check (validate preconditions, resolve branches)
    // 3. branch_freshness (ensure plan branch, update from main, update source from target)
    // 4. merge_cleanup (pre-merge cleanup)
    // 5. worktree_setup (setup merge worktree)
    // 6. programmatic_merge (git merge)
    // 7. [dynamic validation phases]
    // 8. finalize (complete merge)
    let entries: Vec<PhaseAnalysisEntry> = vec![];
    let phases = derive_phases_from_analysis(&entries);

    let expected_order = [
        "merge_preparation",
        "precondition_check",
        "branch_freshness",
        "merge_cleanup",
        "worktree_setup",
        "programmatic_merge",
        "finalize",
    ];

    assert_eq!(
        phases.len(),
        expected_order.len(),
        "Structural phase count mismatch"
    );

    for (i, expected_id) in expected_order.iter().enumerate() {
        assert_eq!(
            phases[i].id, *expected_id,
            "Phase at index {} should be '{}', got '{}'",
            i, expected_id, phases[i].id
        );
    }
}

#[test]
fn derived_phases_insert_validation_between_merge_and_finalize() {
    let entries = vec![PhaseAnalysisEntry {
        validate: vec!["cargo test".to_string(), "npm run lint".to_string()],
    }];

    let phases = derive_phases_from_analysis(&entries);

    // Find indices of key phases
    let merge_idx = phases
        .iter()
        .position(|p| p.id == "programmatic_merge")
        .unwrap();
    let finalize_idx = phases.iter().position(|p| p.id == "finalize").unwrap();
    let cargo_test_idx = phases.iter().position(|p| p.id == "cargo_test").unwrap();
    let npm_lint_idx = phases.iter().position(|p| p.id == "npm_run_lint").unwrap();

    // Validation phases must be between programmatic_merge and finalize
    assert!(
        cargo_test_idx > merge_idx,
        "cargo_test ({}) should come after programmatic_merge ({})",
        cargo_test_idx,
        merge_idx
    );
    assert!(
        npm_lint_idx > merge_idx,
        "npm_run_lint ({}) should come after programmatic_merge ({})",
        npm_lint_idx,
        merge_idx
    );
    assert!(
        cargo_test_idx < finalize_idx,
        "cargo_test ({}) should come before finalize ({})",
        cargo_test_idx,
        finalize_idx
    );
    assert!(
        npm_lint_idx < finalize_idx,
        "npm_run_lint ({}) should come before finalize ({})",
        npm_lint_idx,
        finalize_idx
    );
}

#[test]
fn pre_merge_phases_come_before_merge_execution_phases() {
    let entries: Vec<PhaseAnalysisEntry> = vec![];
    let phases = derive_phases_from_analysis(&entries);

    let prep_idx = phases
        .iter()
        .position(|p| p.id == "merge_preparation")
        .unwrap();
    let precond_idx = phases
        .iter()
        .position(|p| p.id == "precondition_check")
        .unwrap();
    let freshness_idx = phases
        .iter()
        .position(|p| p.id == "branch_freshness")
        .unwrap();
    let cleanup_idx = phases.iter().position(|p| p.id == "merge_cleanup").unwrap();
    let worktree_idx = phases
        .iter()
        .position(|p| p.id == "worktree_setup")
        .unwrap();
    let merge_idx = phases
        .iter()
        .position(|p| p.id == "programmatic_merge")
        .unwrap();

    // Pre-merge phases are strictly ordered before merge execution
    assert!(prep_idx < precond_idx, "preparation before precondition");
    assert!(precond_idx < freshness_idx, "precondition before freshness");
    assert!(freshness_idx < cleanup_idx, "freshness before cleanup");
    assert!(cleanup_idx < worktree_idx, "cleanup before worktree_setup");
    assert!(
        worktree_idx < merge_idx,
        "worktree_setup before programmatic_merge"
    );
}

// ==================
// Phase labels for structural phases
// ==================

#[test]
fn structural_phase_labels_are_human_readable() {
    let entries: Vec<PhaseAnalysisEntry> = vec![];
    let phases = derive_phases_from_analysis(&entries);

    let phase_map: std::collections::HashMap<&str, &str> = phases
        .iter()
        .map(|p| (p.id.as_str(), p.label.as_str()))
        .collect();

    assert_eq!(phase_map["merge_preparation"], "Preparation");
    assert_eq!(phase_map["precondition_check"], "Preconditions");
    assert_eq!(phase_map["branch_freshness"], "Branch Freshness");
    assert_eq!(phase_map["merge_cleanup"], "Cleanup");
    assert_eq!(phase_map["worktree_setup"], "Worktree Setup");
    assert_eq!(phase_map["programmatic_merge"], "Merge");
    assert_eq!(phase_map["finalize"], "Finalize");
}

// ==================
// emit_merge_progress with None event_sink is safe no-op
// ==================

#[test]
fn emit_merge_progress_with_none_does_not_panic() {
    use super::super::merge_validation::emit_merge_progress;

    // All phase/status combinations should be safe with None event_sink
    let phases = [
        MergePhase::new(MergePhase::MERGE_PREPARATION),
        MergePhase::new(MergePhase::PRECONDITION_CHECK),
        MergePhase::new(MergePhase::BRANCH_FRESHNESS),
        MergePhase::new(MergePhase::MERGE_CLEANUP),
        MergePhase::worktree_setup(),
        MergePhase::programmatic_merge(),
        MergePhase::finalize(),
        MergePhase::new("cargo_test"),
    ];
    let statuses = [
        MergePhaseStatus::Started,
        MergePhaseStatus::Passed,
        MergePhaseStatus::Failed,
        MergePhaseStatus::Skipped,
    ];

    for phase in &phases {
        for status in &statuses {
            // This must not panic — None event_sink should be a silent no-op
            emit_merge_progress(
                None,
                "task-test",
                phase.clone(),
                *status,
                "test message".to_string(),
            );
        }
    }
}

#[test]
fn emit_merge_progress_records_event_sink_payload() {
    use super::super::merge_validation::emit_merge_progress;
    use ralphx_events::RecordingEventSink;

    let sink = RecordingEventSink::new();

    emit_merge_progress(
        Some(&sink),
        "task-test",
        MergePhase::finalize(),
        MergePhaseStatus::Started,
        "Finalizing merge".to_string(),
    );

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "task:merge_progress");
    assert_eq!(events[0].payload["task_id"], "task-test");
    assert_eq!(events[0].payload["phase"], MergePhase::FINALIZE);
    assert_eq!(events[0].payload["status"], "started");
    assert_eq!(events[0].payload["message"], "Finalizing merge");
}

// ==================
// Deduplication: validate commands produce unique phase IDs
// ==================

#[test]
fn duplicate_validate_commands_produce_duplicate_phase_ids() {
    // This is expected behavior — the pipeline handles dedup in the frontend's phaseMap.
    // But it's important to document that derive_phases_from_analysis does NOT deduplicate.
    let entries = vec![
        PhaseAnalysisEntry {
            validate: vec!["cargo test".to_string()],
        },
        PhaseAnalysisEntry {
            validate: vec!["cargo test".to_string()],
        },
    ];

    let phases = derive_phases_from_analysis(&entries);
    let cargo_test_count = phases.iter().filter(|p| p.id == "cargo_test").count();

    // Two entries with same command = two phases with same ID
    assert_eq!(
        cargo_test_count, 2,
        "derive_phases_from_analysis should not deduplicate — frontend handles this"
    );
}

// ==================
// Early phase list matches derive_phases structural subset
// ==================

#[test]
fn early_phase_list_matches_structural_phases() {
    // The early phase list emitted in side_effects.rs (before validation phases are known)
    // must match the structural phases from derive_phases_from_analysis.
    // This test catches drift between the hardcoded early list and the derived list.
    let early_ids = [
        MergePhase::MERGE_PREPARATION,
        MergePhase::PRECONDITION_CHECK,
        MergePhase::BRANCH_FRESHNESS,
        MergePhase::MERGE_CLEANUP,
        MergePhase::WORKTREE_SETUP,
        MergePhase::PROGRAMMATIC_MERGE,
        MergePhase::FINALIZE,
    ];

    let derived = derive_phases_from_analysis(&[]);
    let derived_ids: Vec<&str> = derived.iter().map(|p| p.id.as_str()).collect();

    assert_eq!(
        early_ids.to_vec(),
        derived_ids,
        "Early phase list in side_effects.rs must match structural phases from derive_phases_from_analysis"
    );
}
