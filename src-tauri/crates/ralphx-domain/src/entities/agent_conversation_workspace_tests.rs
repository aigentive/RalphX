use super::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewMonitor,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewRuntimeState, AgentWorkspaceReviewTargetScope, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
};
use chrono::Utc;
use std::str::FromStr;

#[test]
fn workspace_modes_round_trip_tasks_autopilot_and_legacy_ideation() {
    for (value, mode) in [
        ("tasks", AgentConversationWorkspaceMode::Tasks),
        ("autopilot", AgentConversationWorkspaceMode::Autopilot),
        ("ideation", AgentConversationWorkspaceMode::Ideation),
    ] {
        assert_eq!(AgentConversationWorkspaceMode::from_str(value), Ok(mode));
        assert_eq!(mode.to_string(), value);
        assert_eq!(
            serde_json::from_str::<AgentConversationWorkspaceMode>(&format!(r#""{value}""#))
                .expect("mode should deserialize"),
            mode
        );
    }
}

#[test]
fn owned_pr_mutation_eligibility_is_positive_and_shape_aware() {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::new(),
        ProjectId::new(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test".to_string(),
        "/tmp/ralphx-test".to_string(),
    );

    assert!(workspace.allows_owned_pr_mutation());

    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1"));
    assert!(!workspace.allows_owned_pr_mutation());

    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string("session-1"));
    assert!(workspace.allows_owned_pr_mutation());

    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    assert!(!workspace.allows_owned_pr_mutation());

    workspace.mode = AgentConversationWorkspaceMode::Plan;
    assert!(!workspace.allows_owned_pr_mutation());

    workspace.mode = AgentConversationWorkspaceMode::Edit;
    workspace.linked_plan_branch_id = None;
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    assert!(!workspace.allows_owned_pr_mutation());
}

#[test]
fn workspace_review_runtime_states_use_stable_response_values() {
    for (state, value) in [
        (
            AgentWorkspaceReviewRuntimeState::ActiveOwned,
            "active_owned",
        ),
        (AgentWorkspaceReviewRuntimeState::Terminal, "terminal"),
        (
            AgentWorkspaceReviewRuntimeState::MissingRuntimeIdentity,
            "missing_runtime_identity",
        ),
        (
            AgentWorkspaceReviewRuntimeState::MalformedRuntimeIdentity,
            "malformed_runtime_identity",
        ),
        (
            AgentWorkspaceReviewRuntimeState::StaleRuntime,
            "stale_runtime",
        ),
    ] {
        assert_eq!(state.to_string(), value);
    }
}

fn monitor_and_action() -> (AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewAction) {
    let conversation_id = ChatConversationId::new();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id,
        ProjectId("project-1".to_string()),
        42,
        Some("head-1".to_string()),
    );
    let action = AgentWorkspacePrReviewAction::new(
        conversation_id,
        42,
        "head-1".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "review passed".to_string(),
        "Looks good.".to_string(),
        None,
        Some("run-1".to_string()),
    );
    monitor.last_review_run_id = Some("run-1".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1".to_string()));
    monitor.review_artifact_head_sha = Some("head-1".to_string());
    (monitor, action)
}

#[test]
fn auto_approval_defaults_on_but_requires_a_resolved_first_action() {
    let (monitor, action) = monitor_and_action();

    assert!(monitor.auto_approve_enabled);
    assert!(!monitor.first_action_resolved);
    assert!(!monitor.can_auto_approve(&action));
}

#[test]
fn auto_approval_requires_current_approve_artifact_and_run() {
    let (mut monitor, action) = monitor_and_action();
    monitor.first_action_resolved = true;

    assert!(monitor.can_auto_approve(&action));

    monitor.review_artifact_head_sha = Some("other-head".to_string());
    assert!(!monitor.can_auto_approve(&action));

    monitor.review_artifact_head_sha = Some("head-1".to_string());
    monitor.last_review_run_id = Some("other-run".to_string());
    assert!(!monitor.can_auto_approve(&action));

    monitor.last_review_run_id = Some("run-1".to_string());
    monitor.auto_approve_enabled = false;
    assert!(!monitor.can_auto_approve(&action));
}

#[test]
fn workspace_review_auto_merge_guard_status_round_trips_persisted_values() {
    for (status, persisted) in [
        (AgentWorkspaceReviewAutoMergeGuardStatus::Pausing, "pausing"),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
            "paused_for_review",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
            "awaiting_publish",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
            "restoring",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed,
            "restore_failed",
        ),
    ] {
        assert_eq!(status.to_string(), persisted);
        assert_eq!(persisted.parse(), Ok(status));
    }

    assert_eq!(
        "unknown".parse::<AgentWorkspaceReviewAutoMergeGuardStatus>(),
        Err("unknown workspace review auto-merge guard status: 'unknown'".to_string())
    );
}

#[test]
fn blocking_review_bypass_authorizes_only_the_exact_artifact_and_target() {
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        ChatConversationId::new(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-1".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-1".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
    monitor.review_artifact_version = Some(3);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string("changes-1"));
    monitor.review_requested_changes_artifact_version = Some(3);
    monitor.review_gate_bypassed_at = Some(Utc::now());
    monitor.review_gate_bypassed_target_scope =
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.review_gate_bypassed_diff_fingerprint = Some("diff-1".to_string());
    monitor.review_gate_bypassed_artifact_id = Some(ArtifactId::from_string("artifact-1"));
    monitor.review_gate_bypassed_artifact_version = Some(3);

    assert!(monitor.has_current_review_bypass_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        None,
        "diff-1",
    ));
    assert!(monitor.has_current_review_publish_authorization_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        None,
        "diff-1",
    ));
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );

    monitor.review_artifact_version = Some(4);
    assert!(!monitor.has_current_review_bypass_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        None,
        "diff-1",
    ));
}
#[cfg(test)]
mod pr_comment_evidence_tests {
    use super::super::agent_conversation_workspace::{
        pr_comment_body_excerpt, pr_comment_body_sha256,
    };

    #[test]
    fn pr_comment_body_excerpt_respects_tiny_limits() {
        assert_eq!(pr_comment_body_excerpt("abcdef", 0), "");
        assert_eq!(pr_comment_body_excerpt("abcdef", 2), "..");
        assert_eq!(pr_comment_body_excerpt("abcdef", 3), "...");
        assert_eq!(pr_comment_body_excerpt("abcdef", 5), "ab...");
        assert_eq!(pr_comment_body_excerpt("abcdef", 6), "abcdef");
    }

    #[test]
    fn pr_comment_body_sha256_is_stable() {
        assert_eq!(
            pr_comment_body_sha256("coverage"),
            pr_comment_body_sha256("coverage")
        );
        assert_ne!(
            pr_comment_body_sha256("coverage"),
            pr_comment_body_sha256("different")
        );
    }
}

#[cfg(test)]
mod publication_status_helpers_tests {
    use super::super::agent_conversation_workspace::{
        is_pr_status_pollable_push_status, is_terminal_publication_pr_status,
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
    };
    use super::super::{ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranchId, ProjectId};

    fn workspace() -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::new(),
            ProjectId("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "feature/branch".to_string(),
            "/tmp/worktree".to_string(),
        )
    }

    #[test]
    fn terminal_publication_pr_status_matches_merged_and_closed_only() {
        assert!(is_terminal_publication_pr_status(Some("merged")));
        assert!(is_terminal_publication_pr_status(Some("closed")));
        assert!(!is_terminal_publication_pr_status(Some("open")));
        assert!(!is_terminal_publication_pr_status(Some("draft")));
        assert!(!is_terminal_publication_pr_status(None));
    }

    #[test]
    fn pr_status_pollable_push_status_matches_none_pushed_and_refreshed() {
        assert!(is_pr_status_pollable_push_status(None));
        assert!(is_pr_status_pollable_push_status(Some("pushed")));
        assert!(is_pr_status_pollable_push_status(Some("refreshed")));
        assert!(!is_pr_status_pollable_push_status(Some("pending")));
        assert!(!is_pr_status_pollable_push_status(Some("failed")));
    }

    #[test]
    fn has_terminal_publication_pr_status_delegates() {
        let mut ws = workspace();
        ws.publication_pr_status = Some("merged".to_string());
        assert!(ws.has_terminal_publication_pr_status());
        ws.publication_pr_status = Some("open".to_string());
        assert!(!ws.has_terminal_publication_pr_status());
        ws.publication_pr_status = None;
        assert!(!ws.has_terminal_publication_pr_status());
    }

    #[test]
    fn has_pr_status_pollable_push_status_delegates() {
        let mut ws = workspace();
        ws.publication_push_status = None;
        assert!(ws.has_pr_status_pollable_push_status());
        ws.publication_push_status = Some("pushed".to_string());
        assert!(ws.has_pr_status_pollable_push_status());
        ws.publication_push_status = Some("failed".to_string());
        assert!(!ws.has_pr_status_pollable_push_status());
    }

    #[test]
    fn is_execution_owned_tracks_linked_plan_branch() {
        let mut ws = workspace();
        assert!(!ws.is_execution_owned());
        ws.linked_plan_branch_id = Some(PlanBranchId::new());
        assert!(ws.is_execution_owned());
    }

    #[test]
    fn new_workspace_uses_default_auto_merge_method_and_active_status() {
        let ws = workspace();
        assert_eq!(
            ws.pr_auto_merge_method,
            super::super::agent_conversation_workspace::DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
        );
        assert_eq!(
            ws.status,
            super::super::agent_conversation_workspace::AgentConversationWorkspaceStatus::Active
        );
        assert!(ws.auto_publish_enabled);
        assert!(!ws.auto_publish_initial_pr_enabled);
    }
}

#[cfg(test)]
mod pr_description_tests {
    use super::super::agent_conversation_workspace::AgentWorkspacePrDescription;

    #[test]
    fn new_trims_title_and_drops_blank() {
        let desc = AgentWorkspacePrDescription::new(
            Some("  My PR title  ".to_string()),
            "body".to_string(),
        );
        assert_eq!(desc.title.as_deref(), Some("My PR title"));
        assert_eq!(desc.body_markdown, "body");
    }

    #[test]
    fn new_drops_blank_or_absent_title() {
        assert!(
            AgentWorkspacePrDescription::new(Some("   ".to_string()), "b".to_string())
                .title
                .is_none()
        );
        assert!(AgentWorkspacePrDescription::new(None, "b".to_string())
            .title
            .is_none());
    }
}

#[cfg(test)]
mod monitor_and_action_constructor_tests {
    use super::super::agent_conversation_workspace::{
        AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
        AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
        AgentWorkspacePrReviewMonitorStatus,
    };
    use super::super::{ChatConversationId, ProjectId};

    #[test]
    fn monitor_new_starts_idle_and_disabled() {
        let monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId("p".to_string()),
            42,
            Some("abc".to_string()),
        );
        assert_eq!(monitor.status, AgentWorkspacePrReviewMonitorStatus::Idle);
        assert!(!monitor.monitor_enabled);
        assert!(!monitor.first_review_completed);
        assert_eq!(monitor.pr_number, 42);
        assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("abc"));
        assert!(monitor.last_reviewed_head_sha.is_none());
    }

    #[test]
    fn monitor_settlement_status_preserves_terminal_and_reports_live_state() {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId("p".to_string()),
            42,
            Some("abc".to_string()),
        );

        assert_eq!(
            monitor.settlement_status(),
            AgentWorkspacePrReviewMonitorStatus::Paused
        );
        monitor.monitor_enabled = true;
        assert_eq!(
            monitor.settlement_status(),
            AgentWorkspacePrReviewMonitorStatus::Watching
        );
        monitor.last_error = Some("review failed".to_string());
        assert_eq!(
            monitor.settlement_status(),
            AgentWorkspacePrReviewMonitorStatus::Blocked
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
        assert_eq!(
            monitor.settlement_status(),
            AgentWorkspacePrReviewMonitorStatus::Terminal
        );
    }

    #[test]
    fn action_new_starts_pending_with_generated_id() {
        let action = AgentWorkspacePrReviewAction::new(
            ChatConversationId::new(),
            7,
            "head-sha".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "summary".to_string(),
            "review body".to_string(),
            Some("{}".to_string()),
            Some("run-1".to_string()),
        );
        assert_eq!(action.status, AgentWorkspacePrReviewActionStatus::Pending);
        assert!(!action.id.is_empty());
        assert_eq!(
            action.proposed_action,
            AgentWorkspacePrReviewActionKind::Approve
        );
        assert_eq!(action.head_sha, "head-sha");
        assert_eq!(action.created_by_run_id.as_deref(), Some("run-1"));
        assert!(action.submitted_review_id.is_none());
        assert!(action.resolved_at.is_none());
    }
}

#[cfg(test)]
mod enum_roundtrip_tests {
    use super::super::agent_conversation_workspace::{
        AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode,
        AgentConversationWorkspaceStatus, AgentWorkspacePrReviewActionKind,
        AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitorStatus,
        AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus,
        AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope,
    };
    use std::str::FromStr;

    #[test]
    fn workspace_mode_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentConversationWorkspaceMode::Chat, "chat"),
            (AgentConversationWorkspaceMode::Edit, "edit"),
            (AgentConversationWorkspaceMode::Plan, "plan"),
            (AgentConversationWorkspaceMode::Ideation, "ideation"),
            (AgentConversationWorkspaceMode::ReviewPr, "review_pr"),
            (AgentConversationWorkspaceMode::Automation, "automation"),
            (
                AgentConversationWorkspaceMode::PersonaBuilder,
                "persona_builder",
            ),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentConversationWorkspaceMode::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentConversationWorkspaceMode::from_str("bogus").is_err());
    }

    #[test]
    fn persona_builder_mode_display_fromstr_round_trip() {
        assert_eq!(
            AgentConversationWorkspaceMode::PersonaBuilder.to_string(),
            "persona_builder"
        );
        assert_eq!(
            AgentConversationWorkspaceMode::from_str("persona_builder").unwrap(),
            AgentConversationWorkspaceMode::PersonaBuilder
        );
    }

    #[test]
    fn workspace_status_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentConversationWorkspaceStatus::Active, "active"),
            (AgentConversationWorkspaceStatus::Archived, "archived"),
            (AgentConversationWorkspaceStatus::Missing, "missing"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentConversationWorkspaceStatus::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentConversationWorkspaceStatus::from_str("bogus").is_err());
    }

    #[test]
    fn workspace_branch_mode_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentConversationWorkspaceBranchMode::Isolated, "isolated"),
            (AgentConversationWorkspaceBranchMode::Linked, "linked"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentConversationWorkspaceBranchMode::from_str(text).unwrap(),
                variant
            );
        }
        assert_eq!(
            AgentConversationWorkspaceBranchMode::default(),
            AgentConversationWorkspaceBranchMode::Isolated
        );
        assert!(AgentConversationWorkspaceBranchMode::from_str("bogus").is_err());
    }

    #[test]
    fn monitor_status_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentWorkspacePrReviewMonitorStatus::Idle, "idle"),
            (AgentWorkspacePrReviewMonitorStatus::Reviewing, "reviewing"),
            (
                AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
                "awaiting_user",
            ),
            (AgentWorkspacePrReviewMonitorStatus::Watching, "watching"),
            (
                AgentWorkspacePrReviewMonitorStatus::Submitting,
                "submitting",
            ),
            (AgentWorkspacePrReviewMonitorStatus::Blocked, "blocked"),
            (AgentWorkspacePrReviewMonitorStatus::Paused, "paused"),
            (AgentWorkspacePrReviewMonitorStatus::Terminal, "terminal"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspacePrReviewMonitorStatus::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspacePrReviewMonitorStatus::from_str("bogus").is_err());
    }

    #[test]
    fn workspace_review_monitor_status_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentWorkspaceReviewMonitorStatus::Idle, "idle"),
            (AgentWorkspaceReviewMonitorStatus::Reviewing, "reviewing"),
            (AgentWorkspaceReviewMonitorStatus::Ready, "ready"),
            (AgentWorkspaceReviewMonitorStatus::Blocked, "blocked"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspaceReviewMonitorStatus::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspaceReviewMonitorStatus::from_str("bogus").is_err());
    }

    #[test]
    fn workspace_review_outcome_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentWorkspaceReviewOutcome::None, "none"),
            (AgentWorkspaceReviewOutcome::Passed, "passed"),
            (AgentWorkspaceReviewOutcome::Blocking, "blocking"),
            (AgentWorkspaceReviewOutcome::NoChanges, "no_changes"),
            (AgentWorkspaceReviewOutcome::RunFailed, "run_failed"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspaceReviewOutcome::from_str(text).unwrap(),
                variant
            );
        }
        assert_eq!(
            AgentWorkspaceReviewOutcome::from_str("reviewed").unwrap(),
            AgentWorkspaceReviewOutcome::Passed
        );
        assert_eq!(
            AgentWorkspaceReviewOutcome::from_str("blocked").unwrap(),
            AgentWorkspaceReviewOutcome::RunFailed
        );
        assert!(AgentWorkspaceReviewOutcome::from_str("bogus").is_err());
    }

    #[test]
    fn workspace_review_gate_status_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentWorkspaceReviewGateStatus::NotRequired, "not_required"),
            (AgentWorkspaceReviewGateStatus::Required, "required"),
            (AgentWorkspaceReviewGateStatus::Reviewing, "reviewing"),
            (AgentWorkspaceReviewGateStatus::Passed, "passed"),
            (AgentWorkspaceReviewGateStatus::Blocking, "blocking"),
            (AgentWorkspaceReviewGateStatus::Failed, "failed"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspaceReviewGateStatus::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspaceReviewGateStatus::from_str("bogus").is_err());
    }

    #[test]
    fn workspace_review_target_scope_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (
                AgentWorkspaceReviewTargetScope::SelectedSource,
                "selected_source",
            ),
            (
                AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                "workspace_delta",
            ),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspaceReviewTargetScope::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspaceReviewTargetScope::from_str("bogus").is_err());
    }

    #[test]
    fn action_kind_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (
                AgentWorkspacePrReviewActionKind::RequestChanges,
                "request_changes",
            ),
            (AgentWorkspacePrReviewActionKind::Approve, "approve"),
            (AgentWorkspacePrReviewActionKind::Comment, "comment"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspacePrReviewActionKind::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspacePrReviewActionKind::from_str("bogus").is_err());
    }

    #[test]
    fn action_status_display_and_from_str_roundtrip() {
        for (variant, text) in [
            (AgentWorkspacePrReviewActionStatus::Pending, "pending"),
            (AgentWorkspacePrReviewActionStatus::Approved, "approved"),
            (AgentWorkspacePrReviewActionStatus::Skipped, "skipped"),
            (AgentWorkspacePrReviewActionStatus::Submitting, "submitting"),
            (AgentWorkspacePrReviewActionStatus::Submitted, "submitted"),
            (AgentWorkspacePrReviewActionStatus::Failed, "failed"),
            (AgentWorkspacePrReviewActionStatus::Superseded, "superseded"),
        ] {
            assert_eq!(variant.to_string(), text);
            assert_eq!(
                AgentWorkspacePrReviewActionStatus::from_str(text).unwrap(),
                variant
            );
        }
        assert!(AgentWorkspacePrReviewActionStatus::from_str("bogus").is_err());
    }
}

#[cfg(test)]
mod is_open_pr_tests {
    use super::super::agent_conversation_workspace::{
        is_open_pr, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    };
    use super::super::{ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId};

    #[test]
    fn no_pr_number_is_not_open() {
        // No PR number → never open, regardless of status.
        assert!(!is_open_pr(None, None));
        assert!(!is_open_pr(None, Some("open")));
        assert!(!is_open_pr(None, Some("draft")));
    }

    #[test]
    fn terminal_status_is_not_open() {
        assert!(!is_open_pr(Some(42), Some("merged")));
        assert!(!is_open_pr(Some(42), Some("closed")));
    }

    #[test]
    fn non_terminal_status_with_number_is_open() {
        assert!(is_open_pr(Some(42), Some("draft")));
        assert!(is_open_pr(Some(42), Some("open")));
        // An unknown / not-yet-known status with a PR number is treated as open.
        assert!(is_open_pr(Some(42), None));
        assert!(is_open_pr(Some(42), Some("queued")));
    }

    fn workspace_with_pr(
        publication_pr_number: Option<i64>,
        publication_pr_status: Option<&str>,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::new(),
            ProjectId("project-1".to_string()),
            AgentConversationWorkspaceMode::Chat,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "feature/branch".to_string(),
            "/tmp/worktree".to_string(),
        );
        workspace.publication_pr_number = publication_pr_number;
        workspace.publication_pr_status = publication_pr_status.map(str::to_string);
        workspace
    }

    #[test]
    fn has_open_pr_delegates_to_is_open_pr() {
        assert!(!workspace_with_pr(None, None).has_open_pr());
        assert!(!workspace_with_pr(Some(7), Some("merged")).has_open_pr());
        assert!(!workspace_with_pr(Some(7), Some("closed")).has_open_pr());
        assert!(workspace_with_pr(Some(7), Some("open")).has_open_pr());
        assert!(workspace_with_pr(Some(7), Some("draft")).has_open_pr());
        assert!(workspace_with_pr(Some(7), None).has_open_pr());
    }
}
