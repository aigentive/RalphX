const ACTIONABLE_PAUSED_REASON_CODES: [&str; 12] = [
    "judge_failed",
    "plan_judge_failed",
    "plan_revision_exhausted",
    "workspace_review_blocked",
    "max_runs_exhausted",
    "max_consecutive_failures",
    "signal_verification_failed",
    "judge_loop_suspected",
    "judge_stopped_unmet",
    "goal_replan_stale",
    "ideation_bridge_verification_failed",
    "ideation_bridge_missing_session",
];

pub(crate) fn is_actionable_paused_reason(reason: &str) -> bool {
    ACTIONABLE_PAUSED_REASON_CODES.contains(&reason)
}

pub(crate) fn paused_reason_label(reason: &str) -> &'static str {
    match reason {
        "judge_failed" => "judge failed",
        "plan_judge_failed" => "plan judge failed",
        "plan_revision_exhausted" => "plan revision limit reached",
        "workspace_review_blocked" => "workspace review blocked",
        "max_runs_exhausted" => "maximum run count reached",
        "max_consecutive_failures" => "too many consecutive failures",
        "signal_verification_failed" => "PR status verification failed",
        "judge_loop_suspected" => "judge loop suspected",
        "judge_stopped_unmet" => "judge stopped before the goal was met",
        "goal_replan_stale" => "goal re-plan proposal became stale",
        "ideation_bridge_verification_failed" => "ideation bridge plan verification failed",
        "ideation_bridge_missing_session" => "ideation bridge planning session missing",
        _ => "automation needs attention",
    }
}
