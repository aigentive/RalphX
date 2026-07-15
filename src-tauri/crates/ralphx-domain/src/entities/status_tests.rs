use super::*;
use std::str::FromStr;

// ===== All 28 Variants Exist Tests =====

#[test]
fn internal_status_has_28_variants() {
    assert_eq!(InternalStatus::all_variants().len(), 28);
}

#[test]
fn all_variants_returns_correct_statuses() {
    use InternalStatus::*;
    let expected = vec![
        Backlog,
        Ready,
        Blocked,
        Executing,
        QaRefining,
        QaTesting,
        QaPassed,
        QaFailed,
        PendingReview,
        Reviewing,
        ReviewPassed,
        Escalated,
        RevisionNeeded,
        ReExecuting,
        Approved,
        PendingMerge,
        Merging,
        WaitingOnPr,
        UpdatingPlanBranch,
        UpdatingTaskBranch,
        BranchUpdateBlocked,
        MergeIncomplete,
        MergeConflict,
        Merged,
        Failed,
        Cancelled,
        Paused,
        Stopped,
    ];
    assert_eq!(InternalStatus::all_variants(), expected.as_slice());
}

// ===== Serialization Tests (snake_case) =====

#[test]
fn serializes_to_snake_case_backlog() {
    let json = serde_json::to_string(&InternalStatus::Backlog).unwrap();
    assert_eq!(json, "\"backlog\"");
}

#[test]
fn serializes_to_snake_case_qa_refining() {
    let json = serde_json::to_string(&InternalStatus::QaRefining).unwrap();
    assert_eq!(json, "\"qa_refining\"");
}

#[test]
fn serializes_to_snake_case_pending_review() {
    let json = serde_json::to_string(&InternalStatus::PendingReview).unwrap();
    assert_eq!(json, "\"pending_review\"");
}

#[test]
fn serializes_to_snake_case_revision_needed() {
    let json = serde_json::to_string(&InternalStatus::RevisionNeeded).unwrap();
    assert_eq!(json, "\"revision_needed\"");
}

#[test]
fn serializes_to_snake_case_escalated() {
    let json = serde_json::to_string(&InternalStatus::Escalated).unwrap();
    assert_eq!(json, "\"escalated\"");
}

#[test]
fn all_variants_serialize_correctly() {
    let expected_serializations = vec![
        ("backlog", InternalStatus::Backlog),
        ("ready", InternalStatus::Ready),
        ("blocked", InternalStatus::Blocked),
        ("executing", InternalStatus::Executing),
        ("qa_refining", InternalStatus::QaRefining),
        ("qa_testing", InternalStatus::QaTesting),
        ("qa_passed", InternalStatus::QaPassed),
        ("qa_failed", InternalStatus::QaFailed),
        ("pending_review", InternalStatus::PendingReview),
        ("reviewing", InternalStatus::Reviewing),
        ("review_passed", InternalStatus::ReviewPassed),
        ("escalated", InternalStatus::Escalated),
        ("revision_needed", InternalStatus::RevisionNeeded),
        ("re_executing", InternalStatus::ReExecuting),
        ("approved", InternalStatus::Approved),
        ("pending_merge", InternalStatus::PendingMerge),
        ("merging", InternalStatus::Merging),
        ("waiting_on_pr", InternalStatus::WaitingOnPr),
        ("updating_plan_branch", InternalStatus::UpdatingPlanBranch),
        ("updating_task_branch", InternalStatus::UpdatingTaskBranch),
        ("branch_update_blocked", InternalStatus::BranchUpdateBlocked),
        ("merge_incomplete", InternalStatus::MergeIncomplete),
        ("merge_conflict", InternalStatus::MergeConflict),
        ("merged", InternalStatus::Merged),
        ("failed", InternalStatus::Failed),
        ("cancelled", InternalStatus::Cancelled),
        ("paused", InternalStatus::Paused),
        ("stopped", InternalStatus::Stopped),
    ];

    for (expected_str, status) in expected_serializations {
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(
            json,
            format!("\"{}\"", expected_str),
            "Failed for {:?}",
            status
        );
    }
}

// ===== Deserialization Tests =====

#[test]
fn deserializes_all_variants() {
    for status in InternalStatus::all_variants() {
        let json = format!("\"{}\"", status.as_str());
        let parsed: InternalStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, status);
    }
}

#[test]
fn deserialize_invalid_returns_error() {
    let result: Result<InternalStatus, _> = serde_json::from_str("\"invalid_status\"");
    assert!(result.is_err());
}

// ===== FromStr / Parse Tests =====

#[test]
fn from_str_parses_all_variants() {
    for status in InternalStatus::all_variants() {
        let parsed = InternalStatus::from_str(status.as_str()).unwrap();
        assert_eq!(&parsed, status);
    }
}

#[test]
fn from_str_returns_error_for_invalid() {
    let result = InternalStatus::from_str("not_a_status");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.value, "not_a_status");
}

#[test]
fn from_str_error_displays_correctly() {
    let err = ParseInternalStatusError {
        value: "unknown".to_string(),
    };
    assert_eq!(err.to_string(), "unknown internal status: 'unknown'");
}

// ===== Display / as_str Tests =====

#[test]
fn display_matches_as_str() {
    for status in InternalStatus::all_variants() {
        assert_eq!(format!("{}", status), status.as_str());
    }
}

#[test]
fn as_str_returns_snake_case() {
    assert_eq!(InternalStatus::QaRefining.as_str(), "qa_refining");
    assert_eq!(InternalStatus::PendingReview.as_str(), "pending_review");
}

// ===== Valid Transitions Tests =====

#[test]
fn backlog_transitions() {
    use InternalStatus::*;
    let transitions = Backlog.valid_transitions();
    assert_eq!(transitions, &[Ready, Cancelled]);
}

#[test]
fn ready_transitions() {
    use InternalStatus::*;
    let transitions = Ready.valid_transitions();
    assert_eq!(transitions, &[Executing, Blocked, PendingMerge, Cancelled]);
}

#[test]
fn blocked_transitions() {
    use InternalStatus::*;
    let transitions = Blocked.valid_transitions();
    assert_eq!(transitions, &[Ready, Cancelled]);
}

#[test]
fn executing_transitions() {
    use InternalStatus::*;
    let transitions = Executing.valid_transitions();
    assert_eq!(
        transitions,
        &[
            QaRefining,
            PendingReview,
            UpdatingPlanBranch,
            UpdatingTaskBranch,
            Failed,
            Blocked,
            Stopped,
            Paused
        ]
    );
}

#[test]
fn qa_refining_transitions() {
    use InternalStatus::*;
    let transitions = QaRefining.valid_transitions();
    assert_eq!(transitions, &[QaTesting, Stopped, Paused]);
}

#[test]
fn qa_testing_transitions() {
    use InternalStatus::*;
    let transitions = QaTesting.valid_transitions();
    assert_eq!(transitions, &[QaPassed, QaFailed, Stopped, Paused]);
}

#[test]
fn qa_passed_transitions() {
    use InternalStatus::*;
    let transitions = QaPassed.valid_transitions();
    assert_eq!(transitions, &[PendingReview]);
}

#[test]
fn qa_failed_transitions() {
    use InternalStatus::*;
    let transitions = QaFailed.valid_transitions();
    assert_eq!(transitions, &[RevisionNeeded]);
}

#[test]
fn pending_review_transitions() {
    use InternalStatus::*;
    let transitions = PendingReview.valid_transitions();
    assert_eq!(transitions, &[Reviewing]);
}

#[test]
fn revision_needed_transitions() {
    use InternalStatus::*;
    let transitions = RevisionNeeded.valid_transitions();
    assert_eq!(transitions, &[ReExecuting, Cancelled]);
}

#[test]
fn approved_transitions() {
    use InternalStatus::*;
    let transitions = Approved.valid_transitions();
    assert_eq!(transitions, &[PendingMerge, Ready]); // Merge workflow + Re-open
}

#[test]
fn failed_transitions() {
    use InternalStatus::*;
    let transitions = Failed.valid_transitions();
    assert_eq!(transitions, &[Ready]); // Re-open
}

#[test]
fn cancelled_transitions() {
    use InternalStatus::*;
    let transitions = Cancelled.valid_transitions();
    assert_eq!(transitions, &[Ready]); // Re-open
}

// ===== can_transition_to Tests =====

#[test]
fn can_transition_to_valid_returns_true() {
    use InternalStatus::*;

    // Backlog -> Ready is valid
    assert!(Backlog.can_transition_to(Ready));

    // Ready -> Executing is valid
    assert!(Ready.can_transition_to(Executing));

    // Executing -> QaRefining is valid
    assert!(Executing.can_transition_to(QaRefining));

    // Executing -> PendingReview is valid
    assert!(Executing.can_transition_to(PendingReview));

    // QaTesting -> QaPassed is valid
    assert!(QaTesting.can_transition_to(QaPassed));

    // PendingReview -> Reviewing is valid
    assert!(PendingReview.can_transition_to(Reviewing));

    // ReviewPassed -> Approved is valid
    assert!(ReviewPassed.can_transition_to(Approved));
}

#[test]
fn can_transition_to_invalid_returns_false() {
    use InternalStatus::*;

    // Backlog -> Executing is NOT valid (must go through Ready first)
    assert!(!Backlog.can_transition_to(Executing));

    // Backlog -> Approved is NOT valid
    assert!(!Backlog.can_transition_to(Approved));

    // Executing -> Approved is NOT valid (must go through review)
    assert!(!Executing.can_transition_to(Approved));

    // QaPassed -> Approved is NOT valid (must go through review)
    assert!(!QaPassed.can_transition_to(Approved));

    // Approved -> Executing is NOT valid (must go through Ready)
    assert!(!Approved.can_transition_to(Executing));
}

#[test]
fn self_transition_is_not_valid() {
    for status in InternalStatus::all_variants() {
        assert!(
            !status.can_transition_to(*status),
            "Self-transition should not be valid for {:?}",
            status
        );
    }
}

#[test]
fn terminal_states_can_only_reopen() {
    use InternalStatus::*;

    // Terminal states can only go to Ready (re-open)
    // Note: Approved now also transitions to PendingMerge, so it's not purely terminal
    // Note: Stopped is also terminal (requires manual restart)
    for terminal in &[Merged, Failed, Cancelled, Stopped] {
        assert!(terminal.can_transition_to(Ready));

        // But can't go anywhere else
        for target in InternalStatus::all_variants() {
            if *target != Ready {
                assert!(
                    !terminal.can_transition_to(*target),
                    "{:?} should not transition to {:?}",
                    terminal,
                    target
                );
            }
        }
    }
}

// ===== Happy Path Flow Tests =====

#[test]
fn happy_path_without_qa() {
    use InternalStatus::*;

    // Backlog -> Ready -> Executing -> PendingReview -> Reviewing -> ReviewPassed -> Approved
    assert!(Backlog.can_transition_to(Ready));
    assert!(Ready.can_transition_to(Executing));
    assert!(Executing.can_transition_to(PendingReview));
    assert!(PendingReview.can_transition_to(Reviewing));
    assert!(Reviewing.can_transition_to(ReviewPassed));
    assert!(ReviewPassed.can_transition_to(Approved));
}

#[test]
fn happy_path_with_qa() {
    use InternalStatus::*;

    // Backlog -> Ready -> Executing -> QaRefining ->
    // QaTesting -> QaPassed -> PendingReview -> Reviewing -> ReviewPassed -> Approved
    assert!(Backlog.can_transition_to(Ready));
    assert!(Ready.can_transition_to(Executing));
    assert!(Executing.can_transition_to(QaRefining));
    assert!(QaRefining.can_transition_to(QaTesting));
    assert!(QaTesting.can_transition_to(QaPassed));
    assert!(QaPassed.can_transition_to(PendingReview));
    assert!(PendingReview.can_transition_to(Reviewing));
    assert!(Reviewing.can_transition_to(ReviewPassed));
    assert!(ReviewPassed.can_transition_to(Approved));
}

#[test]
fn qa_failure_retry_path() {
    use InternalStatus::*;

    // QaTesting -> QaFailed -> RevisionNeeded -> ReExecuting
    assert!(QaTesting.can_transition_to(QaFailed));
    assert!(QaFailed.can_transition_to(RevisionNeeded));
    assert!(RevisionNeeded.can_transition_to(ReExecuting));
}

#[test]
fn review_rejection_path() {
    use InternalStatus::*;

    // PendingReview -> Reviewing -> RevisionNeeded -> ReExecuting
    assert!(PendingReview.can_transition_to(Reviewing));
    assert!(Reviewing.can_transition_to(RevisionNeeded));
    assert!(RevisionNeeded.can_transition_to(ReExecuting));
}

// ===== Review State Transition Tests =====

#[test]
fn pending_review_to_reviewing() {
    use InternalStatus::*;
    assert!(PendingReview.can_transition_to(Reviewing));
}

#[test]
fn reviewing_to_review_passed() {
    use InternalStatus::*;
    assert!(Reviewing.can_transition_to(ReviewPassed));
}

#[test]
fn reviewing_to_revision_needed() {
    use InternalStatus::*;
    assert!(Reviewing.can_transition_to(RevisionNeeded));
}

#[test]
fn review_passed_to_approved() {
    use InternalStatus::*;
    assert!(ReviewPassed.can_transition_to(Approved));
}

#[test]
fn review_passed_to_revision_needed() {
    use InternalStatus::*;
    assert!(ReviewPassed.can_transition_to(RevisionNeeded));
}

// ===== Escalated State Transition Tests =====

#[test]
fn reviewing_to_escalated() {
    use InternalStatus::*;
    assert!(Reviewing.can_transition_to(Escalated));
}

#[test]
fn escalated_transitions() {
    use InternalStatus::*;
    let transitions = Escalated.valid_transitions();
    assert_eq!(transitions, &[Approved, RevisionNeeded, PendingReview]);
}

#[test]
fn escalated_to_approved() {
    use InternalStatus::*;
    assert!(Escalated.can_transition_to(Approved));
}

#[test]
fn escalated_to_revision_needed() {
    use InternalStatus::*;
    assert!(Escalated.can_transition_to(RevisionNeeded));
}

#[test]
fn revision_needed_to_re_executing() {
    use InternalStatus::*;
    assert!(RevisionNeeded.can_transition_to(ReExecuting));
}

#[test]
fn re_executing_to_pending_review() {
    use InternalStatus::*;
    assert!(ReExecuting.can_transition_to(PendingReview));
}

#[test]
fn blocking_and_unblocking_path() {
    use InternalStatus::*;

    // Ready -> Blocked -> Ready
    assert!(Ready.can_transition_to(Blocked));
    assert!(Blocked.can_transition_to(Ready));

    // Can also block during execution
    assert!(Executing.can_transition_to(Blocked));
}

// ===== Merge State Transition Tests =====

#[test]
fn approved_to_pending_merge() {
    use InternalStatus::*;
    assert!(Approved.can_transition_to(PendingMerge));
}

#[test]
fn pending_merge_transitions() {
    use InternalStatus::*;
    let transitions = PendingMerge.valid_transitions();
    assert_eq!(
        transitions,
        &[
            Merged,
            Merging,
            WaitingOnPr,
            UpdatingPlanBranch,
            UpdatingTaskBranch,
            MergeIncomplete,
            Stopped,
            Paused,
            Cancelled
        ]
    );
}

#[test]
fn pending_merge_to_merged() {
    use InternalStatus::*;
    // Programmatic merge success - skips agent
    assert!(PendingMerge.can_transition_to(Merged));
}

#[test]
fn pending_merge_to_merging() {
    use InternalStatus::*;
    // Conflict detected - needs agent
    assert!(PendingMerge.can_transition_to(Merging));
}

#[test]
fn merging_transitions() {
    use InternalStatus::*;
    let transitions = Merging.valid_transitions();
    assert_eq!(
        transitions,
        &[
            Merged,
            WaitingOnPr,
            UpdatingPlanBranch,
            MergeConflict,
            MergeIncomplete,
            Stopped,
            Paused,
            Cancelled
        ]
    );
}

#[test]
fn merging_to_merged() {
    use InternalStatus::*;
    // Agent resolved conflicts
    assert!(Merging.can_transition_to(Merged));
}

#[test]
fn merging_to_merge_conflict() {
    use InternalStatus::*;
    // Agent couldn't resolve - needs manual intervention
    assert!(Merging.can_transition_to(MergeConflict));
}

#[test]
fn merging_to_merge_incomplete() {
    use InternalStatus::*;
    // Non-conflict failure → MergeIncomplete
    assert!(Merging.can_transition_to(MergeIncomplete));
}

#[test]
fn waiting_on_pr_transitions() {
    use InternalStatus::*;
    let transitions = WaitingOnPr.valid_transitions();
    assert_eq!(
        transitions,
        &[
            Merged,
            MergeIncomplete,
            PendingMerge,
            UpdatingPlanBranch,
            Stopped,
            Paused,
            Cancelled
        ]
    );
}

#[test]
fn merge_incomplete_transitions() {
    use InternalStatus::*;
    let transitions = MergeIncomplete.valid_transitions();
    assert_eq!(
        transitions,
        &[
            PendingMerge,
            Merging,
            WaitingOnPr,
            Merged,
            Stopped,
            Paused,
            Cancelled
        ]
    );
}

#[test]
fn merge_incomplete_to_pending_merge() {
    use InternalStatus::*;
    // Retry merge → PendingMerge (re-attempt programmatic merge)
    assert!(MergeIncomplete.can_transition_to(PendingMerge));
}

#[test]
fn merge_incomplete_to_merged() {
    use InternalStatus::*;
    // Manual resolution
    assert!(MergeIncomplete.can_transition_to(Merged));
}

#[test]
fn serializes_to_snake_case_merge_incomplete() {
    let json = serde_json::to_string(&InternalStatus::MergeIncomplete).unwrap();
    assert_eq!(json, "\"merge_incomplete\"");
}

#[test]
fn merge_incomplete_parses_correctly() {
    use InternalStatus::*;
    let parsed = InternalStatus::from_str("merge_incomplete").unwrap();
    assert_eq!(parsed, MergeIncomplete);
}

#[test]
fn serializes_to_snake_case_waiting_on_pr() {
    let json = serde_json::to_string(&InternalStatus::WaitingOnPr).unwrap();
    assert_eq!(json, "\"waiting_on_pr\"");
}

#[test]
fn waiting_on_pr_parses_correctly() {
    use InternalStatus::*;
    let parsed = InternalStatus::from_str("waiting_on_pr").unwrap();
    assert_eq!(parsed, WaitingOnPr);
}

#[test]
fn branch_update_statuses_serialize_and_parse() {
    use InternalStatus::*;

    for (wire, status) in [
        ("updating_plan_branch", UpdatingPlanBranch),
        ("updating_task_branch", UpdatingTaskBranch),
        ("branch_update_blocked", BranchUpdateBlocked),
    ] {
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{wire}\"")
        );
        assert_eq!(InternalStatus::from_str(wire).unwrap(), status);
    }
}

#[test]
fn freshness_origins_can_enter_directional_update_states() {
    use InternalStatus::*;

    for origin in [Executing, ReExecuting, Reviewing, PendingMerge] {
        assert!(origin.can_transition_to(UpdatingPlanBranch));
        assert!(origin.can_transition_to(UpdatingTaskBranch));
    }
    assert!(WaitingOnPr.can_transition_to(UpdatingPlanBranch));
    assert!(Merging.can_transition_to(UpdatingPlanBranch));
}

#[test]
fn branch_update_transitions_preserve_truthful_state_family() {
    use InternalStatus::*;

    assert!(UpdatingPlanBranch.can_transition_to(UpdatingTaskBranch));
    assert!(UpdatingPlanBranch.can_transition_to(BranchUpdateBlocked));
    assert!(UpdatingTaskBranch.can_transition_to(BranchUpdateBlocked));
    assert!(BranchUpdateBlocked.can_transition_to(UpdatingPlanBranch));
    assert!(BranchUpdateBlocked.can_transition_to(UpdatingTaskBranch));

    assert!(!UpdatingPlanBranch.can_transition_to(Merging));
    assert!(!UpdatingTaskBranch.can_transition_to(Merging));
}

#[test]
fn branch_update_continuations_return_to_closed_targets() {
    use InternalStatus::*;

    for update in [UpdatingPlanBranch, UpdatingTaskBranch] {
        for target in [Executing, ReExecuting, Reviewing, PendingMerge, WaitingOnPr] {
            assert!(update.can_transition_to(target), "{update:?} -> {target:?}");
        }
    }

    assert!(UpdatingPlanBranch.can_transition_to(Merged));
    assert!(!UpdatingTaskBranch.can_transition_to(Merged));
}

#[test]
fn pause_resume_supports_active_branch_updates() {
    use InternalStatus::*;

    assert!(UpdatingPlanBranch.can_transition_to(Paused));
    assert!(UpdatingTaskBranch.can_transition_to(Paused));
    assert!(Paused.can_transition_to(UpdatingPlanBranch));
    assert!(Paused.can_transition_to(UpdatingTaskBranch));
}

#[test]
fn merge_conflict_transitions() {
    use InternalStatus::*;
    let transitions = MergeConflict.valid_transitions();
    assert_eq!(
        transitions,
        &[PendingMerge, Merging, Merged, Stopped, Paused, Cancelled]
    );
}

#[test]
fn merge_conflict_to_merged() {
    use InternalStatus::*;
    // User manually resolved
    assert!(MergeConflict.can_transition_to(Merged));
}

#[test]
fn merged_transitions() {
    use InternalStatus::*;
    let transitions = Merged.valid_transitions();
    assert_eq!(transitions, &[Ready]); // Re-open only
}

#[test]
fn merged_to_ready() {
    use InternalStatus::*;
    // Re-open completed task
    assert!(Merged.can_transition_to(Ready));
}

#[test]
fn merge_workflow_happy_path() {
    use InternalStatus::*;
    // Approved -> PendingMerge -> Merged (no conflicts)
    assert!(Approved.can_transition_to(PendingMerge));
    assert!(PendingMerge.can_transition_to(Merged));
}

#[test]
fn merge_workflow_with_agent() {
    use InternalStatus::*;
    // Approved -> PendingMerge -> Merging -> Merged
    assert!(Approved.can_transition_to(PendingMerge));
    assert!(PendingMerge.can_transition_to(Merging));
    assert!(Merging.can_transition_to(Merged));
}

#[test]
fn merge_workflow_manual_resolution() {
    use InternalStatus::*;
    // Approved -> PendingMerge -> Merging -> MergeConflict -> Merged
    assert!(Approved.can_transition_to(PendingMerge));
    assert!(PendingMerge.can_transition_to(Merging));
    assert!(Merging.can_transition_to(MergeConflict));
    assert!(MergeConflict.can_transition_to(Merged));
}

#[test]
fn merge_conflict_retry_workflow() {
    use InternalStatus::*;
    // MergeConflict -> PendingMerge (retry) -> Merging -> Merged
    assert!(MergeConflict.can_transition_to(PendingMerge));
    assert!(MergeConflict.can_transition_to(Merging));
    assert!(PendingMerge.can_transition_to(Merging));
    assert!(Merging.can_transition_to(Merged));
}

#[test]
fn serializes_to_snake_case_pending_merge() {
    let json = serde_json::to_string(&InternalStatus::PendingMerge).unwrap();
    assert_eq!(json, "\"pending_merge\"");
}

#[test]
fn serializes_to_snake_case_merging() {
    let json = serde_json::to_string(&InternalStatus::Merging).unwrap();
    assert_eq!(json, "\"merging\"");
}

#[test]
fn serializes_to_snake_case_merge_conflict() {
    let json = serde_json::to_string(&InternalStatus::MergeConflict).unwrap();
    assert_eq!(json, "\"merge_conflict\"");
}

#[test]
fn serializes_to_snake_case_merged() {
    let json = serde_json::to_string(&InternalStatus::Merged).unwrap();
    assert_eq!(json, "\"merged\"");
}

// ===== Clone, Copy, Eq, Hash Tests =====

#[test]
fn clone_works() {
    let status = InternalStatus::Executing;
    let cloned = status;
    assert_eq!(status, cloned);
}

#[test]
fn copy_works() {
    let status = InternalStatus::Approved;
    let copied = status;
    assert_eq!(status, copied);
}

#[test]
fn hash_works_in_hashset() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(InternalStatus::Ready);
    set.insert(InternalStatus::Executing);
    set.insert(InternalStatus::Ready); // Duplicate

    assert_eq!(set.len(), 2);
    assert!(set.contains(&InternalStatus::Ready));
    assert!(set.contains(&InternalStatus::Executing));
    assert!(!set.contains(&InternalStatus::Approved));
}

#[test]
fn debug_format_works() {
    let debug = format!("{:?}", InternalStatus::Executing);
    assert_eq!(debug, "Executing");
}

#[test]
fn equality_works() {
    assert_eq!(InternalStatus::Ready, InternalStatus::Ready);
    assert_ne!(InternalStatus::Ready, InternalStatus::Executing);
}

// ===== Paused and Stopped State Tests =====

#[test]
fn stopped_is_terminal() {
    use InternalStatus::*;
    // Stopped can only go to Ready (re-open, like other terminals)
    let transitions = Stopped.valid_transitions();
    assert_eq!(transitions, &[Ready]);
}

#[test]
fn stopped_to_ready() {
    use InternalStatus::*;
    assert!(Stopped.can_transition_to(Ready));
}

#[test]
fn paused_can_resume_to_agent_active_states() {
    use InternalStatus::*;
    // Paused can resume to any agent-active state (via status history lookup)
    let transitions = Paused.valid_transitions();
    assert_eq!(
        transitions,
        &[
            Executing,
            ReExecuting,
            QaRefining,
            QaTesting,
            Reviewing,
            Merging,
            WaitingOnPr,
            UpdatingPlanBranch,
            UpdatingTaskBranch
        ]
    );
}

#[test]
fn paused_serializes_correctly() {
    let json = serde_json::to_string(&InternalStatus::Paused).unwrap();
    assert_eq!(json, "\"paused\"");
}

#[test]
fn stopped_serializes_correctly() {
    let json = serde_json::to_string(&InternalStatus::Stopped).unwrap();
    assert_eq!(json, "\"stopped\"");
}

#[test]
fn paused_parses_correctly() {
    use InternalStatus::*;
    let parsed = InternalStatus::from_str("paused").unwrap();
    assert_eq!(parsed, Paused);
}

#[test]
fn stopped_parses_correctly() {
    use InternalStatus::*;
    let parsed = InternalStatus::from_str("stopped").unwrap();
    assert_eq!(parsed, Stopped);
}

// ===== is_terminal Tests =====

#[test]
fn test_is_terminal() {
    use InternalStatus::*;

    // MergeIncomplete is terminal for dependency purposes (requires manual intervention)
    let terminal = [Merged, Failed, Cancelled, Stopped, MergeIncomplete];
    for status in &terminal {
        assert!(status.is_terminal(), "{:?} should be terminal", status);
    }

    let non_terminal: Vec<&InternalStatus> = InternalStatus::all_variants()
        .iter()
        .filter(|s| !terminal.contains(s))
        .collect();
    for status in non_terminal {
        assert!(!status.is_terminal(), "{:?} should NOT be terminal", status);
    }
}

/// Exhaustive test: every InternalStatus variant must explicitly be terminal or non-terminal.
/// This test will fail if new variants are added without updating is_terminal().
#[test]
fn test_is_terminal_covers_all_variants() {
    use InternalStatus::*;
    let terminal = [Merged, Failed, Cancelled, Stopped, MergeIncomplete];
    let non_terminal = [
        Backlog,
        Ready,
        Blocked,
        Executing,
        QaRefining,
        QaTesting,
        QaPassed,
        QaFailed,
        PendingReview,
        Reviewing,
        ReviewPassed,
        Escalated,
        RevisionNeeded,
        ReExecuting,
        Approved,
        PendingMerge,
        Merging,
        WaitingOnPr,
        UpdatingPlanBranch,
        UpdatingTaskBranch,
        BranchUpdateBlocked,
        MergeConflict,
        Paused,
    ];
    // Verify total coverage matches all_variants()
    assert_eq!(
        terminal.len() + non_terminal.len(),
        InternalStatus::all_variants().len(),
        "is_terminal coverage is incomplete — update this test when adding new variants"
    );
    for s in &terminal {
        assert!(s.is_terminal(), "{:?} should be terminal", s);
    }
    for s in &non_terminal {
        assert!(!s.is_terminal(), "{:?} should NOT be terminal", s);
    }
}

// ===== is_dependency_satisfied Tests =====

#[test]
fn test_is_dependency_satisfied() {
    use InternalStatus::*;

    // These statuses satisfy dependencies (unblock dependents)
    let satisfied = [Merged, Cancelled];
    for status in &satisfied {
        assert!(
            status.is_dependency_satisfied(),
            "{:?} should satisfy dependencies",
            status
        );
    }

    // Failed is terminal but does NOT satisfy dependencies
    assert!(
        !Failed.is_dependency_satisfied(),
        "Failed should NOT satisfy dependencies"
    );
    assert!(Failed.is_terminal(), "Failed should still be terminal");

    // Stopped is terminal but does NOT satisfy dependencies —
    // the task was interrupted, not completed
    assert!(
        !Stopped.is_dependency_satisfied(),
        "Stopped should NOT satisfy dependencies"
    );
    assert!(Stopped.is_terminal(), "Stopped should still be terminal");

    // MergeIncomplete is terminal but does NOT satisfy dependencies —
    // the merge failed, code is not on the target branch
    assert!(
        !MergeIncomplete.is_dependency_satisfied(),
        "MergeIncomplete should NOT satisfy dependencies"
    );
    assert!(
        MergeIncomplete.is_terminal(),
        "MergeIncomplete should still be terminal"
    );

    // All non-satisfied statuses should not satisfy dependencies
    let non_satisfied: Vec<&InternalStatus> = InternalStatus::all_variants()
        .iter()
        .filter(|s| !satisfied.contains(s))
        .collect();
    for status in non_satisfied {
        assert!(
            !status.is_dependency_satisfied(),
            "{:?} should NOT satisfy dependencies",
            status
        );
    }
}

/// Exhaustive test: every variant must be explicitly satisfied or not.
#[test]
fn test_is_dependency_satisfied_covers_all_variants() {
    use InternalStatus::*;
    let satisfied = [Merged, Cancelled];
    let not_satisfied = [
        Backlog,
        Ready,
        Blocked,
        Executing,
        QaRefining,
        QaTesting,
        QaPassed,
        QaFailed,
        PendingReview,
        Reviewing,
        ReviewPassed,
        Escalated,
        RevisionNeeded,
        ReExecuting,
        Approved,
        PendingMerge,
        Merging,
        WaitingOnPr,
        UpdatingPlanBranch,
        UpdatingTaskBranch,
        BranchUpdateBlocked,
        MergeConflict,
        MergeIncomplete,
        Paused,
        Failed,
        Stopped,
    ];
    assert_eq!(
        satisfied.len() + not_satisfied.len(),
        InternalStatus::all_variants().len(),
        "is_dependency_satisfied coverage is incomplete — update this test when adding new variants"
    );
    for s in &satisfied {
        assert!(s.is_dependency_satisfied(), "{:?} should satisfy", s);
    }
    for s in &not_satisfied {
        assert!(!s.is_dependency_satisfied(), "{:?} should NOT satisfy", s);
    }
}

#[test]
fn test_is_active_dependency_blocker_is_inverse_of_dependency_satisfied() {
    for status in InternalStatus::all_variants() {
        assert_eq!(
            status.is_active_dependency_blocker(),
            !status.is_dependency_satisfied(),
            "{:?} must use the shared dependency-satisfied classifier",
            status
        );
    }
}
