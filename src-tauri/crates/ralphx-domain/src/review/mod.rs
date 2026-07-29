// Review domain module
// Configuration and logic for AI and human code review

pub mod config;
pub mod complete_followup;
pub mod complete_history;
pub mod complete_issues;
pub mod complete_review;
pub mod complete_policy;
pub mod complete_result;
pub mod complete_support;
pub mod merge_backstop;
pub mod review_points;
pub mod scope_drift;

pub use config::ReviewSettings;
pub use complete_followup::{
    build_followup_activity_event, build_unrelated_drift_followup_draft,
    matching_unrelated_drift_followup_session_id, should_spawn_unrelated_drift_followup,
    update_review_scope_metadata, UnrelatedDriftFollowupDraft,
};
pub use complete_history::{build_ai_review_note, count_revision_cycles, pending_review_or_new};
pub use complete_issues::{build_review_issue_entities, build_review_note_issues};
pub use complete_review::{
    CompleteReviewInput, CompleteReviewValidationError, ParseReviewToolOutcomeError,
    ReviewIssueInput, ReviewIssueValidationError, ReviewToolOutcome,
};
pub use complete_policy::{
    parse_review_decision, review_outcome_for_tool, validate_complete_review_policy,
    CompleteReviewPolicyError, ParseReviewDecisionError,
};
pub use complete_result::{
    apply_review_outcome, approved_no_changes_target_status, approved_target_status,
    complete_review_response_message, review_note_content,
};
pub use complete_support::{
    build_unrelated_drift_followup_prompt, parse_review_issue, parse_review_issues,
    ParsedReviewIssue, RawReviewIssueInput,
};
pub use merge_backstop::{evaluate_merge_scope_backstop, MergeScopeBackstopViolation};
pub use review_points::{
    get_review_point_type, is_complex_task, is_destructive_task, should_auto_insert_review_point,
    ReviewPointConfig, ReviewPointType,
};
pub use scope_drift::{
    compute_out_of_scope_blocker_fingerprint, compute_scope_drift, matches_planned_scope,
    normalize_scope_path, ParseScopeDriftClassificationError, ScopeDriftClassification,
};
