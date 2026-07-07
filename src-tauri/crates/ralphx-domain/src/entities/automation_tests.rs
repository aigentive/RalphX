use super::{
    automation_is_transition_allowed, automation_run_is_transition_allowed, is_open_automation_run,
    judge_is_transition_allowed, judge_transition_clears_verdict, AutomationContextRefKind,
    AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRunId,
    AutomationRunStatus, AutomationStatus,
};

#[test]
fn automation_newtypes_display_and_default_to_generated_uuid() {
    let automation_id = AutomationId::from_string("automation-1");
    assert_eq!(automation_id.as_str(), "automation-1");
    assert_eq!(automation_id.to_string(), "automation-1");

    let generated_automation_id = AutomationId::default();
    assert!(uuid::Uuid::parse_str(generated_automation_id.as_str()).is_ok());

    let run_id = AutomationRunId::from_string("run-1");
    assert_eq!(run_id.as_str(), "run-1");
    assert_eq!(run_id.to_string(), "run-1");

    let generated_run_id = AutomationRunId::default();
    assert!(uuid::Uuid::parse_str(generated_run_id.as_str()).is_ok());
}

#[test]
fn automation_enum_strings_round_trip_and_reject_unknown_values() {
    use AutomationStatus::*;

    for (status, raw) in [
        (Draft, "draft"),
        (Active, "active"),
        (Paused, "paused"),
        (Completed, "completed"),
        (Stopped, "stopped"),
    ] {
        assert_eq!(status.as_str(), raw);
        assert_eq!(AutomationStatus::parse(raw), Some(status));
    }
    assert_eq!(AutomationStatus::parse("archived"), None);

    for (status, raw) in [
        (AutomationRunStatus::Pending, "pending"),
        (AutomationRunStatus::Provisioning, "provisioning"),
        (AutomationRunStatus::Running, "running"),
        (AutomationRunStatus::Published, "published"),
        (AutomationRunStatus::Merged, "merged"),
        (AutomationRunStatus::PrClosed, "pr_closed"),
        (AutomationRunStatus::AgentFailed, "agent_failed"),
        (AutomationRunStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(status.as_str(), raw);
        assert_eq!(AutomationRunStatus::parse(raw), Some(status));
    }
    assert_eq!(AutomationRunStatus::parse("judging"), None);

    for (state, raw) in [
        (AutomationJudgeState::None, "none"),
        (AutomationJudgeState::InProgress, "in_progress"),
        (AutomationJudgeState::Done, "done"),
        (AutomationJudgeState::Failed, "failed"),
        (AutomationJudgeState::Skipped, "skipped"),
    ] {
        assert_eq!(state.as_str(), raw);
        assert_eq!(AutomationJudgeState::parse(raw), Some(state));
    }
    assert_eq!(AutomationJudgeState::parse("retrying"), None);

    for (author, raw) in [
        (AutomationPromptAuthor::SetupAgent, "setup_agent"),
        (AutomationPromptAuthor::Judge, "judge"),
        (
            AutomationPromptAuthor::SkipJudgeTemplate,
            "skip_judge_template",
        ),
    ] {
        assert_eq!(author.as_str(), raw);
        assert_eq!(AutomationPromptAuthor::parse(raw), Some(author));
    }
    assert_eq!(AutomationPromptAuthor::parse("user"), None);

    for (kind, raw) in [
        (AutomationContextRefKind::Project, "project"),
        (AutomationContextRefKind::Integration, "integration"),
        (AutomationContextRefKind::Artifact, "artifact"),
    ] {
        assert_eq!(kind.as_str(), raw);
        assert_eq!(AutomationContextRefKind::parse(raw), Some(kind));
    }
    assert_eq!(AutomationContextRefKind::parse("ticket"), None);
}

#[test]
fn automation_status_transition_matrix_matches_spec() {
    use AutomationStatus::*;

    let statuses = [Draft, Active, Paused, Completed, Stopped];
    let allowed = [
        (Draft, Active),
        (Draft, Stopped),
        (Active, Paused),
        (Active, Completed),
        (Active, Stopped),
        (Paused, Active),
        (Paused, Stopped),
    ];

    for from in statuses {
        for to in statuses {
            assert_eq!(
                automation_is_transition_allowed(from, to),
                allowed.contains(&(from, to)),
                "unexpected automation transition {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn run_status_transition_matrix_matches_signal_status_contract() {
    use AutomationRunStatus::*;

    let statuses = [
        Pending,
        Provisioning,
        Running,
        Published,
        Completed,
        Merged,
        PrClosed,
        AgentFailed,
        Cancelled,
    ];
    let allowed = [
        (Pending, Provisioning),
        (Pending, Cancelled),
        (Provisioning, Running),
        (Provisioning, AgentFailed),
        (Provisioning, Cancelled),
        (Running, Published),
        (Running, Completed),
        (Running, AgentFailed),
        (Running, Cancelled),
        (Published, Merged),
        (Published, PrClosed),
        (Published, Cancelled),
    ];

    for from in statuses {
        for to in statuses {
            assert_eq!(
                automation_run_is_transition_allowed(from, to),
                allowed.contains(&(from, to)),
                "unexpected run transition {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn judge_lifecycle_transition_matrix_matches_spec() {
    use AutomationJudgeState::*;

    let states = [None, InProgress, Done, Failed, Skipped];
    let allowed = [
        (None, InProgress),
        (None, Skipped),
        (InProgress, Done),
        (InProgress, Failed),
        (Failed, InProgress),
    ];

    for from in states {
        for to in states {
            assert_eq!(
                judge_is_transition_allowed(from, to),
                allowed.contains(&(from, to)),
                "unexpected judge transition {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn judge_retry_entry_clears_previous_verdict_only_when_no_new_verdict_exists() {
    assert!(judge_transition_clears_verdict(
        AutomationJudgeState::InProgress,
        None
    ));
    assert!(!judge_transition_clears_verdict(
        AutomationJudgeState::InProgress,
        Some(r#"{"result":"new"}"#)
    ));
    assert!(!judge_transition_clears_verdict(
        AutomationJudgeState::Failed,
        None
    ));
    assert!(!judge_transition_clears_verdict(
        AutomationJudgeState::Skipped,
        None
    ));
}

#[test]
fn open_run_predicate_keeps_unjudged_signal_terminal_runs_open() {
    use AutomationJudgeState::*;
    use AutomationRunStatus::*;

    for status in [Pending, Provisioning, Running, Published] {
        for judge_state in [None, InProgress, Done, Failed, Skipped] {
            assert!(
                is_open_automation_run(status, judge_state),
                "{status:?}/{judge_state:?} should be open while run is in flight"
            );
        }
    }

    for status in [Merged, PrClosed, AgentFailed] {
        for judge_state in [None, InProgress, Failed] {
            assert!(
                is_open_automation_run(status, judge_state),
                "{status:?}/{judge_state:?} should stay open until judge resolves"
            );
        }
        for judge_state in [Done, Skipped] {
            assert!(
                !is_open_automation_run(status, judge_state),
                "{status:?}/{judge_state:?} should be closed after judge resolution"
            );
        }
    }

    for judge_state in [None, InProgress, Done, Failed, Skipped] {
        assert!(
            !is_open_automation_run(Cancelled, judge_state),
            "cancelled runs are terminal for every judge state"
        );
    }
}
